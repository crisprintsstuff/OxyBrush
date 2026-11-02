//! Opt-in update checks (design: `docs/rfcs/0002-updates.md`).
//!
//! Nothing here touches the network unless the user turned updates on
//! (Settings > Updates) or clicked "Check now". Checks and downloads run on
//! a worker thread and report back through a channel; `Updater::tick` is
//! called once per frame to collect results and to start the daily check.
//! Installing always waits for the user.

use crate::settings::UpdateMode;
use oxy_update::release::Update;
use oxy_update::{InstallKind, Version};
use std::path::PathBuf;
use std::sync::mpsc;

/// Where the updater is.
#[derive(Debug, Clone, PartialEq)]
pub enum Phase {
    Idle,
    Checking,
    UpToDate,
    Available(Box<Update>),
    Downloading {
        update: Box<Update>,
        received: u64,
        total: u64,
    },
    /// Downloaded and verified, ready to install.
    Ready {
        update: Box<Update>,
        path: PathBuf,
    },
    Failed(String),
}

/// What the worker thread reports.
#[derive(Debug)]
pub enum Msg {
    Checked(Result<Option<Box<Update>>, String>),
    Progress(u64, u64),
    Downloaded(Result<PathBuf, String>),
}

pub struct Updater {
    pub mode: UpdateMode,
    pub phase: Phase,
    /// Seconds since 1970 of the last check (automatic or manual).
    pub last_check: u64,
    /// Version the user chose to skip ("" for none).
    pub skip: String,
    pub kind: InstallKind,
    pub show_window: bool,
    rx: Option<mpsc::Receiver<Msg>>,
}

impl Default for Updater {
    fn default() -> Self {
        Self {
            mode: UpdateMode::Off,
            phase: Phase::Idle,
            last_check: 0,
            skip: String::new(),
            kind: InstallKind::detect(),
            show_window: false,
            rx: None,
        }
    }
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Whether this build contains the network code.
pub const AVAILABLE: bool = cfg!(feature = "updater");

impl Updater {
    pub fn busy(&self) -> bool {
        self.rx.is_some()
    }

    /// The update to point the user at (not skipped), if any.
    pub fn offered(&self) -> Option<&Update> {
        let u = match &self.phase {
            Phase::Available(u) | Phase::Downloading { update: u, .. } | Phase::Ready { update: u, .. } => u,
            _ => return None,
        };
        (u.release.version.to_string() != self.skip).then_some(u)
    }

    /// Whether Flatpak (not the app) handles updates for this copy.
    pub fn managed_by_flatpak(&self) -> bool {
        self.kind == InstallKind::Flatpak
    }

    /// Call once per frame: collect worker results and start the daily
    /// check when updates are on.
    pub fn tick(&mut self, now: u64) {
        if let Some(rx) = self.rx.take() {
            let mut keep = true;
            loop {
                match rx.try_recv() {
                    Ok(msg) => {
                        let done = !matches!(msg, Msg::Progress(..));
                        self.apply(msg);
                        if done {
                            keep = false;
                            break;
                        }
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        keep = false;
                        self.phase = Phase::Failed("the update check stopped unexpectedly".into());
                        break;
                    }
                }
            }
            if keep {
                self.rx = Some(rx);
            }
        }
        if self.due(now) {
            self.check(now);
        }
        // Download automatically when asked to (installing still waits).
        if self.mode == UpdateMode::Download
            && !self.busy()
            && let Phase::Available(u) = &self.phase
            && u.downloadable()
            && u.release.version.to_string() != self.skip
        {
            self.download();
        }
    }

    /// Whether an automatic check should start now.
    pub fn due(&self, now: u64) -> bool {
        AVAILABLE
            && self.mode != UpdateMode::Off
            && !self.busy()
            && !matches!(self.phase, Phase::Available(_) | Phase::Downloading { .. } | Phase::Ready { .. })
            && now.saturating_sub(self.last_check) >= oxy_update::CHECK_INTERVAL_SECS
    }

    /// Apply a worker result (separate from the thread so it can be tested).
    pub fn apply(&mut self, msg: Msg) {
        self.phase = match (msg, std::mem::replace(&mut self.phase, Phase::Idle)) {
            (Msg::Checked(Ok(Some(u))), _) => Phase::Available(u),
            (Msg::Checked(Ok(None)), _) => Phase::UpToDate,
            (Msg::Checked(Err(e)), _) => Phase::Failed(e),
            (Msg::Progress(received, total), Phase::Downloading { update, .. }) => {
                Phase::Downloading { update, received, total }
            }
            (Msg::Progress(..), other) => other,
            (Msg::Downloaded(Ok(path)), Phase::Downloading { update, .. }) => Phase::Ready { update, path },
            (Msg::Downloaded(Err(e)), _) => Phase::Failed(e),
            (Msg::Downloaded(Ok(_)), other) => other,
        };
    }

    /// Check now (also when updates are off: the user asked).
    pub fn check(&mut self, now: u64) {
        if self.busy() {
            return;
        }
        self.last_check = now;
        if !AVAILABLE {
            self.phase = Phase::Failed("this build of OxyBrush was made without the updater".into());
            return;
        }
        self.phase = Phase::Checking;
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let kind = self.kind;
        spawn("oxy-update-check", move || {
            let _ = tx.send(Msg::Checked(check(&Version::current(), kind).map(|u| u.map(Box::new))));
        });
    }

    /// Download the offered update into the Downloads folder.
    pub fn download(&mut self) {
        let Phase::Available(update) = &self.phase else { return };
        if self.busy() || !update.downloadable() {
            return;
        }
        let update = update.clone();
        let dir = download_dir();
        self.phase = Phase::Downloading { update: update.clone(), received: 0, total: 0 };
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        spawn("oxy-update-download", move || {
            let progress_tx = tx.clone();
            let result = download(&update, &dir, &mut |r, t| {
                let _ = progress_tx.send(Msg::Progress(r, t));
            });
            let _ = tx.send(Msg::Downloaded(result));
        });
    }

    /// Skip the offered version (no more notices for it).
    pub fn skip_offered(&mut self) {
        if let Some(u) = self.offered() {
            self.skip = u.release.version.to_string();
        }
    }

    /// One line for the status bar / window.
    pub fn summary(&self) -> String {
        match &self.phase {
            Phase::Idle => match self.mode {
                UpdateMode::Off => "Update checks are off".into(),
                _ => "Update checks are on".into(),
            },
            Phase::Checking => "Checking for updates…".into(),
            Phase::UpToDate => format!("OxyBrush {} is up to date", env!("CARGO_PKG_VERSION")),
            Phase::Available(u) => format!("OxyBrush {} is available", u.release.version),
            Phase::Downloading { update, received, total } => {
                let pct = if *total > 0 { *received as f64 / *total as f64 * 100.0 } else { 0.0 };
                format!("Downloading OxyBrush {}… {pct:.0}%", update.release.version)
            }
            Phase::Ready { update, .. } => format!("OxyBrush {} is downloaded and verified", update.release.version),
            Phase::Failed(e) => format!("Update check failed: {e}"),
        }
    }
}

fn spawn(name: &str, f: impl FnOnce() + Send + 'static) {
    if let Err(e) = std::thread::Builder::new().name(name.into()).spawn(f) {
        log::warn!("could not start {name}: {e}");
    }
}

/// Where downloads go: the user's Downloads folder, so they can find the file.
pub fn download_dir() -> PathBuf {
    dirs::download_dir()
        .or_else(|| dirs::data_local_dir().map(|d| d.join(if cfg!(windows) { "OxyBrush" } else { "oxybrush" })))
        .unwrap_or_else(std::env::temp_dir)
}

#[cfg(feature = "updater")]
fn check(current: &Version, kind: InstallKind) -> Result<Option<Update>, String> {
    oxy_update::net::check(current, kind)
}

#[cfg(not(feature = "updater"))]
fn check(_: &Version, _: InstallKind) -> Result<Option<Update>, String> {
    Err("this build of OxyBrush was made without the updater".into())
}

#[cfg(feature = "updater")]
fn download(update: &Update, dir: &std::path::Path, progress: &mut dyn FnMut(u64, u64)) -> Result<PathBuf, String> {
    oxy_update::net::download(update, dir, progress)
}

#[cfg(not(feature = "updater"))]
fn download(_: &Update, _: &std::path::Path, _: &mut dyn FnMut(u64, u64)) -> Result<PathBuf, String> {
    Err("this build of OxyBrush was made without the updater".into())
}

/// What installing a downloaded update does on this system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallStep {
    /// Start the Windows installer, then quit so it can replace the app.
    RunInstaller(PathBuf),
    /// Show the file in its folder, with instructions.
    ShowFile { folder: PathBuf, how: &'static str },
}

pub fn install_step(kind: InstallKind, path: PathBuf) -> InstallStep {
    let folder = path.parent().map(PathBuf::from).unwrap_or_default();
    match kind {
        InstallKind::WindowsInstaller => InstallStep::RunInstaller(path),
        InstallKind::WindowsPortable => InstallStep::ShowFile {
            folder,
            how: "Close OxyBrush, unzip the new version and replace your old OxyBrush folder with it.",
        },
        InstallKind::MacDmg => InstallStep::ShowFile {
            folder,
            how: "Close OxyBrush, open the .dmg and drag OxyBrush onto Applications (replace the old copy).",
        },
        _ => InstallStep::ShowFile {
            folder,
            how: "Close OxyBrush, unpack the archive and replace your old oxybrush files with the new ones.",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_update::release::{Asset, Release};

    fn update(version: &str, downloadable: bool) -> Update {
        let asset = |n: &str| Asset { name: n.into(), url: format!("https://x/{n}"), size: 10 };
        Update {
            release: Release {
                version: Version::parse(version).unwrap(),
                tag: format!("v{version}"),
                page_url: "https://x".into(),
                prerelease: true,
                assets: Vec::new(),
            },
            file: downloadable.then(|| asset("OxyBrush.tar.gz")),
            checksums: downloadable.then(|| asset("SHA256SUMS.txt")),
            signature: None,
        }
    }

    #[test]
    fn off_means_no_automatic_checks() {
        let mut u = Updater::default();
        let day = oxy_update::CHECK_INTERVAL_SECS;
        assert!(!u.due(10 * day), "off by default: never checks on its own");
        u.mode = UpdateMode::Notify;
        assert_eq!(u.due(10 * day), AVAILABLE);
        u.last_check = 10 * day;
        assert!(!u.due(10 * day + 60), "at most once a day");
        assert_eq!(u.due(11 * day), AVAILABLE);
    }

    #[test]
    fn results_move_through_the_phases() {
        let mut u = Updater::default();
        u.apply(Msg::Checked(Ok(None)));
        assert_eq!(u.phase, Phase::UpToDate);
        u.apply(Msg::Checked(Ok(Some(Box::new(update("9.0.0", true))))));
        assert!(matches!(u.phase, Phase::Available(_)));
        assert_eq!(u.offered().unwrap().release.version.to_string(), "9.0.0");
        assert!(!u.due(u64::MAX), "no new check while an update is on offer");
        // Download progress and completion.
        let Phase::Available(up) = u.phase.clone() else { unreachable!() };
        u.phase = Phase::Downloading { update: up, received: 0, total: 0 };
        u.apply(Msg::Progress(5, 10));
        assert!(u.summary().contains("50%"), "{}", u.summary());
        u.apply(Msg::Downloaded(Ok(PathBuf::from("/tmp/OxyBrush.tar.gz"))));
        assert!(matches!(u.phase, Phase::Ready { .. }));
        // Skipping hides the notice for that version only.
        u.skip_offered();
        assert!(u.offered().is_none());
        u.apply(Msg::Checked(Ok(Some(Box::new(update("9.0.1", false))))));
        assert!(u.offered().is_some());
        u.apply(Msg::Downloaded(Err("checksum mismatch".into())));
        assert!(u.summary().contains("checksum mismatch"));
    }

    #[test]
    fn install_steps_per_platform() {
        let p = PathBuf::from("/d/OxyBrush-setup.exe");
        assert_eq!(install_step(InstallKind::WindowsInstaller, p.clone()), InstallStep::RunInstaller(p));
        for kind in [InstallKind::WindowsPortable, InstallKind::MacDmg, InstallKind::LinuxTarball] {
            match install_step(kind, PathBuf::from("/d/file")) {
                InstallStep::ShowFile { folder, how } => {
                    assert_eq!(folder, PathBuf::from("/d"));
                    assert!(how.starts_with("Close OxyBrush"));
                }
                other => panic!("{other:?}"),
            }
        }
    }
}

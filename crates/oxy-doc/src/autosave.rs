//! Periodic autosave and crash recovery.
//!
//! Each running instance writes `autosave-<pid>.oxb` into the per-user data
//! directory (`%LOCALAPPDATA%\OxyBrush\autosave` on Windows,
//! `~/.local/share/oxybrush/autosave` on Linux). A clean exit deletes it, so
//! any autosave left behind by another process id means that session ended
//! unexpectedly and can be offered for recovery.

use crate::SceneData;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

pub fn autosave_dir() -> Option<PathBuf> {
    let base = dirs::data_local_dir()?;
    let app = if cfg!(windows) { "OxyBrush" } else { "oxybrush" };
    Some(base.join(app).join("autosave"))
}

/// An autosave left behind by a session that didn't exit cleanly.
#[derive(Debug, Clone)]
pub struct Recoverable {
    pub path: PathBuf,
    pub modified: SystemTime,
}

/// Find autosaves from other (crashed) sessions, newest first.
pub fn find_recoverable(dir: &Path) -> Vec<Recoverable> {
    let own = own_file_name();
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<Recoverable> = entries
        .filter_map(Result::ok)
        .filter(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.starts_with("autosave-") && n.ends_with(".oxb") && n != own
        })
        .filter_map(|e| Some(Recoverable { modified: e.metadata().ok()?.modified().ok()?, path: e.path() }))
        .collect();
    out.sort_by_key(|r| std::cmp::Reverse(r.modified));
    out
}

fn own_file_name() -> String {
    format!("autosave-{}.oxb", std::process::id())
}

/// Saves on a background thread at a fixed interval when the document changed.
pub struct Autosaver {
    dir: Option<PathBuf>,
    pub interval: Duration,
    last: Instant,
    in_flight: Option<mpsc::Receiver<Result<(), String>>>,
    pub last_error: Option<String>,
    pub last_success: Option<Instant>,
}

impl Autosaver {
    pub fn new(dir: Option<PathBuf>, interval: Duration) -> Self {
        Self { dir, interval, last: Instant::now(), in_flight: None, last_error: None, last_success: None }
    }

    pub fn path(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(own_file_name()))
    }

    /// Whether a save is due now (the caller then provides a snapshot).
    pub fn due(&mut self, dirty: bool) -> bool {
        self.poll();
        dirty && self.in_flight.is_none() && self.dir.is_some() && self.last.elapsed() >= self.interval
    }

    /// Start writing `snapshot` in the background.
    pub fn start(&mut self, snapshot: SceneData) {
        let Some(path) = self.path() else { return };
        self.last = Instant::now();
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("autosave".into())
            .spawn(move || {
                let r = path
                    .parent()
                    .map_or(Ok(()), std::fs::create_dir_all)
                    .map_err(|e| e.to_string())
                    .and_then(|()| crate::format::save(&snapshot, &path).map_err(|e| e.to_string()));
                let _ = tx.send(r);
            })
            .map(|_| self.in_flight = Some(rx))
            .unwrap_or_else(|e| self.last_error = Some(e.to_string()));
    }

    fn poll(&mut self) {
        if let Some(rx) = &self.in_flight {
            match rx.try_recv() {
                Ok(Ok(())) => {
                    self.last_success = Some(Instant::now());
                    self.last_error = None;
                    self.in_flight = None;
                }
                Ok(Err(e)) => {
                    log::warn!("autosave failed: {e}");
                    self.last_error = Some(e);
                    self.in_flight = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => self.in_flight = None,
            }
        }
    }

    /// Call on clean exit: waits for a pending save, then removes our autosave.
    pub fn finish(&mut self) {
        if let Some(rx) = self.in_flight.take() {
            let _ = rx.recv_timeout(Duration::from_secs(10));
        }
        if let Some(p) = self.path() {
            let _ = std::fs::remove_file(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PieceData;
    use oxy_mesh::primitives;

    #[test]
    fn autosave_writes_and_finish_cleans_up() {
        let dir = std::env::temp_dir().join(format!("oxb-autosave-{}", std::process::id()));
        let mut a = Autosaver::new(Some(dir.clone()), Duration::ZERO);
        assert!(!a.due(false));
        assert!(a.due(true));
        let scene = SceneData {
            pieces: vec![PieceData {
                name: "s".into(),
                visible: true,
                mesh: primitives::icosphere(2, 1.0).to_data(),
                multires: None,
            }],
            active: 0,
        };
        a.start(scene.clone());
        let deadline = Instant::now() + Duration::from_secs(10);
        while a.last_success.is_none() && Instant::now() < deadline {
            a.due(true);
            std::thread::sleep(Duration::from_millis(5));
        }
        let path = a.path().unwrap();
        assert_eq!(crate::format::load(&path).unwrap(), scene);
        // Our own file is never offered for recovery; another pid's is.
        assert!(find_recoverable(&dir).is_empty());
        let other = dir.join("autosave-999999999.oxb");
        std::fs::copy(&path, &other).unwrap();
        assert_eq!(find_recoverable(&dir).len(), 1);
        a.finish();
        assert!(!path.exists());
        std::fs::remove_dir_all(dir).ok();
    }
}

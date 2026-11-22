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
    /// Original saved project path this autosave came from, if any.
    pub source_path: Option<PathBuf>,
}

fn source_path_file(autosave_path: &Path) -> PathBuf {
    let mut os = autosave_path.as_os_str().to_os_string();
    os.push(".path");
    PathBuf::from(os)
}

/// Delete an autosave file and its source-path sidecar, if present.
pub fn remove_recoverable(path: &Path) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(source_path_file(path));
}

/// Find autosaves from other (crashed) sessions, newest first.
pub fn find_recoverable(dir: &Path) -> Vec<Recoverable> {
    let pid = std::process::id();
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<Recoverable> = entries
        .filter_map(Result::ok)
        .filter(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.starts_with("autosave-") && n.ends_with(".oxb") && !is_own_autosave(&n, pid)
        })
        .filter_map(|e| {
            let path = e.path();
            let modified = e.metadata().ok()?.modified().ok()?;
            let source_path = std::fs::read_to_string(source_path_file(&path))
                .ok()
                .map(|t| t.trim().to_owned())
                .filter(|t| !t.is_empty())
                .map(PathBuf::from);
            Some(Recoverable { path, modified, source_path })
        })
        .collect();
    out.sort_by_key(|r| std::cmp::Reverse(r.modified));
    out
}

fn is_own_autosave(name: &str, pid: u32) -> bool {
    let prefix = format!("autosave-{pid}-");
    let legacy = format!("autosave-{pid}.oxb");
    name == legacy || (name.starts_with(&prefix) && name.ends_with(".oxb"))
}

fn is_own_autosave_artifact(name: &str, pid: u32) -> bool {
    let prefix = format!("autosave-{pid}-");
    let legacy = format!("autosave-{pid}.oxb");
    let legacy_path = format!("autosave-{pid}.oxb.path");
    name == legacy
        || name == legacy_path
        || (name.starts_with(&prefix) && (name.ends_with(".oxb") || name.ends_with(".oxb.path")))
}

fn own_file_name() -> String {
    format!("autosave-{}.oxb", std::process::id())
}

/// Saves on a background thread at a fixed interval when the document changed.
pub struct Autosaver {
    dir: Option<PathBuf>,
    pub interval: Duration,
    pub enabled: bool,
    last: Instant,
    in_flight: Option<mpsc::Receiver<Result<(), String>>>,
    pub last_error: Option<String>,
    pub last_success: Option<Instant>,
}

impl Autosaver {
    pub fn new(dir: Option<PathBuf>, interval: Duration) -> Self {
        Self {
            dir,
            interval,
            enabled: true,
            last: Instant::now(),
            in_flight: None,
            last_error: None,
            last_success: None,
        }
    }

    pub fn path(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(own_file_name()))
    }

    /// Autosave file path for a specific document id (`autosave-<pid>-<doc_id>.oxb`).
    pub fn path_for(&self, doc_id: u64) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(format!("autosave-{}-{}.oxb", std::process::id(), doc_id)))
    }

    /// Whether a save is due now (the caller then provides a snapshot).
    pub fn due(&mut self, dirty: bool) -> bool {
        self.poll();
        self.enabled && dirty && self.in_flight.is_none() && self.dir.is_some() && self.last.elapsed() >= self.interval
    }

    /// Start writing `snapshot` in the background for a specific document id.
    /// When `source_path` is provided, writes it atomically into a sibling
    /// `.path` sidecar so crash recovery knows which file it came from.
    pub fn start_for(&mut self, doc_id: u64, snapshot: SceneData, source_path: Option<PathBuf>) {
        if !self.enabled {
            return;
        }
        let Some(path) = self.path_for(doc_id) else { return };
        self.last = Instant::now();
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("autosave".into())
            .spawn(move || {
                let r = path
                    .parent()
                    .map_or(Ok(()), std::fs::create_dir_all)
                    .map_err(|e| e.to_string())
                    .and_then(|()| crate::format::save(&snapshot, &path).map_err(|e| e.to_string()))
                    .and_then(|()| {
                        let meta = source_path_file(&path);
                        match source_path {
                            Some(src) => {
                                let mut tmp_os = meta.as_os_str().to_os_string();
                                tmp_os.push(".tmp");
                                let tmp = PathBuf::from(tmp_os);
                                std::fs::write(&tmp, src.to_string_lossy().as_bytes())
                                    .and_then(|()| std::fs::rename(&tmp, &meta))
                                    .map_err(|e| e.to_string())
                            }
                            None => {
                                let _ = std::fs::remove_file(&meta);
                                Ok(())
                            }
                        }
                    });
                let _ = tx.send(r);
            })
            .map(|_| self.in_flight = Some(rx))
            .unwrap_or_else(|e| self.last_error = Some(e.to_string()));
    }

    /// Delete the autosave file and sidecar for a specific document id (e.g. on clean tab close).
    pub fn remove_file_for(&self, doc_id: u64) {
        if let Some(p) = self.path_for(doc_id) {
            remove_recoverable(&p);
        }
    }

    /// Start writing `snapshot` in the background.
    pub fn start(&mut self, snapshot: SceneData) {
        if !self.enabled {
            return;
        }
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

    /// Call on clean exit: waits for a pending save, then removes all autosaves for this pid.
    pub fn finish(&mut self) {
        if let Some(rx) = self.in_flight.take() {
            let _ = rx.recv_timeout(Duration::from_secs(10));
        }
        if let Some(dir) = &self.dir {
            let pid = std::process::id();
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.filter_map(Result::ok) {
                    let n = entry.file_name().to_string_lossy().into_owned();
                    if is_own_autosave_artifact(&n, pid) {
                        let _ = std::fs::remove_file(entry.path());
                    }
                }
            }
        }
        if let Some(p) = self.path() {
            remove_recoverable(&p);
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
                vertex_map: None,
                symmetry_center: None,
                cage: None,
                tubes: Vec::new(),
                curve: None,
                strokes: None,
                material: None,
                instancers: Vec::new(),
                parent: None,
                link: None,
                polygons: None,
            }],
            active: 0,
            alphas: Vec::new(),
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
        // Disabled autosave is never due even if dirty.
        a.enabled = false;
        assert!(!a.due(true));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn multi_document_autosaves_and_per_tab_cleanup() {
        let dir = std::env::temp_dir().join(format!("oxb-autosave-multi-{}", std::process::id()));
        let mut a = Autosaver::new(Some(dir.clone()), Duration::ZERO);
        let scene = SceneData {
            pieces: vec![PieceData {
                name: "p".into(),
                visible: true,
                mesh: primitives::cube(1, 1.0).to_data(),
                multires: None,
                vertex_map: None,
                symmetry_center: None,
                cage: None,
                tubes: Vec::new(),
                curve: None,
                strokes: None,
                material: None,
                instancers: Vec::new(),
                parent: None,
                link: None,
                polygons: None,
            }],
            active: 0,
            alphas: Vec::new(),
        };

        let p1 = a.path_for(1).unwrap();
        let p2 = a.path_for(2).unwrap();
        assert_eq!(p1, dir.join(format!("autosave-{}-1.oxb", std::process::id())));
        assert_eq!(p2, dir.join(format!("autosave-{}-2.oxb", std::process::id())));

        a.start_for(1, scene.clone(), None);
        let deadline = Instant::now() + Duration::from_secs(10);
        while a.last_success.is_none() && Instant::now() < deadline {
            a.due(true);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(p1.exists());

        // Verify own multi-tab files are ignored by find_recoverable
        assert!(find_recoverable(&dir).is_empty());

        // Simulate a crashed session's multi-tab file
        let other_crash = dir.join("autosave-88888888-1.oxb");
        std::fs::copy(&p1, &other_crash).unwrap();
        let rec = find_recoverable(&dir);
        assert_eq!(rec.len(), 1);
        assert_eq!(rec[0].source_path, None);

        // Remove single doc file on clean tab close
        a.remove_file_for(1);
        assert!(!p1.exists());
        assert_eq!(find_recoverable(&dir).len(), 1);

        remove_recoverable(&other_crash);
        assert!(find_recoverable(&dir).is_empty());

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn autosave_source_path_sidecar_roundtrip_and_cleanup() {
        let dir = std::env::temp_dir().join(format!("oxb-autosave-src-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut a = Autosaver::new(Some(dir.clone()), Duration::ZERO);
        let scene = SceneData {
            pieces: vec![PieceData {
                name: "p".into(),
                visible: true,
                mesh: primitives::cube(1, 1.0).to_data(),
                multires: None,
                vertex_map: None,
                symmetry_center: None,
                cage: None,
                tubes: Vec::new(),
                curve: None,
                strokes: None,
                material: None,
                instancers: Vec::new(),
                parent: None,
                link: None,
                polygons: None,
            }],
            active: 0,
            alphas: Vec::new(),
        };
        let origin = PathBuf::from("/tmp/project_one.oxb");
        a.start_for(7, scene.clone(), Some(origin.clone()));
        let deadline = Instant::now() + Duration::from_secs(10);
        while a.last_success.is_none() && Instant::now() < deadline {
            a.due(true);
            std::thread::sleep(Duration::from_millis(5));
        }
        let p7 = a.path_for(7).unwrap();
        let s7 = source_path_file(&p7);
        assert!(p7.exists() && s7.exists());

        // Simulate crashed sibling session files and read source_path back
        let crash_oxb = dir.join("autosave-77777777-7.oxb");
        let crash_path = source_path_file(&crash_oxb);
        std::fs::copy(&p7, &crash_oxb).unwrap();
        std::fs::copy(&s7, &crash_path).unwrap();
        let rec = find_recoverable(&dir);
        assert_eq!(rec.len(), 1);
        assert_eq!(rec[0].source_path.as_ref(), Some(&origin));

        // remove_recoverable cleans up both .oxb and .oxb.path
        remove_recoverable(&crash_oxb);
        assert!(!crash_oxb.exists() && !crash_path.exists());

        // Clearing source_path to None removes any existing sidecar on next autosave
        a.last_success = None;
        a.start_for(7, scene, None);
        let deadline = Instant::now() + Duration::from_secs(10);
        while a.last_success.is_none() && Instant::now() < deadline {
            a.due(true);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(p7.exists() && !s7.exists());

        // Re-write sidecar and check finish() deletes both own .oxb and .oxb.path
        std::fs::write(&s7, "/tmp/other.oxb").unwrap();
        a.finish();
        assert!(!p7.exists() && !s7.exists());
        std::fs::remove_dir_all(dir).ok();
    }
}

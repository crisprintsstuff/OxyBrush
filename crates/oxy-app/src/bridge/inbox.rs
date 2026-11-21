//! Local handoff for `oxybrush --link`. A window holds an OS file lock for
//! its lifetime. Launchers atomically enqueue a request when that lock is
//! held, so simultaneous sends don't start competing receiver windows.

use fs4::fs_std::FileExt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(crate) enum Registration {
    Receiver(Inbox),
    Forwarded,
    OtherWindow,
}

pub(crate) struct Inbox {
    directory: PathBuf,
    _lock: File,
    poll_at: Instant,
}

impl Inbox {
    /// Use the current profile so separate accounts and test profiles do
    /// not exchange requests. Returns None if the profile is unavailable.
    pub(crate) fn register(outgoing: Option<&Path>) -> io::Result<Registration> {
        let Some(settings) = crate::settings::Settings::default_path() else { return Ok(Registration::OtherWindow) };
        let Some(profile) = settings.parent() else { return Ok(Registration::OtherWindow) };
        Self::at(profile.join("bridge-inbox"), outgoing)
    }

    fn at(directory: PathBuf, outgoing: Option<&Path>) -> io::Result<Registration> {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&directory)?;
        let lock =
            OpenOptions::new().read(true).write(true).create(true).truncate(false).open(directory.join("receiver"))?;
        if FileExt::try_lock_exclusive(&lock)? {
            return Ok(Registration::Receiver(Self { directory, _lock: lock, poll_at: Instant::now() }));
        }
        let Some(manifest) = outgoing else { return Ok(Registration::OtherWindow) };
        let path = std::path::absolute(manifest)?;
        let text = serde_json::to_vec(&path)?;
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        let file = directory.join(format!("{stamp:032x}-{:08x}.request", std::process::id()));
        oxy_io::write_atomic(&file, &text)?;
        Ok(Registration::Forwarded)
    }

    /// Process one complete request at a time. The event loop leaves the
    /// queue alone while another dialog or a document job owns the tab.
    pub(crate) fn pop(&mut self, now: Instant) -> Option<PathBuf> {
        if now < self.poll_at {
            return None;
        }
        self.poll_at = now + Duration::from_millis(300);
        let mut requests: Vec<_> = fs::read_dir(&self.directory)
            .ok()?
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|ty| ty.is_file()))
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "request"))
            .collect();
        requests.sort_unstable();
        for request in requests {
            let mut bytes = Vec::new();
            let read = File::open(&request).and_then(|file| file.take(64 * 1024).read_to_end(&mut bytes));
            if fs::remove_file(&request).is_err() {
                continue;
            }
            if read.is_ok()
                && let Ok(path) = serde_json::from_slice::<PathBuf>(&bytes)
                && path.is_absolute()
                && super::is_manifest(&path)
            {
                return Some(path);
            }
            log::warn!("ignoring an invalid bridge inbox request");
        }
        None
    }

    pub(crate) fn wait(&self, now: Instant) -> Duration {
        self.poll_at.saturating_duration_since(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_launchers_queue_while_a_receiver_is_running_and_can_take_over_after_exit() {
        let directory = std::env::temp_dir().join(format!("oxy-bridge-inbox-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        let Registration::Receiver(mut receiver) = Inbox::at(directory.clone(), None).unwrap() else {
            panic!("first launch should receive");
        };
        let first = directory.join("Cone with spaces.oxylink.json");
        let second = directory.join("球.oxylink.json");
        assert!(matches!(Inbox::at(directory.clone(), None).unwrap(), Registration::OtherWindow));
        assert!(matches!(Inbox::at(directory.clone(), Some(&first)).unwrap(), Registration::Forwarded));
        assert!(matches!(Inbox::at(directory.clone(), Some(&second)).unwrap(), Registration::Forwarded));
        fs::write(directory.join("incomplete.request.tmp"), b"partial").unwrap();
        let now = Instant::now();
        assert_eq!(receiver.pop(now), Some(first));
        assert!(receiver.pop(now).is_none(), "polling is rate limited");
        assert_eq!(receiver.pop(now + Duration::from_secs(1)), Some(second));
        assert!(receiver.pop(now + Duration::from_secs(2)).is_none());
        drop(receiver);
        assert!(matches!(Inbox::at(directory.clone(), None).unwrap(), Registration::Receiver(_)));
        fs::remove_dir_all(directory).unwrap();
    }
}

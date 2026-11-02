//! The network side: one HTTPS request to list releases, and downloads.
//!
//! Only HTTPS is allowed (redirects included). Requests send a
//! `User-Agent: OxyBrush/<version>` header and nothing that identifies the
//! user or the machine. Downloads are written to a temporary name and only
//! renamed into place after they verify, so a broken or tampered download
//! never sits where it could be opened as the update.

use crate::release::{self, InstallKind, Update};
use crate::verify;
use crate::version::Version;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Largest file we'll accept (the packages are ~5–15 MB).
const MAX_DOWNLOAD: u64 = 512 * 1024 * 1024;

fn agent(quick: bool) -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .https_only(true)
        .user_agent(format!("OxyBrush/{}", env!("CARGO_PKG_VERSION")))
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        // A check should be quick; a download may take a while on slow links,
        // but must not stall forever between reads.
        .timeout_global(quick.then(|| Duration::from_secs(45)))
        .timeout_per_read(Some(Duration::from_secs(60)))
        .build();
    config.into()
}

/// Ask GitHub for recent releases and pick the update to offer, if any.
pub fn check(current: &Version, kind: InstallKind) -> Result<Option<Update>, String> {
    let url = format!("https://api.github.com/repos/{}/releases?per_page=20", crate::REPO);
    let mut resp = agent(true)
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .call()
        .map_err(|e| format!("could not reach GitHub: {e}"))?;
    let json = resp
        .body_mut()
        .with_config()
        .limit(8 * 1024 * 1024)
        .read_to_string()
        .map_err(|e| format!("could not read the release list: {e}"))?;
    let releases = release::parse_releases(&json)?;
    Ok(release::pick_update(&releases, current, kind))
}

fn fetch_text(url: &str) -> Result<String, String> {
    let mut resp = agent(true).get(url).call().map_err(|e| format!("download failed: {e}"))?;
    resp.body_mut().with_config().limit(1024 * 1024).read_to_string().map_err(|e| format!("download failed: {e}"))
}

/// Download the update's file into `dir`, verify it, and return its path.
/// `progress(received, total)` is called as data arrives.
pub fn download(update: &Update, dir: &Path, progress: &mut dyn FnMut(u64, u64)) -> Result<PathBuf, String> {
    let (Some(file), Some(sums)) = (&update.file, &update.checksums) else {
        return Err("this release has no verifiable download for your system".into());
    };
    // The checksum list first (and its signature, when this build requires one).
    let checksums = fetch_text(&sums.url)?;
    if let Some(public_key) = crate::SIGNING_PUBLIC_KEY {
        let sig = update.signature.as_ref().ok_or("this release is not signed; refusing to download it")?;
        let signature = fetch_text(&sig.url)?;
        verify::verify_signature(checksums.as_bytes(), &signature, public_key)?;
    }
    if !verify::parse_checksums(&checksums).contains_key(&file.name) {
        return Err(format!("{} is not listed in the release checksums", file.name));
    }

    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let target = dir.join(&file.name);
    let partial = dir.join(format!("{}.part", file.name));
    let result = (|| {
        let mut resp = agent(false).get(&file.url).call().map_err(|e| format!("download failed: {e}"))?;
        let total = resp.body().content_length().unwrap_or(file.size);
        let mut reader = resp.body_mut().with_config().limit(MAX_DOWNLOAD).reader();
        let mut out = std::fs::File::create(&partial).map_err(|e| format!("{}: {e}", partial.display()))?;
        let mut buf = vec![0u8; 1 << 16];
        let mut received = 0u64;
        loop {
            let n = reader.read(&mut buf).map_err(|e| format!("download interrupted: {e}"))?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n]).map_err(|e| format!("could not save the download: {e}"))?;
            received += n as u64;
            progress(received, total);
        }
        out.sync_all().map_err(|e| format!("could not save the download: {e}"))?;
        drop(out);
        verify::verify_file(&partial, &file.name, &checksums)?;
        std::fs::rename(&partial, &target).map_err(|e| format!("could not save the download: {e}"))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    result.map(|()| target)
}

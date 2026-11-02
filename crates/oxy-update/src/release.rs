//! Reading GitHub's releases listing and choosing what to offer.
//!
//! Policy:
//! - Drafts are ignored; only versions newer than the running one count.
//! - Someone running a pre-release (`0.3.0-alpha`) is offered pre-releases
//!   too; someone on a stable version is offered stable versions only.
//! - The file offered matches how OxyBrush was installed (the Windows
//!   installer upgrades an installed copy, the portable zip a portable one).
//! - A download is only offered when the release has `SHA256SUMS.txt`, so
//!   every download can be verified.

use crate::version::Version;
use serde_json::Value;

/// How this copy of OxyBrush was installed, which decides the file to get.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallKind {
    WindowsInstaller,
    WindowsPortable,
    MacDmg,
    LinuxTarball,
    /// Updated by Flatpak itself (`flatpak update`), never by the app.
    Flatpak,
    /// No release file for this platform (e.g. Linux on ARM).
    Unsupported,
}

impl InstallKind {
    /// Detect from the platform, the Flatpak environment and, on Windows,
    /// whether the installer's uninstaller sits next to the executable.
    pub fn detect() -> InstallKind {
        if std::env::var_os("FLATPAK_ID").is_some() {
            return InstallKind::Flatpak;
        }
        match (std::env::consts::OS, std::env::consts::ARCH) {
            ("windows", "x86_64") => {
                let installed = std::env::current_exe()
                    .ok()
                    .and_then(|p| p.parent().map(|d| d.join("unins000.exe").exists()))
                    .unwrap_or(false);
                if installed { InstallKind::WindowsInstaller } else { InstallKind::WindowsPortable }
            }
            ("macos", _) => InstallKind::MacDmg,
            ("linux", "x86_64") => InstallKind::LinuxTarball,
            _ => InstallKind::Unsupported,
        }
    }

    /// End of the release file name for this install kind (see the release workflow).
    pub fn asset_suffix(self) -> Option<&'static str> {
        match self {
            InstallKind::WindowsInstaller => Some("-windows-x86_64-setup.exe"),
            InstallKind::WindowsPortable => Some("-windows-x86_64-portable.zip"),
            InstallKind::MacDmg => Some("-macos-universal.dmg"),
            InstallKind::LinuxTarball => Some("-linux-x86_64.tar.gz"),
            InstallKind::Flatpak | InstallKind::Unsupported => None,
        }
    }

    /// Whether the app can download updates itself for this kind.
    pub fn can_download(self) -> bool {
        self.asset_suffix().is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    pub tag: String,
    /// Release page (notes) in the browser.
    pub page_url: String,
    pub prerelease: bool,
    pub assets: Vec<Asset>,
}

/// An update to offer: the release, and the files for this platform (the
/// file itself, the checksum list and its signature) when available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update {
    pub release: Release,
    pub file: Option<Asset>,
    pub checksums: Option<Asset>,
    pub signature: Option<Asset>,
}

impl Update {
    /// Whether the app can download and verify this update itself.
    pub fn downloadable(&self) -> bool {
        self.file.is_some() && self.checksums.is_some()
    }
}

/// Parse the JSON array returned by `GET /repos/{repo}/releases`.
/// Entries that aren't usable (drafts, tags that aren't versions) are skipped.
pub fn parse_releases(json: &str) -> Result<Vec<Release>, String> {
    let value: Value = serde_json::from_str(json).map_err(|e| format!("unreadable release list: {e}"))?;
    let list = value.as_array().ok_or("release list is not an array")?;
    let mut out = Vec::new();
    for r in list {
        if r["draft"].as_bool().unwrap_or(false) {
            continue;
        }
        let Some(tag) = r["tag_name"].as_str() else { continue };
        let Some(version) = Version::parse(tag) else { continue };
        let assets = r["assets"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| {
                        Some(Asset {
                            name: x["name"].as_str()?.to_owned(),
                            url: x["browser_download_url"].as_str()?.to_owned(),
                            size: x["size"].as_u64().unwrap_or(0),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.push(Release {
            version,
            tag: tag.to_owned(),
            page_url: r["html_url"].as_str().unwrap_or_default().to_owned(),
            prerelease: r["prerelease"].as_bool().unwrap_or(false),
            assets,
        });
    }
    Ok(out)
}

/// The newest release worth offering over `current`, if any.
pub fn pick_update(releases: &[Release], current: &Version, kind: InstallKind) -> Option<Update> {
    let allow_pre = !current.pre.is_empty();
    let release = releases
        .iter()
        .filter(|r| r.version > *current && (allow_pre || (!r.prerelease && r.version.pre.is_empty())))
        .max_by(|a, b| a.version.cmp(&b.version))?
        .clone();
    let find = |pred: &dyn Fn(&str) -> bool| release.assets.iter().find(|a| pred(&a.name)).cloned();
    let file = kind.asset_suffix().and_then(|suffix| find(&|n: &str| n.ends_with(suffix)));
    let checksums = find(&|n: &str| n == "SHA256SUMS.txt");
    let signature = find(&|n: &str| n == "SHA256SUMS.txt.sig");
    Some(Update { release, file, checksums, signature })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed-down shape of GitHub's response.
    const LISTING: &str = r#"[
      {"tag_name": "v0.5.0-alpha", "draft": true, "prerelease": true, "html_url": "d", "assets": []},
      {"tag_name": "v0.4.0-alpha", "draft": false, "prerelease": true,
       "html_url": "https://github.com/crisprintsstuff/OxyBrush/releases/tag/v0.4.0-alpha",
       "assets": [
         {"name": "OxyBrush-0.4.0-alpha-windows-x86_64-setup.exe", "browser_download_url": "https://x/setup.exe", "size": 7000000},
         {"name": "OxyBrush-0.4.0-alpha-windows-x86_64-portable.zip", "browser_download_url": "https://x/p.zip", "size": 7100000},
         {"name": "OxyBrush-0.4.0-alpha-macos-universal.dmg", "browser_download_url": "https://x/m.dmg", "size": 13000000},
         {"name": "OxyBrush-0.4.0-alpha-linux-x86_64.tar.gz", "browser_download_url": "https://x/l.tgz", "size": 9000000},
         {"name": "OxyBrush-0.4.0-alpha-linux-x86_64.flatpak", "browser_download_url": "https://x/f", "size": 6000000},
         {"name": "SHA256SUMS.txt", "browser_download_url": "https://x/sums", "size": 549}
       ]},
      {"tag_name": "v0.3.0-alpha", "draft": false, "prerelease": true, "html_url": "o", "assets": []},
      {"tag_name": "nightly", "draft": false, "prerelease": true, "html_url": "n", "assets": []}
    ]"#;

    #[test]
    fn parses_listing_and_skips_drafts_and_odd_tags() {
        let r = parse_releases(LISTING).unwrap();
        let tags: Vec<&str> = r.iter().map(|r| r.tag.as_str()).collect();
        assert_eq!(tags, ["v0.4.0-alpha", "v0.3.0-alpha"]);
        assert_eq!(r[0].assets.len(), 6);
        assert!(parse_releases("{}").is_err());
        assert!(parse_releases("not json").is_err());
    }

    #[test]
    fn offers_the_newest_release_with_the_right_file() {
        let r = parse_releases(LISTING).unwrap();
        let current = Version::parse("0.3.0-alpha").unwrap();
        let u = pick_update(&r, &current, InstallKind::WindowsInstaller).unwrap();
        assert_eq!(u.release.version.to_string(), "0.4.0-alpha");
        assert_eq!(u.file.as_ref().unwrap().name, "OxyBrush-0.4.0-alpha-windows-x86_64-setup.exe");
        assert!(u.downloadable());
        assert!(u.signature.is_none());
        let portable = pick_update(&r, &current, InstallKind::WindowsPortable).unwrap();
        assert!(portable.file.unwrap().name.ends_with("portable.zip"));
        assert!(pick_update(&r, &current, InstallKind::MacDmg).unwrap().file.unwrap().name.ends_with(".dmg"));
        assert!(pick_update(&r, &current, InstallKind::LinuxTarball).unwrap().file.unwrap().name.ends_with(".tar.gz"));
        // Flatpak and unsupported platforms get a notice but no download.
        for kind in [InstallKind::Flatpak, InstallKind::Unsupported] {
            let u = pick_update(&r, &current, kind).unwrap();
            assert!(u.file.is_none() && !u.downloadable());
        }
    }

    #[test]
    fn no_offer_when_up_to_date_or_for_prereleases_on_stable() {
        let r = parse_releases(LISTING).unwrap();
        let latest = Version::parse("0.4.0-alpha").unwrap();
        assert!(pick_update(&r, &latest, InstallKind::LinuxTarball).is_none());
        // Stable users don't get pre-releases.
        let stable = Version::parse("0.2.0").unwrap();
        assert!(pick_update(&r, &stable, InstallKind::LinuxTarball).is_none());
        // Without a checksum list the update is announced but not downloadable.
        let mut no_sums = r.clone();
        no_sums[0].assets.retain(|a| a.name != "SHA256SUMS.txt");
        let current = Version::parse("0.3.0-alpha").unwrap();
        assert!(!pick_update(&no_sums, &current, InstallKind::LinuxTarball).unwrap().downloadable());
    }
}

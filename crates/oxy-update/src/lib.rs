//! Opt-in update checks for OxyBrush (design: `docs/rfcs/0002-updates.md`).
//!
//! The app only talks to the network when the user turned updates on, or
//! asked for a check. Requests go to GitHub's public releases API and to
//! the release download URLs; they carry a `User-Agent` with the OxyBrush
//! version and nothing else (no identifiers, no usage data).
//!
//! This crate is split so the policy is testable without a network:
//! - [`version`]: compare release versions (`0.3.0-alpha` < `0.3.0`).
//! - [`release`]: parse the releases listing, pick the newest applicable
//!   release and the right file for this platform and install kind.
//! - [`verify`]: SHA-256 against `SHA256SUMS.txt`, and an ed25519 signature
//!   over that file when the build carries a public key.
//! - `net` (feature `network`): the HTTPS requests themselves.

pub mod release;
pub mod verify;
pub mod version;

#[cfg(feature = "network")]
pub mod net;

pub use release::{Asset, InstallKind, Release};
pub use version::Version;

/// GitHub repository the releases come from.
pub const REPO: &str = "crisprintsstuff/OxyBrush";

/// Public key (32 bytes, hex) that release checksums are signed with, if
/// the maintainer has set one up (see the RFC). When present, downloads
/// without a valid signature are rejected; when absent, the checksum file
/// is trusted because it comes from GitHub over HTTPS.
pub const SIGNING_PUBLIC_KEY: Option<&str> = None;

/// How often automatic checks may run.
pub const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;

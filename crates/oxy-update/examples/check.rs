//! Check for an update as a given version would:
//!
//!     cargo run -p oxy-update --features network --example check -- 0.2.0-alpha
//!
//! With a second argument, reads a saved `GET /releases` response from that
//! file instead of the network (for offline testing of the selection logic).
use oxy_update::{InstallKind, Version};

fn main() {
    let v = std::env::args().nth(1).and_then(|s| Version::parse(&s)).unwrap_or_else(Version::current);
    let kind = InstallKind::detect();
    println!("as {v} ({kind:?}):");
    let result = match std::env::args().nth(2) {
        Some(path) => std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|json| oxy_update::release::parse_releases(&json))
            .map(|r| oxy_update::release::pick_update(&r, &v, kind)),
        None => oxy_update::net::check(&v, kind),
    };
    match result {
        Ok(Some(u)) => println!(
            "update {} at {}\n  file: {:?}\n  downloadable: {}",
            u.release.version,
            u.release.page_url,
            u.file.as_ref().map(|f| &f.name),
            u.downloadable()
        ),
        Ok(None) => println!("up to date"),
        Err(e) => println!("error: {e}"),
    }
}

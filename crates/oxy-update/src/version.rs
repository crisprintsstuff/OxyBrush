//! Release version ordering, following Semantic Versioning precedence:
//! `major.minor.patch` numerically, then a version with a pre-release tag
//! (`-alpha`, `-beta.2`) sorts before the same version without one, and
//! pre-release identifiers compare numerically when both are numbers,
//! otherwise as text.

use std::cmp::Ordering;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// Dot-separated pre-release identifiers; empty for a release.
    pub pre: Vec<String>,
}

impl Version {
    /// Parse `1.2.3`, `v1.2.3`, `1.2.3-alpha.1` (build metadata after `+`
    /// is ignored). Returns None for anything else.
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim();
        let s = s.strip_prefix('v').unwrap_or(s);
        let s = s.split('+').next()?;
        let (core, pre) = match s.split_once('-') {
            Some((_, "")) => return None,
            Some((c, p)) => (c, p),
            None => (s, ""),
        };
        let mut nums = core.split('.').map(|n| n.parse::<u64>().ok());
        let (major, minor, patch) = (nums.next()??, nums.next()??, nums.next()??);
        if nums.next().is_some() {
            return None;
        }
        let pre: Vec<String> = if pre.is_empty() { Vec::new() } else { pre.split('.').map(str::to_owned).collect() };
        if pre.iter().any(|p| p.is_empty()) {
            return None;
        }
        Some(Version { major, minor, patch, pre })
    }

    /// The version this binary was built as.
    pub fn current() -> Version {
        Version::parse(env!("CARGO_PKG_VERSION")).expect("crate version is valid semver")
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if !self.pre.is_empty() {
            write!(f, "-{}", self.pre.join("."))?;
        }
        Ok(())
    }
}

impl Ord for Version {
    fn cmp(&self, o: &Self) -> Ordering {
        (self.major, self.minor, self.patch).cmp(&(o.major, o.minor, o.patch)).then_with(|| {
            match (self.pre.is_empty(), o.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => {
                    for (a, b) in self.pre.iter().zip(&o.pre) {
                        let c = match (a.parse::<u64>(), b.parse::<u64>()) {
                            (Ok(x), Ok(y)) => x.cmp(&y),
                            (Ok(_), Err(_)) => Ordering::Less,
                            (Err(_), Ok(_)) => Ordering::Greater,
                            (Err(_), Err(_)) => a.cmp(b),
                        };
                        if c != Ordering::Equal {
                            return c;
                        }
                    }
                    self.pre.len().cmp(&o.pre.len())
                }
            }
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn parses_and_prints() {
        assert_eq!(v("v0.3.0-alpha").to_string(), "0.3.0-alpha");
        assert_eq!(v("1.2.3+build.5"), v("1.2.3"));
        for bad in ["", "1.2", "1.2.3.4", "a.b.c", "1.2.3-", "1.2.3-alpha..1"] {
            assert!(Version::parse(bad).is_none(), "{bad:?}");
        }
        assert_eq!(Version::current().to_string(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn semver_precedence() {
        // From the SemVer 2.0 specification's precedence example, plus ours.
        let ordered = [
            "0.2.0-alpha",
            "0.3.0-alpha",
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-rc.1",
            "1.0.0",
            "1.0.1",
            "1.1.0",
            "2.0.0",
        ];
        for w in ordered.windows(2) {
            assert!(v(w[0]) < v(w[1]), "{} < {}", w[0], w[1]);
        }
    }
}

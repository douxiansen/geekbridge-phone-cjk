// SPDX-License-Identifier: GPL-3.0-or-later
//! Semantic versions: `1.2.3`, with an optional leading `v` and `-prerelease` suffix.

use std::cmp::Ordering;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// A pre-release suffix such as `rc.1`; a release without one is newer.
    pub pre: Option<String>,
}

impl Version {
    pub fn parse(text: &str) -> Option<Self> {
        let t = text.trim().trim_start_matches('v');
        let (core, pre) = match t.split_once('-') {
            Some((c, p)) if !p.is_empty() => (c, Some(p.to_string())),
            Some(_) => return None,
            None => (t, None),
        };
        // Build metadata (+...) never takes part in ordering.
        let core = core.split('+').next()?;
        let mut parts = core.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            major,
            minor,
            patch,
            pre,
        })
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (&self.pre, &other.pre) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(a), Some(b)) => a.cmp(b),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(p) = &self.pre {
            write!(f, "-{p}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn parses_and_orders() {
        assert!(v("0.1.1") > v("0.1.0"));
        assert!(v("v1.0.0") > v("0.9.9"));
        assert!(v("0.2.0") > v("0.1.99"));
        assert_eq!(v("v0.1.0"), v("0.1.0"));
        assert!(v("1.0.0") > v("1.0.0-rc.1"));
        assert!(v("1.0.0-rc.2") > v("1.0.0-rc.1"));
        assert_eq!(v("1.2.3+build5"), v("1.2.3"));
        assert_eq!(v("1.2.3-beta").to_string(), "1.2.3-beta");
    }

    #[test]
    fn rejects_garbage() {
        for bad in [
            "", "1", "1.2", "1.2.3.4", "a.b.c", "1.2.x", "1.2.3-", "latest",
        ] {
            assert!(Version::parse(bad).is_none(), "{bad}");
        }
    }
}

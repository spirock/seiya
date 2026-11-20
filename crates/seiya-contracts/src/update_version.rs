//! Strict stable version comparison. This module performs no I/O.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct StableVersion {
    major: u64,
    minor: u64,
    patch: u64,
}

impl StableVersion {
    /// Accept exactly three decimal components without leading zeroes.
    pub fn parse(value: &str) -> Result<Self, &'static str> {
        if value.len() > 62 || !value.is_ascii() {
            return Err("invalid_stable_version");
        }
        let mut components = value.split('.');
        fn component(value: Option<&str>) -> Result<u64, &'static str> {
            let value = value.ok_or("invalid_stable_version")?;
            if value.is_empty()
                || value.len() > 20
                || (value.len() > 1 && value.starts_with('0'))
                || !value.bytes().all(|b| b.is_ascii_digit())
            {
                return Err("invalid_stable_version");
            }
            value.parse().map_err(|_| "invalid_stable_version")
        }
        let version = Self {
            major: component(components.next())?,
            minor: component(components.next())?,
            patch: component(components.next())?,
        };
        if components.next().is_some() {
            return Err("invalid_stable_version");
        }
        Ok(version)
    }
}

/// A candidate is an update only when both versions are valid and it is newer.
pub fn newer_stable(current: &str, candidate: &str) -> Result<bool, &'static str> {
    Ok(StableVersion::parse(candidate)? > StableVersion::parse(current)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comparison_is_numeric_and_never_promotes_downgrades() {
        for (current, candidate, expected) in [
            ("1.2.3", "1.2.3", false),
            ("1.2.3", "1.2.2", false),
            ("1.2.3", "1.2.4", true),
            ("1.9.99", "1.10.0", true),
            ("2.0.0", "1.99.99", false),
            ("0.1.0", "1.0.0", true),
        ] {
            assert_eq!(newer_stable(current, candidate), Ok(expected));
        }
    }

    #[test]
    fn malformed_or_prerelease_versions_never_become_stable_updates() {
        for bad in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "01.2.3",
            "1.02.3",
            "1.2.03",
            "v1.2.3",
            "1.2.3-rc.1",
            "1.2.3+build",
            " 1.2.3",
            "1.2.3\n",
            "-1.2.3",
            "18446744073709551616.0.0",
            "１.2.3",
        ] {
            assert!(newer_stable("1.2.3", bad).is_err(), "{bad:?}");
            assert!(newer_stable(bad, "1.2.3").is_err(), "{bad:?}");
        }
    }
}

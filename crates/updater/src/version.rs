use anyhow::{Context as _, Result};
use semver::Version;

pub fn parse(tag: &str) -> Result<Version> {
    let trimmed = tag.strip_prefix('v').unwrap_or(tag);
    Version::parse(trimmed).with_context(|| format!("invalid version: {tag:?}"))
}

pub fn is_newer(current: &Version, candidate: &Version) -> bool {
    candidate > current
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_with_and_without_v_prefix() {
        assert_eq!(parse("v1.2.3").unwrap(), Version::new(1, 2, 3));
        assert_eq!(parse("1.2.3").unwrap(), Version::new(1, 2, 3));
    }

    #[test]
    fn rejects_non_semver() {
        assert!(parse("nightly").is_err());
        assert!(parse("v1.2").is_err());
    }

    #[test]
    fn newer_requires_strictly_greater() {
        let base = Version::new(0, 2, 0);
        assert!(is_newer(&base, &Version::new(0, 2, 1)));
        assert!(is_newer(&base, &Version::new(0, 3, 0)));
        assert!(is_newer(&base, &Version::new(1, 0, 0)));
        assert!(!is_newer(&base, &base.clone()));
        assert!(!is_newer(&base, &Version::new(0, 1, 9)));
    }
}

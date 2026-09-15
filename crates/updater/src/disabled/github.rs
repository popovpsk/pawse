use anyhow::Result;
use semver::Version;

pub struct Found {
    pub version: Version,
    pub url: String,
    pub digest: Option<String>,
}

pub fn fetch_latest() -> Result<Found> {
    anyhow::bail!("this build was compiled without the updater")
}

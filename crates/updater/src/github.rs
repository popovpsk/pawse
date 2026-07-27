use anyhow::{Context as _, Result};
use semver::Version;
use serde::Deserialize;
use std::time::Duration;

use crate::version;

const REPO: &str = "popovpsk/pawse";
const USER_AGENT: &str = "pawse-updater";

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    digest: Option<String>,
}

pub struct Found {
    pub version: Version,
    pub url: String,
    pub digest: Option<String>,
}

pub fn fetch_latest() -> Result<Found> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(30)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            .timeout_recv_body(Some(Duration::from_secs(30)))
            .build(),
    );
    let mut response = agent
        .get(&url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .call()
        .context("GitHub releases request failed")?;

    let body = response
        .body_mut()
        .read_to_string()
        .context("reading GitHub releases response")?;
    let release: Release =
        serde_json::from_str(&body).context("parsing GitHub releases response")?;

    let version = version::parse(&release.tag_name)?;
    let asset = select_asset(&release.assets).context("no release asset matches this platform")?;
    Ok(Found {
        version,
        url: asset.browser_download_url.clone(),
        digest: asset.digest.clone(),
    })
}

fn select_asset(assets: &[Asset]) -> Option<&Asset> {
    assets.iter().find(|asset| asset_matches(&asset.name))
}

#[cfg(target_os = "macos")]
fn asset_matches(name: &str) -> bool {
    name.ends_with(".dmg")
}

#[cfg(target_os = "windows")]
fn asset_matches(name: &str) -> bool {
    name.ends_with("-setup.exe")
}

#[cfg(target_os = "linux")]
fn asset_matches(name: &str) -> bool {
    name.ends_with(".AppImage") && name.contains(std::env::consts::ARCH)
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn asset_matches(_name: &str) -> bool {
    false
}

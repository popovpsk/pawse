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

#[cfg(any(target_os = "macos", test))]
fn macos_asset(name: &str) -> bool {
    name.ends_with(".dmg")
}

#[cfg(any(target_os = "windows", test))]
fn windows_asset(name: &str, arch: &str) -> bool {
    match arch {
        "x86_64" => name.ends_with("_x64-setup.exe"),
        "aarch64" => name.ends_with("_arm64-installer.exe"),
        _ => false,
    }
}

#[cfg(any(target_os = "linux", test))]
fn linux_asset(name: &str, arch: &str) -> bool {
    name.ends_with(".AppImage") && name.contains(arch)
}

#[cfg(target_os = "macos")]
fn asset_matches(name: &str) -> bool {
    macos_asset(name)
}

#[cfg(target_os = "windows")]
fn asset_matches(name: &str) -> bool {
    windows_asset(name, std::env::consts::ARCH)
}

#[cfg(target_os = "linux")]
fn asset_matches(name: &str) -> bool {
    linux_asset(name, std::env::consts::ARCH)
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn asset_matches(_name: &str) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const RELEASE_ASSETS: &[&str] = &[
        "pawse_1.0.0_aarch64-pacman.tar.gz",
        "pawse_1.0.0_aarch64.AppImage",
        "pawse_1.0.0_aarch64.AppImage.zsync",
        "Pawse_1.0.0_aarch64.dmg",
        "pawse_1.0.0_amd64.deb",
        "pawse_1.0.0_arm64-installer.exe",
        "pawse_1.0.0_arm64-portable.zip",
        "pawse_1.0.0_arm64.deb",
        "pawse_1.0.0_x64-portable.zip",
        "pawse_1.0.0_x64-setup.exe",
        "pawse_1.0.0_x86_64-pacman.tar.gz",
        "pawse_1.0.0_x86_64.AppImage",
        "pawse_1.0.0_x86_64.AppImage.zsync",
    ];

    fn assets() -> Vec<Asset> {
        RELEASE_ASSETS
            .iter()
            .map(|name| Asset {
                name: (*name).to_string(),
                browser_download_url: format!("https://example.invalid/{name}"),
                digest: None,
            })
            .collect()
    }

    #[test]
    fn exactly_one_setup_exe_per_release() {
        let setups: Vec<&&str> = RELEASE_ASSETS
            .iter()
            .filter(|name| name.ends_with("-setup.exe"))
            .collect();
        assert_eq!(setups, vec![&"pawse_1.0.0_x64-setup.exe"]);
    }

    fn only_match(rule: impl Fn(&str) -> bool) -> &'static str {
        let matched: Vec<&&str> = RELEASE_ASSETS.iter().filter(|name| rule(name)).collect();
        assert_eq!(matched.len(), 1, "expected one match, got {matched:?}");
        matched[0]
    }

    #[test]
    fn windows_x86_64_never_picks_the_arm64_installer() {
        assert_eq!(
            only_match(|name| windows_asset(name, "x86_64")),
            "pawse_1.0.0_x64-setup.exe"
        );
    }

    #[test]
    fn windows_aarch64_picks_the_arm64_installer() {
        assert_eq!(
            only_match(|name| windows_asset(name, "aarch64")),
            "pawse_1.0.0_arm64-installer.exe"
        );
    }

    #[test]
    fn macos_picks_the_only_dmg() {
        assert_eq!(only_match(macos_asset), "Pawse_1.0.0_aarch64.dmg");
    }

    #[test]
    fn linux_picks_the_appimage_for_its_arch() {
        assert_eq!(
            only_match(|name| linux_asset(name, "x86_64")),
            "pawse_1.0.0_x86_64.AppImage"
        );
        assert_eq!(
            only_match(|name| linux_asset(name, "aarch64")),
            "pawse_1.0.0_aarch64.AppImage"
        );
    }

    #[test]
    fn select_asset_is_order_independent() {
        let mut assets = assets();
        assets.reverse();
        let reversed = select_asset(&assets).map(|asset| asset.name.clone());
        let forward = select_asset(&self::assets()).map(|asset| asset.name.clone());
        assert_eq!(reversed, forward);
        assert!(forward.is_some());
    }
}

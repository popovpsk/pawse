use anyhow::Result;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

pub struct Staged {
    #[cfg(target_os = "windows")]
    installer: std::path::PathBuf,
}

pub fn download_and_stage(url: &str, app_bundle: Option<std::path::PathBuf>) -> Result<Staged> {
    #[cfg(target_os = "macos")]
    {
        let bundle =
            app_bundle.ok_or_else(|| anyhow::anyhow!("running app path is unavailable"))?;
        macos::install(url, &bundle)?;
        Ok(Staged {})
    }
    #[cfg(target_os = "windows")]
    {
        let _ = app_bundle;
        let installer = windows::download(url)?;
        Ok(Staged { installer })
    }
    #[cfg(target_os = "linux")]
    {
        let _ = app_bundle;
        linux::install(url)?;
        Ok(Staged {})
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = (url, app_bundle);
        anyhow::bail!("auto-update is not supported on this platform")
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn appimage_path() -> Option<std::path::PathBuf> {
    linux::appimage_path()
}

impl Staged {
    pub fn finalize_on_quit(&self, relaunch: bool) {
        #[cfg(target_os = "windows")]
        windows::launch_installer(&self.installer, relaunch);
        #[cfg(not(target_os = "windows"))]
        let _ = relaunch;
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
pub(crate) fn download_file(url: &str, dest: &std::path::Path) -> Result<()> {
    use anyhow::Context as _;
    use std::time::Duration;

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(30))
        .timeout_read(Duration::from_secs(60))
        .build();
    let response = agent
        .get(url)
        .set("User-Agent", "pawse-updater")
        .call()
        .context("download request failed")?;
    let mut reader = response.into_reader();
    let mut file =
        std::fs::File::create(dest).with_context(|| format!("creating {}", dest.display()))?;
    std::io::copy(&mut reader, &mut file).context("writing downloaded file")?;
    Ok(())
}

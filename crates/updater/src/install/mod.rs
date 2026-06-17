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

pub fn download_and_stage(
    url: &str,
    digest: Option<&str>,
    app_bundle: Option<std::path::PathBuf>,
) -> Result<Staged> {
    #[cfg(target_os = "macos")]
    {
        let bundle =
            app_bundle.ok_or_else(|| anyhow::anyhow!("running app path is unavailable"))?;
        macos::install(url, digest, &bundle)?;
        Ok(Staged {})
    }
    #[cfg(target_os = "windows")]
    {
        let _ = app_bundle;
        let installer = windows::download(url, digest)?;
        Ok(Staged { installer })
    }
    #[cfg(target_os = "linux")]
    {
        let _ = app_bundle;
        linux::install(url, digest)?;
        Ok(Staged {})
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = (url, digest, app_bundle);
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
pub(crate) fn download_file(
    url: &str,
    dest: &std::path::Path,
    expected_digest: Option<&str>,
) -> Result<()> {
    use anyhow::Context as _;
    use sha2::{Digest as _, Sha256};
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
    let file =
        std::fs::File::create(dest).with_context(|| format!("creating {}", dest.display()))?;
    let mut writer = HashingWriter {
        inner: file,
        hasher: Sha256::new(),
    };
    std::io::copy(&mut reader, &mut writer).context("writing downloaded file")?;

    match expected_digest {
        None => log::warn!("updater: release asset has no digest; skipping SHA-256 verification"),
        Some(digest) => match digest.strip_prefix("sha256:") {
            Some(expected) => {
                let actual = format!("{:x}", writer.hasher.finalize());
                if !actual.eq_ignore_ascii_case(expected) {
                    let _ = std::fs::remove_file(dest);
                    anyhow::bail!(
                        "downloaded file failed SHA-256 verification (expected {expected}, got {actual})"
                    );
                }
            }
            None => log::warn!(
                "updater: release asset digest {digest:?} is not SHA-256; skipping verification"
            ),
        },
    }
    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
struct HashingWriter<W> {
    inner: W,
    hasher: sha2::Sha256,
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
impl<W: std::io::Write> std::io::Write for HashingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        use sha2::Digest as _;
        let n = self.inner.write(buf)?;
        self.hasher.update(&buf[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

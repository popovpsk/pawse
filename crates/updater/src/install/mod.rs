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

const DOWNLOAD_ATTEMPTS: u32 = 5;

const DOWNLOAD_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(3);

const DOWNLOAD_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

const DOWNLOAD_TOTAL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15 * 60);

const DOWNLOAD_STALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

pub(crate) fn download_file(
    url: &str,
    dest: &std::path::Path,
    expected_digest: Option<&str>,
) -> Result<()> {
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_connect(Some(DOWNLOAD_CONNECT_TIMEOUT))
            .timeout_recv_response(Some(DOWNLOAD_TOTAL_TIMEOUT))
            .timeout_recv_body(Some(DOWNLOAD_STALL_TIMEOUT))
            .build(),
    );

    let mut progress = Progress::default();
    let mut last_error = None;

    for attempt in 1..=DOWNLOAD_ATTEMPTS {
        if attempt > 1 {
            std::thread::sleep(DOWNLOAD_RETRY_DELAY);
        }

        if let Err(error) = fetch_into(&agent, url, dest, &mut progress) {
            log::warn!(
                "updater: download attempt {attempt}/{DOWNLOAD_ATTEMPTS} failed at {} bytes: {error:#}",
                progress.received
            );
            last_error = Some(error);
            continue;
        }

        match verify_digest(progress.hasher.clone(), expected_digest) {
            Ok(()) => return Ok(()),
            Err(error) => {
                log::warn!("updater: {error:#}; re-downloading from scratch");
                progress.reset();
                last_error = Some(error);
            }
        }
    }

    let _ = std::fs::remove_file(dest);
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("download failed")))
}

#[derive(Default)]
struct Progress {
    hasher: sha2::Sha256,
    received: u64,
}

impl Progress {
    fn reset(&mut self) {
        *self = Self::default();
    }
}

fn fetch_into(
    agent: &ureq::Agent,
    url: &str,
    dest: &std::path::Path,
    progress: &mut Progress,
) -> Result<()> {
    use anyhow::Context as _;

    let mut request = agent.get(url).header("User-Agent", "pawse-updater");
    if progress.received > 0 {
        request = request.header("Range", format!("bytes={}-", progress.received));
    }
    let response = request.call().context("download request failed")?;

    if progress.received > 0 && response.status().as_u16() != 206 {
        progress.reset();
    }

    let file = if progress.received > 0 {
        std::fs::OpenOptions::new()
            .append(true)
            .open(dest)
            .with_context(|| format!("opening {}", dest.display()))?
    } else {
        std::fs::File::create(dest).with_context(|| format!("creating {}", dest.display()))?
    };

    let mut reader = response.into_body().into_reader();
    let mut writer = HashingWriter {
        inner: file,
        hasher: std::mem::take(&mut progress.hasher),
        written: 0,
    };
    let copied = std::io::copy(&mut reader, &mut writer);
    progress.received += writer.written;
    progress.hasher = writer.hasher;
    copied.context("writing downloaded file")?;
    Ok(())
}

fn verify_digest(hasher: sha2::Sha256, expected_digest: Option<&str>) -> Result<()> {
    use sha2::Digest as _;

    let Some(digest) = expected_digest else {
        log::warn!("updater: release asset has no digest; skipping SHA-256 verification");
        return Ok(());
    };
    let Some(expected) = digest.strip_prefix("sha256:") else {
        log::warn!(
            "updater: release asset digest {digest:?} is not SHA-256; skipping verification"
        );
        return Ok(());
    };
    let actual = format!("{:x}", hasher.finalize());
    anyhow::ensure!(
        actual.eq_ignore_ascii_case(expected),
        "downloaded file failed SHA-256 verification (expected {expected}, got {actual})"
    );
    Ok(())
}

struct HashingWriter<W> {
    inner: W,
    hasher: sha2::Sha256,
    written: u64,
}

impl<W: std::io::Write> std::io::Write for HashingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        use sha2::Digest as _;
        let n = self.inner.write(buf)?;
        self.hasher.update(&buf[..n]);
        self.written += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

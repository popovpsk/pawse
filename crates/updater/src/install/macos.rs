use anyhow::{Context as _, Result};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Mounted {
    mount_point: PathBuf,
}

impl Drop for Mounted {
    fn drop(&mut self) {
        let _ = Command::new("hdiutil")
            .args(["detach", "-force"])
            .arg(&self.mount_point)
            .output();
    }
}

pub fn install(url: &str, digest: Option<&str>, app_bundle: &Path) -> Result<()> {
    let dir = tempfile::Builder::new()
        .prefix("pawse-update")
        .tempdir()
        .context("creating temp dir")?;
    let dmg = dir.path().join("pawse.dmg");
    super::download_file(url, &dmg, digest)?;

    let output = Command::new("hdiutil")
        .args(["attach", "-nobrowse", "-mountrandom"])
        .arg(dir.path())
        .arg(&dmg)
        .output()
        .context("running hdiutil attach")?;
    anyhow::ensure!(
        output.status.success(),
        "hdiutil attach failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let canonical = std::fs::canonicalize(dir.path()).unwrap_or_else(|_| dir.path().to_path_buf());
    let prefix = canonical.to_string_lossy();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mount_point = stdout
        .split_whitespace()
        .find(|token| token.starts_with(prefix.as_ref()))
        .map(PathBuf::from)
        .context("could not determine dmg mount point")?;
    let _mounted = Mounted {
        mount_point: mount_point.clone(),
    };

    let app_name = app_bundle
        .file_name()
        .context("running app has no file name")?;
    let mut source: OsString = mount_point.join(app_name).into();
    source.push("/");

    let output = Command::new("rsync")
        .args(["-a", "--delete", "--exclude", "Icon?"])
        .arg(&source)
        .arg(app_bundle)
        .output()
        .context("running rsync")?;
    anyhow::ensure!(
        output.status.success(),
        "rsync failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    Ok(())
}

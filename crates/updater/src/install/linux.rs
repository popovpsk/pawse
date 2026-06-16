use anyhow::{Context as _, Result};
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;

pub fn appimage_path() -> Option<PathBuf> {
    std::env::var_os("APPIMAGE").map(PathBuf::from)
}

pub fn install(url: &str) -> Result<()> {
    let target = appimage_path().context("not running as an AppImage")?;
    let parent = target.parent().context("AppImage path has no parent")?;
    let file_name = target
        .file_name()
        .context("AppImage path has no file name")?
        .to_string_lossy();
    let tmp = parent.join(format!("{file_name}.new"));

    super::download_file(url, &tmp)?;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
        .context("setting AppImage permissions")?;
    std::fs::rename(&tmp, &target).context("replacing AppImage")?;
    Ok(())
}

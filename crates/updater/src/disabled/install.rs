use anyhow::Result;

pub struct Staged;

pub fn download_and_stage(
    _url: &str,
    _digest: Option<&str>,
    _app_bundle: Option<std::path::PathBuf>,
) -> Result<Staged> {
    anyhow::bail!("this build was compiled without the updater")
}

#[cfg(target_os = "linux")]
pub(crate) fn appimage_path() -> Option<std::path::PathBuf> {
    None
}

impl Staged {
    pub fn finalize_on_quit(&self, _relaunch: bool) {}
}

use anyhow::{Context as _, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn download(url: &str) -> Result<PathBuf> {
    let dir = dirs::cache_dir()
        .context("no cache directory")?
        .join("pawse")
        .join("updates");
    std::fs::create_dir_all(&dir).context("creating updates directory")?;
    let dest = dir.join("Pawse-setup.exe");
    super::download_file(url, &dest)?;
    Ok(dest)
}

pub fn launch_installer(installer: &Path, relaunch: bool) {
    use std::os::windows::process::CommandExt as _;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let inner = match (relaunch, std::env::current_exe()) {
        (true, Ok(exe)) => format!(
            "\"{}\" /S & start \"\" \"{}\"",
            installer.display(),
            exe.display()
        ),
        _ => format!("\"{}\" /S", installer.display()),
    };
    // why: cmd /C strips one outer quote pair; wrap the whole command so its inner quotes survive
    let raw = format!("/C \"{inner}\"");

    let _ = Command::new("cmd")
        .raw_arg(raw)
        .creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW)
        .spawn();
}

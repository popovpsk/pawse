use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::crypto::{self, KEY_LEN, Key};

pub(crate) fn path(dir: &Path, wrap: &Key) -> PathBuf {
    dir.join(format!("secret-{}.key", wrap.id()))
}

pub(crate) fn load(dir: &Path, wrap: &Key, create: bool) -> Option<Key> {
    let path = path(dir, wrap);
    match fs::read_to_string(&path) {
        Ok(text) => unwrap(wrap, &text, &path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if create {
                create_key(dir, &path, wrap)
            } else {
                None
            }
        }
        Err(error) => {
            log::warn!("secrets: cannot read {}: {error}", path.display());
            None
        }
    }
}

fn unwrap(wrap: &Key, text: &str, path: &Path) -> Option<Key> {
    let key = crypto::open(std::slice::from_ref(wrap), text.trim())
        .and_then(|(bytes, _)| Key::from_slice(&bytes));
    if key.is_none() {
        log::warn!(
            "secrets: {} does not open with this build's key; left as is",
            path.display()
        );
    }
    key
}

fn create_key(dir: &Path, path: &Path, wrap: &Key) -> Option<Key> {
    match write_new(dir, path, wrap) {
        Ok(key) => Some(key),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => fs::read_to_string(path)
            .ok()
            .and_then(|text| unwrap(wrap, &text, path)),
        Err(error) => {
            log::warn!("secrets: cannot create {}: {error}", path.display());
            None
        }
    }
}

fn write_new(dir: &Path, path: &Path, wrap: &Key) -> io::Result<Key> {
    let bytes = crypto::random_bytes::<KEY_LEN>().map_err(io::Error::other)?;
    let sealed = crypto::seal(wrap, &bytes).map_err(io::Error::other)?;
    fs::create_dir_all(dir)?;
    let mut file = open_new(path)?;
    let written = file
        .write_all(sealed.as_bytes())
        .and_then(|()| file.sync_all());
    if let Err(error) = written {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(error);
    }
    Ok(Key::new(bytes))
}

#[cfg(unix)]
fn open_new(path: &Path) -> io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn open_new(path: &Path) -> io::Result<fs::File> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

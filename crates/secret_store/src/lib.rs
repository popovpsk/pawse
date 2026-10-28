mod crypto;
mod embedded;
mod file_key;
#[cfg(any(target_os = "linux", target_os = "windows"))]
mod keychain;
pub mod serde_string;

use std::cell::Cell;
use std::path::Path;
use std::sync::OnceLock;

use crypto::Key;
pub use crypto::{Error, is_sealed};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Keychain,
    File,
    Binary,
}

struct Keys {
    source: Source,
    seal: Key,
    all: Vec<Key>,
}

static KEYS: OnceLock<Keys> = OnceLock::new();

thread_local! {
    static RESEAL: Cell<bool> = const { Cell::new(false) };
}

pub fn init(dir: &Path) -> Source {
    if let Some(keys) = KEYS.get() {
        log::warn!("secrets: keys were in use before init; keeping them");
        return keys.source;
    }
    KEYS.get_or_init(|| load(dir)).source
}

pub fn watch_reseal<T>(work: impl FnOnce() -> T) -> (T, bool) {
    RESEAL.with(|flag| flag.set(false));
    let value = work();
    (value, RESEAL.with(|flag| flag.replace(false)))
}

fn load(dir: &Path) -> Keys {
    let binary = embedded::key();
    let keychain = keychain_key();
    let file = file_key::load(dir, &binary, keychain.is_none());
    let keys = assemble(keychain, file, binary);
    log::info!("secrets: sealing with the {:?} key", keys.source);
    keys
}

fn assemble(keychain: Option<Key>, file: Option<Key>, binary: Key) -> Keys {
    let (source, seal) = match (&keychain, &file) {
        (Some(key), _) => (Source::Keychain, key.clone()),
        (None, Some(key)) => (Source::File, key.clone()),
        (None, None) => (Source::Binary, binary.clone()),
    };
    let all = keychain.into_iter().chain(file).chain([binary]).collect();
    Keys { source, seal, all }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn keychain_key() -> Option<Key> {
    match keychain::read() {
        Ok(Some(bytes)) => {
            let key = Key::from_slice(&bytes);
            if key.is_none() {
                log::warn!("secrets: the keychain item is not a key; left as is");
            }
            key
        }
        Ok(None) => {
            let bytes = crypto::random_bytes::<{ crypto::KEY_LEN }>()
                .inspect_err(|error| log::warn!("secrets: {error}"))
                .ok()?;
            match keychain::write(&bytes) {
                Ok(()) => Some(Key::new(bytes)),
                Err(error) => {
                    log::warn!("secrets: cannot store the key in the keychain: {error}");
                    None
                }
            }
        }
        Err(error) => {
            log::warn!("secrets: keychain unavailable: {error}");
            None
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn keychain_key() -> Option<Key> {
    None
}

fn keys() -> &'static Keys {
    KEYS.get_or_init(|| assemble(None, None, embedded::key()))
}

fn mark_reseal() {
    RESEAL.with(|flag| flag.set(true));
}

fn conceal(plain: &str) -> Result<String, Error> {
    if plain.is_empty() || is_sealed(plain) {
        return Ok(plain.to_string());
    }
    crypto::seal(&keys().seal, plain.as_bytes())
}

fn reveal(text: String) -> String {
    if !is_sealed(&text) {
        if !text.is_empty() {
            mark_reseal();
        }
        return text;
    }
    let keys = keys();
    let opened = crypto::open(&keys.all, &text)
        .and_then(|(plain, key)| Some((String::from_utf8(plain).ok()?, key)));
    match opened {
        Some((plain, key)) => {
            if key.id() != keys.seal.id() {
                mark_reseal();
            }
            plain
        }
        None => {
            log::warn!("secrets: a stored secret does not open with the keys at hand; kept as is");
            text
        }
    }
}

#[cfg(test)]
mod tests;

use base64::Engine;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use chacha20poly1305::aead::Aead;
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce};
use sha2::{Digest, Sha256};

const PREFIX: &str = "pawse:v1:";
const NONCE_LEN: usize = 12;
pub(crate) const KEY_LEN: usize = 32;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no randomness: {0}")]
    Random(getrandom::Error),
    #[error("encryption failed")]
    Cipher,
}

#[derive(Clone)]
pub(crate) struct Key {
    id: String,
    cipher: ChaCha20Poly1305,
}

impl Key {
    pub(crate) fn new(bytes: [u8; KEY_LEN]) -> Self {
        let digest = Sha256::digest(bytes);
        Self {
            id: digest[..4]
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
            cipher: ChaCha20Poly1305::new(&bytes.into()),
        }
    }

    pub(crate) fn from_slice(bytes: &[u8]) -> Option<Self> {
        bytes.try_into().ok().map(Self::new)
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }
}

pub(crate) fn random_bytes<const N: usize>() -> Result<[u8; N], Error> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).map_err(Error::Random)?;
    Ok(bytes)
}

pub fn is_sealed(text: &str) -> bool {
    text.starts_with(PREFIX)
}

pub(crate) fn seal(key: &Key, plain: &[u8]) -> Result<String, Error> {
    let nonce = random_bytes::<NONCE_LEN>()?;
    let sealed = key
        .cipher
        .encrypt(Nonce::from_slice(&nonce), plain)
        .map_err(|_| Error::Cipher)?;
    let mut blob = Vec::with_capacity(NONCE_LEN + sealed.len());
    blob.extend_from_slice(&nonce);
    blob.extend_from_slice(&sealed);
    Ok(format!(
        "{PREFIX}{}:{}",
        key.id,
        STANDARD_NO_PAD.encode(blob)
    ))
}

pub(crate) fn open<'k>(keys: &'k [Key], text: &str) -> Option<(Vec<u8>, &'k Key)> {
    let (id, body) = text.strip_prefix(PREFIX)?.split_once(':')?;
    let blob = STANDARD_NO_PAD.decode(body).ok()?;
    if blob.len() < NONCE_LEN {
        return None;
    }
    let (nonce, sealed) = blob.split_at(NONCE_LEN);
    keys.iter().filter(|key| key.id == id).find_map(|key| {
        let plain = key.cipher.decrypt(Nonce::from_slice(nonce), sealed).ok()?;
        Some((plain, key))
    })
}

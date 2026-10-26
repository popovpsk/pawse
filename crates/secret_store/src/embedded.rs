use sha2::{Digest, Sha256};

use crate::crypto::Key;

const PUBLIC_SEED: &str = "pawse-secret-store-public-seed";

fn seed() -> Option<&'static str> {
    option_env!("PAWSE_SECRETS_KEY").filter(|seed| !seed.is_empty())
}

pub(crate) fn is_public() -> bool {
    seed().is_none()
}

pub(crate) fn key() -> Key {
    let digest = Sha256::new()
        .chain_update(b"pawse-secrets-v1")
        .chain_update(seed().unwrap_or(PUBLIC_SEED).as_bytes())
        .finalize();
    Key::new(digest.into())
}

use keyring_core::api::CredentialStoreApi;
use keyring_core::{Entry, Error};

use super::{ACCOUNT, SERVICE};

fn entry() -> Result<Entry, String> {
    windows_native_keyring_store::Store::new()
        .and_then(|store| store.build(SERVICE, ACCOUNT, None))
        .map_err(|error| error.to_string())
}

pub(crate) fn read() -> Result<Option<Vec<u8>>, String> {
    match entry()?.get_secret() {
        Ok(secret) => Ok(Some(secret)),
        Err(Error::NoEntry) => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

pub(crate) fn write(secret: &[u8]) -> Result<(), String> {
    entry()?
        .set_secret(secret)
        .map_err(|error| error.to_string())
}

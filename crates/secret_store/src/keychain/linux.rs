use futures_lite::future::block_on;
use oo7::Keyring;

use super::{ACCOUNT, SERVICE};

const LABEL: &str = "Pawse";

fn attributes() -> [(&'static str, &'static str); 2] {
    [("application", SERVICE), ("kind", ACCOUNT)]
}

async fn keyring() -> Result<Keyring, String> {
    let keyring = Keyring::new().await.map_err(|error| error.to_string())?;
    keyring.unlock().await.map_err(|error| error.to_string())?;
    Ok(keyring)
}

pub(crate) fn read() -> Result<Option<Vec<u8>>, String> {
    block_on(async {
        let keyring = keyring().await?;
        let items = keyring
            .search_items(&attributes())
            .await
            .map_err(|error| error.to_string())?;
        let Some(item) = items.first() else {
            return Ok(None);
        };
        item.unlock().await.map_err(|error| error.to_string())?;
        let secret = item.secret().await.map_err(|error| error.to_string())?;
        Ok(Some(secret.to_vec()))
    })
}

pub(crate) fn write(secret: &[u8]) -> Result<(), String> {
    block_on(async {
        keyring()
            .await?
            .create_item(LABEL, &attributes(), secret, true)
            .await
            .map_err(|error| error.to_string())
    })
}

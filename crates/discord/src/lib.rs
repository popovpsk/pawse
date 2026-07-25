mod art;
mod ipc;

use std::sync::OnceLock;
use std::time::Duration;

pub use ipc::DiscordHandle;

fn resolved_client_id() -> Option<&'static str> {
    static ID: OnceLock<Option<String>> = OnceLock::new();
    ID.get_or_init(|| {
        std::env::var("DISCORD_CLIENT_ID")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| option_env!("DISCORD_CLIENT_ID").map(str::to_owned))
    })
    .as_deref()
}

pub fn client_id() -> Option<String> {
    resolved_client_id().map(str::to_owned)
}

pub fn is_available() -> bool {
    resolved_client_id().is_some()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Presence {
    pub artist: String,
    pub title: String,
    pub album: Option<String>,
    pub duration: Duration,
    pub started_at: i64,
}

use std::net::SocketAddr;

use tokio::sync::watch;

#[cfg(not(debug_assertions))]
mod assets;
mod http;

pub const DEFAULT_PORT: u16 = 8770;
pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Clone, serde::Serialize)]
pub struct PlayerState {
    pub v: u32,
    pub title: Option<String>,
    pub playing: bool,
}

impl PlayerState {
    pub fn snapshot(title: Option<String>, playing: bool) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            title,
            playing,
        }
    }
}

impl Default for PlayerState {
    fn default() -> Self {
        Self::snapshot(None, false)
    }
}

pub type StateRx = watch::Receiver<PlayerState>;

#[derive(Clone)]
pub struct StateHandle(watch::Sender<PlayerState>);

impl StateHandle {
    pub fn publish(&self, state: PlayerState) {
        let _ = self.0.send(state);
    }
}

pub fn channel() -> (StateHandle, StateRx) {
    let (tx, rx) = watch::channel(PlayerState::default());
    (StateHandle(tx), rx)
}

pub fn spawn(addr: SocketAddr, rx: StateRx) {
    let builder = std::thread::Builder::new().name("pawse-remote".into());
    if let Err(err) = builder.spawn(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(err) => {
                log::error!("pawse-remote: failed to build runtime: {err}");
                return;
            }
        };
        runtime.block_on(async move {
            if let Err(err) = http::serve(addr, rx).await {
                log::error!("pawse-remote: server error: {err}");
            }
        });
    }) {
        log::warn!("pawse-remote: failed to spawn server thread: {err}");
    }
}

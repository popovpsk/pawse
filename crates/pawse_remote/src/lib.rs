use std::net::SocketAddr;

use tokio::sync::{oneshot, watch};

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

pub struct RemoteServer {
    shutdown: Option<oneshot::Sender<()>>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl Drop for RemoteServer {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

pub type ReadyRx = oneshot::Receiver<Result<(), String>>;

#[must_use]
pub fn spawn(addr: SocketAddr, rx: StateRx) -> (RemoteServer, ReadyRx) {
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let (ready_tx, ready_rx) = oneshot::channel();
    let builder = std::thread::Builder::new().name("pawse-remote".into());
    let join = builder.spawn(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(err) => {
                log::error!("pawse-remote: failed to build runtime: {err}");
                let _ = ready_tx.send(Err(err.to_string()));
                return;
            }
        };
        runtime.block_on(async move {
            let listener = match tokio::net::TcpListener::bind(addr).await {
                Ok(listener) => {
                    let _ = ready_tx.send(Ok(()));
                    listener
                }
                Err(err) => {
                    log::error!("pawse-remote: failed to bind {addr}: {err}");
                    let _ = ready_tx.send(Err(err.to_string()));
                    return;
                }
            };
            tokio::select! {
                result = http::serve(listener, rx) => {
                    if let Err(err) = result {
                        log::error!("pawse-remote: server error: {err}");
                    }
                }
                _ = shutdown_rx => {
                    log::info!("pawse-remote: stopped on {addr}");
                }
            }
        });
    });
    let join = match join {
        Ok(handle) => Some(handle),
        Err(err) => {
            log::warn!("pawse-remote: failed to spawn server thread: {err}");
            None
        }
    };
    (
        RemoteServer {
            shutdown: Some(shutdown_tx),
            join,
        },
        ready_rx,
    )
}

use std::net::SocketAddr;
use std::sync::Arc;

use tokio::sync::{oneshot, watch};

#[cfg(not(debug_assertions))]
mod assets;
mod http;

pub const DEFAULT_PORT: u16 = 8770;
pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Clone, Default, serde::Serialize)]
pub struct PlayerState {
    pub v: u32,
    pub has_track: bool,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub playing: bool,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub cover_id: Option<i64>,
    #[serde(skip)]
    pub cover: Option<Arc<Vec<u8>>>,
}

impl PlayerState {
    pub fn idle() -> Self {
        Self {
            v: PROTOCOL_VERSION,
            ..Default::default()
        }
    }
}

pub type StateRx = watch::Receiver<PlayerState>;

#[derive(Clone)]
pub struct StateHandle(watch::Sender<PlayerState>);

impl StateHandle {
    pub fn publish(&self, mut state: PlayerState) {
        state.v = PROTOCOL_VERSION;
        let _ = self.0.send(state);
    }

    pub fn publish_position(&self, position_ms: u64, playing: bool) {
        self.0.send_if_modified(|state| {
            let changed = state.position_ms != position_ms || state.playing != playing;
            state.position_ms = position_ms;
            state.playing = playing;
            changed
        });
    }

    pub fn current_cover_id(&self) -> Option<i64> {
        self.0.borrow().cover_id
    }

    pub fn current_cover(&self) -> Option<Arc<Vec<u8>>> {
        self.0.borrow().cover.clone()
    }

    pub fn has_listeners(&self) -> bool {
        self.0.receiver_count() > 1
    }
}

pub fn channel() -> (StateHandle, StateRx) {
    let (tx, rx) = watch::channel(PlayerState::idle());
    (StateHandle(tx), rx)
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    PlayPause,
    Next,
    Prev,
    Seek { position_ms: u64 },
}

pub type CommandRx = flume::Receiver<Command>;

#[derive(Clone)]
pub struct CommandSink(flume::Sender<Command>);

impl CommandSink {
    pub fn send(&self, command: Command) -> bool {
        self.0.send(command).is_ok()
    }
}

pub fn commands() -> (CommandSink, CommandRx) {
    let (tx, rx) = flume::unbounded();
    (CommandSink(tx), rx)
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
pub fn spawn(addr: SocketAddr, rx: StateRx, commands: CommandSink) -> (RemoteServer, ReadyRx) {
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
                result = http::serve(listener, rx, commands) => {
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

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{oneshot, watch};

#[cfg(not(debug_assertions))]
mod assets;
mod http;

pub const DEFAULT_PORT: u16 = 8770;
pub const PROTOCOL_VERSION: u32 = 3;
const STOP_WAIT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoverSize {
    Small,
    Large,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
    StopAfterTrack,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlbumSort {
    #[default]
    Artist,
    Title,
    Year,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtistSort {
    #[default]
    Name,
    Tracks,
}

#[derive(Clone, serde::Serialize)]
pub struct ArtistEntry {
    pub id: i64,
    pub name: String,
    pub track_count: i64,
    pub cover_ids: Vec<i64>,
}

#[derive(Clone, serde::Serialize)]
pub struct AlbumTrack {
    pub id: i64,
    pub title: String,
    pub track_number: Option<i32>,
    pub disc_number: i32,
    pub duration_ms: u64,
}

#[derive(Clone, serde::Serialize)]
pub struct ArtistAlbum {
    pub album_id: Option<i64>,
    pub title: String,
    pub year: Option<i32>,
    pub cover_id: Option<i64>,
    pub partial: bool,
    pub tracks: Vec<AlbumTrack>,
}

#[derive(Clone, serde::Serialize)]
pub struct ArtistDetail {
    pub id: i64,
    pub name: String,
    pub has_partial: bool,
    pub albums: Vec<ArtistAlbum>,
}

#[derive(Clone, serde::Serialize)]
pub struct AlbumEntry {
    pub id: i64,
    pub title: String,
    pub artist: String,
    pub year: Option<i32>,
    pub cover_id: Option<i64>,
}

#[derive(Clone, serde::Serialize)]
pub struct AlbumDetail {
    pub id: i64,
    pub title: String,
    pub artist: String,
    pub year: Option<i32>,
    pub cover_id: Option<i64>,
    pub tracks: Vec<AlbumTrack>,
}

#[derive(Clone, serde::Serialize)]
pub struct PlaylistEntry {
    pub id: i64,
    pub name: String,
    pub track_count: i64,
}

#[derive(Clone, serde::Serialize)]
pub struct PlaylistTrack {
    pub id: i64,
    pub title: String,
    pub artist: Option<String>,
    pub cover_id: Option<i64>,
    pub duration_ms: u64,
}

#[derive(Clone, serde::Serialize)]
pub struct PlaylistDetail {
    pub id: i64,
    pub name: String,
    pub tracks: Vec<PlaylistTrack>,
}

pub trait LibraryReader: Send + Sync + 'static {
    fn cover(&self, id: i64, size: CoverSize) -> Option<Vec<u8>>;
    fn cover_original(&self, id: i64) -> Option<(Vec<u8>, String)>;
    fn artists(&self, sort: ArtistSort, desc: bool) -> Vec<ArtistEntry>;
    fn artist_detail(&self, artist_id: i64, full: bool) -> Option<ArtistDetail>;
    fn albums(&self, sort: AlbumSort, desc: bool) -> Vec<AlbumEntry>;
    fn album_detail(&self, album_id: i64) -> Option<AlbumDetail>;
    fn playlists(&self) -> Vec<PlaylistEntry>;
    fn playlist_detail(&self, playlist_id: i64) -> Option<PlaylistDetail>;
    fn liked(&self) -> Vec<PlaylistTrack>;
}

#[derive(Clone, serde::Serialize)]
pub struct QueueItem {
    pub id: i64,
    pub title: String,
    pub artist: Option<String>,
    pub cover_id: Option<i64>,
}

#[derive(Clone, Default, serde::Serialize)]
pub struct PlayerState {
    pub v: u32,
    pub has_track: bool,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub playing: bool,
    pub buffering: bool,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub cover_id: Option<i64>,
    pub queue_index: Option<usize>,
    pub queue_rev: u64,
    pub library_rev: u64,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub volume: f32,
    pub volume_locked: bool,
    #[serde(skip)]
    pub queue: Arc<Vec<QueueItem>>,
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

    pub fn is_serving(&self) -> bool {
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
    Seek {
        position_ms: u64,
    },
    PlayAt {
        index: usize,
    },
    RemoveAt {
        index: usize,
    },
    Refresh,
    SetShuffle {
        on: bool,
    },
    SetRepeat {
        mode: RepeatMode,
    },
    SetVolume {
        volume: f32,
    },
    PlayArtistTrack {
        artist_id: i64,
        track_id: i64,
        full: bool,
    },
    QueueArtistTrack {
        artist_id: i64,
        track_id: i64,
        full: bool,
    },
    QueueArtistAlbum {
        artist_id: i64,
        album_id: Option<i64>,
        full: bool,
    },
    PlayAlbumTrack {
        album_id: i64,
        track_id: i64,
    },
    QueueAlbumTrack {
        album_id: i64,
        track_id: i64,
    },
    QueueAlbum {
        album_id: i64,
    },
    PlayPlaylistTrack {
        playlist_id: i64,
        track_id: i64,
    },
    QueuePlaylistTrack {
        playlist_id: i64,
        track_id: i64,
    },
    QueuePlaylist {
        playlist_id: i64,
    },
    PlayLikedTrack {
        track_id: i64,
    },
    QueueLikedTrack {
        track_id: i64,
    },
    QueueLiked,
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
    stop: Option<watch::Sender<()>>,
    stopped: flume::Receiver<()>,
}

impl Drop for RemoteServer {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Err(flume::RecvTimeoutError::Timeout) = self.stopped.recv_timeout(STOP_WAIT) {
            log::warn!("pawse-remote: the server did not stop within {STOP_WAIT:?}");
        }
    }
}

pub type ReadyRx = oneshot::Receiver<Result<(), String>>;

#[must_use]
pub fn spawn(
    addr: SocketAddr,
    rx: StateRx,
    commands: CommandSink,
    library: Arc<dyn LibraryReader>,
) -> (RemoteServer, ReadyRx) {
    let (stop, stopping) = watch::channel(());
    let (ready_tx, ready_rx) = oneshot::channel();
    let (finished, stopped) = flume::bounded::<()>(0);
    let server = RemoteServer {
        stop: Some(stop),
        stopped,
    };
    let runtime = match net_runtime::try_handle() {
        Ok(runtime) => runtime,
        Err(err) => {
            log::error!("pawse-remote: no runtime to serve on: {err}");
            let _ = ready_tx.send(Err(err));
            return (server, ready_rx);
        }
    };
    runtime.spawn(async move {
        let _finished = finished;
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
        let mut stop = stopping.clone();
        tokio::select! {
            result = http::serve(listener, rx, commands, library, stopping) => {
                if let Err(err) = result {
                    log::error!("pawse-remote: server error: {err}");
                }
            }
            _ = stop.changed() => {
                log::info!("pawse-remote: stopped on {addr}");
            }
        }
    });
    (server, ready_rx)
}

#[cfg(test)]
mod tests;

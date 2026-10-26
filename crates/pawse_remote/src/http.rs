use axum::{
    Json, Router,
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::sync::watch;

use crate::{
    AlbumSort, ArtistSort, Command, CommandSink, CoverSize, LibraryReader, PlayerState, StateRx,
};

#[derive(Clone)]
struct AppState {
    rx: StateRx,
    commands: CommandSink,
    library: Arc<dyn LibraryReader>,
    stop: watch::Receiver<()>,
}

pub async fn serve(
    listener: TcpListener,
    rx: StateRx,
    commands: CommandSink,
    library: Arc<dyn LibraryReader>,
    stop: watch::Receiver<()>,
) -> anyhow::Result<()> {
    let router = build_router(AppState {
        rx,
        commands,
        library,
        stop,
    });
    log::info!(
        "pawse-remote: listening on http://{}",
        listener.local_addr()?
    );
    axum::serve(listener, router).await?;
    Ok(())
}

fn build_router(state: AppState) -> Router {
    let router = Router::new()
        .route("/api/state", get(state_handler))
        .route("/api/queue", get(queue_handler))
        .route("/api/cover", get(cover_handler))
        .route("/api/artists", get(artists_handler))
        .route("/api/artist", get(artist_handler))
        .route("/api/albums", get(albums_handler))
        .route("/api/album", get(album_handler))
        .route("/api/playlists", get(playlists_handler))
        .route("/api/playlist", get(playlist_handler))
        .route("/api/liked", get(liked_handler))
        .route("/api/lyrics", get(lyrics_handler))
        .route("/api/command", post(command_handler))
        .route("/ws", get(ws_handler));

    #[cfg(not(debug_assertions))]
    let router = router.fallback(crate::assets::static_handler);

    router.with_state(state)
}

async fn state_handler(State(state): State<AppState>) -> impl IntoResponse {
    Json(state.rx.borrow().clone())
}

async fn queue_handler(State(state): State<AppState>) -> impl IntoResponse {
    let queue = state.rx.borrow().queue.clone();
    Json(queue.as_ref().clone())
}

#[derive(serde::Deserialize)]
struct CoverQuery {
    id: i64,
    size: Option<String>,
}

async fn read(
    state: &AppState,
    respond: impl FnOnce(&dyn LibraryReader) -> Response + Send + 'static,
) -> Response {
    let library = state.library.clone();
    tokio::task::spawn_blocking(move || respond(&*library))
        .await
        .unwrap_or_else(|err| {
            log::error!("pawse-remote: a library read failed: {err}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        })
}

async fn cover_handler(State(state): State<AppState>, Query(query): Query<CoverQuery>) -> Response {
    if query.size.as_deref() == Some("full") {
        return read(&state, move |library| {
            match library.cover_original(query.id) {
                Some((bytes, content_type)) => (
                    [
                        (header::CONTENT_TYPE, content_type),
                        (header::CACHE_CONTROL, "no-store".to_string()),
                    ],
                    bytes,
                )
                    .into_response(),
                None => StatusCode::NOT_FOUND.into_response(),
            }
        })
        .await;
    }
    let size = match query.size.as_deref() {
        Some("small") => CoverSize::Small,
        _ => CoverSize::Large,
    };
    read(&state, move |library| match library.cover(query.id, size) {
        Some(bytes) => (
            [
                (header::CONTENT_TYPE, "image/jpeg"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            bytes,
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    })
    .await
}

#[derive(serde::Deserialize)]
struct ArtistsQuery {
    #[serde(default)]
    sort: ArtistSort,
    desc: Option<u8>,
}

async fn artists_handler(
    State(state): State<AppState>,
    Query(query): Query<ArtistsQuery>,
) -> Response {
    let desc = query.desc.unwrap_or(0) != 0;
    read(&state, move |library| {
        Json(library.artists(query.sort, desc)).into_response()
    })
    .await
}

#[derive(serde::Deserialize)]
struct ArtistQuery {
    id: i64,
    full: Option<u8>,
}

async fn artist_handler(
    State(state): State<AppState>,
    Query(query): Query<ArtistQuery>,
) -> Response {
    let full = query.full.unwrap_or(0) != 0;
    read(&state, move |library| {
        match library.artist_detail(query.id, full) {
            Some(detail) => Json(detail).into_response(),
            None => StatusCode::NOT_FOUND.into_response(),
        }
    })
    .await
}

#[derive(serde::Deserialize)]
struct AlbumsQuery {
    #[serde(default)]
    sort: AlbumSort,
    desc: Option<u8>,
}

async fn albums_handler(
    State(state): State<AppState>,
    Query(query): Query<AlbumsQuery>,
) -> Response {
    let desc = query.desc.unwrap_or(0) != 0;
    read(&state, move |library| {
        Json(library.albums(query.sort, desc)).into_response()
    })
    .await
}

#[derive(serde::Deserialize)]
struct AlbumQuery {
    id: i64,
}

async fn album_handler(State(state): State<AppState>, Query(query): Query<AlbumQuery>) -> Response {
    read(&state, move |library| {
        match library.album_detail(query.id) {
            Some(detail) => Json(detail).into_response(),
            None => StatusCode::NOT_FOUND.into_response(),
        }
    })
    .await
}

async fn playlists_handler(State(state): State<AppState>) -> Response {
    read(&state, |library| Json(library.playlists()).into_response()).await
}

#[derive(serde::Deserialize)]
struct PlaylistQuery {
    id: i64,
}

async fn playlist_handler(
    State(state): State<AppState>,
    Query(query): Query<PlaylistQuery>,
) -> Response {
    read(&state, move |library| {
        match library.playlist_detail(query.id) {
            Some(detail) => Json(detail).into_response(),
            None => StatusCode::NOT_FOUND.into_response(),
        }
    })
    .await
}

async fn liked_handler(State(state): State<AppState>) -> Response {
    read(&state, |library| Json(library.liked()).into_response()).await
}

#[derive(serde::Deserialize)]
struct LyricsQuery {
    id: i64,
}

async fn lyrics_handler(
    State(state): State<AppState>,
    Query(query): Query<LyricsQuery>,
) -> Response {
    read(&state, move |library| {
        (
            [(header::CACHE_CONTROL, "no-store")],
            Json(library.lyrics(query.id)),
        )
            .into_response()
    })
    .await
}

async fn command_handler(
    State(state): State<AppState>,
    Json(command): Json<Command>,
) -> StatusCode {
    if state.commands.send(command) {
        StatusCode::ACCEPTED
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

async fn ws_handler(State(state): State<AppState>, upgrade: WebSocketUpgrade) -> impl IntoResponse {
    upgrade.on_upgrade(move |socket| stream_state(socket, state.rx, state.commands, state.stop))
}

async fn stream_state(
    mut socket: WebSocket,
    mut rx: StateRx,
    commands: CommandSink,
    mut stop: watch::Receiver<()>,
) {
    commands.send(Command::Refresh);
    let snapshot = rx.borrow_and_update().clone();
    if send_state(&mut socket, &snapshot).await.is_err() {
        return;
    }
    loop {
        tokio::select! {
            changed = rx.changed() => {
                if changed.is_err() {
                    break;
                }
            }
            _ = stop.changed() => break,
        }
        let next = rx.borrow_and_update().clone();
        if send_state(&mut socket, &next).await.is_err() {
            break;
        }
    }
}

async fn send_state(socket: &mut WebSocket, state: &PlayerState) -> Result<(), axum::Error> {
    let payload = match serde_json::to_string(state) {
        Ok(payload) => payload,
        Err(err) => {
            log::error!("pawse-remote: failed to serialize state: {err}");
            return Ok(());
        }
    };
    socket.send(Message::Text(payload.into())).await
}

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

use crate::{Command, CommandSink, CoverSize, LibraryReader, PlayerState, StateRx};

#[derive(Clone)]
struct AppState {
    rx: StateRx,
    commands: CommandSink,
    library: Arc<dyn LibraryReader>,
}

pub async fn serve(
    listener: TcpListener,
    rx: StateRx,
    commands: CommandSink,
    library: Arc<dyn LibraryReader>,
) -> anyhow::Result<()> {
    let router = build_router(AppState {
        rx,
        commands,
        library,
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
        .route("/api/playlists", get(playlists_handler))
        .route("/api/playlist", get(playlist_handler))
        .route("/api/liked", get(liked_handler))
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

async fn cover_handler(State(state): State<AppState>, Query(query): Query<CoverQuery>) -> Response {
    if query.size.as_deref() == Some("full") {
        return match state.library.cover_original(query.id) {
            Some((bytes, content_type)) => (
                [
                    (header::CONTENT_TYPE, content_type),
                    (header::CACHE_CONTROL, "no-store".to_string()),
                ],
                bytes,
            )
                .into_response(),
            None => StatusCode::NOT_FOUND.into_response(),
        };
    }
    let size = match query.size.as_deref() {
        Some("small") => CoverSize::Small,
        _ => CoverSize::Large,
    };
    match state.library.cover(query.id, size) {
        Some(bytes) => (
            [
                (header::CONTENT_TYPE, "image/jpeg"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            bytes,
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn artists_handler(State(state): State<AppState>) -> impl IntoResponse {
    Json(state.library.artists())
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
    match state
        .library
        .artist_detail(query.id, query.full.unwrap_or(0) != 0)
    {
        Some(detail) => Json(detail).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn playlists_handler(State(state): State<AppState>) -> impl IntoResponse {
    Json(state.library.playlists())
}

#[derive(serde::Deserialize)]
struct PlaylistQuery {
    id: i64,
}

async fn playlist_handler(
    State(state): State<AppState>,
    Query(query): Query<PlaylistQuery>,
) -> Response {
    match state.library.playlist_detail(query.id) {
        Some(detail) => Json(detail).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn liked_handler(State(state): State<AppState>) -> impl IntoResponse {
    Json(state.library.liked())
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
    upgrade.on_upgrade(move |socket| stream_state(socket, state.rx, state.commands))
}

async fn stream_state(mut socket: WebSocket, mut rx: StateRx, commands: CommandSink) {
    commands.send(Command::Refresh);
    let snapshot = rx.borrow_and_update().clone();
    if send_state(&mut socket, &snapshot).await.is_err() {
        return;
    }
    while rx.changed().await.is_ok() {
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

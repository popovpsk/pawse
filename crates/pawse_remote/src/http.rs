use axum::{
    Json, Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use tokio::net::TcpListener;

use crate::{Command, CommandSink, PlayerState, StateRx};

#[derive(Clone)]
struct AppState {
    rx: StateRx,
    commands: CommandSink,
}

pub async fn serve(
    listener: TcpListener,
    rx: StateRx,
    commands: CommandSink,
) -> anyhow::Result<()> {
    let router = build_router(AppState { rx, commands });
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
        .route("/api/cover", get(cover_handler))
        .route("/api/command", post(command_handler))
        .route("/ws", get(ws_handler));

    #[cfg(not(debug_assertions))]
    let router = router.fallback(crate::assets::static_handler);

    router.with_state(state)
}

async fn state_handler(State(state): State<AppState>) -> impl IntoResponse {
    Json(state.rx.borrow().clone())
}

async fn cover_handler(State(state): State<AppState>) -> Response {
    let cover = state.rx.borrow().cover.clone();
    match cover {
        Some(bytes) => (
            [
                (header::CONTENT_TYPE, "image/jpeg"),
                (header::CACHE_CONTROL, "no-cache"),
            ],
            bytes.as_ref().clone(),
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
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
    upgrade.on_upgrade(move |socket| stream_state(socket, state.rx))
}

async fn stream_state(mut socket: WebSocket, mut rx: StateRx) {
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

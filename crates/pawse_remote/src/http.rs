use std::net::SocketAddr;

use axum::{
    Json, Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
    routing::get,
};
use tokio::net::TcpListener;

use crate::{PlayerState, StateRx};

#[derive(Clone)]
struct AppState {
    rx: StateRx,
}

pub async fn serve(addr: SocketAddr, rx: StateRx) -> anyhow::Result<()> {
    let router = build_router(AppState { rx });
    let listener = TcpListener::bind(addr).await?;
    log::info!("pawse-remote: listening on http://{addr}");
    axum::serve(listener, router).await?;
    Ok(())
}

fn build_router(state: AppState) -> Router {
    let router = Router::new()
        .route("/api/state", get(state_handler))
        .route("/ws", get(ws_handler));

    #[cfg(not(debug_assertions))]
    let router = router.fallback(crate::assets::static_handler);

    router.with_state(state)
}

async fn state_handler(State(state): State<AppState>) -> impl IntoResponse {
    Json(state.rx.borrow().clone())
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

use std::borrow::Cow;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use flume::{Receiver, Sender};
use zbus::Connection;
use zbus::object_server::{Interface, SignalEmitter};

use super::interfaces::{self, Player, Root, Shared, lock};
use super::state::{Changes, State};
use crate::{Driver, MediaCommand, MediaPlaybackState, NowPlayingInfo, SystemMediaIntegration};

const BUS_NAME: &str = "org.mpris.MediaPlayer2.pawse";
const OBJECT_PATH: &str = "/org/mpris/MediaPlayer2";

pub(crate) struct MprisIntegration {
    state: Shared,
    changes: Sender<Changes>,
}

impl MprisIntegration {
    pub(crate) fn new(commands: Sender<MediaCommand>) -> (Self, Driver) {
        let state = Arc::new(Mutex::new(State::new(Instant::now())));
        let (changes, pending) = flume::unbounded();
        let driver = Box::pin(drive(state.clone(), commands, pending));
        (Self { state, changes }, driver)
    }

    fn apply(&self, update: impl FnOnce(&mut State, Instant) -> Changes) {
        let changes = update(&mut lock(&self.state), Instant::now());
        if !changes.is_empty() {
            let _ = self.changes.send(changes);
        }
    }
}

impl SystemMediaIntegration for MprisIntegration {
    fn update_now_playing(&self, info: NowPlayingInfo, state: MediaPlaybackState) {
        self.apply(|current, now| current.set_track(&info, state, now));
    }

    fn set_playback_state(&self, state: MediaPlaybackState) {
        self.apply(|current, now| current.set_status(state, now));
    }

    fn update_position(&self, elapsed_secs: f64, state: MediaPlaybackState) {
        self.apply(|current, now| current.set_position(elapsed_secs, state, now));
    }
}

async fn drive(state: Shared, commands: Sender<MediaCommand>, pending: Receiver<Changes>) {
    let connection = match connect(state.clone(), commands).await {
        Ok(connection) => connection,
        Err(err) => {
            log::warn!("media_integration: MPRIS unavailable: {err}");
            return;
        }
    };
    let executor = connection.executor().clone();
    let ticking = async move {
        loop {
            executor.tick().await;
        }
    };
    futures_lite::future::or(emit_changes(&connection, &state, pending), ticking).await;
}

async fn connect(state: Shared, commands: Sender<MediaCommand>) -> zbus::Result<Connection> {
    zbus::connection::Builder::session()?
        .name(BUS_NAME)?
        .allow_name_replacements(false)
        .replace_existing_names(false)
        .serve_at(OBJECT_PATH, Root)?
        .serve_at(OBJECT_PATH, Player::new(state, commands))?
        .internal_executor(false)
        .build()
        .await
}

async fn emit_changes(connection: &Connection, state: &Shared, pending: Receiver<Changes>) {
    let emitter = match SignalEmitter::new(connection, OBJECT_PATH) {
        Ok(emitter) => emitter,
        Err(err) => {
            log::warn!("media_integration: MPRIS signals unavailable: {err}");
            return;
        }
    };
    while let Ok(mut changes) = pending.recv_async().await {
        for more in pending.drain() {
            changes.merge(more);
        }
        let (properties, seeked) = {
            let state = lock(state);
            let seeked = changes
                .seeked
                .then(|| state.announced_position_us(Instant::now()));
            (interfaces::changed_properties(&state, changes), seeked)
        };
        if !properties.is_empty() {
            let sent = zbus::fdo::Properties::properties_changed(
                &emitter,
                Player::name(),
                properties,
                Cow::Borrowed(&[]),
            )
            .await;
            if let Err(err) = sent {
                log::debug!("media_integration: PropertiesChanged failed: {err}");
            }
        }
        if let Some(position) = seeked
            && let Err(err) = Player::seeked(&emitter, position).await
        {
            log::debug!("media_integration: Seeked failed: {err}");
        }
    }
}

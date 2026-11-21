use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use flume::Sender;
use zbus::interface;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{ObjectPath, Value};

use super::state::{Changes, State};
use crate::{MediaCommand, MediaPlaybackState};

pub(super) type Shared = Arc<Mutex<State>>;

pub(super) fn lock(state: &Shared) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(super) struct Root;

#[interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn raise(&self) {}

    fn quit(&self) {}

    #[zbus(property(emits_changed_signal = "const"))]
    fn can_quit(&self) -> bool {
        false
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn can_raise(&self) -> bool {
        false
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn has_track_list(&self) -> bool {
        false
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn identity(&self) -> String {
        "Pawse".to_owned()
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn desktop_entry(&self) -> String {
        std::env::var("FLATPAK_ID").unwrap_or_else(|_| "pawse".to_owned())
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn supported_uri_schemes(&self) -> Vec<String> {
        Vec::new()
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn supported_mime_types(&self) -> Vec<String> {
        Vec::new()
    }
}

pub(super) struct Player {
    state: Shared,
    commands: Sender<MediaCommand>,
}

impl Player {
    pub(super) fn new(state: Shared, commands: Sender<MediaCommand>) -> Self {
        Self { state, commands }
    }

    fn send(&self, command: MediaCommand) {
        let _ = self.commands.send(command);
    }
}

#[interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    fn next(&self) {
        self.send(MediaCommand::Next);
    }

    fn previous(&self) {
        self.send(MediaCommand::Previous);
    }

    fn pause(&self) {
        self.send(MediaCommand::Pause);
    }

    fn play_pause(&self) {
        self.send(MediaCommand::TogglePlayPause);
    }

    fn stop(&self) {
        self.send(MediaCommand::Pause);
    }

    fn play(&self) {
        self.send(MediaCommand::Play);
    }

    fn seek(&self, offset: i64) {
        let command = lock(&self.state).seek_by(offset, Instant::now());
        if let Some(command) = command {
            self.send(command);
        }
    }

    fn set_position(&self, track_id: ObjectPath<'_>, position: i64) {
        let command = lock(&self.state).seek_to(track_id.as_str(), position);
        if let Some(command) = command {
            self.send(command);
        }
    }

    fn open_uri(&self, _uri: String) -> zbus::fdo::Result<()> {
        Err(zbus::fdo::Error::NotSupported(
            "Pawse does not open URIs".to_owned(),
        ))
    }

    #[zbus(signal)]
    pub(super) async fn seeked(emitter: &SignalEmitter<'_>, position: i64) -> zbus::Result<()>;

    #[zbus(property)]
    fn playback_status(&self) -> String {
        status_name(lock(&self.state).status()).to_owned()
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn rate(&self) -> f64 {
        1.0
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn minimum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn maximum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn metadata(&self) -> HashMap<&'static str, Value<'static>> {
        metadata(&lock(&self.state))
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn volume(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn set_volume(&mut self, _volume: f64) {}

    #[zbus(property(emits_changed_signal = "false"))]
    fn position(&self) -> i64 {
        lock(&self.state).position_us(Instant::now())
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn can_go_next(&self) -> bool {
        true
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn can_go_previous(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_play(&self) -> bool {
        lock(&self.state).has_track()
    }

    #[zbus(property)]
    fn can_pause(&self) -> bool {
        lock(&self.state).has_track()
    }

    #[zbus(property)]
    fn can_seek(&self) -> bool {
        lock(&self.state).can_seek()
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn can_control(&self) -> bool {
        true
    }
}

pub(super) fn changed_properties(
    state: &State,
    changes: Changes,
) -> HashMap<&'static str, Value<'static>> {
    let mut properties = HashMap::new();
    if changes.playback_status {
        properties.insert("PlaybackStatus", Value::from(status_name(state.status())));
    }
    if changes.metadata {
        properties.insert("Metadata", Value::from(metadata(state)));
    }
    if changes.can_play_pause {
        properties.insert("CanPlay", Value::from(state.has_track()));
        properties.insert("CanPause", Value::from(state.has_track()));
    }
    if changes.can_seek {
        properties.insert("CanSeek", Value::from(state.can_seek()));
    }
    properties
}

fn status_name(status: MediaPlaybackState) -> &'static str {
    match status {
        MediaPlaybackState::Playing => "Playing",
        MediaPlaybackState::Paused => "Paused",
        MediaPlaybackState::Stopped => "Stopped",
    }
}

fn metadata(state: &State) -> HashMap<&'static str, Value<'static>> {
    let mut metadata = HashMap::new();
    metadata.insert(
        "mpris:trackid",
        Value::from(ObjectPath::from_string_unchecked(state.track_id())),
    );
    let Some(track) = state.track() else {
        return metadata;
    };
    if track.length_us > 0 {
        metadata.insert("mpris:length", Value::from(track.length_us));
    }
    if let Some(art_url) = &track.art_url {
        metadata.insert("mpris:artUrl", Value::from(art_url.clone()));
    }
    if !track.title.is_empty() {
        metadata.insert("xesam:title", Value::from(track.title.clone()));
    }
    if !track.artist.is_empty() {
        metadata.insert("xesam:artist", Value::from(vec![track.artist.clone()]));
    }
    if !track.album.is_empty() {
        metadata.insert("xesam:album", Value::from(track.album.clone()));
    }
    metadata
}

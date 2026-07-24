use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use audio_engine::EngineEvent;
use gpui::{App, Global};
use scrobble::{NowPlaying, PlayAccumulator, Scrobble, ScrobbleHandle, should_scrobble};

use crate::library_service::LibraryEvent;
use crate::services::Services;
use crate::settings_store::{SettingsStore, notify_save_error};

pub struct ScrobbleService {
    state: Rc<RefCell<BridgeState>>,
}

impl Global for ScrobbleService {}

impl ScrobbleService {
    pub fn handle(&self) -> Option<ScrobbleHandle> {
        self.state.borrow().handle.clone()
    }
}

struct CapturedMeta {
    artist: String,
    title: String,
    album: Option<String>,
}

struct Current {
    meta: CapturedMeta,
    duration: Duration,
    timestamp: u64,
    started: bool,
    scrobbled: bool,
}

struct BridgeState {
    handle: Option<ScrobbleHandle>,
    current: Option<Current>,
    accumulator: PlayAccumulator,
}

pub fn setup(cx: &mut App) {
    let handle = build_handle(cx);
    let state = Rc::new(RefCell::new(BridgeState {
        handle,
        current: None,
        accumulator: PlayAccumulator::new(),
    }));
    cx.set_global(ScrobbleService {
        state: state.clone(),
    });

    let services = cx.global::<Services>();
    let engine_event_bus = services.engine_event_bus.clone();
    let library_event_bus = services.library_event_bus.clone();

    {
        let state = state.clone();
        cx.subscribe(&engine_event_bus, move |_, event: &EngineEvent, cx| {
            on_engine_event(cx, &state, event);
        })
        .detach();
    }
    {
        cx.subscribe(&library_event_bus, move |_, event: &LibraryEvent, cx| {
            on_library_event(cx, &state, event);
        })
        .detach();
    }
}

pub fn set_session(cx: &mut App, session: Option<scrobble::Session>) {
    if let Err(e) = cx
        .global_mut::<SettingsStore>()
        .set_lastfm_session(session.clone())
    {
        notify_save_error(cx, e);
    }
    if let Some(handle) = cx.try_global::<ScrobbleService>().and_then(|s| s.handle()) {
        handle.set_session(session);
    }
}

pub fn finalize_on_quit(cx: &mut App) {
    if !is_active(cx) {
        return;
    }
    let Some(state) = cx.try_global::<ScrobbleService>().map(|s| s.state.clone()) else {
        return;
    };
    let now = Instant::now();
    let mut st = state.borrow_mut();
    st.accumulator.on_pause(now);
    let pending = take_pending_scrobble(&mut st, now);
    if let (Some(handle), Some(pending)) = (st.handle.clone(), pending) {
        handle.persist_blocking(pending, Duration::from_secs(3));
    }
}

fn build_handle(cx: &App) -> Option<ScrobbleHandle> {
    let (key, secret) = scrobble::creds()?;
    let queue_path = dirs::config_dir()?
        .join("pawse")
        .join("scrobble_queue.json");
    let session = cx.global::<SettingsStore>().lastfm_session();
    Some(ScrobbleHandle::spawn(key, secret, queue_path, session))
}

fn on_engine_event(cx: &mut App, state: &Rc<RefCell<BridgeState>>, event: &EngineEvent) {
    if !is_active(cx) {
        return;
    }
    let now = Instant::now();
    match event {
        EngineEvent::Loaded { duration, .. } => {
            let meta = read_current_meta(cx);
            let is_playing = cx.global::<Services>().is_playing.load(Ordering::Relaxed);
            let mut st = state.borrow_mut();
            st.accumulator.on_pause(now);
            try_scrobble_current(&mut st, now);
            st.current = None;
            st.accumulator.reset();
            if let Some(meta) = meta.filter(|m| !m.artist.is_empty() && !m.title.is_empty()) {
                st.current = Some(Current {
                    meta,
                    duration: *duration,
                    timestamp: 0,
                    started: false,
                    scrobbled: false,
                });
                if is_playing {
                    st.accumulator.on_play(now);
                    ensure_now_playing(&mut st);
                }
            }
        }
        EngineEvent::Playing => {
            let mut st = state.borrow_mut();
            st.accumulator.on_play(now);
            ensure_now_playing(&mut st);
        }
        EngineEvent::Paused => {
            let mut st = state.borrow_mut();
            st.accumulator.on_pause(now);
            try_scrobble_current(&mut st, now);
        }
        EngineEvent::TrackEnded | EngineEvent::Stopped => {
            let mut st = state.borrow_mut();
            st.accumulator.on_pause(now);
            try_scrobble_current(&mut st, now);
            st.current = None;
            st.accumulator.reset();
        }
        _ => {}
    }
}

fn on_library_event(cx: &mut App, state: &Rc<RefCell<BridgeState>>, event: &LibraryEvent) {
    if !is_active(cx) {
        return;
    }
    let LibraryEvent::TrackLikedChanged { track_id, liked } = event else {
        return;
    };
    let Some((artist, title)) = read_track_meta(cx, *track_id) else {
        return;
    };
    if artist.is_empty() || title.is_empty() {
        return;
    }
    if let Some(handle) = &state.borrow().handle {
        handle.love(artist, title, *liked);
    }
}

fn ensure_now_playing(st: &mut BridgeState) {
    let now_playing = {
        let Some(current) = st.current.as_mut() else {
            return;
        };
        if current.started {
            return;
        }
        current.started = true;
        current.timestamp = unix_now();
        NowPlaying {
            artist: current.meta.artist.clone(),
            title: current.meta.title.clone(),
            album: current.meta.album.clone(),
            duration_secs: Some(current.duration.as_secs()),
        }
    };
    if let Some(handle) = &st.handle {
        handle.now_playing(now_playing);
    }
}

fn try_scrobble_current(st: &mut BridgeState, now: Instant) {
    if let Some(pending) = take_pending_scrobble(st, now)
        && let Some(handle) = &st.handle
    {
        handle.scrobble(pending);
    }
}

fn take_pending_scrobble(st: &mut BridgeState, now: Instant) -> Option<Scrobble> {
    let played = st.accumulator.played(now);
    let current = st.current.as_mut()?;
    if current.scrobbled
        || !current.started
        || current.meta.artist.is_empty()
        || current.meta.title.is_empty()
        || !should_scrobble(played, current.duration)
    {
        return None;
    }
    current.scrobbled = true;
    Some(Scrobble {
        artist: current.meta.artist.clone(),
        title: current.meta.title.clone(),
        album: current.meta.album.clone(),
        duration_secs: Some(current.duration.as_secs()),
        timestamp: current.timestamp,
    })
}

fn is_active(cx: &App) -> bool {
    let store = cx.global::<SettingsStore>();
    store.lastfm_enabled() && store.lastfm_session().is_some()
}

fn read_current_meta(cx: &App) -> Option<CapturedMeta> {
    let services = cx.global::<Services>();
    let queue = services.playback_queue.borrow();
    let track = queue.current_track()?;
    let artist = services.library.track_artists(track.id).join(", ");
    let album = track
        .album_id
        .and_then(|id| services.library.album_title(id));
    Some(CapturedMeta {
        artist,
        title: track.title.clone(),
        album,
    })
}

fn read_track_meta(cx: &App, track_id: i64) -> Option<(String, String)> {
    let services = cx.global::<Services>();
    let track = services.library.track(track_id)?;
    let artist = services.library.track_artists(track.id).join(", ");
    Some((artist, track.title))
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use audio_engine::EngineEvent;
use gpui::{App, AppContext, Entity, Global, SharedString};
use scrobble::{
    AudioscrobblerClient, CsvLog, ListenBrainzClient, NowPlaying, PlayAccumulator, Profile,
    Scrobble, ScrobbleHandle, ScrobbleTarget, StatusEvent, TargetId, should_scrobble,
};

use crate::library_service::LibraryEvent;
use crate::localization::tr;
use crate::services::Services;
use crate::settings_store::SettingsStore;

#[derive(Default)]
pub struct ScrobbleStatus {
    pub auth_failed: HashSet<TargetId>,
    pub pending: usize,
}

pub struct ScrobbleService {
    state: Rc<RefCell<BridgeState>>,
    status: Entity<ScrobbleStatus>,
}

impl Global for ScrobbleService {}

impl ScrobbleService {
    pub fn status(&self) -> Entity<ScrobbleStatus> {
        self.status.clone()
    }
}

struct CapturedMeta {
    artist: String,
    title: String,
    album: Option<String>,
    album_artist: Option<String>,
    track_number: Option<u32>,
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
    active: bool,
    first_artist_only: bool,
    current: Option<Current>,
    accumulator: PlayAccumulator,
    notified: HashMap<TargetId, String>,
}

pub fn setup(cx: &mut App) {
    let (status_tx, status_rx) = flume::unbounded();
    let queue_path = dirs::config_dir().map(|dir| dir.join("pawse").join("scrobble_queue.json"));
    let targets = build_targets(cx);
    let active = !targets.is_empty();
    let handle = queue_path.map(|path| ScrobbleHandle::spawn(path, targets, status_tx));

    let first_artist_only = cx.global::<SettingsStore>().scrobble().first_artist_only;
    let state = Rc::new(RefCell::new(BridgeState {
        handle,
        active,
        first_artist_only,
        current: None,
        accumulator: PlayAccumulator::new(),
        notified: HashMap::new(),
    }));
    let status = cx.new(|_| ScrobbleStatus::default());
    cx.set_global(ScrobbleService {
        state: state.clone(),
        status: status.clone(),
    });

    let notify_state = state.clone();
    cx.spawn(async move |cx| {
        while let Ok(event) = status_rx.recv_async().await {
            let updated = status.update(cx, |status, cx| {
                match event {
                    StatusEvent::AuthFailed { target, message } => {
                        status.auth_failed.insert(target);
                        report_failure(cx, &notify_state, target, message);
                    }
                    StatusEvent::Rejected { target, message } => {
                        report_failure(cx, &notify_state, target, message);
                    }
                    StatusEvent::Pending(count) => status.pending = count,
                }
                cx.notify();
            });
            if updated.is_err() {
                break;
            }
        }
    })
    .detach();

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

pub fn apply_settings(cx: &mut App) {
    let targets = build_targets(cx);
    let active = !targets.is_empty();
    let first_artist_only = cx.global::<SettingsStore>().scrobble().first_artist_only;
    let Some(service) = cx.try_global::<ScrobbleService>() else {
        return;
    };
    let state = service.state.clone();
    let status = service.status.clone();
    {
        let mut st = state.borrow_mut();
        st.active = active;
        st.first_artist_only = first_artist_only;
        st.notified.clear();
        if let Some(handle) = &st.handle {
            handle.configure(targets);
        }
    }
    status.update(cx, |status, cx| {
        status.auth_failed.clear();
        cx.notify();
    });
}

fn report_failure(
    cx: &mut App,
    state: &Rc<RefCell<BridgeState>>,
    target: TargetId,
    message: String,
) {
    if state
        .borrow()
        .notified
        .get(&target)
        .is_some_and(|last| last == &message)
    {
        return;
    }
    if crate::error_bridge::push_background_error(cx, target_title(target), message.clone()) {
        state.borrow_mut().notified.insert(target, message);
    }
}

fn target_title(target: TargetId) -> SharedString {
    match target {
        TargetId::Lastfm => SharedString::from("Last.fm"),
        TargetId::Librefm => SharedString::from("Libre.fm"),
        TargetId::ListenBrainz => SharedString::from("ListenBrainz"),
        TargetId::CsvLog => tr().scrobble_csv.clone(),
    }
}

pub fn finalize_on_quit(cx: &mut App) {
    let Some(state) = cx.try_global::<ScrobbleService>().map(|s| s.state.clone()) else {
        return;
    };
    let now = Instant::now();
    let mut st = state.borrow_mut();
    if !st.active {
        return;
    }
    st.accumulator.on_pause(now);
    let pending = take_pending_scrobble(&mut st, now);
    if let (Some(handle), Some(pending)) = (st.handle.clone(), pending) {
        handle.persist(pending);
    }
}

fn build_targets(cx: &App) -> Vec<Box<dyn ScrobbleTarget>> {
    let settings = cx.global::<SettingsStore>().scrobble();
    let mut targets: Vec<Box<dyn ScrobbleTarget>> = Vec::new();

    if settings.lastfm.enabled
        && let Some(session) = &settings.lastfm.session
        && let Some((key, secret)) = scrobble::creds()
    {
        targets.push(Box::new(
            AudioscrobblerClient::new(Profile::lastfm(key, secret))
                .with_session(session.key.clone()),
        ));
    }

    if settings.librefm.enabled
        && let Some(session) = &settings.librefm.session
    {
        targets.push(Box::new(
            AudioscrobblerClient::new(Profile::librefm()).with_session(session.key.clone()),
        ));
    }

    if settings.listenbrainz.enabled && !settings.listenbrainz.token.is_empty() {
        targets.push(Box::new(ListenBrainzClient::new(
            settings.listenbrainz.api_root.clone(),
            settings.listenbrainz.token.clone(),
        )));
    }

    if settings.csv_log.enabled && !settings.csv_log.path.is_empty() {
        targets.push(Box::new(CsvLog::new(
            PathBuf::from(&settings.csv_log.path),
            env!("CARGO_PKG_VERSION").to_string(),
        )));
    }

    targets
}

fn on_engine_event(cx: &mut App, state: &Rc<RefCell<BridgeState>>, event: &EngineEvent) {
    if !state.borrow().active {
        return;
    }
    let now = Instant::now();
    match event {
        EngineEvent::Loaded { duration, .. } => {
            let first_artist_only = state.borrow().first_artist_only;
            let meta = read_current_meta(cx, first_artist_only);
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
    let LibraryEvent::TrackLikedChanged { track_id, liked } = event else {
        return;
    };
    let first_artist_only = {
        let st = state.borrow();
        if !st.active {
            return;
        }
        st.first_artist_only
    };
    let Some((artist, title)) = read_track_meta(cx, *track_id, first_artist_only) else {
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
            album_artist: current.meta.album_artist.clone(),
            track_number: current.meta.track_number,
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
        album_artist: current.meta.album_artist.clone(),
        track_number: current.meta.track_number,
        duration_secs: Some(current.duration.as_secs()),
        timestamp: current.timestamp,
    })
}

fn read_current_meta(cx: &App, first_artist_only: bool) -> Option<CapturedMeta> {
    let services = cx.global::<Services>();
    let queue = services.playback_queue.borrow();
    let track = queue.current_track()?;
    let artist =
        scrobble::primary_artist(&services.library.track_artists(track.id), first_artist_only)
            .unwrap_or_default();
    let album = track
        .album_id
        .and_then(|id| services.library.album_title(id));
    let album_artist = track
        .album_id
        .and_then(|id| scrobble::primary_artist(&services.library.album_artists(id), true));
    Some(CapturedMeta {
        artist,
        title: track.title.clone(),
        album,
        album_artist,
        track_number: track.track_number.and_then(|n| u32::try_from(n).ok()),
    })
}

fn read_track_meta(cx: &App, track_id: i64, first_artist_only: bool) -> Option<(String, String)> {
    let services = cx.global::<Services>();
    let track = services.library.track(track_id)?;
    let artist =
        scrobble::primary_artist(&services.library.track_artists(track.id), first_artist_only)?;
    Some((artist, track.title))
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use audio_engine::EngineEvent;
use discord::{DiscordHandle, Presence};
use gpui::{App, Global};

use crate::services::Services;
use crate::settings_store::SettingsStore;

const SEEK_THRESHOLD: Duration = Duration::from_millis(2500);

pub struct DiscordService {
    state: Rc<RefCell<BridgeState>>,
}

impl Global for DiscordService {}

struct CurrentMeta {
    artist: String,
    title: String,
    album: Option<String>,
    duration: Duration,
}

struct BridgeState {
    handle: Option<DiscordHandle>,
    current: Option<CurrentMeta>,
    playing: bool,
    anchor_pos: Duration,
    anchor_wall: Instant,
}

pub fn setup(cx: &mut App) {
    let handle = build_handle(cx);
    let state = Rc::new(RefCell::new(BridgeState {
        handle,
        current: None,
        playing: false,
        anchor_pos: Duration::ZERO,
        anchor_wall: Instant::now(),
    }));
    cx.set_global(DiscordService {
        state: state.clone(),
    });

    let engine_event_bus = cx.global::<Services>().engine_event_bus.clone();
    {
        let state = state.clone();
        cx.subscribe(&engine_event_bus, move |_, event: &EngineEvent, cx| {
            on_engine_event(cx, &state, event);
        })
        .detach();
    }

    seed(cx, &state);
}

pub fn set_enabled(cx: &mut App, enabled: bool) {
    let Some(state) = cx.try_global::<DiscordService>().map(|s| s.state.clone()) else {
        return;
    };
    if enabled {
        seed(cx, &state);
    } else {
        clear(&mut state.borrow_mut());
    }
}

pub fn finalize_on_quit(cx: &mut App) {
    let Some(state) = cx.try_global::<DiscordService>().map(|s| s.state.clone()) else {
        return;
    };
    if let Some(handle) = &state.borrow().handle {
        handle.shutdown();
    }
}

fn build_handle(_cx: &App) -> Option<DiscordHandle> {
    let client_id = discord::client_id()?;
    let cache_path = dirs::config_dir()?
        .join("pawse")
        .join("discord_art_cache.json");
    DiscordHandle::spawn(client_id, cache_path)
}

fn on_engine_event(cx: &mut App, state: &Rc<RefCell<BridgeState>>, event: &EngineEvent) {
    if !is_active(cx) {
        return;
    }
    match event {
        EngineEvent::Loaded { duration, .. } => {
            let meta = read_current_meta(cx, *duration);
            let playing = is_playing(cx);
            let mut st = state.borrow_mut();
            match meta {
                Some(meta) => {
                    let was_playing = st.playing;
                    st.current = Some(meta);
                    st.playing = playing;
                    st.anchor_pos = Duration::ZERO;
                    st.anchor_wall = Instant::now();
                    if playing {
                        send_current(&st, Duration::ZERO);
                    } else if !was_playing && let Some(handle) = &st.handle {
                        handle.clear();
                    }
                }
                None => clear(&mut st),
            }
        }
        EngineEvent::Playing => {
            let pos = position(cx);
            let mut st = state.borrow_mut();
            st.playing = true;
            st.anchor_pos = pos;
            st.anchor_wall = Instant::now();
            send_current(&st, pos);
        }
        EngineEvent::Paused => {
            let mut st = state.borrow_mut();
            st.playing = false;
            if let Some(handle) = &st.handle {
                handle.clear();
            }
        }
        EngineEvent::PositionChanged(p) => {
            let now = Instant::now();
            let mut st = state.borrow_mut();
            if !st.playing || st.current.is_none() {
                return;
            }
            let expected = st.anchor_pos + now.saturating_duration_since(st.anchor_wall);
            let drift = p.abs_diff(expected);
            if drift > SEEK_THRESHOLD {
                st.anchor_pos = *p;
                st.anchor_wall = now;
                send_current(&st, *p);
            }
        }
        EngineEvent::TrackEnded | EngineEvent::Stopped if !has_current_track(cx) => {
            clear(&mut state.borrow_mut());
        }
        EngineEvent::Error(_) => clear(&mut state.borrow_mut()),
        _ => {}
    }
}

fn send_current(st: &BridgeState, position: Duration) {
    let (Some(meta), Some(handle)) = (st.current.as_ref(), st.handle.as_ref()) else {
        return;
    };
    handle.set(Presence {
        artist: meta.artist.clone(),
        title: meta.title.clone(),
        album: meta.album.clone(),
        duration: meta.duration,
        started_at: unix_now() - position.as_secs() as i64,
    });
}

fn clear(st: &mut BridgeState) {
    st.current = None;
    st.playing = false;
    if let Some(handle) = &st.handle {
        handle.clear();
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn seed(cx: &mut App, state: &Rc<RefCell<BridgeState>>) {
    if !is_active(cx) {
        return;
    }
    let services = cx.global::<Services>();
    let duration = Duration::from_millis(services.current_duration_ms.load(Ordering::Relaxed));
    let Some(meta) = read_current_meta(cx, duration) else {
        return;
    };
    let services = cx.global::<Services>();
    let position = Duration::from_millis(services.current_position_ms.load(Ordering::Relaxed));
    let playing = services.is_playing.load(Ordering::Relaxed);
    let mut st = state.borrow_mut();
    st.current = Some(meta);
    st.playing = playing;
    st.anchor_pos = position;
    st.anchor_wall = Instant::now();
    if playing {
        send_current(&st, position);
    }
}

fn is_active(cx: &App) -> bool {
    cx.global::<SettingsStore>().discord_enabled()
}

fn is_playing(cx: &App) -> bool {
    cx.global::<Services>().is_playing.load(Ordering::Relaxed)
}

fn position(cx: &App) -> Duration {
    Duration::from_millis(
        cx.global::<Services>()
            .current_position_ms
            .load(Ordering::Relaxed),
    )
}

fn has_current_track(cx: &App) -> bool {
    cx.global::<Services>()
        .playback_queue
        .borrow()
        .current_track()
        .is_some()
}

fn read_current_meta(cx: &App, duration: Duration) -> Option<CurrentMeta> {
    let services = cx.global::<Services>();
    let queue = services.playback_queue.borrow();
    let track = queue.current_track()?;
    let artist = services.library.track_artists(track.id).join(", ");
    let album = track
        .album_id
        .and_then(|id| services.library.album_title(id));
    Some(CurrentMeta {
        artist,
        title: track.title.clone(),
        album,
        duration,
    })
}

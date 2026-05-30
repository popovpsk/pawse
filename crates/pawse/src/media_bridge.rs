use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use audio_engine::EngineEvent;
use gpui::{App, AsyncApp};
#[cfg(not(target_os = "macos"))]
use gpui::{Context, Entity, Subscription, Window};
use media_integration::{MediaCommand, MediaPlaybackState, NowPlayingInfo, SystemMediaIntegration};

#[cfg(not(target_os = "macos"))]
use crate::services::EngineEventsBus;
use crate::playback_queue::{PlaybackQueue, PreviousAction};
use crate::services::Services;

#[cfg(target_os = "macos")]
pub fn setup(cx: &mut App) {
    let current_position = Rc::new(RefCell::new(0.0f64));
    let current_duration = Rc::new(RefCell::new(0.0f64));
    let last_state = Rc::new(RefCell::new(MediaPlaybackState::Stopped));
    let Some(integration) = create_integration(
        cx,
        None,
        current_position.clone(),
        current_duration.clone(),
        last_state.clone(),
    ) else {
        return;
    };

    seed_from_services(
        cx,
        &integration,
        &last_state,
        &current_position,
        &current_duration,
    );

    let engine_event_bus = cx.global::<Services>().engine_event_bus.clone();
    cx.subscribe(&engine_event_bus, move |_, event: &EngineEvent, cx| {
        apply_engine_event(
            cx,
            event,
            &integration,
            &last_state,
            &current_position,
            &current_duration,
        );
    })
    .detach();
}

#[cfg(not(target_os = "macos"))]
pub struct MediaBridge {
    _subscription: Subscription,
}

#[cfg(not(target_os = "macos"))]
impl MediaBridge {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let engine_event_bus = cx.global::<Services>().engine_event_bus.clone();
        let hwnd = window_hwnd(window);

        let subscription = Self::init(cx, &engine_event_bus, hwnd)
            .unwrap_or_else(|| cx.subscribe(&engine_event_bus, |_, _, _, _| {}));

        Self {
            _subscription: subscription,
        }
    }

    fn init(
        cx: &mut Context<Self>,
        engine_event_bus: &Entity<EngineEventsBus>,
        hwnd: Option<*mut std::ffi::c_void>,
    ) -> Option<Subscription> {
        let current_position = Rc::new(RefCell::new(0.0f64));
        let current_duration = Rc::new(RefCell::new(0.0f64));
        let last_state = Rc::new(RefCell::new(MediaPlaybackState::Stopped));
        let integration = create_integration(
            cx,
            hwnd,
            current_position.clone(),
            current_duration.clone(),
            last_state.clone(),
        )?;

        seed_from_services(
            cx,
            &integration,
            &last_state,
            &current_position,
            &current_duration,
        );

        let subscription = cx.subscribe(
            engine_event_bus,
            move |_, _, event: &EngineEvent, cx| {
                apply_engine_event(
                    cx,
                    event,
                    &integration,
                    &last_state,
                    &current_position,
                    &current_duration,
                );
            },
        );
        Some(subscription)
    }
}

fn apply_engine_event(
    cx: &mut App,
    event: &EngineEvent,
    integration: &Rc<dyn SystemMediaIntegration>,
    last_state: &RefCell<MediaPlaybackState>,
    current_position: &RefCell<f64>,
    current_duration: &RefCell<f64>,
) {
    match event {
        EngineEvent::Loaded { duration, .. } => {
            let dur_secs = duration.as_secs_f64();
            current_position.replace(0.0);
            current_duration.replace(dur_secs);
            if publish_track(cx, integration, dur_secs, 0.0, MediaPlaybackState::Playing) {
                last_state.replace(MediaPlaybackState::Playing);
            }
        }
        EngineEvent::Playing => {
            last_state.replace(MediaPlaybackState::Playing);
            integration.set_playback_state(MediaPlaybackState::Playing);
            integration.update_position(*current_position.borrow(), MediaPlaybackState::Playing);
        }
        EngineEvent::Paused => {
            last_state.replace(MediaPlaybackState::Paused);
            integration.set_playback_state(MediaPlaybackState::Paused);
            integration.update_position(*current_position.borrow(), MediaPlaybackState::Paused);
        }
        EngineEvent::TrackEnded | EngineEvent::Stopped => {
            let has_track = cx
                .global::<Services>()
                .playback_queue
                .borrow()
                .current_track()
                .is_some();
            if !has_track {
                last_state.replace(MediaPlaybackState::Stopped);
                integration.set_playback_state(MediaPlaybackState::Stopped);
                integration.update_position(0.0, MediaPlaybackState::Stopped);
            }
        }
        EngineEvent::PositionChanged(position) => {
            if *last_state.borrow() == MediaPlaybackState::Stopped {
                return;
            }
            let secs = position.as_secs_f64();
            current_position.replace(secs);
            let state = *last_state.borrow();
            integration.update_position(secs, state);
        }
        _ => {}
    }
}

fn seed_from_services(
    cx: &mut App,
    integration: &Rc<dyn SystemMediaIntegration>,
    last_state: &RefCell<MediaPlaybackState>,
    current_position: &RefCell<f64>,
    current_duration: &RefCell<f64>,
) {
    let seed = {
        let services = cx.global::<Services>();
        if services.playback_queue.borrow().current_track().is_some() {
            let is_playing = services.is_playing.load(Ordering::Relaxed);
            let elapsed = services.current_position_ms.load(Ordering::Relaxed) as f64 / 1000.0;
            let dur = services.current_duration_ms.load(Ordering::Relaxed) as f64 / 1000.0;
            Some((is_playing, elapsed, dur))
        } else {
            None
        }
    };
    if let Some((is_playing, elapsed, dur)) = seed {
        let state = if is_playing {
            MediaPlaybackState::Playing
        } else {
            MediaPlaybackState::Paused
        };
        current_position.replace(elapsed);
        current_duration.replace(dur);
        if publish_track(cx, integration, dur, elapsed, state) {
            last_state.replace(state);
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn window_hwnd(window: &Window) -> Option<*mut std::ffi::c_void> {
    #[cfg(target_os = "windows")]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        match HasWindowHandle::window_handle(window).ok()?.as_raw() {
            RawWindowHandle::Win32(handle) => Some(handle.hwnd.get() as *mut std::ffi::c_void),
            _ => None,
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = window;
        None
    }
}

fn create_integration(
    cx: &mut App,
    hwnd: Option<*mut std::ffi::c_void>,
    current_position: Rc<RefCell<f64>>,
    current_duration: Rc<RefCell<f64>>,
    last_state: Rc<RefCell<MediaPlaybackState>>,
) -> Option<Rc<dyn SystemMediaIntegration>> {
    let (command_tx, command_rx) = flume::unbounded::<MediaCommand>();
    let integration = media_integration::create_integration(command_tx, hwnd)?;

    let engine_manager = cx.global::<Services>().engine_manager.clone();
    let queue = cx.global::<Services>().playback_queue.clone();

    cx.spawn(async move |cx| {
        run_command_loop(
            cx,
            command_rx,
            engine_manager,
            queue,
            current_position,
            current_duration,
            last_state,
        )
        .await;
    })
    .detach();

    Some(integration)
}

async fn run_command_loop(
    cx: &mut AsyncApp,
    rx: flume::Receiver<MediaCommand>,
    engine_manager: std::rc::Rc<audio_engine::EngineManager>,
    queue: std::rc::Rc<RefCell<PlaybackQueue>>,
    current_position: Rc<RefCell<f64>>,
    current_duration: Rc<RefCell<f64>>,
    last_state: Rc<RefCell<MediaPlaybackState>>,
) {
    while let Ok(command) = rx.recv_async().await {
        let result = cx.update(|_cx| match command {
            MediaCommand::Play => {
                engine_manager.play();
            }
            MediaCommand::Pause => {
                engine_manager.pause();
            }
            MediaCommand::TogglePlayPause => match *last_state.borrow() {
                MediaPlaybackState::Playing => engine_manager.pause(),
                _ => engine_manager.play(),
            },
            MediaCommand::Next => {
                let next = queue.borrow_mut().next_track().cloned();
                if let Some(track) = next {
                    let start_offset = if track.start_offset_ms > 0 {
                        Some(Duration::from_millis(track.start_offset_ms as u64))
                    } else {
                        None
                    };
                    let track_duration =
                        track.duration_ms.map(|ms| Duration::from_millis(ms as u64));
                    engine_manager.set_track_with_offset(
                        std::path::PathBuf::from(&track.path),
                        start_offset,
                        track_duration,
                    );
                    engine_manager.play();
                    crate::services::save_playback(_cx);
                }
            }
            MediaCommand::Previous => {
                let position_secs = *current_position.borrow() as f32;
                let mut q = queue.borrow_mut();
                let action = q.previous(position_secs);
                match action {
                    PreviousAction::SeekToStart => {
                        drop(q);
                        engine_manager.seek(0.0);
                        engine_manager.play();
                    }
                    PreviousAction::PreviousTrack(track) => {
                        let start_offset = if track.start_offset_ms > 0 {
                            Some(Duration::from_millis(track.start_offset_ms as u64))
                        } else {
                            None
                        };
                        let track_duration =
                            track.duration_ms.map(|ms| Duration::from_millis(ms as u64));
                        let path = std::path::PathBuf::from(&track.path);
                        drop(q);
                        engine_manager.set_track_with_offset(path, start_offset, track_duration);
                        engine_manager.play();
                        crate::services::save_playback(_cx);
                    }
                }
            }
            MediaCommand::Seek(position_secs) => {
                let duration = *current_duration.borrow();
                if duration > 0.0 {
                    let fraction = (position_secs / duration) as f32;
                    engine_manager.seek(fraction.clamp(0.0, 1.0));
                }
            }
        });
        if result.is_err() {
            break;
        }
    }
}

fn publish_track(
    cx: &App,
    integration: &Rc<dyn SystemMediaIntegration>,
    duration_secs: f64,
    elapsed_secs: f64,
    state: MediaPlaybackState,
) -> bool {
    let services = cx.global::<Services>();
    let queue = services.playback_queue.borrow();
    let Some(track) = queue.current_track() else {
        return false;
    };
    let artwork_path = track
        .cover_art_id
        .and_then(|id| services.library.get_cover_art_path_for_media(id));
    let mut info = build_now_playing_info(track, artwork_path, duration_secs);
    info.artist = services.library.track_artists(track.id).join(", ");
    info.album = track
        .album_id
        .and_then(|id| services.library.album_title(id))
        .unwrap_or_default();
    info.elapsed_secs = Some(elapsed_secs);
    integration.update_now_playing(info);
    integration.set_playback_state(state);
    integration.update_position(elapsed_secs, state);
    true
}

fn build_now_playing_info(
    track: &music_library::Track,
    artwork_path: Option<std::path::PathBuf>,
    duration_secs: f64,
) -> NowPlayingInfo {
    NowPlayingInfo {
        title: track.title.clone(),
        artist: String::new(),
        album: String::new(),
        artwork_path,
        duration_secs,
        elapsed_secs: Some(0.0),
    }
}

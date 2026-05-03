use std::cell::RefCell;
use std::rc::Rc;

use audio_engine::EngineEvent;
use gpui::{App, AsyncApp, Context, Entity, Subscription, Window};
use media_integration::{MediaCommand, MediaPlaybackState, NowPlayingInfo, SystemMediaIntegration};

use crate::playback_queue::{PreviousAction, PlaybackQueue};
use crate::services::{EngineEventsBus, Services};

pub struct MediaBridge {
    _subscription: Subscription,
}

impl MediaBridge {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let engine_event_bus = cx.global::<Services>().engine_event_bus.clone();

        #[cfg(target_os = "macos")]
        let subscription = Self::init_macos(cx, &engine_event_bus);

        #[cfg(not(target_os = "macos"))]
        let subscription = cx.subscribe(&engine_event_bus, |_, _, _, _| {});

        Self {
            _subscription: subscription,
        }
    }

    #[cfg(target_os = "macos")]
    fn init_macos(
        cx: &mut Context<Self>,
        engine_event_bus: &Entity<EngineEventsBus>,
    ) -> Subscription {
        let current_position = Rc::new(RefCell::new(0.0f64));
        let current_duration = Rc::new(RefCell::new(0.0f64));
        let last_state = Rc::new(RefCell::new(MediaPlaybackState::Stopped));
        let integration = create_integration(
            cx,
            current_position.clone(),
            current_duration.clone(),
            last_state.clone(),
        )
        .expect("macOS integration should initialize");
        let integration_clone = integration.clone();

        cx.subscribe(engine_event_bus, move |_, _, event: &EngineEvent, cx| {
            match event {
                EngineEvent::Loaded { duration, .. } => {
                    let dur_secs = duration.as_secs_f64();
                    current_position.replace(0.0);
                    current_duration.replace(dur_secs);
                    let services = cx.global::<Services>();
                    let queue = services.playback_queue.borrow();
                    if let Some(track) = queue.current_track() {
                        let mut info = build_now_playing_info(track, dur_secs);
                        info.artist = services.library.track_artists(track.id).join(", ");
                        info.album = track.album_id
                            .and_then(|id| services.library.album_title(id))
                            .unwrap_or_default();
                        integration_clone.update_now_playing(info);
                        integration_clone.set_playback_state(MediaPlaybackState::Playing);
                        last_state.replace(MediaPlaybackState::Playing);
                    }
                }
                EngineEvent::Playing => {
                    last_state.replace(MediaPlaybackState::Playing);
                    integration_clone.set_playback_state(MediaPlaybackState::Playing);
                    integration_clone.update_position(
                        *current_position.borrow(),
                        MediaPlaybackState::Playing,
                    );
                }
                EngineEvent::Paused => {
                    last_state.replace(MediaPlaybackState::Paused);
                    integration_clone.set_playback_state(MediaPlaybackState::Paused);
                    integration_clone.update_position(
                        *current_position.borrow(),
                        MediaPlaybackState::Paused,
                    );
                }
                EngineEvent::TrackEnded => {
                    let services = cx.global::<Services>();
                    let queue = services.playback_queue.borrow();
                    if queue.current_track().is_none() {
                        last_state.replace(MediaPlaybackState::Stopped);
                        integration_clone.set_playback_state(MediaPlaybackState::Stopped);
                        integration_clone.update_position(0.0, MediaPlaybackState::Stopped);
                    }
                }
                EngineEvent::PositionChanged(position) => {
                    if *last_state.borrow() == MediaPlaybackState::Stopped {
                        return;
                    }
                    let secs = position.as_secs_f64();
                    current_position.replace(secs);
                    let state = *last_state.borrow();
                    integration_clone.update_position(secs, state);
                }
                _ => {}
            }
        })
    }
}

#[cfg(target_os = "macos")]
fn create_integration(
    cx: &mut App,
    current_position: Rc<RefCell<f64>>,
    current_duration: Rc<RefCell<f64>>,
    last_state: Rc<RefCell<MediaPlaybackState>>,
) -> Option<Rc<dyn SystemMediaIntegration>> {
    use macos_integration::MacOsIntegration;

    let (command_tx, command_rx) = flume::unbounded::<MediaCommand>();
    let integration = MacOsIntegration::new(command_tx)?;
    let integration: Rc<dyn SystemMediaIntegration> = Rc::new(integration);

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
        let result = cx.update(|_cx| {
            match command {
                MediaCommand::Play => {
                    engine_manager.play();
                }
                MediaCommand::Pause => {
                    engine_manager.pause();
                }
                MediaCommand::TogglePlayPause => {
                    match *last_state.borrow() {
                        MediaPlaybackState::Playing => engine_manager.pause(),
                        _ => engine_manager.play(),
                    }
                }
                MediaCommand::Next => {
                    let next = queue.borrow_mut().next_track().cloned();
                    if let Some(track) = next {
                        let path = track.path.clone();
                        engine_manager.set_track(path.into());
                        engine_manager.play();
                    }
                }
                MediaCommand::Previous => {
                    let position_secs = *current_position.borrow() as f32;
                    let mut q = queue.borrow_mut();
                    let action = q.previous(position_secs);
                    match action {
                        PreviousAction::SeekToStart => {
                            engine_manager.seek(0.0);
                            engine_manager.play();
                        }
                        PreviousAction::PreviousTrack(track) => {
                            let path = track.path.clone();
                            engine_manager.set_track(path.into());
                            engine_manager.play();
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
            }
        });
        if result.is_err() {
            break;
        }
    }
}

fn build_now_playing_info(
    track: &music_library::Track,
    duration_secs: f64,
) -> NowPlayingInfo {
    NowPlayingInfo {
        title: track.title.clone(),
        artist: String::new(),
        album: String::new(),
        artwork_path: track.cover_art_path.as_ref().map(|p| p.into()),
        duration_secs,
        elapsed_secs: Some(0.0),
    }
}

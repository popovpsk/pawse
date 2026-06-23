use std::{
    cell::RefCell,
    io::Read,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use audio_engine::{AudioEngine, EngineEvent, EngineManager};
use audio_output::Output;
use gpui::{App, AppContext, AsyncApp, Entity, EventEmitter, Global};
use gpui_component::WindowExt;
use gpui_component::notification::Notification;
use music_library::Track;

use crate::cover_art_cache::CoverArtCache;
use crate::library_service::{LibraryEvent, LibraryService};

#[derive(Clone)]
pub struct Services {
    pub engine_manager: Rc<EngineManager>,
    pub output: Arc<Output>,
    pub engine_event_bus: Entity<EngineEventsBus>,
    pub library: Arc<LibraryService>,
    pub library_event_bus: Entity<LibraryEventsBus>,
    pub playback_queue: Rc<RefCell<crate::playback_queue::PlaybackQueue>>,
    pub cover_art_cache: Rc<RefCell<CoverArtCache>>,
    pub current_position_ms: Arc<AtomicU64>,
    pub current_duration_ms: Arc<AtomicU64>,
    pub is_playing: Arc<AtomicBool>,
    pub playlist_popup_bus: Entity<crate::playlist_popup::PlaylistPopupBus>,
    pub lang_event_bus: Entity<crate::localization::LangEventBus>,
    pub library_watcher: Rc<RefCell<Option<crate::library_watcher::LibraryWatcher>>>,
    pub watcher_ping_tx: flume::Sender<()>,
    pub remote_handle: pawse_remote::StateHandle,
    pub remote_state_rx: pawse_remote::StateRx,
    pub remote_command_tx: pawse_remote::CommandSink,
    pub remote_server: Rc<RefCell<Option<pawse_remote::RemoteServer>>>,
}

impl Services {
    pub fn initialize(cx: &mut App) -> Self {
        let output = Arc::new(Output::new());
        let audio_engine = Rc::new(AudioEngine::new(output.clone()));
        let engine_manager = Rc::new(EngineManager::new(audio_engine).start(cx));
        let engine_event_bus = cx.new(|_| EngineEventsBus);

        let (library_event_tx, library_event_rx) = flume::unbounded();
        let library = Arc::new(LibraryService::new(
            library_event_tx,
            cx.background_executor().clone(),
        ));
        let library_event_bus = cx.new(|_| LibraryEventsBus);
        let library_event_bus_clone = library_event_bus.clone();

        cx.spawn(async move |cx| {
            while let Ok(event) = library_event_rx.recv_async().await {
                if cx
                    .update(|cx| {
                        // If a playlist that's currently backing the playback
                        // queue has its track list changed, sync the queue first
                        // so subscribers (queue_view, etc.) see fresh state when
                        // they receive the emitted event below.
                        if let LibraryEvent::PlaylistTracksChanged { playlist_id } = &event {
                            sync_queue_with_playlist(*playlist_id, cx);
                        }
                        if let LibraryEvent::ScanComplete { changed: true } = &event {
                            remap_queue_after_rescan(cx);
                        }
                        if let LibraryEvent::TrackLikedChanged { track_id, liked } = &event {
                            cx.global::<Services>()
                                .playback_queue
                                .borrow_mut()
                                .set_track_liked(*track_id, *liked);
                        }
                        notify_scan_event(&event, cx);
                        library_event_bus_clone.update(cx, |_, cx| cx.emit(event));
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        let (watcher_ping_tx, watcher_ping_rx) = flume::bounded::<()>(1);
        let watcher_library = library.clone();
        cx.spawn(async move |cx| {
            while watcher_ping_rx.recv_async().await.is_ok() {
                if cx
                    .update(|cx| {
                        let folders = cx
                            .global::<crate::settings_store::SettingsStore>()
                            .music_folders()
                            .to_vec();
                        watcher_library.request_rescan(folders, false, false);
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        let playlist_popup_bus = cx.new(|_| crate::playlist_popup::PlaylistPopupBus);
        let lang_event_bus = cx.new(|_| crate::localization::LangEventBus);

        let (remote_handle, remote_state_rx) = pawse_remote::channel();
        let (remote_command_tx, remote_command_rx) = pawse_remote::commands();

        cx.spawn(async move |cx| {
            while let Ok(command) = remote_command_rx.recv_async().await {
                if cx.update(|cx| apply_remote_command(cx, command)).is_err() {
                    break;
                }
            }
        })
        .detach();

        Services {
            output,
            engine_manager,
            engine_event_bus,
            library,
            library_event_bus,
            playback_queue: Rc::new(RefCell::new(crate::playback_queue::PlaybackQueue::new())),
            cover_art_cache: Rc::new(RefCell::new(CoverArtCache::new())),
            current_position_ms: Arc::new(AtomicU64::new(0)),
            current_duration_ms: Arc::new(AtomicU64::new(0)),
            is_playing: Arc::new(AtomicBool::new(false)),
            playlist_popup_bus,
            lang_event_bus,
            library_watcher: Rc::new(RefCell::new(None)),
            watcher_ping_tx,
            remote_handle,
            remote_state_rx,
            remote_command_tx,
            remote_server: Rc::new(RefCell::new(None)),
        }
    }

    pub fn play_track(&self, track: &Track) {
        self.load_track(track);
        self.engine_manager.play();
    }

    /// Like `play_track` but skips the 300ms fade-in for gapless transitions.
    pub fn play_track_gapless(&self, track: &Track) {
        self.load_track(track);
        self.engine_manager.play_gapless();
    }

    /// Load a track into the engine without starting playback. Fires
    /// `EngineEvent::Loaded` so subscribers (now-playing, queue view) update,
    /// but leaves the engine paused at position 0.
    pub fn load_track(&self, track: &Track) {
        self.current_position_ms.store(0, Ordering::Relaxed);
        let path = std::path::PathBuf::from(&track.path);
        let start_offset = if track.start_offset_ms > 0 {
            Some(Duration::from_millis(track.start_offset_ms as u64))
        } else {
            None
        };
        let track_duration = track.duration_ms.map(|ms| Duration::from_millis(ms as u64));
        self.engine_manager
            .set_track_with_offset(path, start_offset, track_duration);
    }
}

impl Services {
    pub fn shutdown(&self) {
        self.output.shutdown();
        self.engine_manager.shutdown();
    }

    pub fn snapshot_playback(&self) -> crate::settings_store::PlaybackState {
        let queue = self.playback_queue.borrow();
        // The queue holds `Rc<Track>` for cheap cloning during scroll/clicks;
        // persistence needs owned `Track`, so deep-clone here (rare: app quit /
        // track change).
        crate::settings_store::PlaybackState {
            queue: queue.tracks_vec().iter().map(|t| (**t).clone()).collect(),
            original_queue: queue
                .original_order_vec()
                .map(|v| v.iter().map(|t| (**t).clone()).collect()),
            current_index: queue.current_index(),
            position_ms: self.current_position_ms.load(Ordering::Relaxed),
            shuffle: queue.shuffle(),
            repeat: queue.repeat().into(),
            source: queue.source().into(),
            custom: queue.is_custom(),
        }
    }
}

fn notify_scan_event(event: &LibraryEvent, cx: &mut App) {
    let notification = match event {
        LibraryEvent::ScanStarted => {
            Notification::info(crate::localization::tr().library_updating.clone())
        }
        LibraryEvent::ScanSucceeded => {
            Notification::success(crate::localization::tr().library_updated.clone())
        }
        LibraryEvent::ScanUpToDate => {
            Notification::success(crate::localization::tr().library_up_to_date.clone())
        }
        LibraryEvent::ScanFailed => {
            Notification::error(crate::localization::tr().library_update_failed.clone())
        }
        _ => return,
    };
    let Some(handle) = cx.windows().into_iter().next() else {
        return;
    };
    let _ = handle.update(cx, |_, window, cx| {
        window.push_notification(notification, cx);
    });
}

/// If the active playback queue is backed by the given playlist, replace its
/// track list with a fresh copy from the library, preserving the currently-
/// playing track by id. Also snapshots the new queue into the settings store
/// so the on-disk `playback` block doesn't reference a track that was just
/// removed. No-op otherwise.
fn sync_queue_with_playlist(playlist_id: i64, cx: &mut App) {
    let services = cx.global::<Services>();
    let matches = matches!(
        services.playback_queue.borrow().source(),
        crate::playback_queue::QueueSource::Playlist(id) if id == playlist_id,
    );
    if !matches {
        return;
    }
    let new_tracks = services.library.tracks_for_playlist(playlist_id);
    services
        .playback_queue
        .borrow_mut()
        .refresh_keeping_current(new_tracks.into_iter().map(Rc::new).collect());
    save_playback(cx);
}

fn advance_on_track_end(cx: &mut App) {
    let services = cx.global::<Services>();
    let next = services.playback_queue.borrow_mut().next_track().cloned();
    if let Some(track) = next {
        services.play_track_gapless(&track);
        save_playback(cx);
    }
}

fn remap_queue_after_rescan(cx: &mut App) {
    let services = cx.global::<Services>().clone();
    let keys: Vec<(String, i32)> = {
        let queue = services.playback_queue.borrow();
        if queue.is_empty() {
            return;
        }
        let mut keys: Vec<(String, i32)> = queue
            .tracks_vec()
            .iter()
            .map(|t| (t.path.clone(), t.start_offset_ms))
            .collect();
        if let Some(orig) = queue.original_order_vec() {
            keys.extend(orig.iter().map(|t| (t.path.clone(), t.start_offset_ms)));
        }
        keys
    };

    let fresh: std::collections::HashMap<(String, i32), Rc<Track>> = services
        .library
        .tracks_by_keys(&keys)
        .into_iter()
        .map(|t| ((t.path.clone(), t.start_offset_ms), Rc::new(t)))
        .collect();

    if fresh.is_empty() {
        return;
    }

    services
        .playback_queue
        .borrow_mut()
        .remap_to_fresh_tracks(&fresh);
    save_playback(cx);
}

pub fn save_playback(cx: &mut App) {
    let state = cx.global::<Services>().snapshot_playback();
    if let Err(e) = cx
        .global_mut::<crate::settings_store::SettingsStore>()
        .set_playback(state)
    {
        crate::settings_store::notify_save_error(cx, e);
    }
}

pub fn apply_remote_state(cx: &mut App) {
    let store = cx.global::<crate::settings_store::SettingsStore>();
    let enabled = store.remote_enabled();
    let port = store.remote_port();
    let services = cx.global::<Services>().clone();

    let ready = {
        let mut slot = services.remote_server.borrow_mut();
        *slot = None;
        if enabled {
            let (server, ready) = pawse_remote::spawn(
                std::net::SocketAddr::from(([0, 0, 0, 0], port)),
                services.remote_state_rx.clone(),
                services.remote_command_tx.clone(),
            );
            *slot = Some(server);
            Some(ready)
        } else {
            None
        }
    };

    if enabled {
        let state = build_remote_state(cx, &services.remote_handle);
        services.remote_handle.publish(state);
    } else {
        services
            .remote_handle
            .publish(pawse_remote::PlayerState::idle());
    }

    if let Some(ready) = ready {
        cx.spawn(async move |cx| {
            if let Ok(Err(err)) = ready.await {
                let _ = cx.update(|cx| notify_remote_error(cx, port, &err));
            }
        })
        .detach();
    }
}

fn notify_remote_error(cx: &mut App, port: u16, err: &str) {
    let s = crate::localization::tr();
    let Some(handle) = cx.windows().into_iter().next() else {
        return;
    };
    let message = s.remote_start_failed(port, err);
    let title = s.settings.clone();
    let _ = handle.update(cx, |_, window, cx| {
        window.push_notification(Notification::error(message).title(title), cx);
    });
}

// why: play() with no track emits no event, leaving the is_playing mirror stuck true
pub fn toggle_play_pause(cx: &mut App) -> Option<bool> {
    let services = cx.global::<Services>();
    services.playback_queue.borrow().current_track()?;
    let was_playing = services.is_playing.fetch_xor(true, Ordering::Relaxed);
    if was_playing {
        services.engine_manager.pause();
    } else {
        services.engine_manager.play();
    }
    Some(!was_playing)
}

pub fn play_next(cx: &mut App) {
    let services = cx.global::<Services>();
    let next = services.playback_queue.borrow_mut().next_track().cloned();
    if let Some(track) = next {
        services.play_track(&track);
        save_playback(cx);
    }
}

pub fn play_previous(cx: &mut App) {
    let services = cx.global::<Services>();
    let position_secs = services.current_position_ms.load(Ordering::Relaxed) as f32 / 1000.0;
    let previous = {
        let mut queue = services.playback_queue.borrow_mut();
        match queue.previous(position_secs) {
            crate::playback_queue::PreviousAction::SeekToStart => None,
            crate::playback_queue::PreviousAction::PreviousTrack(track) => Some(track.clone()),
        }
    };
    match previous {
        None => {
            services.engine_manager.seek(0.0);
            services.engine_manager.play();
        }
        Some(track) => {
            services.play_track(&track);
            save_playback(cx);
        }
    }
}

pub fn seek_to_ms(cx: &mut App, position_ms: u64) {
    let services = cx.global::<Services>();
    if services.playback_queue.borrow().current_track().is_none() {
        return;
    }
    let duration = services.current_duration_ms.load(Ordering::Relaxed);
    if duration == 0 {
        return;
    }
    let clamped = position_ms.min(duration);
    let ratio = (clamped as f32 / duration as f32).clamp(0.0, 1.0);
    services
        .current_position_ms
        .store(clamped, Ordering::Relaxed);
    services.engine_manager.seek(ratio);
}

fn apply_remote_command(cx: &mut App, command: pawse_remote::Command) {
    match command {
        pawse_remote::Command::PlayPause => {
            toggle_play_pause(cx);
        }
        pawse_remote::Command::Next => play_next(cx),
        pawse_remote::Command::Prev => play_previous(cx),
        pawse_remote::Command::Seek { position_ms } => seek_to_ms(cx, position_ms),
    }
}

impl Global for Services {}

pub struct EngineEventsBus;

impl EngineEventsBus {}

impl EventEmitter<EngineEvent> for EngineEventsBus {}
impl Global for EngineEventsBus {}

pub struct LibraryEventsBus;

impl LibraryEventsBus {}

impl EventEmitter<LibraryEvent> for LibraryEventsBus {}
impl Global for LibraryEventsBus {}

pub async fn run_engine_events_bus(
    cx: &mut AsyncApp,
    engine_manager: Rc<EngineManager>,
    engine_event_bus: Entity<EngineEventsBus>,
    current_position_ms: Arc<AtomicU64>,
    current_duration_ms: Arc<AtomicU64>,
    is_playing: Arc<AtomicBool>,
    remote: pawse_remote::StateHandle,
) {
    let mut current_duration: Option<Duration> = None;
    let mut prefetched = false;
    let rx = engine_manager.events();
    while let Ok(event) = rx.recv_async().await {
        match &event {
            EngineEvent::Loaded { duration, .. } => {
                current_duration = Some(*duration);
                current_duration_ms.store(duration.as_millis() as u64, Ordering::Relaxed);
                prefetched = false;
                publish_now_playing(cx, &remote);
            }
            EngineEvent::PositionChanged(dur) => {
                current_position_ms.store(dur.as_millis() as u64, Ordering::Relaxed);
                if remote.has_listeners() {
                    remote.publish_position(
                        dur.as_millis() as u64,
                        is_playing.load(Ordering::Relaxed),
                    );
                }
                maybe_prefetch_next_track(cx, dur, current_duration, &mut prefetched);
            }
            EngineEvent::Playing => {
                is_playing.store(true, Ordering::Relaxed);
                publish_now_playing(cx, &remote);
            }
            EngineEvent::Paused => {
                is_playing.store(false, Ordering::Relaxed);
                publish_now_playing(cx, &remote);
            }
            EngineEvent::TrackEnded => {
                is_playing.store(false, Ordering::Relaxed);
                let _ = cx.update(advance_on_track_end);
                publish_now_playing(cx, &remote);
            }
            EngineEvent::Stopped => {
                is_playing.store(false, Ordering::Relaxed);
                current_position_ms.store(0, Ordering::Relaxed);
                current_duration_ms.store(0, Ordering::Relaxed);
                publish_now_playing(cx, &remote);
            }
            _ => {}
        }
        if cx
            .update(|cx| engine_event_bus.update(cx, |_, cx| cx.emit(event)))
            .is_err()
        {
            break;
        }
    }
}

fn publish_now_playing(cx: &mut AsyncApp, remote: &pawse_remote::StateHandle) {
    if !remote.has_listeners() {
        return;
    }
    if let Ok(state) = cx.update(|cx| build_remote_state(cx, remote)) {
        remote.publish(state);
    }
}

fn build_remote_state(
    cx: &mut App,
    remote: &pawse_remote::StateHandle,
) -> pawse_remote::PlayerState {
    let services = cx.global::<Services>();
    let queue = services.playback_queue.borrow();
    let Some(track) = queue.current_track() else {
        return pawse_remote::PlayerState::idle();
    };
    let track_id = track.id;
    let title = track.title.clone();
    let album_id = track.album_id;
    let cover_id = track.cover_art_id;
    let duration_ms = track.duration_ms.unwrap_or(0).max(0) as u64;
    drop(queue);

    let artist = services.library.track_artists(track_id).into_iter().next();
    let album = album_id.and_then(|id| services.library.album_title(id));
    let cover = if cover_id.is_some() && cover_id == remote.current_cover_id() {
        remote.current_cover()
    } else {
        cover_id
            .and_then(|id| services.library.get_cover_art_large(id))
            .map(Arc::new)
    };

    pawse_remote::PlayerState {
        v: 0,
        has_track: true,
        title: Some(title),
        artist,
        album,
        playing: services.is_playing.load(Ordering::Relaxed),
        position_ms: services.current_position_ms.load(Ordering::Relaxed),
        duration_ms,
        cover_id: if cover.is_some() { cover_id } else { None },
        cover,
    }
}

/// If the next track is known and we're within 5 seconds of the end of the
/// current one, warm the OS page cache by reading the first 64 KiB of the next
/// track's file. This eliminates decoder-open latency for gapless transitions,
/// especially on spinning disks.
fn maybe_prefetch_next_track(
    cx: &AsyncApp,
    position: &Duration,
    track_duration: Option<Duration>,
    prefetched: &mut bool,
) {
    if *prefetched {
        return;
    }
    let near_end = track_duration
        .map(|d| d.saturating_sub(*position) <= Duration::from_secs(2))
        .unwrap_or(false);
    if !near_end {
        return;
    }
    *prefetched = true;

    let Some(path) = cx
        .update(|cx| {
            cx.global::<Services>()
                .playback_queue
                .borrow()
                .peek_next()
                .map(|t| std::path::PathBuf::from(&t.path))
        })
        .ok()
        .flatten()
    else {
        return;
    };

    cx.background_spawn(async move {
        if let Ok(mut file) = std::fs::File::open(&path) {
            let mut buf = [0u8; 65536];
            let _ = file.read(&mut buf);
        }
    })
    .detach();
}

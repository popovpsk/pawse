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
                        watcher_library.request_rescan(folders, false);
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
            }
            EngineEvent::PositionChanged(dur) => {
                current_position_ms.store(dur.as_millis() as u64, Ordering::Relaxed);
                maybe_prefetch_next_track(cx, dur, current_duration, &mut prefetched);
            }
            EngineEvent::Playing => is_playing.store(true, Ordering::Relaxed),
            EngineEvent::Paused => {
                is_playing.store(false, Ordering::Relaxed);
            }
            EngineEvent::TrackEnded => {
                is_playing.store(false, Ordering::Relaxed);
                let _ = cx.update(advance_on_track_end);
            }
            EngineEvent::Stopped => {
                is_playing.store(false, Ordering::Relaxed);
                current_position_ms.store(0, Ordering::Relaxed);
                current_duration_ms.store(0, Ordering::Relaxed);
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

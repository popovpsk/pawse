use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use audio_common::{ChannelCount, StreamParams};
use audio_engine::{AudioEngine, EngineEvent, EngineManager, TrackResolver};
use gpui::{App, AppContext, BorrowAppContext, Global};
use gpui_component::WindowExt;
use gpui_component::notification::Notification;
use music_library::Track;
use ui_resources::i18n::cast_strings;

use crate::library_service::LibraryService;
use crate::playback_opener::{AfterLoad, OpenerBackend, PlaybackOpener, TrackRequest};
use crate::services::Services;

mod media;

use media::CastTrack;

const LOCAL: u64 = 0;
const AIRPLAY_START_VOLUME: f32 = 0.5;
const SHUTDOWN_WAIT: Duration = Duration::from_secs(2);
const LOCAL_FADE_OUT: Duration = Duration::from_millis(400);

#[derive(Default)]
pub struct CastState {
    pub receivers: Vec<cast::Receiver>,
    pub active: Option<cast::Receiver>,
    pub connecting: Option<cast::Receiver>,
    pub volume: Option<f32>,
    pub searching: bool,
    search_round: u64,
    discovery: Option<Rc<cast::Discovery>>,
    refreshed: Option<std::time::Instant>,
}

impl Global for CastState {}

impl CastState {
    pub fn is_casting(&self) -> bool {
        self.active.is_some()
    }

    fn mark_in_use(&self) {
        if let Some(discovery) = &self.discovery {
            discovery.set_in_use(
                self.active
                    .iter()
                    .chain(&self.connecting)
                    .map(|receiver| receiver.id.clone())
                    .collect(),
            );
        }
    }
}

struct AirPlayTarget {
    id: u64,
    engine: Rc<AudioEngine>,
    opener: PlaybackOpener,
    output: Arc<cast::AirPlayOutput>,
}

struct RendererTarget {
    id: u64,
    session: Arc<cast::Session>,
    loads: Arc<AtomicU64>,
    duration_ms: Arc<AtomicU64>,
}

enum Target {
    Local,
    AirPlay(AirPlayTarget),
    Renderer(RendererTarget),
}

impl Target {
    fn id(&self) -> u64 {
        match self {
            Target::Local => LOCAL,
            Target::AirPlay(target) => target.id,
            Target::Renderer(target) => target.id,
        }
    }

    fn release(self, wait: bool) {
        match self {
            Target::Local => {}
            Target::AirPlay(target) => {
                target.opener.stop();
                target.engine.shutdown();
                let output = target.output;
                if wait {
                    output.close();
                } else if let Err(e) = std::thread::Builder::new()
                    .name("airplay-close".into())
                    .spawn(move || output.close())
                {
                    log::warn!("cast: closing the AirPlay stream in the background failed: {e}");
                }
            }
            Target::Renderer(target) => {
                target.loads.fetch_add(1, Ordering::AcqRel);
                drop(target.session);
            }
        }
    }
}

enum Connected {
    AirPlay(Arc<cast::AirPlayOutput>),
    Renderer(cast::Session),
}

fn engine_sink(engine: &AudioEngine) -> crate::playback_opener::Sink {
    let commander = engine.commander();
    Arc::new(move |command| commander.send(command))
}

pub struct Player {
    local: Rc<EngineManager>,
    local_opener: PlaybackOpener,
    backend: Arc<dyn OpenerBackend>,
    library: Arc<LibraryService>,
    resolver: TrackResolver,
    target: RefCell<Target>,
    active: Arc<AtomicU64>,
    next_id: Cell<u64>,
    local_fades: Cell<u64>,
    events_tx: flume::Sender<EngineEvent>,
    events_rx: flume::Receiver<EngineEvent>,
    server: RefCell<Option<Arc<cast::MediaServer>>>,
}

impl Player {
    pub fn new(
        local: Rc<EngineManager>,
        backend: Arc<dyn OpenerBackend>,
        library: Arc<LibraryService>,
        resolver: TrackResolver,
        cx: &mut App,
    ) -> Rc<Self> {
        let commander = local.commander();
        let local_opener = PlaybackOpener::new(
            backend.clone(),
            Arc::new(move |command| commander.send(command)),
        );
        let (events_tx, events_rx) = flume::unbounded();
        let active = Arc::new(AtomicU64::new(LOCAL));
        let local_events = local.events().clone();
        let forward = events_tx.clone();
        let local_active = active.clone();
        cx.spawn(async move |_| {
            while let Ok(event) = local_events.recv_async().await {
                if local_active.load(Ordering::Acquire) == LOCAL && forward.send(event).is_err() {
                    break;
                }
            }
        })
        .detach();
        cx.set_global(CastState::default());
        Rc::new(Self {
            local,
            local_opener,
            backend,
            library,
            resolver,
            target: RefCell::new(Target::Local),
            active,
            next_id: Cell::new(LOCAL + 1),
            local_fades: Cell::new(0),
            events_tx,
            events_rx,
            server: RefCell::new(None),
        })
    }

    pub fn events(&self) -> flume::Receiver<EngineEvent> {
        self.events_rx.clone()
    }

    pub fn local(&self) -> &EngineManager {
        &self.local
    }

    pub fn is_casting(&self) -> bool {
        !matches!(*self.target.borrow(), Target::Local)
    }

    pub fn start(&self, track: &Track, after: AfterLoad) {
        match &*self.target.borrow() {
            Target::Local => self.local_opener.start(&TrackRequest::from(track), after),
            Target::AirPlay(target) => target.opener.start(&TrackRequest::from(track), after),
            Target::Renderer(target) => self.start_renderer(
                target,
                CastTrack::from(track),
                Duration::ZERO,
                after != AfterLoad::Stay,
            ),
        }
    }

    pub fn stop(&self) {
        match &*self.target.borrow() {
            Target::Local => self.local_opener.stop(),
            Target::AirPlay(target) => target.opener.stop(),
            Target::Renderer(target) => {
                target.loads.fetch_add(1, Ordering::AcqRel);
                target.session.stop();
                let _ = self.events_tx.send(EngineEvent::Stopped);
            }
        }
    }

    pub fn play(&self) {
        match &*self.target.borrow() {
            Target::Local => self.local.play(),
            Target::AirPlay(target) => target.engine.play(),
            Target::Renderer(target) => target.session.play(),
        }
    }

    pub fn pause(&self) {
        match &*self.target.borrow() {
            Target::Local => self.local.pause(),
            Target::AirPlay(target) => target.engine.pause(),
            Target::Renderer(target) => target.session.pause(),
        }
    }

    pub fn seek(&self, ratio: f32) {
        match &*self.target.borrow() {
            Target::Local => self.local.seek(ratio),
            Target::AirPlay(target) => target.engine.seek(ratio),
            Target::Renderer(target) => {
                let duration = target.duration_ms.load(Ordering::Acquire);
                let position = (duration as f64 * f64::from(ratio.clamp(0.0, 1.0))) as u64;
                target.session.seek(Duration::from_millis(position));
            }
        }
    }

    pub fn set_cast_volume(&self, volume: f32) -> bool {
        match &*self.target.borrow() {
            Target::Local => false,
            Target::AirPlay(target) => {
                target.output.set_device_volume(volume);
                true
            }
            Target::Renderer(target) => {
                target.session.set_volume(volume);
                true
            }
        }
    }

    pub fn shutdown(&self) {
        self.active.store(LOCAL, Ordering::Release);
        let target = self.target.replace(Target::Local);
        if let Target::Renderer(renderer) = &target {
            renderer.session.close(SHUTDOWN_WAIT);
        }
        target.release(true);
        self.local.shutdown();
    }

    fn server(&self) -> Result<Arc<cast::MediaServer>, String> {
        if let Some(server) = self.server.borrow().as_ref() {
            return Ok(server.clone());
        }
        let server = cast::MediaServer::start().map_err(|e| e.to_string())?;
        *self.server.borrow_mut() = Some(server.clone());
        Ok(server)
    }

    fn take_id(&self) -> u64 {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        id
    }

    fn start_renderer(
        &self,
        target: &RendererTarget,
        track: CastTrack,
        start: Duration,
        autoplay: bool,
    ) {
        let load = target.loads.fetch_add(1, Ordering::AcqRel) + 1;
        target.duration_ms.store(
            track.duration.map_or(0, |d| d.as_millis() as u64),
            Ordering::Release,
        );
        let _ = self.events_tx.send(EngineEvent::Preparing {
            duration: track.duration,
        });
        let backend = self.backend.clone();
        let library = self.library.clone();
        let session = target.session.clone();
        let loads = target.loads.clone();
        let events = self.events_tx.clone();
        let active = self.active.clone();
        let id = target.id;
        let spawned = std::thread::Builder::new()
            .name("cast-open".into())
            .spawn(move || {
                let abandoned = || loads.load(Ordering::Acquire) != load;
                match media::resolve(&backend, &library, &track, &abandoned) {
                    Ok(media) if !abandoned() => session.load(cast::Load {
                        media,
                        start,
                        autoplay,
                    }),
                    Ok(_) => {}
                    Err(_) if abandoned() => {}
                    Err(e) => {
                        if active.load(Ordering::Acquire) == id {
                            let _ = events.send(EngineEvent::Error(e));
                        }
                    }
                }
            });
        if let Err(e) = spawned {
            let _ = self.events_tx.send(EngineEvent::Error(e.to_string()));
        }
    }
}

const REFRESH_EVERY: Duration = Duration::from_secs(20);
const FIRST_SEARCH: Duration = Duration::from_secs(6);
const REFRESH_SEARCH: Duration = Duration::from_secs(4);

fn mark_searching(lasts: Duration, cx: &mut App) {
    let round = cx.update_global::<CastState, _>(|state, _| {
        state.searching = true;
        state.search_round += 1;
        state.search_round
    });
    cx.spawn(async move |cx| {
        cx.background_executor().timer(lasts).await;
        cx.update(|cx| {
            cx.update_global::<CastState, _>(|state, _| {
                if state.search_round == round {
                    state.searching = false;
                }
            });
        });
    })
    .detach();
}

pub fn start_discovery(cx: &mut App) {
    let state = cx.global::<CastState>();
    if let Some(discovery) = &state.discovery {
        if state
            .refreshed
            .is_none_or(|at| at.elapsed() >= REFRESH_EVERY)
        {
            discovery.refresh();
            cx.update_global::<CastState, _>(|state, _| {
                state.refreshed = Some(std::time::Instant::now());
            });
            mark_searching(REFRESH_SEARCH, cx);
        }
        return;
    }
    let discovery = Rc::new(cast::Discovery::start());
    let changes = discovery.changes();
    cx.update_global::<CastState, _>(|state, _| {
        state.discovery = Some(discovery.clone());
        state.refreshed = Some(std::time::Instant::now());
    });
    mark_searching(FIRST_SEARCH, cx);
    cx.spawn(async move |cx| {
        while changes.recv_async().await.is_ok() {
            cx.update(|cx| {
                let receivers = discovery.receivers();
                cx.update_global::<CastState, _>(|state, _| state.receivers = receivers);
            });
        }
    })
    .detach();
}

fn notify(cx: &mut App, notification: Notification) {
    let Some(handle) = cx.windows().into_iter().next() else {
        return;
    };
    let _ = handle.update(cx, |_, window, cx| {
        window.push_notification(notification, cx);
    });
}

pub fn connect(receiver: cast::Receiver, cx: &mut App) {
    let state = cx.global::<CastState>();
    if state
        .active
        .as_ref()
        .is_some_and(|active| active.id == receiver.id)
        || state
            .connecting
            .as_ref()
            .is_some_and(|connecting| connecting.id == receiver.id)
    {
        return;
    }
    let player = cx.global::<Services>().player.clone();
    let server = if receiver.airplay_device().is_some() {
        None
    } else {
        match player.server() {
            Ok(server) => Some(server),
            Err(e) => {
                notify(
                    cx,
                    Notification::error(cast_strings().connect_failed(&receiver.name, &e))
                        .title(cast_strings().streaming.clone()),
                );
                return;
            }
        }
    };
    let volume = cx
        .global::<crate::settings_store::SettingsStore>()
        .volume()
        .min(AIRPLAY_START_VOLUME);
    cx.update_global::<CastState, _>(|state, _| {
        state.connecting = Some(receiver.clone());
        state.mark_in_use();
    });
    let wanted = receiver.clone();
    let task = cx.background_spawn(async move {
        match (wanted.airplay_device(), server) {
            (Some(device), _) => {
                cast::AirPlayOutput::connect(device.clone(), volume).map(Connected::AirPlay)
            }
            (None, Some(server)) => cast::connect(&wanted, server).map(Connected::Renderer),
            (None, None) => Err("no media server".to_string()),
        }
    });
    cx.spawn(async move |cx| {
        let result = task.await;
        cx.update(|cx| {
            let still_wanted = cx
                .global::<CastState>()
                .connecting
                .as_ref()
                .is_some_and(|connecting| connecting.id == receiver.id);
            if !still_wanted {
                return;
            }
            cx.update_global::<CastState, _>(|state, _| {
                state.connecting = None;
                state.mark_in_use();
            });
            match result {
                Ok(connected) => activate(receiver, connected, volume, cx),
                Err(e) => {
                    log::warn!("cast: connecting to {} failed: {e}", receiver.name);
                    notify(
                        cx,
                        Notification::error(cast_strings().connect_failed(&receiver.name, &e))
                            .title(cast_strings().streaming.clone()),
                    );
                }
            }
        });
    })
    .detach();
}

pub fn disconnect(cx: &mut App) {
    cx.update_global::<CastState, _>(|state, _| {
        state.connecting = None;
        state.mark_in_use();
    });
    if !cx.global::<Services>().player.is_casting() {
        return;
    }
    switch(Target::Local, None, None, cx);
}

fn activate(receiver: cast::Receiver, connected: Connected, volume: f32, cx: &mut App) {
    let services = cx.global::<Services>().clone();
    let player = services.player.clone();
    let id = player.take_id();
    let (target, volume) = match connected {
        Connected::AirPlay(output) => {
            let engine = Rc::new(AudioEngine::with_resolver(
                output.clone(),
                player.resolver.clone(),
            ));
            let opener = PlaybackOpener::new(player.backend.clone(), engine_sink(&engine));
            forward_airplay(id, &engine, &output, &player, cx);
            (
                Target::AirPlay(AirPlayTarget {
                    id,
                    engine,
                    opener,
                    output,
                }),
                Some(volume),
            )
        }
        Connected::Renderer(session) => {
            let target = RendererTarget {
                id,
                session: Arc::new(session),
                loads: Arc::new(AtomicU64::new(0)),
                duration_ms: Arc::new(AtomicU64::new(0)),
            };
            forward_session(id, &target, &player, cx);
            (Target::Renderer(target), None)
        }
    };
    log::info!("cast: playing on {} ({:?})", receiver.name, receiver.kind);
    switch(target, Some(receiver), volume, cx);
}

fn switch(target: Target, receiver: Option<cast::Receiver>, volume: Option<f32>, cx: &mut App) {
    let services = cx.global::<Services>().clone();
    let player = services.player.clone();
    let track = services.playback_queue.borrow().current_track().cloned();
    let position_ms = services.current_position_ms.load(Ordering::Relaxed);
    let playing = services.is_playing.load(Ordering::Relaxed);
    services.resume_at.set(None);
    services.resume_playing.set(false);
    player.active.store(target.id(), Ordering::Release);
    let old = player.target.replace(target);
    if matches!(old, Target::Local) {
        fade_out_local(&player, playing, cx);
    }
    old.release(false);
    services
        .is_buffering
        .store(false, std::sync::atomic::Ordering::Relaxed);
    cx.update_global::<CastState, _>(|state, _| {
        state.active = receiver;
        state.volume = volume;
        state.mark_in_use();
    });
    if let Some(track) = track {
        resume_on_target(&services, &track, position_ms, playing);
    }
    crate::services::publish_remote_state(cx);
}

fn fade_out_local(player: &Rc<Player>, playing: bool, cx: &mut App) {
    player.local_opener.cancel();
    if !playing {
        player.local.stop();
        return;
    }
    player.local.pause();
    let fade = player.local_fades.get() + 1;
    player.local_fades.set(fade);
    let player = player.clone();
    cx.spawn(async move |cx| {
        cx.background_executor().timer(LOCAL_FADE_OUT).await;
        if player.local_fades.get() == fade && player.active.load(Ordering::Acquire) != LOCAL {
            player.local.stop();
        }
    })
    .detach();
}

fn resume_on_target(services: &Services, track: &Track, position_ms: u64, playing: bool) {
    let player = &services.player;
    services
        .current_position_ms
        .store(position_ms, Ordering::Relaxed);
    if let Target::Renderer(target) = &*player.target.borrow() {
        player.start_renderer(
            target,
            CastTrack::from(track),
            Duration::from_millis(position_ms),
            playing,
        );
        return;
    }
    if position_ms == 0 {
        player.start(
            track,
            if playing {
                AfterLoad::Play
            } else {
                AfterLoad::Stay
            },
        );
        return;
    }
    services.resume_at.set(Some((track.id, position_ms)));
    services.resume_playing.set(playing);
    player.start(track, AfterLoad::Stay);
}

fn forward_airplay(
    id: u64,
    engine: &AudioEngine,
    output: &Arc<cast::AirPlayOutput>,
    player: &Player,
    cx: &mut App,
) {
    let events = engine.events();
    let forward = player.events_tx.clone();
    let active = player.active.clone();
    let pending = output.clone();
    cx.spawn(async move |_| {
        while let Ok(event) = events.recv_async().await {
            if active.load(Ordering::Acquire) != id {
                continue;
            }
            let event = match event {
                EngineEvent::PositionChanged(position) => {
                    EngineEvent::PositionChanged(position.saturating_sub(pending.pending()))
                }
                other => other,
            };
            if forward.send(event).is_err() {
                break;
            }
        }
    })
    .detach();
    let lost = output.lost();
    let name = output.name().to_string();
    cx.spawn(async move |cx| {
        if let Ok(reason) = lost.recv_async().await {
            cx.update(|cx| lose(id, &name, &reason, cx));
        }
    })
    .detach();
}

fn forward_session(id: u64, target: &RendererTarget, player: &Player, cx: &mut App) {
    let events = target.session.events();
    let forward = player.events_tx.clone();
    let active = player.active.clone();
    let duration_ms = target.duration_ms.clone();
    cx.spawn(async move |cx| {
        while let Ok(event) = events.recv_async().await {
            if active.load(Ordering::Acquire) != id {
                continue;
            }
            let mapped = match event {
                cast::SessionEvent::Loaded {
                    duration,
                    sample_rate,
                    bit_depth,
                } => {
                    let known = duration.unwrap_or_else(|| {
                        Duration::from_millis(duration_ms.load(Ordering::Acquire))
                    });
                    duration_ms.store(known.as_millis() as u64, Ordering::Release);
                    EngineEvent::Loaded {
                        params: StreamParams::new(sample_rate, ChannelCount::Stereo, bit_depth),
                        duration: known,
                    }
                }
                cast::SessionEvent::Playing => EngineEvent::Playing,
                cast::SessionEvent::Paused => EngineEvent::Paused,
                cast::SessionEvent::Buffering(buffering) => EngineEvent::Buffering(buffering),
                cast::SessionEvent::Position(position) => EngineEvent::PositionChanged(position),
                cast::SessionEvent::Ended => EngineEvent::TrackEnded,
                cast::SessionEvent::Failed(reason) => EngineEvent::Error(reason),
                cast::SessionEvent::Volume(volume) => {
                    cx.update(|cx| {
                        cx.update_global::<CastState, _>(|state, _| state.volume = Some(volume));
                    });
                    continue;
                }
                cast::SessionEvent::Lost(reason) => {
                    cx.update(|cx| {
                        let name = cx
                            .global::<CastState>()
                            .active
                            .as_ref()
                            .map(|receiver| receiver.name.clone())
                            .unwrap_or_default();
                        lose(id, &name, &reason, cx);
                    });
                    break;
                }
            };
            if forward.send(mapped).is_err() {
                break;
            }
        }
    })
    .detach();
}

fn lose(id: u64, name: &str, reason: &str, cx: &mut App) {
    let player = cx.global::<Services>().player.clone();
    if player.active.load(Ordering::Acquire) != id {
        return;
    }
    log::warn!("cast: lost {name}: {reason}");
    cx.global::<Services>()
        .is_playing
        .store(false, Ordering::Relaxed);
    switch(Target::Local, None, None, cx);
    notify(
        cx,
        Notification::warning(cast_strings().connection_lost(name))
            .title(cast_strings().streaming.clone()),
    );
}

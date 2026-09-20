use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use audio_engine::EngineEvent;
use gpui::{App, AppContext, BackgroundExecutor, Entity, Global, SharedString, Task};
use scrobble::{
    AudioscrobblerClient, CsvLog, ListenBrainzClient, Love, NowPlaying, Play, PlayAccumulator,
    Profile, Scrobble, ScrobbleHandle, ScrobbleTarget, StatusEvent, TargetId, should_scrobble,
};

use crate::library_service::LibraryEvent;
use crate::localization::tr;
use crate::scrobble_store::LibraryScrobbleStore;
use crate::services::Services;
use crate::settings_store::SettingsStore;

const HISTORY_MIN_SECS: u64 = 15;

#[derive(Default)]
pub struct ScrobbleStatus {
    pub auth_failed: HashSet<TargetId>,
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
    track_id: Option<i64>,
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
}

struct BridgeState {
    executor: BackgroundExecutor,
    handle: Option<ScrobbleHandle>,
    active: bool,
    first_artist_only: bool,
    current: Option<Current>,
    accumulator: PlayAccumulator,
    notified: HashMap<TargetId, String>,
}

pub fn setup(cx: &mut App) {
    let (status_tx, status_rx) = flume::unbounded();
    let repo = cx.global::<Services>().library.repo();
    import_legacy_queue(&repo);
    let store = Arc::new(LibraryScrobbleStore::new(repo));
    let targets = build_targets(cx);
    let active = !targets.is_empty();
    let handle = Some(ScrobbleHandle::spawn(store, targets, status_tx));

    let first_artist_only = cx.global::<SettingsStore>().scrobble().first_artist_only;
    let state = Rc::new(RefCell::new(BridgeState {
        executor: cx.background_executor().clone(),
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

pub fn finalize_on_quit(cx: &mut App) -> Option<Task<()>> {
    let state = cx
        .try_global::<ScrobbleService>()
        .map(|s| s.state.clone())?;
    let now = Instant::now();
    let mut st = state.borrow_mut();
    st.accumulator.on_pause(now);
    commit_play_blocking(&mut st, now)
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
                .with_session(session.key.clone())
                .with_loves(settings.lastfm.send_loves),
        ));
    }

    if settings.librefm.enabled
        && let Some(session) = &settings.librefm.session
    {
        targets.push(Box::new(
            AudioscrobblerClient::new(Profile::librefm())
                .with_session(session.key.clone())
                .with_loves(settings.librefm.send_loves),
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
    let now = Instant::now();
    match event {
        EngineEvent::Loaded { duration, .. } => {
            let first_artist_only = state.borrow().first_artist_only;
            let meta = read_current_meta(cx, first_artist_only);
            let is_playing = cx.global::<Services>().is_playing.load(Ordering::Relaxed);
            let mut st = state.borrow_mut();
            st.accumulator.on_pause(now);
            finish_current(&mut st, now);
            st.current = None;
            st.accumulator.reset();
            if let Some(meta) = meta.filter(|m| !m.artist.is_empty() && !m.title.is_empty()) {
                st.current = Some(Current {
                    meta,
                    duration: *duration,
                    timestamp: 0,
                    started: false,
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
            scrobble_if_qualified(&mut st, now);
        }
        EngineEvent::TrackEnded | EngineEvent::Stopped => {
            let mut st = state.borrow_mut();
            st.accumulator.on_pause(now);
            finish_current(&mut st, now);
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
    let st = state.borrow();
    if let Some(handle) = st.handle.clone() {
        let love = Love {
            track_id: Some(*track_id),
            artist,
            title,
            loved: *liked,
            at: unix_now(),
        };
        st.executor
            .spawn(async move {
                handle.love(love);
            })
            .detach();
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

fn scrobble_if_qualified(st: &mut BridgeState, now: Instant) {
    let played = st.accumulator.played(now);
    if !st.active || !should_scrobble(played, current_duration(st)) {
        return;
    }
    commit_play(st, now, Commit::Live);
}

fn finish_current(st: &mut BridgeState, now: Instant) {
    commit_play(st, now, Commit::Final);
}

#[derive(Clone, Copy)]
enum Commit {
    Live,
    Final,
}

fn current_duration(st: &BridgeState) -> Duration {
    st.current
        .as_ref()
        .map(|current| current.duration)
        .unwrap_or_default()
}

fn pending_play(current: &Current, played: Duration, active: bool, commit: Commit) -> Option<Play> {
    if !current.started || current.meta.artist.is_empty() || current.meta.title.is_empty() {
        return None;
    }
    let qualified = active && should_scrobble(played, current.duration);
    if !qualified && (matches!(commit, Commit::Live) || played.as_secs() < HISTORY_MIN_SECS) {
        return None;
    }
    Some(Play {
        track_id: current.meta.track_id,
        scrobble: Scrobble {
            artist: current.meta.artist.clone(),
            title: current.meta.title.clone(),
            album: current.meta.album.clone(),
            album_artist: current.meta.album_artist.clone(),
            track_number: current.meta.track_number,
            duration_secs: Some(current.duration.as_secs()),
            timestamp: current.timestamp,
        },
        played_secs: played.as_secs(),
        qualified,
    })
}

fn commit_play(st: &mut BridgeState, now: Instant, commit: Commit) {
    let played = st.accumulator.played(now);
    let active = st.active;
    let Some(play) = st
        .current
        .as_ref()
        .and_then(|current| pending_play(current, played, active, commit))
    else {
        return;
    };
    let Some(handle) = st.handle.clone() else {
        return;
    };
    let qualified = play.qualified;
    st.executor
        .spawn(async move {
            if qualified {
                handle.scrobble(play);
            } else {
                handle.persist(play);
            }
        })
        .detach();
}

fn commit_play_blocking(st: &mut BridgeState, now: Instant) -> Option<Task<()>> {
    let played = st.accumulator.played(now);
    let active = st.active;
    let play = st
        .current
        .as_ref()
        .and_then(|current| pending_play(current, played, active, Commit::Final))?;
    let handle = st.handle.clone()?;
    Some(st.executor.spawn(async move {
        handle.persist(play);
    }))
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
        track_id: Some(track.id),
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

fn import_legacy_queue(repo: &Arc<dyn music_library::LibraryRepository>) {
    let Some(path) = dirs::config_dir().map(|dir| dir.join("pawse").join("scrobble_queue.json"))
    else {
        return;
    };
    import_queue_file(repo, &path);
}

fn import_queue_file(repo: &Arc<dyn music_library::LibraryRepository>, path: &Path) {
    if !path.exists() {
        return;
    }
    let raw = std::fs::read_to_string(path).unwrap_or_default();
    let items = legacy_items(&raw);
    if items.is_empty() && !raw.trim().is_empty() {
        let kept = path.with_extension("json.unreadable");
        let _ = std::fs::remove_file(&kept);
        if let Err(e) = std::fs::rename(path, &kept) {
            log::warn!("scrobble: could not set the unreadable queue file aside: {e}");
        }
        return;
    }
    let kept = path.with_extension("json.imported");
    let _ = std::fs::remove_file(&kept);
    if let Err(e) = std::fs::rename(path, &kept) {
        log::error!("scrobble: refusing to import the old queue, cannot claim the file: {e}");
        return;
    }
    let (plays, loves) = import_items(repo, &items);
    log::info!("scrobble: imported {plays} play(s) and {loves} love(s) from the old queue file");
}

fn import_items(
    repo: &Arc<dyn music_library::LibraryRepository>,
    items: &[LegacyItem],
) -> (usize, usize) {
    let mut plays = 0usize;
    let mut loves = 0usize;
    for item in items {
        let targets: Vec<&str> = item
            .targets
            .iter()
            .filter_map(|key| TargetId::from_key(key).map(|target| target.key()))
            .collect();
        if targets.is_empty() {
            continue;
        }
        match &item.event {
            LegacyEvent::Scrobble(scrobble) => {
                let play = music_library::models::NewPlay {
                    track_id: None,
                    artist: scrobble.artist.clone(),
                    title: scrobble.title.clone(),
                    album: scrobble.album.clone(),
                    album_artist: scrobble.album_artist.clone(),
                    track_number: scrobble.track_number,
                    duration_secs: scrobble.duration_secs,
                    played_secs: None,
                    started_at: scrobble.timestamp,
                    qualified: true,
                };
                match repo.record_play(&play, &targets) {
                    Ok(_) => plays += 1,
                    Err(e) => log::warn!("scrobble: failed to import a queued play: {e}"),
                }
            }
            LegacyEvent::Love {
                artist,
                title,
                love,
            } => {
                let record = music_library::models::NewLove {
                    track_id: None,
                    artist: artist.clone(),
                    title: title.clone(),
                    loved: *love,
                    at: unix_now(),
                };
                match repo.record_love(&record, &targets) {
                    Ok(_) => loves += 1,
                    Err(e) => log::warn!("scrobble: failed to import a queued love: {e}"),
                }
            }
        }
    }
    (plays, loves)
}

#[derive(serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum LegacyEvent {
    Scrobble(Scrobble),
    Love {
        artist: String,
        title: String,
        love: bool,
    },
}

#[derive(serde::Deserialize)]
struct LegacyItem {
    event: LegacyEvent,
    targets: Vec<String>,
}

#[derive(serde::Deserialize)]
struct LegacyFile {
    items: Vec<serde_json::Value>,
}

fn legacy_items(raw: &str) -> Vec<LegacyItem> {
    if raw.trim().is_empty() {
        return Vec::new();
    }
    if let Ok(file) = serde_json::from_str::<LegacyFile>(raw) {
        let total = file.items.len();
        let items: Vec<LegacyItem> = file
            .items
            .into_iter()
            .filter_map(|value| serde_json::from_value::<LegacyItem>(value).ok())
            .collect();
        if items.len() < total {
            log::warn!(
                "scrobble: skipped {} unreadable item(s) in the old queue file",
                total - items.len()
            );
        }
        return items;
    }
    match serde_json::from_str::<Vec<Scrobble>>(raw) {
        Ok(legacy) => legacy
            .into_iter()
            .map(|scrobble| LegacyItem {
                event: LegacyEvent::Scrobble(scrobble),
                targets: vec!["lastfm".to_string()],
            })
            .collect(),
        Err(e) => {
            log::warn!("scrobble: the old queue file is unreadable, skipping import: {e}");
            Vec::new()
        }
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use music_library::SqliteLibrary;

    use super::*;

    fn a_current(duration_secs: u64) -> Current {
        Current {
            meta: CapturedMeta {
                track_id: Some(7),
                artist: "Tool".into(),
                title: "Pneuma".into(),
                album: None,
                album_artist: None,
                track_number: None,
            },
            duration: Duration::from_secs(duration_secs),
            timestamp: 1_700_000_000,
            started: true,
        }
    }

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn pausing_before_the_scrobble_threshold_commits_nothing() {
        let current = a_current(300);
        assert!(
            pending_play(&current, secs(30), true, Commit::Live).is_none(),
            "a 30s pause in a 300s track must not consume the play"
        );
    }

    #[test]
    fn the_same_track_finished_later_still_qualifies() {
        let current = a_current(300);
        let play = pending_play(&current, secs(300), true, Commit::Final).unwrap();
        assert!(play.qualified);
        assert_eq!(play.played_secs, 300);
    }

    #[test]
    fn a_qualified_pause_commits_early_and_the_end_commits_the_real_total() {
        let current = a_current(300);
        let early = pending_play(&current, secs(200), true, Commit::Live).unwrap();
        assert!(early.qualified);
        assert_eq!(early.played_secs, 200);

        let total = pending_play(&current, secs(300), true, Commit::Final).unwrap();
        assert_eq!(total.played_secs, 300);
        assert_eq!(
            total.scrobble.timestamp, early.scrobble.timestamp,
            "both commits must land on the same row, so the store merges them"
        );
    }

    #[test]
    fn a_click_through_is_never_recorded() {
        let current = a_current(300);
        assert!(pending_play(&current, secs(14), true, Commit::Final).is_none());
        assert!(pending_play(&current, secs(15), true, Commit::Final).is_some());
    }

    #[test]
    fn a_skip_past_the_floor_is_history_without_delivery() {
        let current = a_current(600);
        let play = pending_play(&current, secs(25), true, Commit::Final).unwrap();
        assert!(!play.qualified);
        assert_eq!(play.played_secs, 25);
    }

    #[test]
    fn history_is_recorded_with_scrobbling_switched_off() {
        let current = a_current(300);
        let play = pending_play(&current, secs(300), false, Commit::Final).unwrap();
        assert!(!play.qualified, "nothing may be queued for delivery");
        assert_eq!(play.played_secs, 300, "but the listen is still history");
    }

    #[test]
    fn a_track_that_never_started_is_ignored() {
        let mut current = a_current(300);
        current.started = false;
        assert!(pending_play(&current, secs(300), true, Commit::Final).is_none());
    }

    #[test]
    fn a_track_under_thirty_seconds_never_qualifies() {
        let current = a_current(29);
        let play = pending_play(&current, secs(29), true, Commit::Final).unwrap();
        assert!(!play.qualified);
    }

    fn repo() -> (Arc<dyn music_library::LibraryRepository>, PathBuf) {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join("pawse-scrobble-import");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!(
            "test-{}-{}.db",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_file(&path);
        (Arc::new(SqliteLibrary::open_at(&path).unwrap()), path)
    }

    #[test]
    fn a_v2_queue_file_is_imported_with_its_targets() {
        let raw = r#"{"version":2,"items":[
            {"id":1,"event":{"kind":"scrobble","artist":"Tool","title":"Pneuma",
             "album":null,"duration_secs":713,"timestamp":1700000000},
             "targets":["lastfm","listen_brainz"]},
            {"id":2,"event":{"kind":"love","artist":"Tool","title":"Pneuma","love":true},
             "targets":["lastfm"]}
        ]}"#;
        let items = legacy_items(raw);
        assert_eq!(items.len(), 2);

        let (repo, path) = repo();
        let (plays, loves) = import_items(&repo, &items);

        assert_eq!((plays, loves), (1, 1));
        assert_eq!(repo.pending_scrobble_count(&["lastfm"]).unwrap(), 2);
        assert_eq!(repo.pending_scrobble_count(&["listen_brainz"]).unwrap(), 1);
        let imported = repo.pending_plays("lastfm", 10).unwrap();
        assert_eq!(imported[0].title, "Pneuma");
        assert_eq!(imported[0].started_at, 1_700_000_000);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_v1_bare_array_is_imported_as_lastfm_plays() {
        let raw = r#"[{"artist":"Tool","title":"Pneuma","album":null,
                       "duration_secs":713,"timestamp":1700000000}]"#;
        let items = legacy_items(raw);

        let (repo, path) = repo();
        let (plays, loves) = import_items(&repo, &items);

        assert_eq!((plays, loves), (1, 0));
        assert_eq!(repo.pending_scrobble_count(&["lastfm"]).unwrap(), 1);
        assert_eq!(repo.pending_scrobble_count(&["csv_log"]).unwrap(), 0);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_unknown_target_is_dropped_without_losing_the_rest() {
        let raw = r#"{"version":2,"items":[
            {"id":1,"event":{"kind":"scrobble","artist":"A","title":"T","album":null,
             "duration_secs":180,"timestamp":1},"targets":["lastfm","maloja"]},
            {"id":2,"event":{"kind":"scrobble","artist":"B","title":"U","album":null,
             "duration_secs":180,"timestamp":2},"targets":["maloja"]}
        ]}"#;
        let (repo, path) = repo();
        let (plays, _) = import_items(&repo, &legacy_items(raw));

        assert_eq!(plays, 1);
        assert_eq!(repo.pending_scrobble_count(&["lastfm"]).unwrap(), 1);
        let _ = std::fs::remove_file(&path);
    }

    fn queue_file(contents: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join("pawse-queue-import");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!(
            "q-{}-{}.json",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        for ext in ["json.imported", "json.unreadable"] {
            let _ = std::fs::remove_file(path.with_extension(ext));
        }
        std::fs::write(&path, contents).unwrap();
        path
    }

    const ONE_ITEM: &str = r#"{"version":2,"items":[
        {"id":1,"event":{"kind":"scrobble","artist":"Tool","title":"Pneuma","album":null,
         "duration_secs":713,"timestamp":1700000000},"targets":["lastfm"]}
    ]}"#;

    #[test]
    fn importing_claims_the_file_so_a_second_launch_cannot_double_import() {
        let (repo, db) = repo();
        let path = queue_file(ONE_ITEM);

        import_queue_file(&repo, &path);
        assert_eq!(repo.pending_scrobble_count(&["lastfm"]).unwrap(), 1);
        assert!(
            !path.exists(),
            "the queue file must be claimed, not left behind"
        );
        assert!(path.with_extension("json.imported").exists());

        import_queue_file(&repo, &path);
        assert_eq!(
            repo.pending_scrobble_count(&["lastfm"]).unwrap(),
            1,
            "a second launch must not re-import the same scrobbles"
        );
        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn a_file_that_parses_to_nothing_is_not_called_imported() {
        let (repo, db) = repo();
        let path = queue_file("{not json");

        import_queue_file(&repo, &path);

        assert!(path.with_extension("json.unreadable").exists());
        assert!(
            !path.with_extension("json.imported").exists(),
            "naming a dropped file 'imported' would hide the loss"
        );
        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn one_unreadable_entry_does_not_discard_the_whole_queue() {
        let raw = r#"{"version":2,"items":[
            {"id":1,"event":{"kind":"scrobble","artist":"A","title":"T","album":null,
             "duration_secs":180,"timestamp":1},"targets":["lastfm"]},
            {"id":2,"event":{"kind":"nonsense"},"targets":["lastfm"]},
            {"id":3,"event":{"kind":"scrobble","artist":"B","title":"U","album":null,
             "duration_secs":180,"timestamp":2},"targets":["lastfm"]}
        ]}"#;
        let items = legacy_items(raw);
        assert_eq!(
            items.len(),
            2,
            "the two good items must survive the bad one"
        );
    }

    #[test]
    fn an_empty_or_corrupt_queue_file_imports_nothing() {
        assert!(legacy_items("").is_empty());
        assert!(legacy_items("   ").is_empty());
        assert!(legacy_items("{not json").is_empty());
    }
}

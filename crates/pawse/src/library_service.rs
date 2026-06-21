use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use music_indexer::{PreparedTrack, ScanEvent};
use music_library::{LibraryRepository, LyricsRef, PlaylistTrackRef, ScanTrack, SqliteLibrary};

const SCAN_DEBOUNCE: Duration = Duration::from_secs(2);

#[derive(Default)]
struct ScanState {
    scanning: AtomicBool,
    pending: AtomicBool,
    manual: AtomicBool,
    debounce_gen: AtomicU64,
    folders: Mutex<Vec<PathBuf>>,
}

#[derive(Clone, Debug)]
pub enum LibraryEvent {
    ScanStarted,
    ScanProgress {
        scanned: usize,
    },
    /// `changed` is false on the fast path (library unchanged, no DB work).
    ScanComplete {
        changed: bool,
    },
    ScanUpToDate,
    ScanSucceeded,
    ScanFailed,
    TrackLikedChanged {
        track_id: i64,
        liked: bool,
    },
    PlaylistsChanged,
    PlaylistTracksChanged {
        playlist_id: i64,
    },
    QueueChanged,
    LyricsChanged {
        track_id: i64,
    },
}

pub struct LibraryService {
    repo: Arc<dyn LibraryRepository>,
    event_tx: flume::Sender<LibraryEvent>,
    executor: gpui::BackgroundExecutor,
    scan_state: Arc<ScanState>,
}

impl LibraryService {
    pub fn new(event_tx: flume::Sender<LibraryEvent>, executor: gpui::BackgroundExecutor) -> Self {
        let repo = Arc::new(SqliteLibrary::open().expect("open library db"));
        Self {
            repo,
            event_tx,
            executor,
            scan_state: Arc::new(ScanState::default()),
        }
    }

    pub fn albums(&self) -> Vec<music_library::AlbumSummary> {
        self.repo.albums().unwrap_or_default()
    }

    pub fn tracks_for_album(&self, album_id: i64) -> Vec<music_library::Track> {
        self.repo.tracks_for_album(album_id).unwrap_or_default()
    }

    pub fn album_track_counts(&self) -> HashMap<i64, i64> {
        self.repo.album_track_counts().unwrap_or_default()
    }

    pub fn has_tracks(&self) -> bool {
        self.repo.has_tracks().unwrap_or(false)
    }

    pub fn album_search_entries(&self) -> Vec<music_library::AlbumSearchEntry> {
        self.repo.album_search_entries().unwrap_or_default()
    }

    pub fn track_artists(&self, track_id: i64) -> Vec<String> {
        self.repo.track_artists(track_id).unwrap_or_default()
    }

    pub fn track_artists_with_ids(&self, track_id: i64) -> Vec<(i64, String)> {
        self.repo
            .track_artists_with_ids(track_id)
            .unwrap_or_default()
    }

    pub fn unique_track_artists(&self, track_id: i64) -> Vec<(i64, String)> {
        let mut seen = std::collections::HashSet::new();
        self.track_artists_with_ids(track_id)
            .into_iter()
            .filter(|(id, _)| seen.insert(*id))
            .collect()
    }

    pub fn track_artists_map(&self, track_ids: &[i64]) -> HashMap<i64, Vec<String>> {
        self.repo.track_artists_map(track_ids).unwrap_or_default()
    }

    pub fn artists(&self) -> Vec<music_library::ArtistSummary> {
        self.repo.artists().unwrap_or_default()
    }

    pub fn artist_album_covers(&self) -> HashMap<i64, Vec<i64>> {
        self.repo.artist_album_covers().unwrap_or_default()
    }

    pub fn tracks_by_artist(&self, artist_id: i64) -> Vec<music_library::Track> {
        self.repo.tracks_by_artist(artist_id).unwrap_or_default()
    }

    pub fn liked_tracks(&self) -> Vec<music_library::Track> {
        self.repo.liked_tracks().unwrap_or_default()
    }

    pub fn all_tracks(&self) -> Vec<music_library::Track> {
        self.repo.all_tracks().unwrap_or_default()
    }

    pub fn track_count(&self) -> i64 {
        self.repo.track_count().unwrap_or(0)
    }

    pub fn set_liked(&self, track_id: i64, liked: bool) {
        if let Err(e) = self.repo.set_liked(track_id, liked) {
            log::error!("Failed to set liked for track {}: {}", track_id, e);
            return;
        }
        let _ = self
            .event_tx
            .send(LibraryEvent::TrackLikedChanged { track_id, liked });
    }

    pub fn lyrics_for_track(&self, track_id: i64) -> Option<music_library::StoredLyrics> {
        self.repo.lyrics_for_track(track_id).unwrap_or_default()
    }

    pub fn save_lyrics(&self, track_id: i64, text: String, synced: bool, source: &str) {
        if let Err(e) = self.repo.upsert_lyrics(track_id, &text, synced, source) {
            log::error!("Failed to save lyrics for track {}: {}", track_id, e);
            return;
        }
        let _ = self.event_tx.send(LibraryEvent::LyricsChanged { track_id });
    }

    pub fn is_multitrack_file(&self, path: &str) -> bool {
        self.repo.track_count_for_path(path).unwrap_or(0) > 1
    }

    pub fn save_lyrics_file(
        &self,
        track_id: i64,
        audio_path: PathBuf,
        text: String,
        synced: bool,
    ) {
        let repo = self.repo.clone();
        let event_tx = self.event_tx.clone();
        self.executor
            .spawn(async move {
                let lrc_path = audio_path.with_extension("lrc");
                if let Err(e) = std::fs::write(&lrc_path, &text) {
                    log::error!("Failed to write lyrics file {:?}: {}", lrc_path, e);
                    return;
                }
                // why: don't re-baseline the scan fingerprint — collect_sources hashes the whole tree, so a not-yet-indexed change would be absorbed and skipped by the next rescan
                if let Err(e) = repo.upsert_lyrics(track_id, &text, synced, "lrc") {
                    log::error!(
                        "Failed to update lyrics after export for {}: {}",
                        track_id,
                        e
                    );
                } else {
                    let _ = event_tx.send(LibraryEvent::LyricsChanged { track_id });
                }
            })
            .detach();
    }

    pub fn playlists(&self) -> Vec<music_library::PlaylistSummary> {
        self.repo.playlists().unwrap_or_default()
    }

    pub fn tracks_for_playlist(&self, playlist_id: i64) -> Vec<music_library::Track> {
        self.repo
            .tracks_for_playlist(playlist_id)
            .unwrap_or_default()
    }

    pub fn playlists_containing_track(&self, track_id: i64) -> Vec<i64> {
        self.repo
            .playlists_containing_track(track_id)
            .unwrap_or_default()
    }

    pub fn tracks_by_keys(&self, keys: &[(String, i32)]) -> Vec<music_library::Track> {
        self.repo.tracks_by_keys(keys).unwrap_or_default()
    }

    pub fn create_playlist(&self, name: &str) -> Option<i64> {
        match self.repo.create_playlist(name) {
            Ok(id) => {
                let _ = self.event_tx.send(LibraryEvent::PlaylistsChanged);
                Some(id)
            }
            Err(e) => {
                log::error!("Failed to create playlist: {}", e);
                None
            }
        }
    }

    pub fn delete_playlist(&self, playlist_id: i64) {
        if let Err(e) = self.repo.delete_playlist(playlist_id) {
            log::error!("Failed to delete playlist {}: {}", playlist_id, e);
            return;
        }
        let _ = self.event_tx.send(LibraryEvent::PlaylistsChanged);
    }

    pub fn add_track_to_playlist(&self, playlist_id: i64, track_id: i64) {
        if let Err(e) = self.repo.add_track_to_playlist(playlist_id, track_id) {
            log::error!(
                "Failed to add track {} to playlist {}: {}",
                track_id,
                playlist_id,
                e
            );
            return;
        }
        let _ = self.event_tx.send(LibraryEvent::PlaylistsChanged);
        let _ = self
            .event_tx
            .send(LibraryEvent::PlaylistTracksChanged { playlist_id });
    }

    pub fn remove_track_from_playlist(&self, playlist_id: i64, track_id: i64) {
        if let Err(e) = self.repo.remove_track_from_playlist(playlist_id, track_id) {
            log::error!(
                "Failed to remove track {} from playlist {}: {}",
                track_id,
                playlist_id,
                e
            );
            return;
        }
        let _ = self.event_tx.send(LibraryEvent::PlaylistsChanged);
        let _ = self
            .event_tx
            .send(LibraryEvent::PlaylistTracksChanged { playlist_id });
    }

    pub fn move_track_in_playlist(&self, playlist_id: i64, from: usize, to: usize) {
        if let Err(e) = self.repo.move_track_in_playlist(playlist_id, from, to) {
            log::error!(
                "Failed to move track from {} to {} in playlist {}: {}",
                from,
                to,
                playlist_id,
                e
            );
            return;
        }
        let _ = self
            .event_tx
            .send(LibraryEvent::PlaylistTracksChanged { playlist_id });
    }

    pub fn move_liked_track(&self, from: usize, to: usize) {
        if let Err(e) = self.repo.move_liked_track(from, to) {
            log::error!(
                "Failed to reorder liked track from {} to {}: {}",
                from,
                to,
                e
            );
        }
    }

    pub fn album_title(&self, album_id: i64) -> Option<String> {
        self.repo.album_title(album_id).ok().flatten()
    }

    pub fn album_genres(&self, album_id: i64) -> Vec<String> {
        self.repo.album_genres(album_id).unwrap_or_default()
    }

    pub fn album_genres_map(&self) -> std::collections::HashMap<i64, Vec<String>> {
        self.repo.album_genres_map().unwrap_or_default()
    }

    pub fn get_cover_art_small(&self, id: i64) -> Option<Vec<u8>> {
        self.repo.get_cover_art_small(id).ok().flatten()
    }

    pub fn get_cover_art_large(&self, id: i64) -> Option<Vec<u8>> {
        self.repo.get_cover_art_large(id).ok().flatten()
    }

    pub fn get_cover_art_source(&self, id: i64) -> Option<(String, bool)> {
        self.repo.get_cover_art_source(id).ok().flatten()
    }

    pub fn get_cover_art_path_for_media(&self, id: i64) -> Option<std::path::PathBuf> {
        let bytes = self.repo.get_cover_art_large(id).ok()??;
        let temp_dir = std::env::temp_dir().join("pawse-artwork");
        std::fs::create_dir_all(&temp_dir).ok()?;
        let path = temp_dir.join(format!("{}.jpg", id));
        if let Ok(entries) = std::fs::read_dir(&temp_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p != path && p.extension().and_then(|e| e.to_str()) == Some("jpg") {
                    let _ = std::fs::remove_file(&p);
                }
            }
        }
        std::fs::write(&path, &bytes).ok()?;
        Some(path)
    }

    pub fn is_scanning(&self) -> bool {
        self.scan_state.scanning.load(Ordering::Acquire)
    }

    pub fn clear_and_rescan(&self, paths: Vec<PathBuf>) {
        self.request_rescan(paths, true, false);
    }

    pub fn request_rescan(&self, folders: Vec<PathBuf>, force: bool, manual: bool) {
        *self.scan_state.folders.lock().unwrap() = folders;
        if manual {
            self.scan_state.manual.store(true, Ordering::Release);
        }
        let generation = self.scan_state.debounce_gen.fetch_add(1, Ordering::AcqRel) + 1;

        if force {
            Self::spawn_scan(
                self.repo.clone(),
                self.event_tx.clone(),
                self.executor.clone(),
                self.scan_state.clone(),
            );
            return;
        }

        let state = self.scan_state.clone();
        let repo = self.repo.clone();
        let event_tx = self.event_tx.clone();
        let executor = self.executor.clone();
        self.executor
            .spawn(async move {
                executor.timer(SCAN_DEBOUNCE).await;
                if state.debounce_gen.load(Ordering::Acquire) != generation {
                    return;
                }
                Self::spawn_scan(repo, event_tx, executor.clone(), state);
            })
            .detach();
    }

    fn spawn_scan(
        repo: Arc<dyn LibraryRepository>,
        event_tx: flume::Sender<LibraryEvent>,
        executor: gpui::BackgroundExecutor,
        state: Arc<ScanState>,
    ) {
        if state.scanning.swap(true, Ordering::AcqRel) {
            state.pending.store(true, Ordering::Release);
            return;
        }

        let task_executor = executor.clone();
        executor
            .spawn(async move {
                loop {
                    let folders = state.folders.lock().unwrap().clone();
                    let manual = state.manual.swap(false, Ordering::AcqRel);
                    Self::run_scan(
                        repo.clone(),
                        event_tx.clone(),
                        task_executor.clone(),
                        folders,
                        manual,
                    )
                    .await;

                    if state.pending.swap(false, Ordering::AcqRel) {
                        continue;
                    }
                    state.scanning.store(false, Ordering::Release);
                    if state.pending.load(Ordering::Acquire)
                        && !state.scanning.swap(true, Ordering::AcqRel)
                    {
                        continue;
                    }
                    break;
                }
            })
            .detach();
    }

    async fn run_scan(
        repo: Arc<dyn LibraryRepository>,
        event_tx: flume::Sender<LibraryEvent>,
        inner_executor: gpui::BackgroundExecutor,
        paths: Vec<PathBuf>,
        manual: bool,
    ) {
        // Cheap walk + fingerprint. Fast path: if nothing on disk changed
        // since the last successful scan, skip all DB work entirely. This
        // is what makes run-on-launch / background rescans viable.
        let sources = music_indexer::collect_sources(&paths);
        let folders_key = serialize_folders(&paths);
        let unchanged = matches!(repo.scan_fingerprint(), Ok(Some(fp)) if fp == sources.fingerprint)
            && matches!(repo.scan_folders(), Ok(Some(f)) if f == folders_key);
        if unchanged {
            let _ = event_tx.send(LibraryEvent::ScanComplete { changed: false });
            if manual {
                let _ = event_tx.send(LibraryEvent::ScanUpToDate);
            }
            return;
        }

        let _ = event_tx.send(LibraryEvent::ScanStarted);
        let fingerprint = sources.fingerprint.clone();

        // Snapshot playlist memberships by (path, start_offset_ms) before
        // the clear wipes the `tracks` table — rescanned tracks get fresh
        // ids, so without this the playlist contents would silently
        // disappear from the user's library.
        let playlist_refs = repo.playlist_track_refs().unwrap_or_else(|e| {
            log::error!("Failed to snapshot playlist tracks: {}", e);
            Vec::new()
        });

        // Network-fetched lyrics aren't on disk, so the rescan can't re-read
        // them; snapshot them by content key and restore after, or they'd be
        // cascade-deleted with the tracks row on every rescan.
        let lyrics_refs = repo.lyrics_refs().unwrap_or_else(|e| {
            log::error!("Failed to snapshot lyrics: {}", e);
            Vec::new()
        });

        // Covers survive clear(); hand the pipeline their hashes so it skips
        // regenerating thumbnails that already exist.
        let known_hashes: HashSet<String> = repo
            .cover_art_hashes()
            .map(|pairs| pairs.into_iter().map(|(hash, _)| hash).collect())
            .unwrap_or_default();

        let mut session = match repo.open_scan_session() {
            Ok(session) => session,
            Err(e) => {
                log::error!("Failed to open scan session: {}", e);
                let _ = event_tx.send(LibraryEvent::ScanComplete { changed: false });
                let _ = event_tx.send(LibraryEvent::ScanFailed);
                return;
            }
        };
        if let Err(e) = session.clear() {
            log::error!("Failed to clear library: {}", e);
            let _ = event_tx.send(LibraryEvent::ScanComplete { changed: false });
            let _ = event_tx.send(LibraryEvent::ScanFailed);
            return;
        }

        if paths.is_empty() {
            let ok = match session.finish() {
                Ok(()) => {
                    finalize_rescan(
                        &*repo,
                        &playlist_refs,
                        &lyrics_refs,
                        &fingerprint,
                        &folders_key,
                    );
                    true
                }
                Err(e) => {
                    log::error!("Failed to finish scan session: {}", e);
                    false
                }
            };
            let _ = event_tx.send(LibraryEvent::ScanComplete { changed: true });
            let _ = event_tx.send(scan_outcome(ok));
            return;
        }

        // Run the parallel pipeline on a background pool worker; consume
        // its events here and feed the batched writer. The bounded channel
        // applies backpressure so cover bytes don't pile up in memory.
        // (The pipeline's own parse workers are dedicated threads — the
        // indexer worker pool carve-out.)
        let (scan_tx, scan_rx) = flume::bounded(512);
        inner_executor
            .spawn(async move {
                music_indexer::run(sources, known_hashes, scan_tx);
            })
            .detach();

        loop {
            match scan_rx.recv_async().await {
                Ok(ScanEvent::Cover {
                    hash,
                    small,
                    large,
                    source_path,
                    embedded,
                }) => {
                    if let Err(e) = session.add_cover(&hash, small, large, &source_path, embedded) {
                        log::error!("Failed to insert cover art: {}", e);
                    }
                }
                Ok(ScanEvent::Track(track)) => {
                    if let Err(e) = session.add_track(to_scan_track(track)) {
                        log::error!("Failed to insert track: {}", e);
                    }
                }
                Ok(ScanEvent::Progress { scanned }) => {
                    let _ = event_tx.send(LibraryEvent::ScanProgress { scanned });
                }
                Ok(ScanEvent::Error { path, error }) => {
                    log::error!("Scan error for {}: {}", path.display(), error);
                }
                Ok(ScanEvent::Complete) => break,
                Err(_) => break, // pipeline gone
            }
        }

        // Only finalize (and record the fingerprint) if the final commit
        // succeeded. Otherwise the fast path would lock in a partially
        // written library and never rescan to repair it.
        let ok = match session.finish() {
            Ok(()) => {
                finalize_rescan(
                    &*repo,
                    &playlist_refs,
                    &lyrics_refs,
                    &fingerprint,
                    &folders_key,
                );
                true
            }
            Err(e) => {
                log::error!("Failed to finish scan session: {}", e);
                false
            }
        };
        let _ = event_tx.send(LibraryEvent::ScanComplete { changed: true });
        let _ = event_tx.send(scan_outcome(ok));
    }
}

fn scan_outcome(ok: bool) -> LibraryEvent {
    if ok {
        LibraryEvent::ScanSucceeded
    } else {
        LibraryEvent::ScanFailed
    }
}

/// Serialize the scanned folder set into a stable key, so a fast-path skip only
/// happens when the same folders are being scanned as last time.
fn serialize_folders(paths: &[PathBuf]) -> String {
    let mut items: Vec<String> = paths
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    items.sort();
    items.join("\n")
}

fn to_scan_track(track: PreparedTrack) -> ScanTrack {
    ScanTrack {
        path: track.path.to_string_lossy().into_owned(),
        title: track.title,
        album_title: track.album_title,
        artist_names: track.artist_names,
        album_artist_names: track.album_artist_names,
        track_number: track.track_number,
        disc_number: track.disc_number,
        year: track.year,
        genres: track.genres,
        duration_ms: track.duration_ms,
        cover_hash: track.cover_hash,
        start_offset_ms: track.start_offset_ms,
        bitrate: track.bitrate,
        lyrics: track.lyrics.map(|l| music_library::ScanLyrics {
            text: l.text,
            synced: l.synced,
            source: l.source.as_str().to_string(),
        }),
    }
}

/// Post-scan cleanup, run on the main connection after the writer connection is
/// dropped: re-link playlists by content key, drop orphaned albums/artists/
/// covers, and record the fingerprint that future fast-path checks compare to.
fn finalize_rescan(
    repo: &dyn LibraryRepository,
    playlist_refs: &[PlaylistTrackRef],
    lyrics_refs: &[LyricsRef],
    fingerprint: &str,
    folders_key: &str,
) {
    if let Err(e) = repo.restore_playlist_track_refs(playlist_refs) {
        log::error!("Failed to restore playlist tracks: {}", e);
    }
    if let Err(e) = repo.restore_lyrics_refs(lyrics_refs) {
        log::error!("Failed to restore lyrics: {}", e);
    }
    if let Err(e) = repo.delete_orphaned_albums_and_artists() {
        log::error!("Failed to clean up orphaned cover art: {}", e);
    }
    if let Err(e) = repo.set_scan_meta(fingerprint, folders_key) {
        log::error!("Failed to store scan fingerprint: {}", e);
    }
    if let Err(e) = repo.vacuum() {
        log::error!("Failed to vacuum library: {}", e);
    }
}

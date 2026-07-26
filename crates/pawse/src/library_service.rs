use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use music_indexer::{PreparedTrack, ScanEvent};
use music_library::{
    LibraryRepository, LyricsRef, NewTrack, PlaylistTrackRef, ScanTrack, SqliteLibrary,
};

/// The album-level fields of a tag edit, applied to every track of one album.
/// Kept separate from [`tag_writer::TrackTagEdits`] because these describe a
/// shared `albums` row: writing them into a single file would leave the album's
/// other files disagreeing, and the next full rescan would undo the edit.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AlbumTagEdits {
    pub album: Option<String>,
    pub album_artists: Vec<String>,
    pub year: Option<i32>,
    pub genres: Vec<String>,
}

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
    PlaybackModeChanged,
    LyricsChanged {
        track_id: i64,
    },
    TrackTagsChanged {
        track_id: i64,
    },
    AlbumTagsChanged {
        album_id: i64,
    },
}

pub struct LibraryService {
    repo: Arc<dyn LibraryRepository>,
    event_tx: flume::Sender<LibraryEvent>,
    executor: gpui::BackgroundExecutor,
    scan_state: Arc<ScanState>,
}

/// A cloneable handle bundling the repo + event sender, so the lyrics view can run
/// its DB reads/writes on a background thread instead of blocking the render thread.
#[derive(Clone)]
pub struct LyricsAccess {
    repo: Arc<dyn LibraryRepository>,
    event_tx: flume::Sender<LibraryEvent>,
}

#[derive(Clone)]
pub struct LibraryAccess {
    repo: Arc<dyn LibraryRepository>,
}

impl pawse_remote::LibraryReader for LibraryAccess {
    fn cover(&self, id: i64, size: pawse_remote::CoverSize) -> Option<Vec<u8>> {
        let result = match size {
            pawse_remote::CoverSize::Small => self.repo.get_cover_art_small(id),
            pawse_remote::CoverSize::Large => self.repo.get_cover_art_large(id),
        };
        result.ok().flatten()
    }

    fn cover_original(&self, id: i64) -> Option<(Vec<u8>, String)> {
        let source = self.repo.get_cover_art_source(id).ok().flatten();
        let track_path = self.repo.get_track_path_for_cover(id).ok().flatten();
        let bytes = music_indexer::metadata::load_cover_from_source(source, track_path.as_deref())?;
        Some(transcode_web_cover(bytes))
    }

    fn artists(&self) -> Vec<pawse_remote::ArtistEntry> {
        let covers = self.repo.artist_album_covers().unwrap_or_default();
        self.repo
            .artists()
            .unwrap_or_default()
            .into_iter()
            .map(|artist| pawse_remote::ArtistEntry {
                id: artist.id,
                name: artist.name,
                track_count: artist.track_count,
                cover_ids: covers
                    .get(&artist.id)
                    .into_iter()
                    .flatten()
                    .take(4)
                    .copied()
                    .collect(),
            })
            .collect()
    }

    fn artist_detail(&self, artist_id: i64, full: bool) -> Option<pawse_remote::ArtistDetail> {
        let name = if artist_id == music_library::NO_METADATA_ARTIST_ID {
            String::new()
        } else {
            self.repo.artist_name(artist_id).ok().flatten()?
        };
        let base = self.repo.tracks_by_artist(artist_id).unwrap_or_default();
        let partial = artist_partial_albums(&*self.repo, &base);
        let tracks = if full {
            expand_partial_albums(&*self.repo, base, &partial)
        } else {
            base
        };
        Some(pawse_remote::ArtistDetail {
            id: artist_id,
            name,
            has_partial: !partial.is_empty(),
            albums: group_artist_albums(&*self.repo, &tracks, &partial),
        })
    }

    fn playlists(&self) -> Vec<pawse_remote::PlaylistEntry> {
        self.repo
            .playlists()
            .unwrap_or_default()
            .into_iter()
            .map(|p| pawse_remote::PlaylistEntry {
                id: p.id,
                name: p.name,
                track_count: p.track_count,
            })
            .collect()
    }

    fn playlist_detail(&self, playlist_id: i64) -> Option<pawse_remote::PlaylistDetail> {
        let name = self
            .repo
            .playlists()
            .unwrap_or_default()
            .into_iter()
            .find(|p| p.id == playlist_id)?
            .name;
        let tracks = self
            .repo
            .tracks_for_playlist(playlist_id)
            .unwrap_or_default();
        Some(pawse_remote::PlaylistDetail {
            id: playlist_id,
            name,
            tracks: to_playlist_tracks(&*self.repo, tracks),
        })
    }

    fn liked(&self) -> Vec<pawse_remote::PlaylistTrack> {
        let tracks = self.repo.liked_tracks().unwrap_or_default();
        to_playlist_tracks(&*self.repo, tracks)
    }
}

fn to_playlist_tracks(
    repo: &dyn LibraryRepository,
    tracks: Vec<music_library::Track>,
) -> Vec<pawse_remote::PlaylistTrack> {
    let ids: Vec<i64> = tracks.iter().map(|t| t.id).collect();
    let artists = repo.track_artists_map(&ids).unwrap_or_default();
    tracks
        .into_iter()
        .map(|t| pawse_remote::PlaylistTrack {
            id: t.id,
            title: t.title,
            artist: artists.get(&t.id).and_then(|a| a.first().cloned()),
            cover_id: t.cover_art_id,
            duration_ms: t.duration_ms.unwrap_or(0).max(0) as u64,
        })
        .collect()
}

fn artist_partial_albums(
    repo: &dyn LibraryRepository,
    tracks: &[music_library::Track],
) -> HashSet<i64> {
    let totals = repo.album_track_counts().unwrap_or_default();
    let mut counts: HashMap<i64, i64> = HashMap::new();
    for track in tracks {
        if let Some(album_id) = track.album_id {
            *counts.entry(album_id).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .filter(|(album_id, mine)| totals.get(album_id).copied().unwrap_or(0) > *mine)
        .map(|(album_id, _)| album_id)
        .collect()
}

fn expand_partial_albums(
    repo: &dyn LibraryRepository,
    tracks: Vec<music_library::Track>,
    partial: &HashSet<i64>,
) -> Vec<music_library::Track> {
    if partial.is_empty() {
        return tracks;
    }
    let mut combined = Vec::new();
    let mut i = 0;
    while i < tracks.len() {
        let album_id = tracks[i].album_id;
        let mut j = i;
        while j < tracks.len() && tracks[j].album_id == album_id {
            j += 1;
        }
        match album_id {
            Some(aid) if partial.contains(&aid) => {
                combined.extend(repo.tracks_for_album(aid).unwrap_or_default())
            }
            _ => combined.extend_from_slice(&tracks[i..j]),
        }
        i = j;
    }
    combined
}

pub fn artist_display_tracks(
    repo: &dyn LibraryRepository,
    artist_id: i64,
    full: bool,
) -> Vec<music_library::Track> {
    let base = repo.tracks_by_artist(artist_id).unwrap_or_default();
    if !full {
        return base;
    }
    let partial = artist_partial_albums(repo, &base);
    expand_partial_albums(repo, base, &partial)
}

fn group_artist_albums(
    repo: &dyn LibraryRepository,
    tracks: &[music_library::Track],
    partial: &HashSet<i64>,
) -> Vec<pawse_remote::ArtistAlbum> {
    let mut albums: Vec<pawse_remote::ArtistAlbum> = Vec::new();
    for track in tracks {
        let item = pawse_remote::AlbumTrack {
            id: track.id,
            title: track.title.clone(),
            track_number: track.track_number,
            disc_number: track.disc_number,
            duration_ms: track.duration_ms.unwrap_or(0).max(0) as u64,
        };
        if let Some(last) = albums.last_mut()
            && last.album_id == track.album_id
        {
            last.tracks.push(item);
            continue;
        }
        let title = track
            .album_id
            .and_then(|id| repo.album_title(id).ok().flatten())
            .unwrap_or_default();
        albums.push(pawse_remote::ArtistAlbum {
            album_id: track.album_id,
            title,
            year: track.year,
            cover_id: track.cover_art_id,
            partial: track
                .album_id
                .map(|id| partial.contains(&id))
                .unwrap_or(false),
            tracks: vec![item],
        });
    }
    albums
}

impl LyricsAccess {
    pub fn stored(&self, track_id: i64) -> Option<music_library::StoredLyrics> {
        match self.repo.lyrics_for_track(track_id) {
            Ok(stored) => stored,
            Err(e) => {
                log::error!("Failed to read lyrics for track {}: {}", track_id, e);
                None
            }
        }
    }

    pub fn first_artist(&self, track_id: i64) -> Option<String> {
        self.repo
            .track_artists(track_id)
            .unwrap_or_default()
            .into_iter()
            .next()
    }

    pub fn album_title(&self, album_id: i64) -> Option<String> {
        self.repo.album_title(album_id).ok().flatten()
    }

    /// Returns whether the row was written and a `LyricsChanged` emitted, so the
    /// caller knows a reload will render (vs. a write failure it must handle).
    pub fn save(&self, track_id: i64, text: &str, source: &str) -> bool {
        if let Err(e) = self.repo.upsert_lyrics(track_id, text, source, false) {
            log::error!("Failed to save lyrics for track {}: {}", track_id, e);
            return false;
        }
        let _ = self.event_tx.send(LibraryEvent::LyricsChanged { track_id });
        true
    }

    pub fn mark_not_found(&self, track_id: i64) -> bool {
        if let Err(e) =
            self.repo
                .upsert_lyrics(track_id, "", music_library::lyrics_source::LRCLIB, true)
        {
            log::error!(
                "Failed to mark lyrics not-found for track {}: {}",
                track_id,
                e
            );
            return false;
        }
        let _ = self.event_tx.send(LibraryEvent::LyricsChanged { track_id });
        true
    }
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

    pub fn track(&self, id: i64) -> Option<music_library::Track> {
        self.repo.track(id).unwrap_or_default()
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

    pub fn lyrics_access(&self) -> LyricsAccess {
        LyricsAccess {
            repo: self.repo.clone(),
            event_tx: self.event_tx.clone(),
        }
    }

    pub fn library_access(&self) -> LibraryAccess {
        LibraryAccess {
            repo: self.repo.clone(),
        }
    }

    pub fn artist_display_tracks(&self, artist_id: i64, full: bool) -> Vec<music_library::Track> {
        artist_display_tracks(&*self.repo, artist_id, full)
    }

    pub fn save_lyrics_file(
        &self,
        track_id: i64,
        audio_path: PathBuf,
        text: String,
        folders: Vec<PathBuf>,
    ) {
        let repo = self.repo.clone();
        let event_tx = self.event_tx.clone();
        self.executor
            .spawn(async move {
                let folders_key = serialize_folders(&folders);
                // why: snapshot disk state before our write so we only re-baseline when our .lrc is the sole delta — otherwise advancing the fingerprint would absorb an unrelated, not-yet-indexed change
                let up_to_date = {
                    let pre = music_indexer::collect_sources(&folders).fingerprint;
                    matches!(repo.scan_fingerprint(), Ok(Some(fp)) if fp == pre)
                        && matches!(repo.scan_folders(), Ok(Some(f)) if f == folders_key)
                };

                let lrc_path = audio_path.with_extension("lrc");
                if let Err(e) = std::fs::write(&lrc_path, &text) {
                    log::error!("Failed to write lyrics file {:?}: {}", lrc_path, e);
                    return;
                }
                if let Err(e) =
                    repo.upsert_lyrics(track_id, &text, music_library::lyrics_source::LRC, false)
                {
                    log::error!(
                        "Failed to update lyrics after export for {}: {}",
                        track_id,
                        e
                    );
                } else {
                    let _ = event_tx.send(LibraryEvent::LyricsChanged { track_id });
                }

                if up_to_date {
                    let post = music_indexer::collect_sources(&folders).fingerprint;
                    if let Err(e) = repo.set_scan_meta(&post, &folders_key) {
                        log::error!(
                            "Failed to re-baseline scan fingerprint after lyrics export: {}",
                            e
                        );
                    }
                }
            })
            .detach();
    }

    pub fn update_track_tags(
        &self,
        track_id: i64,
        edits: tag_writer::TrackTagEdits,
        folders: Vec<PathBuf>,
    ) {
        let repo = self.repo.clone();
        let event_tx = self.event_tx.clone();
        let executor = self.executor.clone();
        let scan_state = self.scan_state.clone();
        self.executor
            .spawn(async move {
                let Ok(Some(track)) = repo.track(track_id) else {
                    log::error!("Tag edit requested for unknown track {}", track_id);
                    return;
                };
                let baseline = ScanBaseline::capture(&*repo, &folders);
                let path = PathBuf::from(&track.path);

                match apply_track_tags(&*repo, track_id, &path, &edits) {
                    Ok(()) => {}
                    Err(TagEditFailure::Refused(reason)) => {
                        log::error!("Refusing to write tags for {}: {}", track.path, reason);
                        return;
                    }
                    Err(TagEditFailure::Write(e)) => {
                        report_tag_error(&track.path, &e);
                        return;
                    }
                    Err(TagEditFailure::Reindex(e)) => {
                        log::error!("Failed to re-index {} after tag write: {}", track.path, e);
                        force_rescan(repo, event_tx, executor, scan_state, folders);
                        return;
                    }
                }

                let _ = event_tx.send(LibraryEvent::TrackTagsChanged { track_id });
                baseline.rebaseline(&*repo, &folders);
            })
            .detach();
    }

    pub fn update_album_tags(&self, album_id: i64, edits: AlbumTagEdits, folders: Vec<PathBuf>) {
        let repo = self.repo.clone();
        let event_tx = self.event_tx.clone();
        let executor = self.executor.clone();
        let scan_state = self.scan_state.clone();
        self.executor
            .spawn(async move {
                let tracks: Vec<music_library::Track> = repo
                    .tracks_for_album(album_id)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|t| !t.is_cue)
                    .collect();
                if tracks.is_empty() {
                    log::error!("Album tag edit for {} has no editable tracks", album_id);
                    return;
                }

                let baseline = ScanBaseline::capture(&*repo, &folders);
                let written = match apply_album_tags(&*repo, &tracks, &edits) {
                    Ok(written) => written,
                    Err((path, e)) => {
                        log::error!("Failed to update tags for {}: {}", path, e);
                        report_tag_error(&path, &e);
                        force_rescan(repo, event_tx, executor, scan_state, folders);
                        return;
                    }
                };

                log::info!("Album {} retagged, {} files written", album_id, written);
                let _ = event_tx.send(LibraryEvent::AlbumTagsChanged { album_id });
                if written > 0 {
                    baseline.rebaseline(&*repo, &folders);
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

    pub fn album_artists(&self, album_id: i64) -> Vec<String> {
        self.repo.album_artists(album_id).unwrap_or_default()
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

/// Whether the library matched the disk *before* we wrote to it, captured so the
/// fingerprint is only advanced when our own write is the sole delta. Advancing it
/// otherwise would absorb an unrelated, not-yet-indexed change and the fast path
/// would then skip it forever.
struct ScanBaseline {
    up_to_date: bool,
    folders_key: String,
}

impl ScanBaseline {
    fn capture(repo: &dyn LibraryRepository, folders: &[PathBuf]) -> Self {
        let folders_key = serialize_folders(folders);
        let pre = music_indexer::collect_sources(folders).fingerprint;
        let up_to_date = matches!(repo.scan_fingerprint(), Ok(Some(fp)) if fp == pre)
            && matches!(repo.scan_folders(), Ok(Some(f)) if f == folders_key);
        Self {
            up_to_date,
            folders_key,
        }
    }

    fn rebaseline(&self, repo: &dyn LibraryRepository, folders: &[PathBuf]) {
        if !self.up_to_date {
            return;
        }
        let post = music_indexer::collect_sources(folders).fingerprint;
        if let Err(e) = repo.set_scan_meta(&post, &self.folders_key) {
            log::error!(
                "Failed to re-baseline scan fingerprint after tag write: {}",
                e
            );
        }
    }
}

/// Re-read one file and patch its rows in place. Going back through
/// `read_metadata` rather than mapping the edits straight into SQL is what keeps
/// the row identical to what a full rescan would produce — normalization
/// (genre splitting, year extraction, title fallback) stays in one place.
fn reindex_one(repo: &dyn LibraryRepository, track_id: i64, path: &Path) -> anyhow::Result<()> {
    let scanned = music_indexer::metadata::read_metadata(path)?;

    let mut artist_ids = Vec::with_capacity(scanned.artist_names.len());
    for (position, name) in scanned.artist_names.iter().enumerate() {
        artist_ids.push((repo.upsert_artist(name)?, position as i32));
    }

    let album_id = match scanned.album_title.as_deref() {
        Some(title) => {
            let album_id = repo.upsert_album(title, scanned.year, None)?;
            // why: renaming an album lands the track on a brand new row, which is born coverless —
            // hand it the cover the track already carries, first-cover-wins like the scanner
            if let Some(cover_id) = repo.track(track_id)?.and_then(|t| t.cover_art_id) {
                repo.set_album_cover_if_missing(album_id, cover_id)?;
            }
            // why: mirrors ScanSession, which links album artists only for the first track landing
            // in an album — a per-track edit must not rewrite a row shared with its siblings
            if !repo.album_has_artists(album_id)? {
                let names = if scanned.album_artist_names.is_empty() {
                    &scanned.artist_names
                } else {
                    &scanned.album_artist_names
                };
                let ids = resolve_artists(repo, names)?;
                if !ids.is_empty() {
                    repo.set_album_artists(album_id, &ids)?;
                }
            }
            Some(album_id)
        }
        None => None,
    };

    let new_track = NewTrack {
        path: scanned.path.to_string_lossy().into_owned(),
        title: scanned.title.clone(),
        album_title: scanned.album_title.clone(),
        artist_names: scanned.artist_names.clone(),
        album_artist_names: scanned.album_artist_names.clone(),
        track_number: scanned.track_number,
        disc_number: scanned.disc_number,
        year: scanned.year,
        duration_ms: scanned.duration_ms,
        cover_art_id: None,
        start_offset_ms: scanned.start_offset_ms,
        bitrate: scanned.bitrate,
    };

    let upserted = repo.upsert_track(&new_track, album_id, &artist_ids)?;
    if upserted != track_id {
        log::error!(
            "Tag write moved track {} to a different row ({}); content key changed unexpectedly",
            track_id,
            upserted
        );
    }
    repo.set_track_genres(upserted, &scanned.genres)?;
    Ok(())
}

/// How a track tag edit gave out. Each needs its own recovery: a refusal and a
/// failed write both leave the file and the DB untouched, while a failed re-index
/// means the file already moved on and only a full rescan can catch the DB up.
enum TagEditFailure {
    Refused(&'static str),
    Write(anyhow::Error),
    Reindex(anyhow::Error),
}

/// Everything one track tag edit does to disk and to the DB, in order. Lifted out
/// of the spawned task so tests exercise this sequence rather than a re-creation
/// of it; the task keeps the error handling, which needs its own context.
fn apply_track_tags(
    repo: &dyn LibraryRepository,
    track_id: i64,
    path: &Path,
    edits: &tag_writer::TrackTagEdits,
) -> std::result::Result<(), TagEditFailure> {
    // why: N cue tracks share one audio file and take their fields from the .cue text, so
    // writing tags here would edit the wrong thing for all of them. The modal is read-only
    // for cue, but the guard belongs on the function that does the writing.
    match repo.track(track_id) {
        Ok(Some(track)) if track.is_cue => return Err(TagEditFailure::Refused("cue track")),
        Ok(Some(_)) => {}
        Ok(None) => return Err(TagEditFailure::Refused("unknown track")),
        Err(e) => return Err(TagEditFailure::Reindex(e.into())),
    }
    tag_writer::write_metadata(path, edits).map_err(TagEditFailure::Write)?;
    reindex_one(repo, track_id, path).map_err(TagEditFailure::Reindex)?;
    if let Err(e) = repo.delete_orphaned_albums_and_artists() {
        log::error!("Failed to clean up after tag edit: {}", e);
    }
    Ok(())
}

/// The same for a whole album: write the shared fields into every track, then settle
/// the `albums` row the new `(title, year)` key landed on. Returns how many files
/// actually changed; the error carries the file it stopped on.
fn apply_album_tags(
    repo: &dyn LibraryRepository,
    tracks: &[music_library::Track],
    edits: &AlbumTagEdits,
) -> std::result::Result<usize, (String, anyhow::Error)> {
    let mut written = 0usize;
    for track in tracks {
        let path = PathBuf::from(&track.path);
        match write_album_fields(repo, track.id, &path, edits) {
            Ok(true) => written += 1,
            Ok(false) => {}
            Err(e) => return Err((track.path.clone(), e)),
        }
    }
    if let Err(e) = relink_album_artists(repo, tracks[0].id, edits) {
        log::error!("Failed to relink album artists: {}", e);
    }
    if let Err(e) = repo.delete_orphaned_albums_and_artists() {
        log::error!("Failed to clean up after album tag edit: {}", e);
    }
    Ok(written)
}

/// Overwrite only the album-level fields of one file, keeping its per-track tags
/// as they are on disk.
fn write_album_fields(
    repo: &dyn LibraryRepository,
    track_id: i64,
    path: &Path,
    edits: &AlbumTagEdits,
) -> anyhow::Result<bool> {
    let current = music_indexer::metadata::read_metadata(path)?;
    // why: every write bumps mtime and so feeds the fingerprint the whole design works around —
    // a file the edit does not actually change must not be rewritten
    if current.album_title == edits.album
        && current.year == edits.year
        && current.album_artist_names == edits.album_artists
        && current.genres == edits.genres
    {
        return Ok(false);
    }
    let merged = tag_writer::TrackTagEdits {
        title: current.title,
        artists: current.artist_names,
        album: edits.album.clone(),
        album_artists: edits.album_artists.clone(),
        track_number: current.track_number,
        disc_number: current.disc_number,
        year: edits.year,
        genres: edits.genres.clone(),
        removed_tags: Vec::new(),
        added_tags: Vec::new(),
    };
    tag_writer::write_metadata(path, &merged)?;
    reindex_one(repo, track_id, path)?;
    Ok(true)
}

/// After a whole album was rewritten, its `(title, year)` key may have moved it
/// to a different `albums` row, so resolve the row from a track that now lives in
/// it and set the artists there.
fn relink_album_artists(
    repo: &dyn LibraryRepository,
    track_id: i64,
    edits: &AlbumTagEdits,
) -> anyhow::Result<()> {
    let Some(album_id) = repo.track(track_id)?.and_then(|t| t.album_id) else {
        return Ok(());
    };
    // why: mirrors the scanner's fallback — an album without ALBUMARTIST is credited to its track
    // artists, so clearing the field must land there instead of leaving the old row in place
    let names = if edits.album_artists.is_empty() {
        repo.track_artists(track_id)?
    } else {
        edits.album_artists.clone()
    };
    let ids = resolve_artists(repo, &names)?;
    repo.set_album_artists(album_id, &ids)?;
    Ok(())
}

fn resolve_artists(
    repo: &dyn LibraryRepository,
    names: &[String],
) -> anyhow::Result<Vec<(i64, i32)>> {
    let mut ids = Vec::with_capacity(names.len());
    for (position, name) in names.iter().enumerate() {
        ids.push((repo.upsert_artist(name)?, position as i32));
    }
    Ok(ids)
}

fn report_tag_error(path: &str, err: &anyhow::Error) {
    log::error!("Failed to write tags for {}: {}", path, err);
    let strings = crate::localization::tr();
    diagnostics::notify_error(
        strings.tag_write_failed_title.to_string(),
        strings.tags_save_failed(&err.to_string()),
    );
}

fn force_rescan(
    repo: Arc<dyn LibraryRepository>,
    event_tx: flume::Sender<LibraryEvent>,
    executor: gpui::BackgroundExecutor,
    scan_state: Arc<ScanState>,
    folders: Vec<PathBuf>,
) {
    *scan_state.folders.lock().unwrap() = folders;
    LibraryService::spawn_scan(repo, event_tx, executor, scan_state);
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
        is_cue: track.is_cue,
        lyrics: track.lyrics.map(|l| music_library::ScanLyrics {
            text: l.text,
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

const WEB_COVER_MAX: u32 = 1440;
const WEB_COVER_QUALITY: u8 = 90;

fn transcode_web_cover(bytes: Vec<u8>) -> (Vec<u8>, String) {
    let original_type = cover_content_type(&bytes).to_string();
    let dims = image::ImageReader::new(std::io::Cursor::new(&bytes))
        .with_guessed_format()
        .ok()
        .and_then(|r| r.into_dimensions().ok());

    if let Some((w, h)) = dims
        && w <= WEB_COVER_MAX
        && h <= WEB_COVER_MAX
    {
        return (bytes, original_type);
    }

    let Ok(img) = image::load_from_memory(&bytes) else {
        return (bytes, original_type);
    };
    let rgb = img
        .resize(
            WEB_COVER_MAX,
            WEB_COVER_MAX,
            image::imageops::FilterType::Lanczos3,
        )
        .into_rgb8();
    let mut out = std::io::Cursor::new(Vec::new());
    let ok = {
        let mut encoder =
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, WEB_COVER_QUALITY);
        encoder.encode_image(&rgb).is_ok()
    };
    if ok {
        (out.into_inner(), "image/jpeg".to_string())
    } else {
        (bytes, original_type)
    }
}

fn cover_content_type(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0xFF, 0xD8]) {
        "image/jpeg"
    } else if bytes.starts_with(b"\x89PNG") {
        "image/png"
    } else if bytes.len() > 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else if bytes.starts_with(b"GIF8") {
        "image/gif"
    } else if bytes.starts_with(b"BM") {
        "image/bmp"
    } else {
        "application/octet-stream"
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use music_library::SqliteLibrary;

    use super::*;

    struct Workspace {
        repo: SqliteLibrary,
        folder: PathBuf,
    }

    impl Workspace {
        fn new() -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let folder = std::env::temp_dir().join(format!(
                "pawse-tag-service-{}-{}",
                std::process::id(),
                n
            ));
            let _ = std::fs::remove_dir_all(&folder);
            std::fs::create_dir_all(&folder).unwrap();
            let repo = SqliteLibrary::open_at(folder.join("library.db")).unwrap();
            Self { repo, folder }
        }

        fn add_file(&self, name: &str, fixture: &str) -> PathBuf {
            let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
            let src = PathBuf::from(manifest).join("../../fixtures").join(fixture);
            let dst = self.folder.join(name);
            std::fs::copy(&src, &dst).unwrap();
            dst
        }

        fn folders(&self) -> Vec<PathBuf> {
            vec![self.folder.clone()]
        }

        /// Index the folder with the real pipeline, exactly as `run_scan` drives it —
        /// only the GPUI executor and the event bus are left out. A hand-rolled stand-in
        /// can only seed the state its author remembered to model, which is how the
        /// missing album cover went unnoticed.
        fn scan(&self) {
            let sources = music_indexer::collect_sources(&self.folders());
            let known_hashes: HashSet<String> = self
                .repo
                .cover_art_hashes()
                .map(|pairs| pairs.into_iter().map(|(hash, _)| hash).collect())
                .unwrap_or_default();
            let mut session = self.repo.open_scan_session().unwrap();
            session.clear().unwrap();

            let (tx, rx) = flume::unbounded();
            let worker = std::thread::spawn(move || music_indexer::run(sources, known_hashes, tx));
            while let Ok(event) = rx.recv() {
                match event {
                    ScanEvent::Cover {
                        hash,
                        small,
                        large,
                        source_path,
                        embedded,
                    } => session
                        .add_cover(&hash, small, large, &source_path, embedded)
                        .unwrap(),
                    ScanEvent::Track(track) => session.add_track(to_scan_track(track)).unwrap(),
                    ScanEvent::Error { path, error } => panic!("scan failed on {path:?}: {error}"),
                    ScanEvent::Complete => break,
                    ScanEvent::Progress { .. } => {}
                }
            }
            worker.join().unwrap();
            session.finish().unwrap();
        }

        /// Index one file as a cue track. Only the scanner ever sets that flag, and
        /// there is no cue fixture, so the row goes in through the same scan session
        /// the scanner writes through rather than a back door built for the test.
        fn add_cue_track(&self, path: &Path) -> i64 {
            let mut session = self.repo.open_scan_session().unwrap();
            session.clear().unwrap();
            session
                .add_track(ScanTrack {
                    path: path.to_string_lossy().into_owned(),
                    title: Some("Cue Track".into()),
                    album_title: Some("Cue Album".into()),
                    artist_names: vec!["Band".into()],
                    album_artist_names: vec!["Band".into()],
                    track_number: Some(1),
                    disc_number: Some(1),
                    year: Some(1999),
                    genres: vec!["Rock".into()],
                    duration_ms: Some(1000),
                    cover_hash: None,
                    start_offset_ms: Some(0),
                    bitrate: None,
                    is_cue: true,
                    lyrics: None,
                })
                .unwrap();
            session.finish().unwrap();
            self.track_id(path)
        }

        fn track_id(&self, path: &Path) -> i64 {
            let wanted = path.to_string_lossy();
            self.repo
                .all_tracks()
                .unwrap()
                .into_iter()
                .find(|t| t.path == wanted)
                .unwrap_or_else(|| panic!("{wanted} was not indexed"))
                .id
        }

        /// The library as the screens read it, rendered so two runs can be compared.
        /// Row ids are left out — a rescan mints new ones — but cover ids survive
        /// `clear()`, so those stay in.
        fn snapshot(&self) -> String {
            let mut out = String::new();
            let mut albums = self.repo.albums().unwrap();
            albums.sort_by(|a, b| (&a.title, a.year).cmp(&(&b.title, b.year)));
            for album in &albums {
                out += &format!(
                    "album {:?} year={:?} cover={:?} artists={:?} genres={:?}\n",
                    album.title,
                    album.year,
                    album.cover_art_id,
                    self.repo.album_artists(album.id).unwrap(),
                    self.repo.album_genres(album.id).unwrap(),
                );
                let mut tracks = self.repo.tracks_for_album(album.id).unwrap();
                tracks.sort_by(|a, b| a.path.cmp(&b.path));
                for track in tracks {
                    out += &format!(
                        "  {:?} title={:?} artists={:?} genres={:?} n={:?} disc={} \
                         year={:?} cover={:?}\n",
                        Path::new(&track.path).file_name().unwrap(),
                        track.title,
                        self.repo.track_artists(track.id).unwrap(),
                        self.repo.track_genres(track.id).unwrap(),
                        track.track_number,
                        track.disc_number,
                        track.year,
                        track.cover_art_id,
                    );
                }
            }
            out
        }

        fn mark_in_sync(&self) {
            let fingerprint = music_indexer::collect_sources(&self.folders()).fingerprint;
            self.repo
                .set_scan_meta(&fingerprint, &serialize_folders(&self.folders()))
                .unwrap();
        }

        fn disk_fingerprint(&self) -> String {
            music_indexer::collect_sources(&self.folders()).fingerprint
        }
    }

    impl Drop for Workspace {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.folder);
        }
    }

    fn edits_titled(title: &str) -> tag_writer::TrackTagEdits {
        tag_writer::TrackTagEdits {
            title: Some(title.to_string()),
            artists: vec!["Seed Artist".into()],
            album: Some("Seed Album".into()),
            year: Some(2001),
            ..Default::default()
        }
    }

    #[test]
    fn a_tag_edit_keeps_the_track_id_its_like_and_its_playlist() {
        let ws = Workspace::new();
        let path = ws.add_file("one.flac", "tagged_basic.flac");
        ws.scan();
        let track_id = ws.track_id(&path);

        ws.repo.set_liked(track_id, true).unwrap();
        let playlist = ws.repo.create_playlist("Mix").unwrap();
        ws.repo.add_track_to_playlist(playlist, track_id).unwrap();

        tag_writer::write_metadata(&path, &edits_titled("Renamed")).unwrap();
        reindex_one(&ws.repo, track_id, &path).unwrap();

        let track = ws.repo.track(track_id).unwrap().expect("row kept its id");
        assert_eq!(track.title, "Renamed");
        assert!(track.liked, "a like is not part of the file");
        assert_eq!(
            ws.repo.tracks_for_playlist(playlist).unwrap().len(),
            1,
            "playlist membership hangs off the track id"
        );
    }

    #[test]
    fn clearing_the_album_artists_falls_back_to_the_track_artists() {
        let ws = Workspace::new();
        let path = ws.add_file("one.flac", "tagged_basic.flac");
        tag_writer::write_metadata(
            &path,
            &tag_writer::TrackTagEdits {
                title: Some("One".into()),
                artists: vec!["Real Artist".into()],
                album: Some("Comp".into()),
                album_artists: vec!["Various Artists".into()],
                ..Default::default()
            },
        )
        .unwrap();
        ws.scan();
        let track_id = ws.track_id(&path);
        let album_id = ws.repo.track(track_id).unwrap().unwrap().album_id.unwrap();
        assert_eq!(
            ws.repo.album_artists(album_id).unwrap(),
            ["Various Artists"]
        );

        let edits = AlbumTagEdits {
            album: Some("Comp".into()),
            album_artists: Vec::new(),
            year: None,
            genres: Vec::new(),
        };
        assert!(write_album_fields(&ws.repo, track_id, &path, &edits).unwrap());
        relink_album_artists(&ws.repo, track_id, &edits).unwrap();

        let album_id = ws.repo.track(track_id).unwrap().unwrap().album_id.unwrap();
        assert_eq!(
            ws.repo.album_artists(album_id).unwrap(),
            ["Real Artist"],
            "the scanner credits an album with no ALBUMARTIST to its track artists"
        );
    }

    #[test]
    fn renaming_an_album_merges_it_into_the_one_it_now_matches() {
        let ws = Workspace::new();
        let keeper = ws.add_file("keeper.flac", "tagged_basic.flac");
        let stray = ws.add_file("stray.flac", "tagged_basic.flac");
        for (path, album) in [(&keeper, "Kid A"), (&stray, "Kid A (typo)")] {
            tag_writer::write_metadata(
                path,
                &tag_writer::TrackTagEdits {
                    title: Some("T".into()),
                    artists: vec!["A".into()],
                    album: Some(album.to_string()),
                    year: Some(2000),
                    ..Default::default()
                },
            )
            .unwrap();
        }
        ws.scan();
        let stray_id = ws.track_id(&stray);
        assert_eq!(ws.repo.albums().unwrap().len(), 2);

        let edits = AlbumTagEdits {
            album: Some("Kid A".into()),
            album_artists: Vec::new(),
            year: Some(2000),
            genres: Vec::new(),
        };
        write_album_fields(&ws.repo, stray_id, &stray, &edits).unwrap();
        ws.repo.delete_orphaned_albums_and_artists().unwrap();

        let albums = ws.repo.albums().unwrap();
        assert_eq!(
            albums.len(),
            1,
            "the vacated row must not linger: {albums:?}"
        );
        assert_eq!(ws.repo.tracks_for_album(albums[0].id).unwrap().len(), 2);
    }

    #[test]
    fn a_per_track_year_edit_would_split_the_album() {
        let ws = Workspace::new();
        let stay = ws.add_file("stay.flac", "tagged_basic.flac");
        let move_me = ws.add_file("move.flac", "tagged_basic.flac");
        for path in [&stay, &move_me] {
            tag_writer::write_metadata(path, &edits_titled("T")).unwrap();
        }
        ws.scan();
        let moved_id = ws.track_id(&move_me);
        assert_eq!(ws.repo.albums().unwrap().len(), 1);

        let mut edits = edits_titled("T");
        edits.year = Some(2002);
        tag_writer::write_metadata(&move_me, &edits).unwrap();
        reindex_one(&ws.repo, moved_id, &move_me).unwrap();

        assert_eq!(
            ws.repo.albums().unwrap().len(),
            2,
            "albums are keyed on (title, year), which is why the form locks the year \
             for a track that has siblings"
        );
    }

    #[test]
    fn an_album_save_that_changes_nothing_rewrites_no_file() {
        let ws = Workspace::new();
        let path = ws.add_file("one.flac", "tagged_basic.flac");
        tag_writer::write_metadata(&path, &edits_titled("One")).unwrap();
        ws.scan();
        let track_id = ws.track_id(&path);
        let before = std::fs::metadata(&path).unwrap().modified().unwrap();

        let edits = AlbumTagEdits {
            album: Some("Seed Album".into()),
            album_artists: Vec::new(),
            year: Some(2001),
            genres: Vec::new(),
        };
        assert!(
            !write_album_fields(&ws.repo, track_id, &path, &edits).unwrap(),
            "nothing to change, so nothing to write"
        );
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            before,
            "an untouched mtime is what keeps the watcher quiet"
        );
    }

    /// The oracle the rest of these tests lean on. The point-update path exists only
    /// as an optimisation: it must leave the library exactly where a full rescan of the
    /// same files would. Comparing against a real rescan checks every column at once,
    /// including the ones nobody thought to assert — the missing album cover was found
    /// by hand precisely because no test asked this question.
    #[test]
    fn an_album_edit_lands_exactly_where_a_full_rescan_would() {
        let ws = Workspace::new();
        let with_art = ws.add_file("01.flac", "tagged_with_cover.flac");
        let plain = ws.add_file("02.flac", "tagged_basic.flac");
        for (path, n) in [(&with_art, 1u32), (&plain, 2)] {
            tag_writer::write_metadata(
                path,
                &tag_writer::TrackTagEdits {
                    title: Some(format!("Track {n}")),
                    artists: vec!["Band".into()],
                    album: Some("Original".into()),
                    album_artists: vec!["Band".into()],
                    track_number: Some(n),
                    year: Some(1999),
                    genres: vec!["Rock".into()],
                    ..Default::default()
                },
            )
            .unwrap();
        }
        ws.scan();
        let album_id = ws.repo.albums().unwrap()[0].id;

        let tracks = ws.repo.tracks_for_album(album_id).unwrap();
        let edits = AlbumTagEdits {
            album: Some("Renamed".into()),
            album_artists: vec!["Band & Friends".into()],
            year: Some(2000),
            genres: vec!["Post-Rock".into()],
        };
        assert_eq!(apply_album_tags(&ws.repo, &tracks, &edits).unwrap(), 2);
        let after_edit = ws.snapshot();

        ws.scan();
        assert_eq!(
            after_edit,
            ws.snapshot(),
            "the point update is only an optimisation — it has to agree with the scanner"
        );
    }

    #[test]
    fn a_track_edit_lands_exactly_where_a_full_rescan_would() {
        let ws = Workspace::new();
        let edited = ws.add_file("01.flac", "tagged_with_cover.flac");
        let sibling = ws.add_file("02.flac", "tagged_with_cover.flac");
        for (path, n) in [(&edited, 1u32), (&sibling, 2)] {
            tag_writer::write_metadata(
                path,
                &tag_writer::TrackTagEdits {
                    title: Some(format!("Track {n}")),
                    artists: vec!["Band".into()],
                    album: Some("Shared".into()),
                    album_artists: vec!["Band".into()],
                    track_number: Some(n),
                    year: Some(1999),
                    genres: vec!["Rock".into()],
                    ..Default::default()
                },
            )
            .unwrap();
        }
        ws.scan();

        let track_id = ws.track_id(&edited);
        let edits = tag_writer::TrackTagEdits {
            title: Some("Retitled".into()),
            artists: vec!["Soloist".into(), "Guest".into()],
            album: Some("Shared".into()),
            album_artists: vec!["Band".into()],
            track_number: Some(9),
            disc_number: Some(2),
            year: Some(1999),
            genres: vec!["Ambient".into(), "Drone".into()],
            ..Default::default()
        };
        assert!(apply_track_tags(&ws.repo, track_id, &edited, &edits).is_ok());
        let after_edit = ws.snapshot();

        ws.scan();
        assert_eq!(after_edit, ws.snapshot());
    }

    /// A cue album is N rows over one audio file whose fields come from the `.cue`
    /// text. Writing tags into that file would rewrite the wrong thing for every one
    /// of them, so the write path refuses regardless of what the UI allowed.
    #[test]
    fn a_cue_track_is_never_written_to() {
        let ws = Workspace::new();
        let path = ws.add_file("one.flac", "tagged_basic.flac");
        let track_id = ws.add_cue_track(&path);
        let before = std::fs::read(&path).unwrap();

        let failure = apply_track_tags(&ws.repo, track_id, &path, &edits_titled("Nope"));
        assert!(matches!(failure, Err(TagEditFailure::Refused("cue track"))));
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    /// The file the user picked may not be writable — a read-only mount, a locked
    /// file. The write has to fail before anything else runs, or the DB would start
    /// describing tags the file never got.
    #[test]
    fn a_file_that_cannot_be_written_leaves_the_library_alone() {
        let ws = Workspace::new();
        let path = ws.add_file("one.flac", "tagged_basic.flac");
        ws.scan();
        let track_id = ws.track_id(&path);
        let before = ws.snapshot();

        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&path, perms).unwrap();

        let failure = apply_track_tags(&ws.repo, track_id, &path, &edits_titled("Nope"));
        assert!(
            matches!(failure, Err(TagEditFailure::Write(_))),
            "a read-only file is a write failure, not a re-index one"
        );
        assert_eq!(
            ws.snapshot(),
            before,
            "nothing was written, so nothing moved"
        );
    }

    #[test]
    fn a_renamed_album_keeps_the_cover_its_tracks_carry() {
        let ws = Workspace::new();
        let path = ws.add_file("one.flac", "tagged_with_cover.flac");
        tag_writer::write_metadata(&path, &edits_titled("One")).unwrap();
        ws.scan();
        let track_id = ws.track_id(&path);
        let cover = ws.repo.albums().unwrap()[0]
            .cover_art_id
            .expect("the fixture carries embedded art");

        let edits = AlbumTagEdits {
            album: Some("Brand New Name".into()),
            album_artists: Vec::new(),
            year: Some(2001),
            genres: Vec::new(),
        };
        assert!(write_album_fields(&ws.repo, track_id, &path, &edits).unwrap());
        ws.repo.delete_orphaned_albums_and_artists().unwrap();

        let albums = ws.repo.albums().unwrap();
        assert_eq!(albums.len(), 1);
        assert_eq!(
            albums[0].cover_art_id,
            Some(cover),
            "a rename moves the track to a row that is born coverless, so the cover has \
             to be carried over from the track"
        );
    }

    #[test]
    fn a_current_library_is_re_baselined_so_the_watcher_rescan_stays_cheap() {
        let ws = Workspace::new();
        let path = ws.add_file("one.flac", "tagged_basic.flac");
        ws.scan();
        let track_id = ws.track_id(&path);
        ws.mark_in_sync();

        let baseline = ScanBaseline::capture(&ws.repo, &ws.folders());
        tag_writer::write_metadata(&path, &edits_titled("Renamed")).unwrap();
        reindex_one(&ws.repo, track_id, &path).unwrap();
        baseline.rebaseline(&ws.repo, &ws.folders());

        assert_eq!(
            ws.repo.scan_fingerprint().unwrap().as_deref(),
            Some(ws.disk_fingerprint().as_str()),
            "the write moved mtime, so the stored fingerprint has to move with it"
        );
    }

    #[test]
    fn a_stale_library_is_left_stale_instead_of_being_marked_current() {
        let ws = Workspace::new();
        let path = ws.add_file("one.flac", "tagged_basic.flac");
        ws.scan();
        let track_id = ws.track_id(&path);
        ws.repo
            .set_scan_meta("someone-else-changed-the-disk", "")
            .unwrap();

        let baseline = ScanBaseline::capture(&ws.repo, &ws.folders());
        tag_writer::write_metadata(&path, &edits_titled("Renamed")).unwrap();
        reindex_one(&ws.repo, track_id, &path).unwrap();
        baseline.rebaseline(&ws.repo, &ws.folders());

        assert_eq!(
            ws.repo.scan_fingerprint().unwrap().as_deref(),
            Some("someone-else-changed-the-disk"),
            "re-baselining a library that was already out of date would suppress the \
             full rescan it still needs"
        );
    }
}

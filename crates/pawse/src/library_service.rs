use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use music_indexer::{DirectoryScanner, ScanEvent};
use music_library::{LibraryRepository, NewTrack, SqliteLibrary};

#[derive(Clone, Debug)]
pub enum LibraryEvent {
    ScanStarted,
    ScanProgress { scanned: usize },
    ScanComplete,
    TrackLikedChanged { track_id: i64, liked: bool },
    PlaylistsChanged,
    PlaylistTracksChanged { playlist_id: i64 },
    QueueChanged,
}

pub struct LibraryService {
    repo: Arc<dyn LibraryRepository>,
    event_tx: flume::Sender<LibraryEvent>,
}

impl LibraryService {
    pub fn new(event_tx: flume::Sender<LibraryEvent>) -> Self {
        let repo = Arc::new(SqliteLibrary::open().expect("open library db"));
        Self { repo, event_tx }
    }

    pub fn albums(&self) -> Vec<music_library::AlbumSummary> {
        self.repo.albums().unwrap_or_default()
    }

    pub fn tracks_for_album(&self, album_id: i64) -> Vec<music_library::Track> {
        self.repo.tracks_for_album(album_id).unwrap_or_default()
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

    pub fn track_artists_map(&self, track_ids: &[i64]) -> HashMap<i64, Vec<String>> {
        self.repo.track_artists_map(track_ids).unwrap_or_default()
    }

    pub fn artists(&self) -> Vec<music_library::ArtistSummary> {
        self.repo.artists().unwrap_or_default()
    }

    pub fn tracks_by_artist(&self, artist_id: i64) -> Vec<music_library::Track> {
        self.repo.tracks_by_artist(artist_id).unwrap_or_default()
    }

    pub fn liked_tracks(&self) -> Vec<music_library::Track> {
        self.repo.liked_tracks().unwrap_or_default()
    }

    pub fn set_liked(&self, track_id: i64, liked: bool) {
        if let Err(e) = self.repo.set_liked(track_id, liked) {
            eprintln!("Failed to set liked for track {}: {}", track_id, e);
            return;
        }
        let _ = self
            .event_tx
            .send(LibraryEvent::TrackLikedChanged { track_id, liked });
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

    pub fn create_playlist(&self, name: &str) -> Option<i64> {
        match self.repo.create_playlist(name) {
            Ok(id) => {
                let _ = self.event_tx.send(LibraryEvent::PlaylistsChanged);
                Some(id)
            }
            Err(e) => {
                eprintln!("Failed to create playlist: {}", e);
                None
            }
        }
    }

    pub fn delete_playlist(&self, playlist_id: i64) {
        if let Err(e) = self.repo.delete_playlist(playlist_id) {
            eprintln!("Failed to delete playlist {}: {}", playlist_id, e);
            return;
        }
        let _ = self.event_tx.send(LibraryEvent::PlaylistsChanged);
    }

    pub fn add_track_to_playlist(&self, playlist_id: i64, track_id: i64) {
        if let Err(e) = self.repo.add_track_to_playlist(playlist_id, track_id) {
            eprintln!(
                "Failed to add track {} to playlist {}: {}",
                track_id, playlist_id, e
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
            eprintln!(
                "Failed to remove track {} from playlist {}: {}",
                track_id, playlist_id, e
            );
            return;
        }
        let _ = self.event_tx.send(LibraryEvent::PlaylistsChanged);
        let _ = self
            .event_tx
            .send(LibraryEvent::PlaylistTracksChanged { playlist_id });
    }

    pub fn album_title(&self, album_id: i64) -> Option<String> {
        self.repo.album_title(album_id).ok().flatten()
    }

    pub fn get_cover_art_small(&self, id: i64) -> Option<Vec<u8>> {
        self.repo.get_cover_art_small(id).ok().flatten()
    }

    pub fn get_cover_art_large(&self, id: i64) -> Option<Vec<u8>> {
        self.repo.get_cover_art_large(id).ok().flatten()
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

    pub fn clear_and_rescan(&self, paths: Vec<PathBuf>) {
        let repo = self.repo.clone();
        let event_tx = self.event_tx.clone();

        std::thread::spawn(move || {
            let _ = event_tx.send(LibraryEvent::ScanStarted);

            // Snapshot playlist memberships by (path, start_offset_ms) before
            // the clear wipes the `tracks` table — rescanned tracks get fresh
            // ids, so without this the playlist contents would silently
            // disappear from the user's library.
            let playlist_refs = repo.playlist_track_refs().unwrap_or_else(|e| {
                eprintln!("Failed to snapshot playlist tracks: {}", e);
                Vec::new()
            });

            if let Err(e) = repo.clear() {
                eprintln!("Failed to clear library: {}", e);
            }

            if paths.is_empty() {
                if let Err(e) = repo.restore_playlist_track_refs(&playlist_refs) {
                    eprintln!("Failed to restore playlist tracks: {}", e);
                }
                let _ = event_tx.send(LibraryEvent::ScanComplete);
                return;
            }

            let (scan_tx, scan_rx) = flume::unbounded();
            let scan_paths = paths.clone();
            std::thread::spawn(move || {
                for path in scan_paths {
                    DirectoryScanner::scan(path, scan_tx.clone());
                }
                // Dropping `scan_tx` closes the channel; main thread loop exits.
                drop(scan_tx);
            });

            let mut total_scanned: usize = 0;
            loop {
                match scan_rx.recv() {
                    Ok(ScanEvent::Track(track)) => {
                        if let Err(e) = insert_scanned_track(&*repo, &track) {
                            eprintln!("Failed to insert track: {}", e);
                        }
                    }
                    Ok(ScanEvent::Progress { scanned }) => {
                        total_scanned = total_scanned.saturating_add(scanned);
                        let _ = event_tx.send(LibraryEvent::ScanProgress {
                            scanned: total_scanned,
                        });
                    }
                    Ok(ScanEvent::Complete) => {
                        // Per-folder Complete is collapsed into a single
                        // ScanComplete emitted after the last folder finishes.
                    }
                    Ok(ScanEvent::Error { path, error }) => {
                        eprintln!("Scan error for {}: {}", path.display(), error);
                    }
                    Err(_) => {
                        if let Err(e) = repo.restore_playlist_track_refs(&playlist_refs) {
                            eprintln!("Failed to restore playlist tracks: {}", e);
                        }
                        let _ = event_tx.send(LibraryEvent::ScanComplete);
                        break;
                    }
                }
            }
        });
    }
}

fn insert_scanned_track(
    repo: &dyn LibraryRepository,
    track: &music_indexer::ScannedTrack,
) -> anyhow::Result<()> {
    let mut artist_ids = Vec::new();
    for (pos, name) in track.artist_names.iter().enumerate() {
        let id = repo.upsert_artist(name)?;
        artist_ids.push((id, pos as i32));
    }

    let cover_art_id = match track.cover_art.as_ref() {
        None => {
            eprintln!("[COVER-DBG] scan: no cover source for {:?}", track.path);
            None
        }
        Some(data) => match repo.save_cover_art(data) {
            Ok(id) => {
                eprintln!("[COVER-DBG] scan: stored cover id={id} ({} bytes raw) for {:?}", data.len(), track.path);
                Some(id)
            }
            Err(e) => {
                eprintln!("[COVER-DBG] scan: save_cover_art FAILED for {:?}: {e}", track.path);
                None
            }
        },
    };

    let album_id = if let Some(ref album_title) = track.album_title {
        let album_id = repo.upsert_album(album_title, track.year, cover_art_id)?;

        if !repo.album_has_artists(album_id)? {
            let album_artist_names = if !track.album_artist_names.is_empty() {
                &track.album_artist_names
            } else {
                &track.artist_names
            };
            let mut album_artist_ids = Vec::new();
            for (pos, name) in album_artist_names.iter().enumerate() {
                let id = repo.upsert_artist(name)?;
                album_artist_ids.push((id, pos as i32));
            }
            if !album_artist_ids.is_empty() {
                repo.set_album_artists(album_id, &album_artist_ids)?;
            }
        }
        Some(album_id)
    } else {
        None
    };

    let new_track = NewTrack {
        path: track.path.to_string_lossy().into_owned(),
        title: track.title.clone(),
        album_title: track.album_title.clone(),
        artist_names: track.artist_names.clone(),
        album_artist_names: track.album_artist_names.clone(),
        track_number: track.track_number,
        disc_number: track.disc_number,
        year: track.year,
        duration_ms: track.duration_ms,
        cover_art_id,
        start_offset_ms: track.start_offset_ms,
    };
    repo.upsert_track(&new_track, album_id, &artist_ids)?;

    Ok(())
}

use std::path::PathBuf;
use std::sync::Arc;

use music_indexer::{DirectoryScanner, ScanEvent};
use music_library::{LibraryRepository, NewTrack, SqliteLibrary};

#[derive(Clone, Debug)]
pub enum LibraryEvent {
    ScanStarted,
    ScanProgress { scanned: usize },
    ScanComplete,
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

    pub fn track_artists(&self, track_id: i64) -> Vec<String> {
        self.repo.track_artists(track_id).unwrap_or_default()
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

    pub fn clear_and_rescan(&self, path: PathBuf) {
        let repo = self.repo.clone();
        let event_tx = self.event_tx.clone();

        std::thread::spawn(move || {
            let _ = event_tx.send(LibraryEvent::ScanStarted);
            if let Err(e) = repo.clear() {
                eprintln!("Failed to clear library: {}", e);
            }

            let (scan_tx, scan_rx) = flume::unbounded();
            let scan_path = path.clone();
            std::thread::spawn(move || {
                DirectoryScanner::scan(scan_path, scan_tx);
            });

            while let Ok(event) = scan_rx.recv() {
                match event {
                    ScanEvent::Track(track) => {
                        if let Err(e) = insert_scanned_track(&*repo, &track) {
                            eprintln!("Failed to insert track: {}", e);
                        }
                    }
                    ScanEvent::Progress { scanned } => {
                        let _ = event_tx.send(LibraryEvent::ScanProgress { scanned });
                    }
                    ScanEvent::Complete => {
                        let _ = event_tx.send(LibraryEvent::ScanComplete);
                        break;
                    }
                    ScanEvent::Error { path, error } => {
                        eprintln!("Scan error for {}: {}", path.display(), error);
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

    let cover_art_id = track
        .cover_art
        .as_ref()
        .and_then(|data| repo.save_cover_art(data).ok());

    let album_id = if let Some(ref album_title) = track.album_title {
        let album_id =
            repo.upsert_album(album_title, track.year, cover_art_id)?;

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

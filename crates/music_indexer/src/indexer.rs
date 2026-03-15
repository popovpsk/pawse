use crate::error::Result;
use crate::metadata::extract_metadata;
use crate::scanner::{scan_directory, scan_directories};
use music_library::MusicLibrary;
use std::path::{Path, PathBuf};

/// Report generated after indexing operation
#[derive(Debug, Clone, Default)]
pub struct IndexReport {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    pub errors: Vec<(PathBuf, String)>,
}

/// The main indexer struct that handles music library indexing
pub struct MusicIndexer {
    library: MusicLibrary,
}

impl MusicIndexer {
    /// Creates a new MusicIndexer with the given library
    pub fn new(library: MusicLibrary) -> Self {
        Self { library }
    }

    /// Returns a reference to the underlying music library
    pub fn library(&self) -> &MusicLibrary {
        &self.library
    }

    /// Scans a single directory and indexes all audio files
    ///
    /// This method:
    /// 1. Scans the directory for audio files
    /// 2. For each file, checks if it exists in the library
    /// 3. If new: extracts metadata and adds to library
    /// 4. If exists: checks if file was modified and updates if needed
    /// 5. Removes tracks for files that no longer exist
    pub async fn scan_directory(&self, dir_path: &Path) -> Result<IndexReport> {
        let audio_files = scan_directory(dir_path)?;
        self.index_files(&audio_files).await
    }

    /// Scans multiple directories and indexes all audio files
    pub async fn scan_directories(&self, dir_paths: &[PathBuf]) -> Result<IndexReport> {
        let audio_files = scan_directories(dir_paths)?;
        self.index_files(&audio_files).await
    }

    /// Core indexing logic that processes a list of audio files
    async fn index_files(&self, audio_files: &[PathBuf]) -> Result<IndexReport> {
        let mut report = IndexReport::default();

        // Track which files we've seen during this scan
        let mut seen_paths: Vec<PathBuf> = Vec::with_capacity(audio_files.len());

        // Process each audio file
        for file_path in audio_files {
            seen_paths.push(file_path.clone());

            match self.process_file(file_path).await {
                Ok(ProcessResult::Added) => {
                    report.added += 1;
                }
                Ok(ProcessResult::Updated) => {
                    report.updated += 1;
                }
                Ok(ProcessResult::Unchanged) => {
                    // File exists and hasn't changed
                }
                Err(e) => {
                    report.errors.push((file_path.clone(), e.to_string()));
                }
            }
        }

        // Remove tracks for files that no longer exist in the scanned directories
        // But only remove files that were previously in the library
        if let Ok(all_tracks) = self.library.get_all_tracks().await {
            for track in all_tracks {
                if !seen_paths.contains(&track.file_path) {
                    // Check if the file still exists on disk
                    if !track.file_path.exists() {
                        if let Err(e) = self.library.delete_track(&track.file_path).await {
                            report.errors.push((track.file_path, e.to_string()));
                        } else {
                            report.removed += 1;
                        }
                    }
                }
            }
        }

        Ok(report)
    }

    /// Processes a single audio file
    async fn process_file(&self, file_path: &Path) -> Result<ProcessResult> {
        // Check if file already exists in library
        let existing_track = self.library.get_track_by_path(file_path).await?;

        if let Some(track) = existing_track {
            // File exists in library - check if it was modified
            if let Ok(metadata) = std::fs::metadata(file_path) {
                if let Ok(modified) = metadata.modified() {
                    let file_modified = chrono::DateTime::<chrono::Utc>::from(modified);

                    // Compare with stored last_modified
                    if let Some(stored_modified) = track.last_modified {
                        if file_modified <= stored_modified {
                            // File hasn't changed, no need to update
                            return Ok(ProcessResult::Unchanged);
                        }
                    }
                }
            }

            // File was modified - update metadata
            let metadata = extract_metadata(file_path)?;
            let artist = metadata.artist.clone();
            let album = metadata.album.clone();
            let year = metadata.year;
            let genre = metadata.genre.clone();

            let mut updated_track = metadata.into_track();
            updated_track.id = track.id;

            // Get or create artist and album
            updated_track.artist_id = self.resolve_artist(artist.as_ref()).await?;
            updated_track.album_id = self
                .resolve_album(album.as_ref(), updated_track.artist_id, year, genre.clone())
                .await?;

            self.library.update_track(&updated_track).await?;
            return Ok(ProcessResult::Updated);
        }

        // New file - extract metadata and add to library
        let metadata = extract_metadata(file_path)?;
        let artist = metadata.artist.clone();
        let album = metadata.album.clone();
        let year = metadata.year;
        let genre = metadata.genre.clone();

        let mut new_track = metadata.into_track();

        // Get or create artist and album
        new_track.artist_id = self.resolve_artist(artist.as_ref()).await?;
        new_track.album_id = self
            .resolve_album(album.as_ref(), new_track.artist_id, year, genre.clone())
            .await?;

        self.library.add_track(&new_track).await?;
        Ok(ProcessResult::Added)
    }

    /// Resolves artist for a track (creates if doesn't exist)
    async fn resolve_artist(&self, artist_name: Option<&String>) -> Result<Option<music_library::ArtistId>> {
        if let Some(name) = artist_name {
            let artist_id = self.library.get_or_create_artist(name).await?;
            Ok(Some(artist_id))
        } else {
            Ok(None)
        }
    }

    /// Resolves album for a track (creates if doesn't exist)
    async fn resolve_album(
        &self,
        album_title: Option<&String>,
        artist_id: Option<music_library::ArtistId>,
        year: Option<i32>,
        genre: Option<String>,
    ) -> Result<Option<music_library::AlbumId>> {
        if let Some(title) = album_title {
            let album_id = self
                .library
                .get_or_create_album(title, artist_id, year, genre)
                .await?;
            Ok(Some(album_id))
        } else {
            Ok(None)
        }
    }
}

/// Result of processing a single file
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcessResult {
    Added,
    Updated,
    Unchanged,
}

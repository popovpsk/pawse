//! Music Library - A high-level async API for managing a music collection
//!
//! This crate provides a database-backed music library with full async support.
//! All database operations are executed in blocking tasks to avoid blocking
//! the async runtime.
//!
//! # Example
//!
//! ```rust,no_run
//! use music_library::MusicLibrary;
//! use std::path::Path;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Open or create the library database
//!     let library = MusicLibrary::open("music_library.db").await?;
//!
//!     // Get all tracks
//!     let tracks = library.get_all_tracks().await?;
//!
//!     // Search for tracks
//!     let results = library.search_tracks("hello").await?;
//!
//!     Ok(())
//! }
//! ```

pub mod db;
pub mod error;
pub mod models;
pub mod repository;

pub use error::{LibraryError, Result};
pub use models::{Album, AlbumId, Artist, ArtistId, Track, TrackId, TrackMetadata};

use std::path::Path;

use crate::repository::Repository;

/// Main entry point for the music library
///
/// This struct provides a high-level async API for all library operations.
/// All database operations are executed asynchronously using `tokio::spawn_blocking`.
pub struct MusicLibrary {
    repo: Repository,
}

impl MusicLibrary {
    /// Opens or creates a music library database at the given path
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the SQLite database file (will be created if not exists)
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use music_library::MusicLibrary;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let library = MusicLibrary::open("music_library.db").await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let db = db::Database::open(path)?;
        let repo = Repository::new(db);
        Ok(Self { repo })
    }

    /// Opens an in-memory music library (useful for testing)
    ///
    /// # Example
    ///
    /// ```rust
    /// use music_library::MusicLibrary;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let library = MusicLibrary::open_in_memory().await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn open_in_memory() -> Result<Self> {
        let db = db::Database::open_in_memory()?;
        let repo = Repository::new(db);
        Ok(Self { repo })
    }

    // ========================================================================
    // Track operations
    // ========================================================================

    /// Gets a track by its file path
    pub async fn get_track_by_path(&self, path: &Path) -> Result<Option<Track>> {
        self.repo.get_track_by_path(path).await
    }

    /// Adds a new track to the library
    pub async fn add_track(&self, track: &Track) -> Result<TrackId> {
        self.repo.create_track(track).await
    }

    /// Updates an existing track
    pub async fn update_track(&self, track: &Track) -> Result<()> {
        self.repo.update_track(track).await
    }

    /// Deletes a track by its file path
    pub async fn delete_track(&self, path: &Path) -> Result<()> {
        self.repo.delete_track(path).await
    }

    /// Gets all tracks in the library
    pub async fn get_all_tracks(&self) -> Result<Vec<Track>> {
        self.repo.get_all_tracks().await
    }

    /// Gets all tracks by a specific artist
    pub async fn get_tracks_by_artist(&self, artist_id: ArtistId) -> Result<Vec<Track>> {
        self.repo.get_tracks_by_artist(artist_id).await
    }

    /// Gets all tracks in a specific album
    pub async fn get_tracks_by_album(&self, album_id: AlbumId) -> Result<Vec<Track>> {
        self.repo.get_tracks_by_album(album_id).await
    }

    /// Searches for tracks by title
    pub async fn search_tracks(&self, query: &str) -> Result<Vec<Track>> {
        self.repo.search_tracks(query).await
    }

    /// Gets the total number of tracks in the library
    pub async fn get_tracks_count(&self) -> Result<usize> {
        self.repo.get_tracks_count().await
    }

    // ========================================================================
    // Artist operations
    // ========================================================================

    /// Gets an artist by name
    pub async fn get_artist_by_name(&self, name: &str) -> Result<Option<Artist>> {
        self.repo.get_artist_by_name(name).await
    }

    /// Gets or creates an artist by name, returns the artist ID
    pub async fn get_or_create_artist(&self, name: &str) -> Result<ArtistId> {
        self.repo.get_or_create_artist(name).await
    }

    /// Gets all artists in the library
    pub async fn get_all_artists(&self) -> Result<Vec<Artist>> {
        self.repo.get_all_artists().await
    }

    // ========================================================================
    // Album operations
    // ========================================================================

    /// Gets all albums in the library
    pub async fn get_all_albums(&self) -> Result<Vec<Album>> {
        self.repo.get_all_albums().await
    }

    /// Gets all albums by a specific artist
    pub async fn get_albums_by_artist(&self, artist_id: ArtistId) -> Result<Vec<Album>> {
        self.repo.get_albums_by_artist(artist_id).await
    }

    /// Gets or creates an album, returns the album ID
    pub async fn get_or_create_album(
        &self,
        title: &str,
        artist_id: Option<ArtistId>,
        year: Option<i32>,
        genre: Option<String>,
    ) -> Result<AlbumId> {
        self.repo
            .get_or_create_album(title, artist_id, year, genre)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Track;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_open_in_memory() {
        let library = MusicLibrary::open_in_memory().await;
        assert!(library.is_ok());
    }

    #[tokio::test]
    async fn test_add_and_get_track() {
        let library = MusicLibrary::open_in_memory().await.unwrap();

        let track = Track::new(
            PathBuf::from("/music/test.mp3"),
            "Test Song".to_string(),
            180000,
        );

        let result = library.add_track(&track).await;
        assert!(result.is_ok());

        let tracks = library.get_all_tracks().await.unwrap();
        assert_eq!(tracks.len(), 1);
    }

    #[tokio::test]
    async fn test_get_artist_by_name() {
        let library = MusicLibrary::open_in_memory().await.unwrap();

        // Artist doesn't exist yet
        let artist = library.get_artist_by_name("Test Artist").await.unwrap();
        assert!(artist.is_none());

        // Create artist
        let artist_id = library.get_or_create_artist("Test Artist").await.unwrap();
        assert!(artist_id.0 > 0);

        // Now artist exists
        let artist = library.get_artist_by_name("Test Artist").await.unwrap();
        assert!(artist.is_some());
    }

    #[tokio::test]
    async fn test_search_tracks() {
        let library = MusicLibrary::open_in_memory().await.unwrap();

        let track = Track::new(
            PathBuf::from("/music/test.mp3"),
            "Hello World".to_string(),
            180000,
        );
        library.add_track(&track).await.unwrap();

        let results = library.search_tracks("Hello").await.unwrap();
        assert_eq!(results.len(), 1);

        let results = library.search_tracks("NonExistent").await.unwrap();
        assert_eq!(results.len(), 0);
    }

    #[tokio::test]
    async fn test_delete_track() {
        let library = MusicLibrary::open_in_memory().await.unwrap();

        let path = PathBuf::from("/music/test.mp3");
        let track = Track::new(path.clone(), "Test Song".to_string(), 180000);
        library.add_track(&track).await.unwrap();

        // Verify track exists
        let found = library.get_track_by_path(&path).await.unwrap();
        assert!(found.is_some());

        // Delete track
        library.delete_track(&path).await.unwrap();

        // Verify track is deleted
        let found = library.get_track_by_path(&path).await.unwrap();
        assert!(found.is_none());
    }
}

use thiserror::Error;

/// Errors that can occur in the music library
#[derive(Debug, Error)]
pub enum LibraryError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Database connection not initialized")]
    ConnectionNotInitialized,

    #[error("Track not found: {0}")]
    TrackNotFound(String),

    #[error("Artist not found: {0}")]
    ArtistNotFound(String),

    #[error("Album not found: {0}")]
    AlbumNotFound(String),

    #[error("Invalid path: {0}")]
    InvalidPath(String),

    #[error("Task join error: {0}")]
    TaskJoinError(String),
}

pub type Result<T> = std::result::Result<T, LibraryError>;

//! Repository trait and implementations for data access.
//!
//! Repositories provide a clean API for CRUD operations on domain entities,
//! abstracting away SQLite query logic.

pub mod artist;
pub mod album;
pub mod track;

pub use artist::ArtistRepository;
pub use album::AlbumRepository;
pub use track::TrackRepository;

/// Error type for repository operations.
#[derive(Debug, thiserror::Error)]
pub enum RepositoryError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Entity not found: {0}")]
    NotFound(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, RepositoryError>;

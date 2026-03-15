use thiserror::Error;

/// Errors that can occur during music indexing
#[derive(Debug, Error)]
pub enum IndexerError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Library error: {0}")]
    Library(#[from] music_library::LibraryError),

    #[error("Symphonia error: {0}")]
    Symphonia(String),

    #[error("Unsupported audio format: {0}")]
    UnsupportedFormat(String),

    #[error("No audio track found in file: {0}")]
    NoAudioTrack(String),

    #[error("Invalid path: {0}")]
    InvalidPath(String),

    #[error("Task join error: {0}")]
    TaskJoinError(String),
}

pub type Result<T> = std::result::Result<T, IndexerError>;

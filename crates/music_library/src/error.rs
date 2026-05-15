#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid data: {0}")]
    InvalidData(String),

    #[error("image error: {0}")]
    Image(#[from] image::ImageError),
}

pub type Result<T> = std::result::Result<T, LibraryError>;

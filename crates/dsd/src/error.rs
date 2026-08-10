#[derive(Debug, thiserror::Error)]
pub enum DsdError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("not a DSD file")]
    NotDsd,
    #[error("truncated or malformed {chunk} chunk")]
    MalformedChunk { chunk: &'static str },
    #[error("DST-compressed DSD is not supported")]
    UnsupportedCompression,
    #[error("unsupported channel layout ({0} channels)")]
    UnsupportedChannels(u32),
}

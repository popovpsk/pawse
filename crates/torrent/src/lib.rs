use std::fmt;
use std::net::SocketAddr;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::time::Duration;

mod body;
mod engine;
#[cfg(any(test, feature = "test-support"))]
pub mod testing;

pub use body::{Body, Probe};
pub use engine::Engine;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Upload {
    WhileActive,
    Limited(NonZeroU32),
    Off,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Network {
    Public,
    Local {
        listen_port: u16,
        peers: Vec<SocketAddr>,
    },
}

#[derive(Clone, Debug)]
pub struct Config {
    pub work_dir: PathBuf,
    pub state_dir: PathBuf,
    pub upload: Upload,
    pub idle_unload: Duration,
    pub work_limit_bytes: u64,
    pub network: Network,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    File(Vec<u8>),
    Magnet(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEntry {
    pub index: usize,
    pub path: PathBuf,
    pub len: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Meta {
    pub info_hash: String,
    pub name: String,
    pub files: Vec<FileEntry>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Swarm {
    pub connected: u32,
    pub known: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Want {
    pub file: usize,
    pub start: u64,
    pub end: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Unknown,
    Timeout,
    Invalid(String),
    Other(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Unknown => f.write_str("the torrent is not added"),
            Error::Timeout => f.write_str("no peers sent the data in time"),
            Error::Invalid(message) => write!(f, "not a usable torrent: {message}"),
            Error::Other(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for Error {}

pub fn is_info_hash(hash: &str) -> bool {
    hash.len() == 40
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[cfg(test)]
mod tests;

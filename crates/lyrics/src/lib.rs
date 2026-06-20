mod parser;
mod web;

pub use parser::{LyricLine, Lyrics, has_timestamps, parse_lrc};
pub use web::{LyricsQuery, RemoteLyrics, fetch};

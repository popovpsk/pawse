mod parser;
mod web;

pub use parser::{LyricLine, Lyrics, parse_lrc};
pub use web::{LyricsQuery, RemoteLyrics, fetch};

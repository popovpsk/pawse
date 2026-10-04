mod parser;
mod web;
mod words;

pub use parser::{LyricLine, Lyrics, parse_lrc};
pub use web::{LyricsQuery, RemoteLyrics, fetch};
pub use words::{Backing, Word, locate_words};

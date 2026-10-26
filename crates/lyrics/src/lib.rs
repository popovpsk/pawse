mod parser;
mod web;
mod words;
mod writer;

pub use parser::{LyricLine, Lyrics, parse_lrc};
pub use web::{LyricsQuery, WebLyrics, fetch};
pub use words::{Backing, Word, locate_words};
pub use writer::to_lrc;

mod candidate;
mod finder;
mod http;
pub mod itunes;
pub mod matching;
pub mod musicbrainz;

pub use candidate::{Candidate, Source};
pub use finder::{Finder, Found};
pub use http::{Error, agent};

pub fn image_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("png")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::image_extension;

    #[test]
    fn sniffs_jpeg_and_png_only() {
        assert_eq!(image_extension(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("jpg"));
        assert_eq!(image_extension(b"\x89PNG\r\n\x1a\n\0\0"), Some("png"));
        assert_eq!(image_extension(b"<html>"), None);
        assert_eq!(image_extension(b"GIF89a"), None);
    }
}

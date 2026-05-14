use std::path::Path;

use lofty::file::AudioFile;
use lofty::picture::PictureType;
use lofty::prelude::{Accessor, TaggedFileExt};
use lofty::tag::ItemKey;

use crate::types::ScannedTrack;

pub fn read_metadata(path: impl AsRef<Path>) -> anyhow::Result<ScannedTrack> {
    let path = path.as_ref();
    let tagged_file = lofty::read_from_path(path)?;

    let properties = tagged_file.properties();
    let duration_ms = properties.duration().as_millis() as u64;

    let tag = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag());

    let mut title = None;
    let mut artist_names = Vec::new();
    let mut album_artist_names = Vec::new();
    let mut album_title = None;
    let mut track_number = None;
    let mut disc_number = None;
    let mut year = None;
    let mut cover_art = None;

    if let Some(tag) = tag {
        title = tag.title().map(|s| s.to_string());
        album_title = tag.album().map(|s| s.to_string());

        // Track artists: prefer all artists, fall back to main artist
        let artists: Vec<String> = tag
            .get_strings(&ItemKey::TrackArtists)
            .map(|s| s.to_string())
            .collect();
        if !artists.is_empty() {
            artist_names = artists;
        } else if let Some(artist) = tag.artist() {
            artist_names.push(artist.to_string());
        }

        // Album artists: prefer AlbumArtist tag, fall back to track artists
        let album_artists: Vec<String> = tag
            .get_strings(&ItemKey::AlbumArtist)
            .map(|s| s.to_string())
            .collect();
        if !album_artists.is_empty() {
            album_artist_names = album_artists;
        }

        // Track number
        if let Some(item) = tag.get(&ItemKey::TrackNumber)
            && let Some(val) = item.value().text()
        {
            track_number = val.split('/').next().and_then(|s| s.parse().ok());
        }

        // Disc number
        if let Some(item) = tag.get(&ItemKey::DiscNumber)
            && let Some(val) = item.value().text()
        {
            disc_number = val.split('/').next().and_then(|s| s.parse().ok());
        }

        // Year
        if let Some(item) = tag.get(&ItemKey::Year)
            && let Some(val) = item.value().text()
        {
            year = val.parse().ok();
        }

        // Cover art
        if let Some(pic) = tag.pictures().iter().find(|p| p.pic_type() == PictureType::CoverFront)
            .or_else(|| tag.pictures().first())
        {
            cover_art = Some(pic.data().to_vec());
        }
    }

    if cover_art.is_none() {
        cover_art = find_external_cover_art(path);
    }

    Ok(ScannedTrack {
        path: path.to_path_buf(),
        title,
        artist_names,
        album_artist_names,
        album_title,
        track_number,
        disc_number,
        year,
        duration_ms: Some(duration_ms),
        cover_art,
        start_offset_ms: None,
    })
}

pub fn find_external_cover_art(path: &Path) -> Option<Vec<u8>> {
    let dir = path.parent()?;

    // 1. Track's own directory (e.g. CD1/, CD2/)
    if let Some(data) = find_cover_art_in_dir(dir) {
        return Some(data);
    }

    // 2. Named artwork subdirectories under track's directory
    if let Some(data) = find_cover_in_subdirs(dir) {
        return Some(data);
    }

    // 3. Parent directory (album root) — common for multi-disc albums
    if let Some(parent) = dir.parent() {
        if let Some(data) = find_cover_art_in_dir(parent) {
            return Some(data);
        }

        // 4. Named artwork subdirectories under parent directory
        if let Some(data) = find_cover_in_subdirs(parent) {
            return Some(data);
        }
    }

    None
}

const ARTWORK_DIR_NAMES: &[&str] = &[
    "artwork", "art", "covers", "scans",
    "images", "img", "pics", "folder", "booklet",
];

fn find_cover_in_subdirs(dir: &Path) -> Option<Vec<u8>> {
    for entry in std::fs::read_dir(dir).ok()? {
        let entry = entry.ok()?;
        if !entry.file_type().ok()?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if !ARTWORK_DIR_NAMES.contains(&name.as_str()) {
            continue;
        }
        if let Some(data) = find_cover_art_in_dir(&entry.path()) {
            return Some(data);
        }
    }
    None
}

fn find_cover_art_in_dir(dir: &Path) -> Option<Vec<u8>> {
    let prefixes = ["cover", "folder", "front", "album", "art"];
    let exts = ["jpg", "jpeg", "png"];
    let negative = [
        "back", "rear", "inside", "booklet", "disc", "cd", "inlay", "tray", "label", "matrix",
        "scan", "photo", "poster",
    ];

    let mut candidates = Vec::new();
    let mut fallback = Vec::new();

    for entry in std::fs::read_dir(dir).ok()? {
        let entry = entry.ok()?;
        let lossy = entry.file_name().to_string_lossy().to_lowercase();
        let (stem, ext) = lossy.rsplit_once('.').unwrap_or((&lossy, ""));
        if !exts.contains(&ext) {
            continue;
        }

        // RED/OPS tracker naming convention (e.g. 2007-WIGCD188J-HSE10043_01.jpg):
        // files ending with _1, _01, _001 etc. are numbered sequentially;
        // _1 / _01 / _001 is always the front cover and takes the highest priority.
        let is_red_ops_front = stem
            .rsplit_once('_')
            .and_then(|(_, suffix)| suffix.parse::<u32>().ok())
            == Some(1);

        // Negative keyword matching uses word boundaries (non-alphanumeric or string
        // start/end), NOT simple substring. This avoids false positives like "cd" matching
        // inside catalog numbers (e.g. WIGCD188J), while still matching standalone tokens
        // like "cd.jpg", "CD.png", "back_cover.jpg".
        let is_negative = negative.iter().any(|&n| contains_word(stem, n));

        let mut priority = None;
        for (idx, &prefix) in prefixes.iter().enumerate() {
            if stem.starts_with(prefix) {
                priority = Some(idx as i32);
                break;
            }
        }

        if let Some(mut priority) = priority {
            if is_negative {
                priority += 100;
            }
            if is_red_ops_front {
                priority -= 50;
            }
            candidates.push((priority, entry.path()));
        } else if is_red_ops_front {
            candidates.push((-50, entry.path()));
        } else if !is_negative {
            let size = std::fs::metadata(entry.path()).map(|m| m.len()).unwrap_or(0);
            fallback.push((size, entry.path()));
        }
    }

    if !candidates.is_empty() {
        candidates.sort_by_key(|(p, _)| *p);
        return candidates.into_iter().next().and_then(|(_, p)| std::fs::read(p).ok());
    }

    fallback.sort_by_key(|(size, _)| std::cmp::Reverse(*size));
    fallback
        .into_iter()
        .next()
        .and_then(|(_, p)| std::fs::read(p).ok())
}

fn contains_word(haystack: &str, needle: &str) -> bool {
    haystack.match_indices(needle).any(|(start, _)| {
        let end = start + needle.len();
        let left_bound = start == 0 || {
            !haystack.as_bytes()[start - 1].is_ascii_alphanumeric()
        };
        let right_bound = end == haystack.len() || {
            !haystack.as_bytes()[end].is_ascii_alphanumeric()
        };
        left_bound && right_bound
    })
}

#[cfg(test)]
mod tests {
    use super::contains_word;

    #[test]
    fn test_contains_word_at_start() {
        assert!(contains_word("cd_cover", "cd"));
    }

    #[test]
    fn test_contains_word_at_end() {
        assert!(contains_word("back_cover_cd", "cd"));
    }

    #[test]
    fn test_contains_word_in_middle() {
        assert!(contains_word("back_cd_cover", "cd"));
    }

    #[test]
    fn test_contains_word_entire_string() {
        assert!(contains_word("cd", "cd"));
    }

    #[test]
    fn test_contains_word_not_found() {
        assert!(!contains_word("front_cover", "cd"));
    }

    #[test]
    fn test_contains_word_substring_without_boundary() {
        assert!(!contains_word("WIGCD188J_front", "cd"));
        assert!(!contains_word("abcd", "bc"));
        assert!(!contains_word("abcdef", "cde"));
    }

    #[test]
    fn test_contains_word_multiple_occurrences_one_has_boundary() {
        assert!(contains_word("cd_WIGCD188J", "cd"));
    }

    #[test]
    fn test_contains_word_empty_haystack() {
        assert!(!contains_word("", "test"));
    }

    #[test]
    fn test_contains_word_empty_needle() {
        assert!(!contains_word("test", ""));
    }

    #[test]
    fn test_contains_word_single_char_word_boundary() {
        assert!(contains_word("a_b", "a"));
        assert!(contains_word("a_b", "b"));
        assert!(!contains_word("ab_c", "b"));
    }
}

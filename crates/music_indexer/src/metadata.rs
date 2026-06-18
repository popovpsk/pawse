use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use lofty::file::AudioFile;
use lofty::picture::PictureType;
use lofty::prelude::{Accessor, TaggedFileExt};
use lofty::tag::{ItemKey, Tag};
use music_library::sha256_hex;

use crate::types::{CoverArt, ScannedTrack};

/// Resolves a directory's external cover at most once per scan. Keyed by the
/// track's parent dir, since [`find_external_cover_art`] depends only on it.
/// Stores hashes (never bytes), so memory stays bounded over a large library:
/// the first track in a dir reads + hashes the cover, siblings reference the
/// hash and touch no disk.
#[derive(Default)]
pub(crate) struct CoverCache {
    resolved: Mutex<HashMap<PathBuf, Option<String>>>,
}

impl CoverCache {
    pub(crate) fn external_cover(&self, path: &Path) -> Option<CoverArt> {
        let dir = path.parent()?.to_path_buf();
        {
            let map = self.resolved.lock().unwrap();
            if let Some(cached) = map.get(&dir) {
                return cached.clone().map(CoverArt::Cached);
            }
        }
        let found = find_external_cover_art(path);
        let hash = found.as_ref().map(|(b, _)| sha256_hex(b));
        self.resolved.lock().unwrap().insert(dir, hash);
        found.map(|(data, source_path)| CoverArt::Bytes {
            data,
            source_path,
            embedded: false,
        })
    }
}

pub fn read_metadata(path: impl AsRef<Path>) -> anyhow::Result<ScannedTrack> {
    read_metadata_inner(path.as_ref(), None)
}

pub(crate) fn read_metadata_cached(
    path: &Path,
    cache: &CoverCache,
) -> anyhow::Result<ScannedTrack> {
    read_metadata_inner(path, Some(cache))
}

fn read_year(tag: &Tag) -> Option<i32> {
    [
        ItemKey::RecordingDate,
        ItemKey::Year,
        ItemKey::OriginalReleaseDate,
        ItemKey::ReleaseDate,
    ]
    .iter()
    .find_map(|key| {
        tag.get(key)
            .and_then(|item| item.value().text())
            .and_then(year_from_str)
    })
}

fn year_from_str(value: &str) -> Option<i32> {
    let digits: String = value
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .take(4)
        .collect();
    if digits.len() == 4 {
        digits.parse().ok()
    } else {
        None
    }
}

pub(crate) fn normalize_genres<'a>(raw: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for value in raw {
        for piece in value.split([',', ';', '/']) {
            let cleaned: String = piece.split_whitespace().collect::<Vec<_>>().join(" ");
            if cleaned.is_empty() || is_junk_genre(&cleaned) {
                continue;
            }
            let key = cleaned.to_lowercase();
            if !out.iter().any(|g| g.to_lowercase() == key) {
                out.push(cleaned);
            }
        }
    }
    out
}

fn is_junk_genre(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(
        lower.as_str(),
        "album"
            | "unknown"
            | "unknown genre"
            | "other"
            | "various"
            | "various artists"
            | "genre"
            | "none"
            | "no genre"
            | "misc"
    ) || lower.chars().all(|c| c.is_ascii_digit())
}

fn read_metadata_inner(path: &Path, cache: Option<&CoverCache>) -> anyhow::Result<ScannedTrack> {
    let tagged_file = lofty::read_from_path(path)?;

    let properties = tagged_file.properties();
    let duration_ms = properties.duration().as_millis() as u64;
    let bitrate = properties.audio_bitrate();

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
    let mut genres = Vec::new();
    let mut embedded = None;

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

        year = read_year(tag);
        genres = normalize_genres(tag.get_strings(&ItemKey::Genre));

        embedded = embedded_cover(tag);
    }

    let cover_art = match embedded {
        Some(data) => Some(CoverArt::Bytes {
            data,
            source_path: path.to_path_buf(),
            embedded: true,
        }),
        None => match cache {
            Some(cache) => cache.external_cover(path),
            None => find_external_cover_art(path).map(|(data, source_path)| CoverArt::Bytes {
                data,
                source_path,
                embedded: false,
            }),
        },
    };

    Ok(ScannedTrack {
        path: path.to_path_buf(),
        title,
        artist_names,
        album_artist_names,
        album_title,
        track_number,
        disc_number,
        year,
        genres,
        duration_ms: Some(duration_ms),
        cover_art,
        start_offset_ms: None,
        bitrate,
    })
}

pub(crate) fn embedded_cover(tag: &Tag) -> Option<Vec<u8>> {
    tag.pictures()
        .iter()
        .find(|p| p.pic_type() == PictureType::CoverFront)
        .or_else(|| tag.pictures().first())
        .map(|pic| pic.data().to_vec())
}

pub fn extract_embedded_cover(path: &Path) -> Option<Vec<u8>> {
    let tagged_file = lofty::read_from_path(path).ok()?;
    tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag())
        .and_then(embedded_cover)
}

pub fn extract_cover_art(path: &Path) -> Option<Vec<u8>> {
    extract_embedded_cover(path).or_else(|| find_external_cover_art(path).map(|(data, _)| data))
}

pub fn load_cover_from_source(
    source: Option<(String, bool)>,
    track_path: Option<&str>,
) -> Option<Vec<u8>> {
    match source {
        Some((path, true)) => extract_embedded_cover(Path::new(&path)),
        Some((path, false)) => std::fs::read(path).ok(),
        None => None,
    }
    .or_else(|| track_path.and_then(|p| extract_cover_art(Path::new(p))))
}

pub fn find_external_cover_art(path: &Path) -> Option<(Vec<u8>, PathBuf)> {
    let dir = path.parent()?;

    // 1. Track's own directory (e.g. CD1/, CD2/)
    if let Some(found) = find_cover_art_in_dir(dir) {
        return Some(found);
    }

    // 2. Named artwork subdirectories under track's directory
    if let Some(found) = find_cover_in_subdirs(dir) {
        return Some(found);
    }

    // 3. Parent directory (album root) — common for multi-disc albums
    if let Some(parent) = dir.parent() {
        if let Some(found) = find_cover_art_in_dir(parent) {
            return Some(found);
        }

        // 4. Named artwork subdirectories under parent directory
        if let Some(found) = find_cover_in_subdirs(parent) {
            return Some(found);
        }
    }

    None
}

const ARTWORK_DIR_NAMES: &[&str] = &[
    "artwork", "art", "covers", "scans", "images", "img", "pics", "folder", "booklet",
];

fn find_cover_in_subdirs(dir: &Path) -> Option<(Vec<u8>, PathBuf)> {
    for entry in std::fs::read_dir(dir).ok()? {
        let entry = entry.ok()?;
        if !entry.file_type().ok()?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if !ARTWORK_DIR_NAMES.contains(&name.as_str()) {
            continue;
        }
        if let Some(found) = find_cover_art_in_dir(&entry.path()) {
            return Some(found);
        }
    }
    None
}

fn find_cover_art_in_dir(dir: &Path) -> Option<(Vec<u8>, PathBuf)> {
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
            let size = std::fs::metadata(entry.path())
                .map(|m| m.len())
                .unwrap_or(0);
            fallback.push((size, entry.path()));
        }
    }

    if !candidates.is_empty() {
        candidates.sort_by_key(|(p, _)| *p);
        return candidates
            .into_iter()
            .next()
            .and_then(|(_, p)| std::fs::read(&p).ok().map(|data| (data, p)));
    }

    fallback.sort_by_key(|(size, _)| std::cmp::Reverse(*size));
    fallback
        .into_iter()
        .next()
        .and_then(|(_, p)| std::fs::read(&p).ok().map(|data| (data, p)))
}

fn contains_word(haystack: &str, needle: &str) -> bool {
    haystack.match_indices(needle).any(|(start, _)| {
        let end = start + needle.len();
        let left_bound = start == 0 || { !haystack.as_bytes()[start - 1].is_ascii_alphanumeric() };
        let right_bound =
            end == haystack.len() || { !haystack.as_bytes()[end].is_ascii_alphanumeric() };
        left_bound && right_bound
    })
}

#[cfg(test)]
mod tests {
    use super::{contains_word, find_external_cover_art, load_cover_from_source};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new() -> Self {
            static COUNTER: AtomicU32 = AtomicU32::new(0);
            let id = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("pawse_meta_test_{}_{}", std::process::id(), id));
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn find_external_cover_art_returns_source_path() {
        let tmp = TempDir::new();
        let cover_path = tmp.path.join("cover.jpg");
        std::fs::write(&cover_path, b"jpegbytes").unwrap();
        let track = tmp.path.join("track.flac");

        let (data, path) = find_external_cover_art(&track).unwrap();
        assert_eq!(data, b"jpegbytes");
        assert_eq!(path, cover_path);
    }

    #[test]
    fn load_cover_from_external_source_path() {
        let tmp = TempDir::new();
        let cover_path = tmp.path.join("art.jpg");
        std::fs::write(&cover_path, b"external").unwrap();

        let source = Some((cover_path.to_string_lossy().into_owned(), false));
        assert_eq!(load_cover_from_source(source, None).unwrap(), b"external");
    }

    #[test]
    fn load_cover_falls_back_to_track_dir_when_source_missing() {
        let tmp = TempDir::new();
        std::fs::write(tmp.path.join("cover.jpg"), b"fallback").unwrap();
        let track = tmp.path.join("track.flac");
        std::fs::write(&track, b"").unwrap();

        let gone = tmp.path.join("deleted.jpg");
        let source = Some((gone.to_string_lossy().into_owned(), false));
        let track_path = track.to_string_lossy().into_owned();
        assert_eq!(
            load_cover_from_source(source, Some(&track_path)).unwrap(),
            b"fallback"
        );
    }

    #[test]
    fn load_cover_falls_back_when_embedded_extraction_fails() {
        let tmp = TempDir::new();
        std::fs::write(tmp.path.join("cover.jpg"), b"fallback").unwrap();
        let not_audio = tmp.path.join("track.flac");
        std::fs::write(&not_audio, b"not really audio").unwrap();

        let source = Some((not_audio.to_string_lossy().into_owned(), true));
        let track_path = not_audio.to_string_lossy().into_owned();
        assert_eq!(
            load_cover_from_source(source, Some(&track_path)).unwrap(),
            b"fallback"
        );
    }

    #[test]
    fn load_cover_none_without_source_or_track() {
        assert!(load_cover_from_source(None, None).is_none());
    }

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

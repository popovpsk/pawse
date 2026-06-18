//! CUE-sheet business logic: expanding one `.cue` file into the individual
//! tracks it describes, resolving the referenced audio file, and inferring
//! disc/album metadata from multi-disc folder layouts. This module is pure
//! parsing logic with no knowledge of threading — the pipeline drives it.

use std::path::{Path, PathBuf};
use std::time::Duration;

use lofty::file::AudioFile;
use lofty::prelude::TaggedFileExt;

use crate::metadata::{embedded_cover, find_external_cover_art};
use crate::pipeline::AUDIO_EXTENSIONS;
use crate::types::{CoverArt, ScannedTrack};

/// Read a `.cue` file as text, tolerating the legacy encodings rippers emit.
/// EAC and friends still write Windows-1252 (e.g. a `0x92` curly apostrophe in
/// "I'm Alive"), which is not valid UTF-8 and makes `read_to_string` fail. Try
/// UTF-8 first, then fall back to Windows-1252, which never fails to decode.
pub fn read_cue_text(cue_path: &Path) -> std::io::Result<String> {
    let bytes = std::fs::read(cue_path)?;
    match std::str::from_utf8(&bytes) {
        Ok(s) => Ok(s.to_owned()),
        Err(_) => Ok(encoding_rs::WINDOWS_1252.decode(&bytes).0.into_owned()),
    }
}

pub fn process_cue_file(cue_path: &Path) -> anyhow::Result<Vec<ScannedTrack>> {
    let content = read_cue_text(cue_path)?;
    let sheet = cue_parser::parse(&content)?;
    let cue_dir = cue_path.parent().unwrap_or(Path::new("."));

    let audio_path = match resolve_audio_file(cue_dir, &sheet.file.name) {
        Some(path) => path,
        None => anyhow::bail!(
            "referenced audio file not found: {}",
            cue_dir.join(&sheet.file.name).display()
        ),
    };

    let tagged_file = lofty::read_from_path(&audio_path)?;
    let file_duration_ms = tagged_file.properties().duration().as_millis() as u64;
    let bitrate = tagged_file.properties().audio_bitrate();

    let cover_art = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag())
        .and_then(embedded_cover)
        .map(|data| (data, audio_path.clone(), true))
        .or_else(|| {
            find_external_cover_art(&audio_path)
                .map(|(data, source_path)| (data, source_path, false))
        });

    // Multi-disc rips lay each disc out in a `CD1`/`CD2` (or `Disc N`) subfolder. The
    // CUE format has no disc field and these whole-disc files usually carry no tags, so
    // the folder name is the only reliable signal. When the cue's own directory is such
    // a disc folder, take the disc number from it and anchor the album title to the
    // shared parent folder so both discs merge into one album despite differing CUE
    // titles. Otherwise keep the CUE title and default to disc 1.
    let folder_disc = cue_dir
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(parse_disc_folder);

    let album_title = match folder_disc {
        Some(_) => cue_dir
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .map(clean_album_folder_title)
            .filter(|s| !s.is_empty())
            .or_else(|| sheet.title.clone()),
        None => sheet.title.clone(),
    };
    let disc_number = Some(folder_disc.unwrap_or(1));

    let mut tracks = Vec::new();
    let track_count = sheet.tracks.len();

    for (i, cue_track) in sheet.tracks.iter().enumerate() {
        let index_01 = cue_track
            .indices
            .iter()
            .find(|idx| idx.number == 1)
            .map(|idx| idx.position)
            .unwrap_or(Duration::ZERO);

        let start_offset_ms = index_01.as_millis() as u64;

        let duration_ms = if i + 1 < track_count {
            let next_index_01 = sheet.tracks[i + 1]
                .indices
                .iter()
                .find(|idx| idx.number == 1)
                .map(|idx| idx.position)
                .unwrap_or(Duration::ZERO);
            next_index_01.saturating_sub(index_01).as_millis() as u64
        } else {
            file_duration_ms.saturating_sub(start_offset_ms)
        };

        let title = Some(cue_track.title.clone());
        let artist_names = cue_track
            .performer
            .clone()
            .or_else(|| sheet.performer.clone())
            .into_iter()
            .collect();
        let album_artist_names = sheet.performer.clone().into_iter().collect();
        let track_number = Some(cue_track.number);
        let year = sheet.date.as_ref().and_then(|d| d.parse::<i32>().ok());

        tracks.push(ScannedTrack {
            path: audio_path.clone(),
            title,
            artist_names,
            album_artist_names,
            album_title: album_title.clone(),
            track_number,
            disc_number,
            year,
            genres: crate::metadata::normalize_genres(sheet.genre.as_deref().into_iter()),
            duration_ms: Some(duration_ms),
            cover_art: cover_art
                .clone()
                .map(|(data, source_path, embedded)| CoverArt::Bytes {
                    data,
                    source_path,
                    embedded,
                }),
            start_offset_ms: Some(start_offset_ms),
            bitrate,
        });
    }

    Ok(tracks)
}

/// Resolve the audio file referenced by a CUE `FILE` line. EAC and similar rippers
/// often leave the original `.wav` name in the cue even after encoding to FLAC, so if
/// the exact name is missing, fall back to a sibling in the same directory with the
/// same stem and a supported audio extension.
pub fn resolve_audio_file(cue_dir: &Path, referenced: &str) -> Option<PathBuf> {
    let exact = cue_dir.join(referenced);
    if exact.exists() {
        return Some(exact);
    }

    let stem = Path::new(referenced).file_stem()?.to_str()?.to_lowercase();

    let mut matches: Vec<PathBuf> = std::fs::read_dir(cue_dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| {
            p.file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.to_lowercase() == stem)
        })
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| AUDIO_EXTENSIONS.iter().any(|a| a.eq_ignore_ascii_case(e)))
        })
        .collect();

    matches.sort();
    matches.into_iter().next()
}

/// Parse a disc number from a folder named exactly like a disc directory: `CD1`,
/// `CD 2`, `Disc03`, `disc 4`. Returns `None` for anything else so normal album
/// folders never trigger multi-disc handling.
pub fn parse_disc_folder(dir_name: &str) -> Option<u32> {
    let lower = dir_name.trim().to_lowercase();
    let rest = lower
        .strip_prefix("disc")
        .or_else(|| lower.strip_prefix("cd"))?
        .trim_start();
    if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    rest.parse().ok()
}

/// Derive a stable album title from a multi-disc album folder name by stripping a
/// leading `YYYY -`/`YYYY ` date prefix and a single trailing `[catalog]` segment.
pub fn clean_album_folder_title(folder_name: &str) -> String {
    let mut s = folder_name.trim();

    if s.len() >= 4 && s.as_bytes()[..4].iter().all(|b| b.is_ascii_digit()) {
        let after = s[4..].trim_start();
        if let Some(stripped) = after.strip_prefix('-') {
            s = stripped.trim_start();
        } else if after.len() != s[4..].len() {
            // had whitespace after the year but no dash
            s = after;
        }
    }

    if s.ends_with(']')
        && let Some(open) = s.rfind('[')
    {
        s = s[..open].trim_end();
    }

    s.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::{clean_album_folder_title, parse_disc_folder, read_cue_text};

    #[test]
    fn test_read_cue_text_decodes_windows_1252() {
        // EAC writes a 0x92 curly apostrophe (Windows-1252) in "I'm Alive",
        // which is not valid UTF-8. `read_cue_text` must still decode it.
        let path = std::env::temp_dir().join(format!(
            "pawse_cue_{}_{}.cue",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut bytes = b"TITLE \"I".to_vec();
        bytes.push(0x92);
        bytes.extend_from_slice(b"m Alive\"\r\n");
        std::fs::write(&path, &bytes).unwrap();

        let text = read_cue_text(&path).unwrap();
        std::fs::remove_file(&path).ok();

        assert_eq!(text, "TITLE \"I\u{2019}m Alive\"\r\n");
    }

    #[test]
    fn test_read_cue_text_passes_through_utf8() {
        let path = std::env::temp_dir().join(format!(
            "pawse_cue_utf8_{}_{}.cue",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, "TITLE \"Café Déjà\"\r\n".as_bytes()).unwrap();

        let text = read_cue_text(&path).unwrap();
        std::fs::remove_file(&path).ok();

        assert_eq!(text, "TITLE \"Café Déjà\"\r\n");
    }

    #[test]
    fn test_parse_disc_folder_accepts_disc_layouts() {
        assert_eq!(parse_disc_folder("CD1"), Some(1));
        assert_eq!(parse_disc_folder("CD 2"), Some(2));
        assert_eq!(parse_disc_folder("Disc03"), Some(3));
        assert_eq!(parse_disc_folder("disc 4"), Some(4));
        assert_eq!(parse_disc_folder(" cd 10 "), Some(10));
    }

    #[test]
    fn test_parse_disc_folder_rejects_non_disc_folders() {
        assert_eq!(parse_disc_folder("CD"), None);
        assert_eq!(parse_disc_folder("Disc"), None);
        assert_eq!(parse_disc_folder("CD1a"), None);
        assert_eq!(parse_disc_folder("Vol.3"), None);
        assert_eq!(parse_disc_folder(""), None);
        assert_eq!(parse_disc_folder("1CD"), None);
        assert_eq!(parse_disc_folder("2004 - In Waves"), None);
    }

    #[test]
    fn test_clean_album_folder_title_strips_date_and_catalog() {
        assert_eq!(
            clean_album_folder_title("2017 - Vol.3 (The Subliminal Verses) [EU RR8158-2]"),
            "Vol.3 (The Subliminal Verses)"
        );
        assert_eq!(clean_album_folder_title("2011 - In Waves"), "In Waves");
    }

    #[test]
    fn test_clean_album_folder_title_leaves_plain_title() {
        assert_eq!(
            clean_album_folder_title("The Subliminal Verses"),
            "The Subliminal Verses"
        );
    }

    #[test]
    fn test_clean_album_folder_title_keeps_numeric_album_name() {
        assert_eq!(clean_album_folder_title("2112"), "2112");
    }
}

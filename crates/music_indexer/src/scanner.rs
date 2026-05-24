use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use flume::Sender;
use jwalk::WalkDir;
use lofty::file::AudioFile;
use lofty::picture::PictureType;
use lofty::prelude::TaggedFileExt;

use crate::metadata::{find_external_cover_art, read_metadata};
use crate::types::ScanEvent;
use crate::types::ScannedTrack;

const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "ogg", "oga", "wav", "m4a", "aac", "wma", "ape", "wv", "opus",
];

const CUE_EXTENSIONS: &[&str] = &["cue"];

pub struct DirectoryScanner;

impl DirectoryScanner {
    pub fn scan(path: impl AsRef<Path>, tx: Sender<ScanEvent>) {
        let path = path.as_ref();
        let scanned = AtomicUsize::new(0);

        let mut skip_set = HashSet::<PathBuf>::new();

        let cue_files = collect_cue_files(path);
        for cue_path in &cue_files {
            match process_cue_file(cue_path) {
                Ok(tracks) => {
                    for track in tracks {
                        let canonical = std::fs::canonicalize(&track.path)
                            .unwrap_or_else(|_| track.path.clone());
                        skip_set.insert(canonical);
                        if tx.send(ScanEvent::Track(track)).is_err() {
                            return;
                        }
                        let count = scanned.fetch_add(1, Ordering::Relaxed) + 1;
                        if count.is_multiple_of(10)
                            && tx.send(ScanEvent::Progress { scanned: count }).is_err()
                        {
                            return;
                        }
                    }
                }
                Err(e) => {
                    if tx
                        .send(ScanEvent::Error {
                            path: cue_path.clone(),
                            error: e.to_string(),
                        })
                        .is_err()
                    {
                        return;
                    }
                }
            }
        }

        let walker = WalkDir::new(path).sort(true);
        for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    let _ = tx.send(ScanEvent::Error {
                        path: PathBuf::new(),
                        error: e.to_string(),
                    });
                    continue;
                }
            };

            if !entry.file_type().is_file() {
                continue;
            }

            let track_path = entry.path();

            let canonical =
                std::fs::canonicalize(&track_path).unwrap_or_else(|_| track_path.clone());
            if skip_set.contains(&canonical) {
                continue;
            }

            let ext = track_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();

            if !AUDIO_EXTENSIONS.contains(&ext.as_str()) {
                continue;
            }

            match read_metadata(&track_path) {
                Ok(track) => {
                    if tx.send(ScanEvent::Track(track)).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    if tx
                        .send(ScanEvent::Error {
                            path: track_path,
                            error: e.to_string(),
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            }

            let count = scanned.fetch_add(1, Ordering::Relaxed) + 1;
            if count.is_multiple_of(10) && tx.send(ScanEvent::Progress { scanned: count }).is_err()
            {
                break;
            }
        }

        let _ = tx.send(ScanEvent::Complete);
    }
}

fn collect_cue_files(root: &Path) -> Vec<PathBuf> {
    let mut cue_files = Vec::new();
    let walker = WalkDir::new(root).sort(true);
    for entry in walker.into_iter().flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        let ext = entry
            .path()
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        if CUE_EXTENSIONS.contains(&ext.as_str()) {
            cue_files.push(entry.path());
        }
    }
    cue_files
}

fn process_cue_file(cue_path: &Path) -> anyhow::Result<Vec<ScannedTrack>> {
    let content = std::fs::read_to_string(cue_path)?;
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

    let cover_art = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag())
        .and_then(|tag| {
            tag.pictures()
                .iter()
                .find(|p| p.pic_type() == PictureType::CoverFront)
                .or_else(|| tag.pictures().first())
                .map(|pic| pic.data().to_vec())
        })
        .or_else(|| find_external_cover_art(&audio_path));

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
            duration_ms: Some(duration_ms),
            cover_art: cover_art.clone(),
            start_offset_ms: Some(start_offset_ms),
        });
    }

    Ok(tracks)
}

/// Resolve the audio file referenced by a CUE `FILE` line. EAC and similar rippers
/// often leave the original `.wav` name in the cue even after encoding to FLAC, so if
/// the exact name is missing, fall back to a sibling in the same directory with the
/// same stem and a supported audio extension.
fn resolve_audio_file(cue_dir: &Path, referenced: &str) -> Option<PathBuf> {
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
                .map(|e| e.to_lowercase())
                .is_some_and(|e| AUDIO_EXTENSIONS.contains(&e.as_str()))
        })
        .collect();

    matches.sort();
    matches.into_iter().next()
}

/// Parse a disc number from a folder named exactly like a disc directory: `CD1`,
/// `CD 2`, `Disc03`, `disc 4`. Returns `None` for anything else so normal album
/// folders never trigger multi-disc handling.
fn parse_disc_folder(dir_name: &str) -> Option<u32> {
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
fn clean_album_folder_title(folder_name: &str) -> String {
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
    use super::{clean_album_folder_title, parse_disc_folder};

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

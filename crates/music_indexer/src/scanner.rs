use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use flume::Sender;
use jwalk::WalkDir;
use lofty::file::AudioFile;

use crate::metadata::read_metadata;
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
                        skip_set.insert(track.path.clone());
                        if tx.send(ScanEvent::Track(track)).is_err() {
                            return;
                        }
                        let count = scanned.fetch_add(1, Ordering::Relaxed) + 1;
                        if count.is_multiple_of(10) && tx.send(ScanEvent::Progress { scanned: count }).is_err() {
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

            if skip_set.contains(&track_path) {
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
            if count.is_multiple_of(10) && tx.send(ScanEvent::Progress { scanned: count }).is_err() {
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

    let audio_path = cue_dir.join(&sheet.file.name);
    if !audio_path.exists() {
        anyhow::bail!(
            "referenced audio file not found: {}",
            audio_path.display()
        );
    }

    let file_duration_ms = get_file_duration_ms(&audio_path)?;

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
        let album_title = sheet.title.clone();
        let track_number = Some(cue_track.number);
        let disc_number = Some(1u32);
        let year = sheet.date.as_ref().and_then(|d| d.parse::<i32>().ok());

        let cover_art = read_metadata(&audio_path)
            .ok()
            .and_then(|t| t.cover_art);

        tracks.push(ScannedTrack {
            path: audio_path.clone(),
            title,
            artist_names,
            album_artist_names,
            album_title,
            track_number,
            disc_number,
            year,
            duration_ms: Some(duration_ms),
            cover_art,
            start_offset_ms: Some(start_offset_ms),
        });
    }

    Ok(tracks)
}

fn get_file_duration_ms(path: &Path) -> anyhow::Result<u64> {
    let tagged_file = lofty::read_from_path(path)?;
    let duration = tagged_file.properties().duration();
    Ok(duration.as_millis() as u64)
}
//! Parallel indexing pipeline. This module owns *only* the concurrency: a
//! single filesystem walk, a fixed pool of parse workers, and the channels that
//! connect them to the DB writer. All parsing rules live in [`crate::metadata`]
//! and [`crate::cue`]; this file calls them but contains none of them, so the
//! business logic can change without touching the pipeline (and vice versa).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::UNIX_EPOCH;

use flume::Sender;
use jwalk::WalkDir;
use music_library::sha256_hex;
use music_library::thumbnail::generate_thumbnails;

use crate::cue;
use crate::metadata::read_metadata;
use crate::types::{PreparedTrack, ScanEvent, ScannedTrack, SourceSet};

pub(crate) const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "ogg", "oga", "wav", "m4a", "aac", "wma", "ape", "wv", "opus",
];

pub(crate) const CUE_EXTENSIONS: &[&str] = &["cue"];

/// Image files are included in the fingerprint so that swapping a cover image
/// (with no audio change) still triggers a rescan, without the noise of every
/// stray file on disk flipping it.
const FINGERPRINT_IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png"];

/// Emit a `Progress` event once every this many tracks.
const PROGRESS_INTERVAL: usize = 10;

enum WorkItem {
    Audio(PathBuf),
    Cue(PathBuf),
}

/// Walk the folders once, classify files, and compute a change-detection
/// fingerprint. Cheap (stat-only, no decoding) — this is the entire cost of the
/// fast path when nothing has changed.
pub fn collect_sources(folders: &[PathBuf]) -> SourceSet {
    let mut listing: Vec<(String, u128, u64)> = Vec::new();
    let mut cue_files = Vec::new();
    let mut audio_files = Vec::new();

    for folder in folders {
        for entry in WalkDir::new(folder).sort(true).into_iter().flatten() {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            let is_audio = AUDIO_EXTENSIONS.contains(&ext.as_str());
            let is_cue = CUE_EXTENSIONS.contains(&ext.as_str());
            let is_image = FINGERPRINT_IMAGE_EXTENSIONS.contains(&ext.as_str());

            if (is_audio || is_cue || is_image)
                && let Ok(md) = entry.metadata()
            {
                let mtime = md
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                listing.push((path.to_string_lossy().into_owned(), mtime, md.len()));
            }

            if is_cue {
                cue_files.push(path);
            } else if is_audio {
                audio_files.push(path);
            }
        }
    }

    // Audio referenced by a cue is expanded via the cue, so drop it from the
    // standalone set to avoid double-indexing.
    let mut skip = HashSet::<PathBuf>::new();
    for cue_path in &cue_files {
        if let Ok(content) = std::fs::read_to_string(cue_path)
            && let Ok(sheet) = cue_parser::parse(&content)
        {
            let cue_dir = cue_path.parent().unwrap_or(Path::new("."));
            if let Some(audio) = cue::resolve_audio_file(cue_dir, &sheet.file.name) {
                let canonical = std::fs::canonicalize(&audio).unwrap_or(audio);
                skip.insert(canonical);
            }
        }
    }
    audio_files.retain(|p| {
        let canonical = std::fs::canonicalize(p).unwrap_or_else(|_| p.clone());
        !skip.contains(&canonical)
    });

    listing.sort();
    let mut buf = String::with_capacity(listing.len() * 64);
    for (path, mtime, size) in &listing {
        use std::fmt::Write;
        let _ = writeln!(buf, "{path}|{mtime}|{size}");
    }
    let fingerprint = sha256_hex(buf.as_bytes());

    SourceSet {
        cue_files,
        audio_files,
        fingerprint,
    }
}

/// Run the parse workers over `sources`, emitting `Cover`/`Track`/`Progress`/
/// `Error` events and a final `Complete`. `known_hashes` are cover hashes
/// already in the DB; covers matching them skip thumbnail generation entirely.
///
/// Blocks until every worker has finished, so callers run this on its own
/// thread and consume `tx`'s receiver elsewhere.
pub fn run(sources: SourceSet, known_hashes: HashSet<String>, tx: Sender<ScanEvent>) {
    let worker_count = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .max(1);

    let (work_tx, work_rx) = flume::bounded::<WorkItem>(worker_count * 4);
    let known = Arc::new(known_hashes);
    let claimed = Arc::new(Mutex::new(HashSet::<String>::new()));
    let counter = Arc::new(AtomicUsize::new(0));

    let mut handles = Vec::with_capacity(worker_count);
    for _ in 0..worker_count {
        let work_rx = work_rx.clone();
        let tx = tx.clone();
        let known = Arc::clone(&known);
        let claimed = Arc::clone(&claimed);
        let counter = Arc::clone(&counter);
        handles.push(thread::spawn(move || {
            worker(&work_rx, &tx, &known, &claimed, &counter);
        }));
    }
    drop(work_rx);

    // Cue files first (so cue tracks are not starved behind a huge standalone
    // backlog), then standalone audio.
    'feed: for cue_path in sources.cue_files {
        if work_tx.send(WorkItem::Cue(cue_path)).is_err() {
            break 'feed;
        }
    }
    for audio_path in sources.audio_files {
        if work_tx.send(WorkItem::Audio(audio_path)).is_err() {
            break;
        }
    }
    drop(work_tx);

    for handle in handles {
        let _ = handle.join();
    }

    let _ = tx.send(ScanEvent::Complete);
}

fn worker(
    work_rx: &flume::Receiver<WorkItem>,
    tx: &Sender<ScanEvent>,
    known: &HashSet<String>,
    claimed: &Mutex<HashSet<String>>,
    counter: &AtomicUsize,
) {
    while let Ok(item) = work_rx.recv() {
        let keep_going = match item {
            WorkItem::Audio(path) => match read_metadata(&path) {
                Ok(track) => emit_track(track, tx, known, claimed, counter),
                Err(e) => tx
                    .send(ScanEvent::Error {
                        path,
                        error: e.to_string(),
                    })
                    .is_ok(),
            },
            WorkItem::Cue(path) => match cue::process_cue_file(&path) {
                Ok(tracks) => tracks
                    .into_iter()
                    .all(|track| emit_track(track, tx, known, claimed, counter)),
                Err(e) => tx
                    .send(ScanEvent::Error {
                        path,
                        error: e.to_string(),
                    })
                    .is_ok(),
            },
        };
        if !keep_going {
            // The receiver was dropped — stop pulling work.
            break;
        }
    }
}

/// Resolve the track's cover (deduping + thumbnailing each unique hash exactly
/// once across all workers) and emit the `Track` event. Returns `false` if a
/// send failed (receiver gone), signalling the worker to stop.
fn emit_track(
    track: ScannedTrack,
    tx: &Sender<ScanEvent>,
    known: &HashSet<String>,
    claimed: &Mutex<HashSet<String>>,
    counter: &AtomicUsize,
) -> bool {
    let cover_hash = match &track.cover_art {
        Some(bytes) => {
            let hash = sha256_hex(bytes);
            if !known.contains(&hash) {
                // Only the first worker to claim a hash generates its thumbnail;
                // peers reference the hash and let the writer resolve the id.
                let newly_claimed = claimed.lock().unwrap().insert(hash.clone());
                if newly_claimed {
                    match generate_thumbnails(bytes) {
                        Ok(thumbs) => {
                            if tx
                                .send(ScanEvent::Cover {
                                    hash: hash.clone(),
                                    small: thumbs.small,
                                    large: thumbs.large,
                                })
                                .is_err()
                            {
                                return false;
                            }
                        }
                        Err(e) => {
                            eprintln!(
                                "Failed to generate cover thumbnail for {:?}: {e}",
                                track.path
                            );
                            // Release the claim so a peer can retry; otherwise the
                            // hash stays claimed-but-unfulfilled and every track
                            // sharing this cover is written art-less at finish.
                            claimed.lock().unwrap().remove(&hash);
                        }
                    }
                }
            }
            Some(hash)
        }
        None => None,
    };

    let prepared = PreparedTrack {
        path: track.path,
        title: track.title,
        artist_names: track.artist_names,
        album_artist_names: track.album_artist_names,
        album_title: track.album_title,
        track_number: track.track_number,
        disc_number: track.disc_number,
        year: track.year,
        duration_ms: track.duration_ms,
        cover_hash,
        start_offset_ms: track.start_offset_ms,
    };

    if tx.send(ScanEvent::Track(prepared)).is_err() {
        return false;
    }

    let count = counter.fetch_add(1, Ordering::Relaxed) + 1;
    if count.is_multiple_of(PROGRESS_INTERVAL)
        && tx.send(ScanEvent::Progress { scanned: count }).is_err()
    {
        return false;
    }
    true
}

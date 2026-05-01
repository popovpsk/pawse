use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use flume::Sender;
use jwalk::WalkDir;

use crate::metadata::read_metadata;
use crate::types::ScanEvent;

const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "ogg", "oga", "wav", "m4a", "aac", "wma", "ape", "wv", "opus",
];

pub struct DirectoryScanner;

impl DirectoryScanner {
    pub fn scan(path: impl AsRef<Path>, tx: Sender<ScanEvent>) {
        let path = path.as_ref();
        let scanned = AtomicUsize::new(0);

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

            let ext = entry
                .path()
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();

            if !AUDIO_EXTENSIONS.contains(&ext.as_str()) {
                continue;
            }

            let track_path = entry.path();
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

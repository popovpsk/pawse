use std::collections::HashSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use audio_decoder::Decoder;
use audio_embedding::{Analyzer, EMBEDDING_VERSION, EmbedError, Job, TrackRange};
use music_library::Track;

use super::Shared;
use crate::remote_media::RemoteMedia;

const SAVE_BATCH: usize = 16;
const PROGRESS_EVERY: usize = 100;
const LOAD_RETRY: Duration = Duration::from_secs(5 * 60);

pub(super) fn spawn(
    shared: Arc<Shared>,
    remote_media: RemoteMedia,
    model_dir: PathBuf,
    inbox: flume::Receiver<()>,
) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("similar-tracks".into())
        .spawn(move || {
            let mut worker = Worker {
                shared,
                remote_media,
                model_dir,
                analyzer: None,
                failed: HashSet::new(),
            };
            loop {
                let next = if worker.analyzer.is_none() {
                    inbox.recv_timeout(LOAD_RETRY)
                } else {
                    inbox
                        .recv()
                        .map_err(|_| flume::RecvTimeoutError::Disconnected)
                };
                if matches!(next, Err(flume::RecvTimeoutError::Disconnected)) || worker.stopped() {
                    break;
                }
                worker.run_pass();
            }
        })
        .map(drop)
}

struct Worker {
    shared: Arc<Shared>,
    remote_media: RemoteMedia,
    model_dir: PathBuf,
    analyzer: Option<Analyzer>,
    failed: HashSet<i64>,
}

impl Worker {
    fn stopped(&self) -> bool {
        self.shared.stop.load(Ordering::Relaxed)
    }

    fn run_pass(&mut self) {
        self.shared.neighbors.invalidate();
        if self.analyzer.is_none() {
            self.analyzer = self.load();
        }
        if self.analyzer.is_some()
            && let Err(e) = self.analyse_missing()
        {
            log::error!("similar tracks: analysis stopped: {e:#}");
        }
    }

    fn load(&self) -> Option<Analyzer> {
        let started = Instant::now();
        let path = match audio_embedding::model_file::ensure(&self.model_dir, &|| self.stopped()) {
            Ok(path) => path,
            Err(EmbedError::Cancelled) => return None,
            Err(e) => {
                log::error!("similar tracks: the model is unavailable: {e}");
                return None;
            }
        };
        if self.stopped() {
            return None;
        }
        let analyzer = match Analyzer::load(&path) {
            Ok(analyzer) => analyzer,
            Err(e) => {
                log::error!("similar tracks: the model does not load: {e}");
                match audio_embedding::model_file::discard_if_corrupt(&self.model_dir) {
                    Ok(true) => {
                        log::warn!("similar tracks: the model file was corrupt and is removed")
                    }
                    Ok(false) => {}
                    Err(e) => log::warn!("similar tracks: could not check the model file: {e}"),
                }
                return None;
            }
        };
        log::info!(
            "similar tracks: model ready in {} ms",
            started.elapsed().as_millis()
        );
        match self.shared.repo.prune_embeddings(EMBEDDING_VERSION) {
            Ok(0) => {}
            Ok(dropped) => {
                log::info!("similar tracks: dropped {dropped} vectors of older versions");
                self.shared.neighbors.invalidate();
            }
            Err(e) => {
                log::error!("similar tracks: could not drop old vectors: {e}");
                return None;
            }
        }
        Some(analyzer)
    }

    fn analyse_missing(&mut self) -> anyhow::Result<()> {
        let Some(analyzer) = self.analyzer.as_ref() else {
            return Ok(());
        };
        let mut skipped = 0usize;
        let todo: Vec<(Track, PathBuf)> = self
            .shared
            .repo
            .embedding_candidates(EMBEDDING_VERSION)?
            .into_iter()
            .filter(|track| !self.failed.contains(&track.id))
            .filter_map(|track| match file_for(&track, &self.remote_media) {
                Some(path) => Some((track, path)),
                None => {
                    skipped += 1;
                    None
                }
            })
            .collect();
        if todo.is_empty() {
            return Ok(());
        }
        log::info!(
            "similar tracks: {} tracks to analyse, {skipped} server tracks not in the cache",
            todo.len()
        );
        let started = Instant::now();
        let mut pending = Vec::with_capacity(SAVE_BATCH);
        let (mut analysed, mut failed) = (0usize, 0usize);
        for (track, path) in &todo {
            if self.shared.stop.load(Ordering::Relaxed) {
                break;
            }
            match analyse(analyzer, path, track) {
                Ok(vector) => {
                    pending.push((track.id, vector));
                    analysed += 1;
                }
                Err(EmbedError::Model(e)) => {
                    save(&self.shared, &mut pending)?;
                    anyhow::bail!("model: {e}");
                }
                Err(e) => {
                    log::warn!("similar tracks: {}: {e}", path.display());
                    self.failed.insert(track.id);
                    failed += 1;
                }
            }
            if pending.len() >= SAVE_BATCH {
                save(&self.shared, &mut pending)?;
            }
            let attempted = analysed + failed;
            if attempted > 0 && attempted % PROGRESS_EVERY == 0 {
                log::info!(
                    "similar tracks: {attempted} of {} done in {} s",
                    todo.len(),
                    started.elapsed().as_secs()
                );
            }
        }
        save(&self.shared, &mut pending)?;
        log::info!(
            "similar tracks: pass finished in {} s: {analysed} analysed, {failed} failed",
            started.elapsed().as_secs()
        );
        Ok(())
    }
}

fn save(shared: &Shared, pending: &mut Vec<(i64, Vec<f32>)>) -> anyhow::Result<()> {
    if pending.is_empty() {
        return Ok(());
    }
    shared.repo.save_embeddings(EMBEDDING_VERSION, pending)?;
    shared.neighbors.invalidate();
    pending.clear();
    Ok(())
}

fn file_for(track: &Track, remote_media: &RemoteMedia) -> Option<PathBuf> {
    match track.local_file() {
        Some(path) => Some(path.to_path_buf()),
        None => remote_media.peek_cached(&track.remote()?),
    }
}

fn range_of(track: &Track) -> Option<TrackRange> {
    track.is_cue.then(|| TrackRange {
        start: Duration::from_millis(track.start_offset_ms.max(0) as u64),
        length: track
            .duration_ms
            .filter(|ms| *ms > 0)
            .map(|ms| Duration::from_millis(ms as u64)),
    })
}

fn analyse(analyzer: &Analyzer, path: &Path, track: &Track) -> Result<Vec<f32>, EmbedError> {
    let run = || {
        let decoder = Decoder::open(path).map_err(|e| EmbedError::Decode(e.to_string()))?;
        let prepared = audio_embedding::prepare(Job {
            source: Box::new(decoder),
            range: range_of(track),
        })?;
        analyzer
            .embed(&[prepared])
            .pop()
            .unwrap_or_else(|| Err(EmbedError::Model("no result".into())))
            .map(|embedding| embedding.vector.into_vec())
    };
    catch_unwind(AssertUnwindSafe(run))
        .unwrap_or_else(|_| Err(EmbedError::Decode("the analysis panicked".into())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(path: &str, is_cue: bool, start: i32, duration: Option<i64>) -> Track {
        Track {
            id: 1,
            path: path.into(),
            title: "t".into(),
            album_id: None,
            track_number: None,
            disc_number: 1,
            duration_ms: duration,
            year: None,
            cover_art_id: None,
            start_offset_ms: start,
            liked: false,
            bitrate: None,
            is_cue,
            available: true,
        }
    }

    #[test]
    fn a_cue_track_is_analysed_over_its_own_range() {
        assert_eq!(
            range_of(&track("/m/image.flac", true, 61_500, Some(200_000))),
            Some(TrackRange {
                start: Duration::from_millis(61_500),
                length: Some(Duration::from_millis(200_000)),
            })
        );
        assert_eq!(
            range_of(&track("/m/image.flac", true, 0, None)),
            Some(TrackRange {
                start: Duration::ZERO,
                length: None,
            })
        );
        assert_eq!(range_of(&track("/m/song.flac", false, 0, Some(1))), None);
    }

    #[test]
    fn a_server_track_is_analysed_only_from_the_cache() {
        let dir = tempfile::tempdir().unwrap();
        let media =
            RemoteMedia::with_cache(crate::remote_media::CacheStore::new(dir.path().into()));
        let remote = track(
            &music_library::remote::locator(4, "song-1", "flac"),
            false,
            0,
            None,
        );
        assert_eq!(file_for(&remote, &media), None);

        let cached = crate::remote_media::CacheStore::new(dir.path().into())
            .path_for(&remote.remote().unwrap());
        std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
        std::fs::write(&cached, b"x").unwrap();
        assert_eq!(file_for(&remote, &media), Some(cached));

        let local = track("/m/song.flac", false, 0, None);
        assert_eq!(
            file_for(&local, &media),
            Some(PathBuf::from("/m/song.flac"))
        );
    }
}

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use music_library::remote::{self, Location};

use crate::library_service::LibraryService;
use crate::playback_opener::OpenerBackend;
use crate::remote_media::PendingStream;

#[derive(Clone)]
pub(crate) struct CastTrack {
    pub id: i64,
    pub path: String,
    pub title: String,
    pub album_id: Option<i64>,
    pub cover_art_id: Option<i64>,
    pub start_offset_ms: i64,
    pub duration: Option<Duration>,
    pub is_cue: bool,
}

impl From<&music_library::Track> for CastTrack {
    fn from(track: &music_library::Track) -> Self {
        Self {
            id: track.id,
            path: track.path.clone(),
            title: track.title.clone(),
            album_id: track.album_id,
            cover_art_id: track.cover_art_id,
            start_offset_ms: i64::from(track.start_offset_ms),
            duration: track
                .duration_ms
                .map(|ms| Duration::from_millis(ms.max(0) as u64)),
            is_cue: track.is_cue,
        }
    }
}

fn extension_of(locator: &str) -> String {
    let extension = match remote::parse(locator) {
        Some(reference) => reference.suffix,
        None => Path::new(locator)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or_default()
            .to_string(),
    };
    extension.to_ascii_lowercase()
}

fn cover_mime(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else {
        "image/jpeg"
    }
}

pub(crate) fn cover(bytes: Vec<u8>) -> cast::Cover {
    cast::Cover {
        mime: cover_mime(&bytes).to_string(),
        bytes: Arc::new(bytes),
    }
}

fn stream_source(backend: &Arc<dyn OpenerBackend>, locator: &str) -> Result<cast::Source, String> {
    let first: PendingStream = backend.open_stream(Path::new(locator))?;
    let keep = Arc::new(Mutex::new(Some(first)));
    let backend = backend.clone();
    let locator = PathBuf::from(locator);
    let opener: cast::StreamOpener = Arc::new(move || {
        let _alive = &keep;
        backend
            .open_stream(&locator)
            .map(|pending| pending.stream)
            .map_err(std::io::Error::other)
    });
    Ok(cast::Source::Stream(opener))
}

pub(crate) fn resolve(
    backend: &Arc<dyn OpenerBackend>,
    library: &LibraryService,
    track: &CastTrack,
    abandoned: &dyn Fn() -> bool,
) -> Result<cast::Media, String> {
    let mut candidates = backend.locators(track.id);
    if candidates.is_empty() {
        candidates.push((track.path.clone(), track.start_offset_ms));
    }
    let mut failure = None;
    let mut found = None;
    for (locator, start_ms) in candidates {
        if abandoned() {
            return Err("superseded".into());
        }
        let source = match remote::location(&locator) {
            Location::File(file) if file.exists() => cast::Source::File(file.to_path_buf()),
            Location::File(_) | Location::Invalid => continue,
            Location::Remote(_) => {
                let path = Path::new(&locator);
                let opened = if let Some(cached) = backend.cached(path) {
                    Ok(cast::Source::File(cached))
                } else if backend.can_stream(path) {
                    stream_source(backend, &locator)
                } else {
                    backend.download(path, abandoned).map(cast::Source::File)
                };
                match opened {
                    Ok(source) => source,
                    Err(e) => {
                        log::warn!("cast: {locator}: {e}");
                        failure = Some(backend.unreachable(&locator).unwrap_or(e));
                        continue;
                    }
                }
            }
        };
        found = Some((source, extension_of(&locator), start_ms));
        break;
    }
    let Some((source, extension, start_ms)) = found else {
        return Err(failure.unwrap_or_else(|| "the file of this track is missing".into()));
    };
    let start = Duration::from_millis(start_ms.max(0) as u64);
    let length = (track.is_cue || !start.is_zero())
        .then_some(track.duration)
        .flatten();
    let artist = library.track_artists(track.id).into_iter().next();
    let album = track.album_id.and_then(|id| library.album_title(id));
    let cover = track
        .cover_art_id
        .and_then(|id| library.get_cover_art_large(id))
        .map(cover);
    Ok(cast::Media {
        source,
        extension,
        start,
        length,
        info: cast::TrackInfo {
            title: track.title.clone(),
            artist,
            album,
        },
        cover,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_come_from_the_file_or_the_server_suffix() {
        assert_eq!(extension_of("/music/A.FLAC"), "flac");
        assert_eq!(extension_of("/music/noext"), "");
    }

    #[test]
    fn covers_are_typed_by_their_magic_bytes() {
        assert_eq!(cover_mime(&[0x89, b'P', b'N', b'G', 0]), "image/png");
        assert_eq!(cover_mime(&[0xff, 0xd8, 0xff]), "image/jpeg");
    }
}

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Read as _, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

use music_indexer::metadata::{best_cover_name, is_artwork_dir_name, is_cover_image_name};
use music_indexer::{AUDIO_EXTENSIONS, CUE_EXTENSIONS, PreparedTrack, ScanEvent};
use music_library::RemoteSong;
use torrent::{Engine, FileEntry, Meta, Want};

use super::error;
use crate::servers::{RemoteError, real_album, real_artist, real_track_number};

const INDEX_VERSION: u32 = 1;
const HEAD_BYTES: u64 = 1024 * 1024;
const TAIL_BYTES: u64 = 256 * 1024;
const METADATA_LIMIT: u64 = 16 * 1024 * 1024;
const SIDECAR_LIMIT: u64 = 1024 * 1024;
const COVER_LIMIT: u64 = 8 * 1024 * 1024;
const PROBE_STALL: Duration = Duration::from_secs(90);
const TAILED_EXTENSIONS: &[&str] = &[
    "mp3", "ogg", "oga", "opus", "m4a", "aac", "wma", "ape", "wv", "dsf",
];
const SIDECAR_EXTENSIONS: &[&str] = &["lrc"];

type Covers = HashMap<String, Vec<u8>>;

#[derive(serde::Serialize, serde::Deserialize)]
struct Stored {
    version: u32,
    songs: Vec<RemoteSong>,
}

fn index_file(state: &Path, info_hash: &str) -> PathBuf {
    state.join(format!("{info_hash}.index.json"))
}

fn covers_dir(state: &Path, info_hash: &str) -> PathBuf {
    state.join(format!("{info_hash}.covers"))
}

pub fn songs(engine: &Engine, info_hash: &str) -> Result<Vec<RemoteSong>, RemoteError> {
    let state = engine.state_dir();
    if let Some(songs) = load(&state, info_hash) {
        return Ok(songs);
    }
    let meta = engine.meta(info_hash).map_err(error)?;
    let (songs, covers) = build(engine, &meta)?;
    let _state = super::state_lock();
    if !engine.is_stored(info_hash) {
        return Err(RemoteError::Other(::torrent::Error::Unknown.to_string()));
    }
    save(&state, info_hash, &songs, &covers)?;
    Ok(songs)
}

pub fn cover(engine: &Engine, info_hash: &str, key: &str) -> Result<Vec<u8>, RemoteError> {
    if key.is_empty() || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(RemoteError::Other(format!("not a cover key: {key}")));
    }
    std::fs::read(covers_dir(&engine.state_dir(), info_hash).join(key))
        .map_err(|e| RemoteError::Other(e.to_string()))
}

fn load(state: &Path, info_hash: &str) -> Option<Vec<RemoteSong>> {
    let bytes = std::fs::read(index_file(state, info_hash)).ok()?;
    let stored: Stored = serde_json::from_slice(&bytes)
        .inspect_err(|e| log::warn!("torrent {info_hash}: the saved index is unreadable: {e}"))
        .ok()?;
    (stored.version == INDEX_VERSION && !stored.songs.is_empty()).then_some(stored.songs)
}

fn save(
    state: &Path,
    info_hash: &str,
    songs: &[RemoteSong],
    covers: &Covers,
) -> Result<(), RemoteError> {
    let failed = |e: std::io::Error| RemoteError::Other(e.to_string());
    let dir = covers_dir(state, info_hash);
    std::fs::create_dir_all(&dir).map_err(failed)?;
    for (hash, bytes) in covers {
        std::fs::write(dir.join(hash), bytes).map_err(failed)?;
    }
    let stored = Stored {
        version: INDEX_VERSION,
        songs: songs.to_vec(),
    };
    let json = serde_json::to_vec(&stored).map_err(|e| RemoteError::Other(e.to_string()))?;
    let target = index_file(state, info_hash);
    let partial = target.with_extension("json.partial");
    std::fs::write(&partial, json).map_err(failed)?;
    std::fs::rename(&partial, &target).map_err(failed)
}

fn extension(path: &Path) -> String {
    path.extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

fn is_audio(path: &Path) -> bool {
    AUDIO_EXTENSIONS.contains(&extension(path).as_str())
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Plan {
    pub wants: Vec<Want>,
    pub files: Vec<usize>,
}

pub(super) fn plan(meta: &Meta) -> Plan {
    let mut plan = Plan::default();
    let mut audio_dirs: HashSet<PathBuf> = HashSet::new();
    for file in &meta.files {
        let ext = extension(&file.path);
        let whole = Want {
            file: file.index,
            start: 0,
            end: file.len,
        };
        if AUDIO_EXTENSIONS.contains(&ext.as_str()) {
            plan.files.push(file.index);
            plan.wants.push(Want {
                end: file.len.min(HEAD_BYTES),
                ..whole
            });
            if TAILED_EXTENSIONS.contains(&ext.as_str()) && file.len > HEAD_BYTES {
                plan.wants.push(Want {
                    start: file.len.saturating_sub(TAIL_BYTES).max(HEAD_BYTES),
                    ..whole
                });
            }
            if let Some(dir) = file.path.parent() {
                audio_dirs.insert(dir.to_path_buf());
            }
        } else if (CUE_EXTENSIONS.contains(&ext.as_str())
            || SIDECAR_EXTENSIONS.contains(&ext.as_str()))
            && file.len <= SIDECAR_LIMIT
        {
            plan.files.push(file.index);
            plan.wants.push(whole);
        }
    }
    let mut covers: Vec<&FileEntry> = audio_dirs
        .iter()
        .filter_map(|dir| cover_for(meta, dir))
        .collect();
    covers.sort_by_key(|file| file.index);
    covers.dedup_by_key(|file| file.index);
    for file in covers {
        plan.files.push(file.index);
        plan.wants.push(Want {
            file: file.index,
            start: 0,
            end: file.len,
        });
    }
    plan
}

fn images_in<'a>(meta: &'a Meta, dir: &Path) -> Vec<&'a FileEntry> {
    meta.files
        .iter()
        .filter(|file| file.path.parent() == Some(dir))
        .filter(|file| {
            file.path
                .file_name()
                .is_some_and(|name| is_cover_image_name(&name.to_string_lossy()))
        })
        .collect()
}

fn artwork_dirs(meta: &Meta, dir: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = meta
        .files
        .iter()
        .filter_map(|file| {
            let parent = file.path.parent()?;
            (parent.parent() == Some(dir)
                && parent
                    .file_name()
                    .is_some_and(|name| is_artwork_dir_name(&name.to_string_lossy())))
            .then(|| parent.to_path_buf())
        })
        .collect();
    dirs.sort();
    dirs.dedup();
    dirs
}

fn best_in<'a>(meta: &'a Meta, dir: &Path) -> Option<&'a FileEntry> {
    let images = images_in(meta, dir);
    let names: Vec<(String, u64)> = images
        .iter()
        .map(|file| {
            let name = file.path.file_name().unwrap_or_default();
            (name.to_string_lossy().into_owned(), file.len)
        })
        .collect();
    let named: Vec<(&str, u64)> = names.iter().map(|(n, len)| (n.as_str(), *len)).collect();
    best_cover_name(&named).map(|ix| images[ix])
}

fn cover_for<'a>(meta: &'a Meta, dir: &Path) -> Option<&'a FileEntry> {
    let mut places = vec![dir.to_path_buf()];
    places.extend(artwork_dirs(meta, dir));
    if let Some(parent) = dir.parent() {
        places.push(parent.to_path_buf());
        places.extend(artwork_dirs(meta, parent));
    }
    places
        .iter()
        .find_map(|place| best_in(meta, place))
        .filter(|file| file.len <= COVER_LIMIT)
}

type Fetched = HashMap<usize, Vec<(u64, u64)>>;

const MORE_ROUNDS: usize = 6;
const MP4_MAX_ATOMS: usize = 64;

fn fetched_of(wants: &[Want]) -> Fetched {
    let mut fetched = Fetched::new();
    add_fetched(&mut fetched, wants);
    fetched
}

fn add_fetched(fetched: &mut Fetched, wants: &[Want]) {
    for want in wants {
        let ranges = fetched.entry(want.file).or_default();
        ranges.push((want.start, want.end));
        ranges.sort_unstable();
        let mut merged: Vec<(u64, u64)> = Vec::with_capacity(ranges.len());
        for &(start, end) in ranges.iter() {
            match merged.last_mut() {
                Some(last) if start <= last.1 => last.1 = last.1.max(end),
                _ => merged.push((start, end)),
            }
        }
        *ranges = merged;
    }
}

fn covered(ranges: &[(u64, u64)], start: u64, end: u64) -> bool {
    start >= end || ranges.iter().any(|&(s, e)| s <= start && end <= e)
}

struct Sparse<'a> {
    file: std::fs::File,
    len: u64,
    have: &'a [(u64, u64)],
}

enum Read<T> {
    Got(T),
    Need(u64, u64),
}

impl Sparse<'_> {
    fn bytes<const N: usize>(&mut self, at: u64) -> Option<Read<[u8; N]>> {
        let end = at.checked_add(N as u64)?;
        if end > self.len {
            return None;
        }
        if !covered(self.have, at, end) {
            return Some(Read::Need(at, end));
        }
        self.file.seek(SeekFrom::Start(at)).ok()?;
        let mut buf = [0u8; N];
        self.file.read_exact(&mut buf).ok()?;
        Some(Read::Got(buf))
    }

    fn need(&self, start: u64, end: u64) -> Option<(u64, u64)> {
        let end = end.min(self.len);
        (end <= METADATA_LIMIT && !covered(self.have, start, end)).then_some((start, end))
    }
}

fn flac_needs(sparse: &mut Sparse) -> Option<(u64, u64)> {
    let Read::Got(magic) = sparse.bytes::<4>(0)? else {
        return None;
    };
    if &magic != b"fLaC" {
        return None;
    }
    let mut at = 4u64;
    loop {
        let header = match sparse.bytes::<4>(at)? {
            Read::Got(header) => header,
            Read::Need(start, end) => return Some((start, end)),
        };
        let len = u64::from(header[1]) << 16 | u64::from(header[2]) << 8 | u64::from(header[3]);
        let next = at + 4 + len;
        let padding = header[0] & 0x7f == 1;
        if !padding && let Some(need) = sparse.need(at + 4, next) {
            return Some(need);
        }
        if header[0] & 0x80 != 0 || next > METADATA_LIMIT {
            return None;
        }
        at = next;
    }
}

fn id3_needs(sparse: &mut Sparse) -> Option<(u64, u64)> {
    let header = match sparse.bytes::<10>(0)? {
        Read::Got(header) => header,
        Read::Need(start, end) => return Some((start, end)),
    };
    if &header[..3] != b"ID3" {
        return None;
    }
    let size = header[6..10]
        .iter()
        .fold(0u64, |acc, b| acc << 7 | u64::from(b & 0x7f));
    let footer = if header[5] & 0x10 != 0 { 10 } else { 0 };
    sparse.need(0, 10 + size + footer)
}

fn mp4_needs(sparse: &mut Sparse) -> Option<(u64, u64)> {
    let mut at = 0u64;
    for _ in 0..MP4_MAX_ATOMS {
        let header = match sparse.bytes::<8>(at)? {
            Read::Got(header) => header,
            Read::Need(start, end) => return Some((start, end)),
        };
        let mut size = u64::from(u32::from_be_bytes(header[..4].try_into().ok()?));
        if size == 1 {
            size = match sparse.bytes::<8>(at + 8)? {
                Read::Got(large) => u64::from_be_bytes(large),
                Read::Need(start, end) => return Some((start, end)),
            };
        } else if size == 0 {
            size = sparse.len.checked_sub(at)?;
        }
        if size < 8 {
            return None;
        }
        if &header[4..8] == b"moov" {
            let end = at.checked_add(size)?.min(sparse.len);
            if end - at > METADATA_LIMIT || covered(sparse.have, at, end) {
                return None;
            }
            return Some((at, end));
        }
        at = at.checked_add(size)?;
    }
    None
}

pub(super) fn metadata_needs(path: &Path, len: u64, have: &[(u64, u64)]) -> Option<(u64, u64)> {
    let file = std::fs::File::open(path).ok()?;
    let mut sparse = Sparse { file, len, have };
    match extension(path).as_str() {
        "flac" => flac_needs(&mut sparse),
        "mp3" | "aac" => id3_needs(&mut sparse),
        "m4a" | "mp4" | "m4b" => mp4_needs(&mut sparse),
        _ => None,
    }
}

fn more_metadata(meta: &Meta, root: &Path, fetched: &Fetched) -> Vec<Want> {
    meta.files
        .iter()
        .filter(|file| is_audio(&file.path))
        .filter_map(|file| {
            let have = fetched.get(&file.index).map_or(&[][..], Vec::as_slice);
            let (start, end) = metadata_needs(&root.join(&file.path), file.len, have)?;
            Some(Want {
                file: file.index,
                start,
                end,
            })
        })
        .collect()
}

fn copy_ranges(
    source: &Path,
    target: &Path,
    len: u64,
    ranges: &[(u64, u64)],
) -> std::io::Result<()> {
    let mut from = std::fs::File::open(source)?;
    let mut to = std::fs::File::create(target)?;
    to.set_len(len)?;
    for &(start, end) in ranges {
        from.seek(SeekFrom::Start(start))?;
        to.seek(SeekFrom::Start(start))?;
        std::io::copy(&mut (&mut from).take(end - start), &mut to)?;
    }
    Ok(())
}

fn link_view(
    meta: &Meta,
    plan: &Plan,
    fetched: &Fetched,
    root: &Path,
    view: &Path,
) -> Result<(), RemoteError> {
    let failed = |e: std::io::Error| RemoteError::Other(e.to_string());
    if view.exists() {
        std::fs::remove_dir_all(view).map_err(failed)?;
    }
    let by_index: HashMap<usize, &FileEntry> =
        meta.files.iter().map(|file| (file.index, file)).collect();
    for index in &plan.files {
        let Some(file) = by_index.get(index) else {
            continue;
        };
        let target = view.join(&file.path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(failed)?;
        }
        let source = root.join(&file.path);
        if std::fs::hard_link(&source, &target).is_err() {
            let ranges = fetched.get(index).map_or(&[][..], Vec::as_slice);
            copy_ranges(&source, &target, file.len, ranges).map_err(failed)?;
        }
    }
    Ok(())
}

fn scan(view: &Path) -> (Vec<PreparedTrack>, Covers) {
    let sources = music_indexer::collect_sources(&[view.to_path_buf()]);
    let (tx, rx) = flume::unbounded();
    music_indexer::run(sources, HashSet::new(), tx);
    let mut tracks = Vec::new();
    let mut covers = HashMap::new();
    for event in rx.drain() {
        match event {
            ScanEvent::Track(track) => tracks.push(track),
            ScanEvent::Cover { hash, large, .. } => {
                covers.insert(hash, large);
            }
            ScanEvent::Error { path, error } => {
                log::warn!("torrent file {path:?} not indexed: {error}");
            }
            _ => {}
        }
    }
    (tracks, covers)
}

pub(super) fn song(track: PreparedTrack, file: &FileEntry) -> RemoteSong {
    let mut artists = track.artist_names.into_iter();
    let artist = real_artist(artists.next());
    let title = track
        .title
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| {
            file.path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
    RemoteSong {
        key: file.index.to_string(),
        title,
        artist,
        album: real_album(track.album_title),
        album_artist: real_artist(track.album_artist_names.into_iter().next()),
        track_number: real_track_number(track.track_number),
        disc_number: track.disc_number,
        year: track.year,
        genre: track.genres.into_iter().next(),
        duration_ms: track.duration_ms.map(|ms| ms as i64),
        size: Some(file.len as i64),
        suffix: Some(extension(&file.path)),
        content_type: None,
        bitrate_kbps: track.bitrate,
        cover_key: track.cover_hash,
        cover_hash: None,
        artist_aliases: artists.collect(),
        start_offset_ms: track
            .is_cue
            .then(|| track.start_offset_ms.unwrap_or(0) as i64),
    }
}

fn build(engine: &Engine, meta: &Meta) -> Result<(Vec<RemoteSong>, Covers), RemoteError> {
    let plan = plan(meta);
    if plan.files.iter().all(|index| {
        meta.files
            .iter()
            .find(|file| file.index == *index)
            .is_none_or(|file| !is_audio(&file.path))
    }) {
        return Err(RemoteError::Other("the torrent has no audio files".into()));
    }
    let head = engine
        .probe(&meta.info_hash, &plan.wants, PROBE_STALL)
        .map_err(error)?;
    let mut fetched = fetched_of(&plan.wants);
    let mut more_probes = Vec::new();
    for _ in 0..MORE_ROUNDS {
        let more = more_metadata(meta, head.root(), &fetched);
        if more.is_empty() {
            break;
        }
        more_probes.push(
            engine
                .probe(&meta.info_hash, &more, PROBE_STALL)
                .map_err(error)?,
        );
        add_fetched(&mut fetched, &more);
    }
    let view = head.root().with_extension("view");
    let scanned = link_view(meta, &plan, &fetched, head.root(), &view).map(|()| scan(&view));
    drop(more_probes);
    if let Err(e) = std::fs::remove_dir_all(&view) {
        log::warn!("torrent {}: {view:?} not removed: {e}", meta.info_hash);
    }
    let (tracks, covers) = scanned?;
    let by_path: HashMap<&Path, &FileEntry> = meta
        .files
        .iter()
        .map(|file| (file.path.as_path(), file))
        .collect();
    let mut songs: BTreeMap<(usize, i64), RemoteSong> = BTreeMap::new();
    for track in tracks {
        let Some(file) = track
            .path
            .strip_prefix(&view)
            .ok()
            .and_then(|relative| by_path.get(relative))
        else {
            log::warn!(
                "torrent {}: {:?} is not in the torrent",
                meta.info_hash,
                track.path
            );
            continue;
        };
        let song = song(track, file);
        songs.insert((file.index, song.start_offset_ms.unwrap_or(-1)), song);
    }
    let songs: Vec<RemoteSong> = songs.into_values().collect();
    if songs.is_empty() {
        return Err(RemoteError::Other(
            "no audio file in the torrent could be read".into(),
        ));
    }
    let used: HashSet<&String> = songs.iter().filter_map(|s| s.cover_key.as_ref()).collect();
    let covers = covers
        .into_iter()
        .filter(|(hash, _)| used.contains(hash))
        .collect();
    log::info!(
        "torrent {}: indexed {} songs from {} files",
        meta.info_hash,
        songs.len(),
        plan.files.len()
    );
    Ok((songs, covers))
}

#[cfg(test)]
mod tests;

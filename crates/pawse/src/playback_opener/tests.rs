use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::sync::atomic::AtomicBool;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use music_library::remote::locator;

use super::*;

const WAIT: Duration = Duration::from_secs(5);
const QUIET: Duration = Duration::from_millis(300);

#[derive(Default)]
struct Gate {
    aborted: AtomicBool,
    reading: AtomicBool,
    lock: Mutex<()>,
    changed: Condvar,
}

impl Gate {
    fn wait_until_read(&self) {
        let deadline = std::time::Instant::now() + WAIT;
        while !self.reading.load(Ordering::Acquire) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl StreamControl for Gate {
    fn abort(&self) {
        self.aborted.store(true, Ordering::Release);
        let _lock = self.lock.lock().unwrap();
        self.changed.notify_all();
    }

    fn failure(&self) -> Option<String> {
        None
    }
}

struct Memory {
    bytes: std::io::Cursor<Vec<u8>>,
    gate: Option<Arc<Gate>>,
}

impl Read for Memory {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if let Some(gate) = &self.gate {
            gate.reading.store(true, Ordering::Release);
            let mut lock = gate.lock.lock().unwrap();
            while !gate.aborted.load(Ordering::Acquire) {
                lock = gate.changed.wait(lock).unwrap();
            }
            return Err(std::io::Error::other("aborted"));
        }
        self.bytes.read(buf)
    }
}

impl Seek for Memory {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.bytes.seek(pos)
    }
}

impl audio_engine::MediaStream for Memory {
    fn byte_len(&self) -> Option<u64> {
        Some(self.bytes.get_ref().len() as u64)
    }
}

enum Remote {
    Wav,
    Blocks(Arc<Gate>),
    Fails(&'static str),
}

#[derive(Default)]
struct Fake {
    locators: HashMap<i64, Vec<(String, i64)>>,
    cached: HashSet<PathBuf>,
    remotes: Mutex<HashMap<PathBuf, Remote>>,
    downloads: Mutex<Vec<PathBuf>>,
    down: Option<&'static str>,
}

fn wav() -> Vec<u8> {
    let root = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    std::fs::read(PathBuf::from(root).join("../../fixtures/sine_440_16_44_stereo.wav")).unwrap()
}

impl OpenerBackend for Fake {
    fn locators(&self, track_id: i64) -> Vec<(String, i64)> {
        self.locators.get(&track_id).cloned().unwrap_or_default()
    }

    fn cached(&self, locator: &Path) -> Option<PathBuf> {
        self.cached.contains(locator).then(|| locator.to_path_buf())
    }

    fn can_stream(&self, locator: &Path) -> bool {
        locator.extension().is_some_and(|ext| ext != "ape")
    }

    fn download(&self, locator: &Path, _: &dyn Fn() -> bool) -> Result<PathBuf, String> {
        self.downloads.lock().unwrap().push(locator.to_path_buf());
        Ok(locator.to_path_buf())
    }

    fn open_stream(&self, locator: &Path) -> Result<PendingStream, String> {
        let remotes = self.remotes.lock().unwrap();
        let (gate, bytes) = match remotes.get(locator) {
            Some(Remote::Wav) => (Arc::new(Gate::default()), wav()),
            Some(Remote::Blocks(gate)) => (gate.clone(), Vec::new()),
            Some(Remote::Fails(message)) => return Err((*message).to_string()),
            None => return Err("unknown".into()),
        };
        let blocking = matches!(remotes.get(locator), Some(Remote::Blocks(_)));
        Ok(PendingStream {
            stream: Box::new(Memory {
                bytes: std::io::Cursor::new(bytes),
                gate: blocking.then(|| gate.clone()),
            }),
            control: gate,
            extension: "wav".into(),
        })
    }

    fn unreachable(&self, _: &str) -> Option<String> {
        self.down.map(str::to_string)
    }
}

fn with_backend(fake: Fake) -> (PlaybackOpener, flume::Receiver<Command>) {
    let (tx, rx) = flume::unbounded();
    let sink: Sink = Arc::new(move |command| {
        let _ = tx.send(command);
    });
    (PlaybackOpener::new(Arc::new(fake), sink), rx)
}

fn request(id: i64, path: &str) -> TrackRequest {
    TrackRequest {
        id,
        path: path.into(),
        start_offset_ms: 0,
        duration: Some(Duration::from_secs(60)),
    }
}

fn next(rx: &flume::Receiver<Command>) -> Command {
    rx.recv_timeout(WAIT).expect("no command")
}

fn assert_quiet(rx: &flume::Receiver<Command>) {
    if let Ok(command) = rx.recv_timeout(QUIET) {
        panic!("unexpected {command:?}");
    }
}

fn local_path(command: &Command, prepared_wanted: bool) -> PathBuf {
    match command {
        Command::SetLocalTrack { path, prepared, .. } if *prepared == prepared_wanted => {
            path.clone()
        }
        other => panic!("expected SetLocalTrack(prepared: {prepared_wanted}), got {other:?}"),
    }
}

#[test]
fn a_local_file_is_set_and_played_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a.flac");
    std::fs::write(&file, b"x").unwrap();
    let (opener, rx) = with_backend(Fake::default());
    opener.start(&request(1, &file.to_string_lossy()), AfterLoad::Play);
    assert_eq!(local_path(&next(&rx), false), file);
    assert!(matches!(next(&rx), Command::Play { fade_in: true }));
    assert_quiet(&rx);
}

#[test]
fn a_cached_server_track_is_not_prepared_and_stays_paused_when_asked() {
    let track = locator(3, "k", "flac");
    let (opener, rx) = with_backend(Fake {
        cached: HashSet::from([PathBuf::from(&track)]),
        ..Default::default()
    });
    opener.start(&request(1, &track), AfterLoad::Stay);
    assert_eq!(local_path(&next(&rx), false), PathBuf::from(&track));
    assert_quiet(&rx);
}

#[test]
fn a_vanished_local_copy_falls_through_to_the_server_stream() {
    let server = locator(3, "k", "wav");
    let (opener, rx) = with_backend(Fake {
        locators: HashMap::from([(1, vec![("/gone/a.wav".to_string(), 0), (server.clone(), 0)])]),
        remotes: Mutex::new(HashMap::from([(PathBuf::from(&server), Remote::Wav)])),
        ..Default::default()
    });
    opener.start(&request(1, "/gone/a.wav"), AfterLoad::PlayGapless);
    assert!(matches!(
        next(&rx),
        Command::Prepare {
            play: Some(false),
            ..
        }
    ));
    assert!(matches!(next(&rx), Command::SetStreamTrack(_)));
    assert_quiet(&rx);
}

#[test]
fn a_copy_that_cannot_stream_is_downloaded_whole_first() {
    let server = locator(3, "k", "ape");
    let fake = Fake {
        locators: HashMap::from([(1, vec![(server.clone(), 1500)])]),
        ..Default::default()
    };
    let (opener, rx) = with_backend(fake);
    opener.start(&request(1, &server), AfterLoad::Play);
    assert!(matches!(next(&rx), Command::Prepare { .. }));
    match next(&rx) {
        Command::SetLocalTrack {
            path,
            start_offset,
            prepared: true,
            ..
        } => {
            assert_eq!(path, PathBuf::from(&server));
            assert_eq!(start_offset, Some(Duration::from_millis(1500)));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn when_every_copy_fails_the_last_reason_is_shown_or_the_server_is_called_down() {
    let server = locator(3, "k", "flac");
    let failing = || Fake {
        locators: HashMap::from([(1, vec![(server.clone(), 0)])]),
        remotes: Mutex::new(HashMap::from([(
            PathBuf::from(&server),
            Remote::Fails("no suitable format reader"),
        )])),
        ..Default::default()
    };
    let (opener, rx) = with_backend(failing());
    opener.start(&request(1, &server), AfterLoad::Play);
    assert!(matches!(next(&rx), Command::Prepare { .. }));
    assert!(matches!(next(&rx), Command::Fail(m) if m == "no suitable format reader"));

    let (opener, rx) = with_backend(Fake {
        down: Some("Server unreachable"),
        ..failing()
    });
    opener.start(&request(1, &server), AfterLoad::Play);
    assert!(matches!(next(&rx), Command::Prepare { .. }));
    assert!(matches!(next(&rx), Command::Fail(m) if m == "Server unreachable"));
}

#[test]
fn without_any_known_copy_the_catalog_path_is_tried_as_is() {
    let (opener, rx) = with_backend(Fake::default());
    opener.start(&request(1, "/gone/a.flac"), AfterLoad::Stay);
    assert!(matches!(next(&rx), Command::Prepare { play: None, .. }));
    assert_eq!(local_path(&next(&rx), true), PathBuf::from("/gone/a.flac"));
}

fn blocking(gate: &Arc<Gate>) -> (PlaybackOpener, flume::Receiver<Command>, String) {
    let server = locator(3, "slow", "flac");
    let (opener, rx) = with_backend(Fake {
        locators: HashMap::from([(1, vec![(server.clone(), 0)])]),
        remotes: Mutex::new(HashMap::from([(
            PathBuf::from(&server),
            Remote::Blocks(gate.clone()),
        )])),
        ..Default::default()
    });
    (opener, rx, server)
}

#[test]
fn a_newer_track_aborts_the_one_still_opening_and_is_never_overtaken() {
    let gate = Arc::new(Gate::default());
    let (opener, rx, slow) = blocking(&gate);
    opener.start(&request(1, &slow), AfterLoad::Play);
    assert!(matches!(next(&rx), Command::Prepare { .. }));
    gate.wait_until_read();

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("b.flac");
    std::fs::write(&file, b"x").unwrap();
    opener.start(&request(2, &file.to_string_lossy()), AfterLoad::Play);

    assert!(gate.aborted.load(Ordering::Acquire));
    assert_eq!(local_path(&next(&rx), false), file);
    assert!(matches!(next(&rx), Command::Play { .. }));
    assert_quiet(&rx);
}

#[test]
fn stopping_cancels_a_track_that_is_still_opening() {
    let gate = Arc::new(Gate::default());
    let (opener, rx, slow) = blocking(&gate);
    opener.start(&request(1, &slow), AfterLoad::Play);
    assert!(matches!(next(&rx), Command::Prepare { .. }));
    gate.wait_until_read();
    opener.stop();
    assert!(gate.aborted.load(Ordering::Acquire));
    assert!(matches!(next(&rx), Command::Stop));
    assert_quiet(&rx);
}

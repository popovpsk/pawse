use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::testing::Seeder as Seeding;
use crate::{Engine, Error, Input, Want};

const PIECE: u32 = 256 * 1024;
const WAIT: Duration = Duration::from_secs(30);

struct Seeder {
    seeding: Seeding,
    _dir: tempfile::TempDir,
    torrent: Vec<u8>,
    files: Vec<(PathBuf, Vec<u8>)>,
}

fn content(seed: u64, len: usize) -> Vec<u8> {
    let mut x = seed.max(1);
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect()
}

fn seeder() -> Seeder {
    let dir = tempfile::tempdir().unwrap();
    let album = dir.path().join("album");
    std::fs::create_dir_all(album.join("scans")).unwrap();
    let files: Vec<(PathBuf, Vec<u8>)> = vec![
        (PathBuf::from("01.flac"), content(1, 3 * 1024 * 1024 + 777)),
        (PathBuf::from("02.flac"), content(2, 2 * 1024 * 1024 + 5)),
        (
            PathBuf::from("album.cue"),
            b"FILE \"01.flac\" WAVE\n".to_vec(),
        ),
        (PathBuf::from("scans/front.jpg"), content(3, 40_000)),
    ];
    for (path, bytes) in &files {
        std::fs::write(album.join(path), bytes).unwrap();
    }
    let seeding = Seeding::new(&album, PIECE);
    Seeder {
        torrent: seeding.torrent.clone(),
        seeding,
        _dir: dir,
        files,
    }
}

fn engine(seeder: &Seeder, root: &Path) -> Engine {
    seeder.seeding.engine(root)
}

fn index_of(meta: &crate::Meta, path: &str) -> usize {
    meta.files
        .iter()
        .find(|f| f.path == Path::new(path))
        .unwrap()
        .index
}

fn wait_until(check: impl Fn() -> bool) -> bool {
    let started = Instant::now();
    while started.elapsed() < WAIT {
        if check() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

#[test]
fn a_torrent_file_is_listed_and_stored() {
    let seeder = seeder();
    let root = tempfile::tempdir().unwrap();
    let engine = engine(&seeder, root.path());
    let meta = engine
        .resolve(Input::File(seeder.torrent.clone()), WAIT)
        .unwrap();
    assert!(crate::is_info_hash(&meta.info_hash), "{}", meta.info_hash);
    assert_eq!(meta.name, "album");
    let mut listed: Vec<(PathBuf, u64)> =
        meta.files.iter().map(|f| (f.path.clone(), f.len)).collect();
    listed.sort();
    let mut expected: Vec<(PathBuf, u64)> = seeder
        .files
        .iter()
        .map(|(p, b)| (p.clone(), b.len() as u64))
        .collect();
    expected.sort();
    assert_eq!(listed, expected);
    assert!(engine.is_stored(&meta.info_hash));
    assert_eq!(engine.meta(&meta.info_hash).unwrap(), meta);
    engine.forget(&meta.info_hash);
    assert!(!engine.is_stored(&meta.info_hash));
    assert_eq!(engine.meta(&meta.info_hash).unwrap_err(), Error::Unknown);
}

#[test]
fn a_probe_fetches_only_the_asked_ranges_and_is_thrown_away() {
    let seeder = seeder();
    let root = tempfile::tempdir().unwrap();
    let engine = engine(&seeder, root.path());
    let meta = engine
        .resolve(Input::File(seeder.torrent.clone()), WAIT)
        .unwrap();
    let first = index_of(&meta, "01.flac");
    let cue = index_of(&meta, "album.cue");
    let len = seeder.files[0].1.len() as u64;
    let probe = engine
        .probe(
            &meta.info_hash,
            &[
                Want {
                    file: first,
                    start: 0,
                    end: 1000,
                },
                Want {
                    file: first,
                    start: len - 100,
                    end: len,
                },
                Want {
                    file: cue,
                    start: 0,
                    end: 20,
                },
            ],
            WAIT,
        )
        .unwrap();
    let on_disk = probe.root().join("01.flac");
    let bytes = std::fs::read(&on_disk).unwrap();
    assert_eq!(bytes.len() as u64, len);
    assert_eq!(bytes[..1000], seeder.files[0].1[..1000]);
    assert_eq!(
        bytes[bytes.len() - 100..],
        seeder.files[0].1[seeder.files[0].1.len() - 100..]
    );
    assert_eq!(
        std::fs::read(probe.root().join("album.cue")).unwrap(),
        seeder.files[2].1
    );
    let middle = len as usize / 2;
    assert!(bytes[middle..middle + 4096].iter().all(|b| *b == 0));
    let dir = probe.root().to_path_buf();
    drop(probe);
    assert!(wait_until(|| !dir.exists() && engine.loaded().is_empty()));
}

#[test]
fn a_read_returns_the_bytes_at_the_offset() {
    let seeder = seeder();
    let root = tempfile::tempdir().unwrap();
    let engine = engine(&seeder, root.path());
    let meta = engine
        .resolve(Input::File(seeder.torrent.clone()), WAIT)
        .unwrap();
    let second = index_of(&meta, "02.flac");
    let source = &seeder.files[1].1;
    let start = 1_000_000u64;
    let mut body = engine
        .read(&meta.info_hash, second, start, Some(start + 600_000), WAIT)
        .unwrap();
    assert_eq!((body.offset(), body.total()), (start, source.len() as u64));
    let mut got = Vec::new();
    body.read_to_end(&mut got).unwrap();
    assert_eq!(got, source[start as usize..start as usize + 600_000]);
    let mut tail = engine
        .read(
            &meta.info_hash,
            second,
            source.len() as u64 - 10,
            None,
            WAIT,
        )
        .unwrap();
    let mut end = Vec::new();
    tail.read_to_end(&mut end).unwrap();
    assert_eq!(end, source[source.len() - 10..]);
    assert_eq!(engine.loaded(), vec![meta.info_hash.clone()]);
    let swarm = engine.swarm(&meta.info_hash).unwrap();
    assert!(
        swarm.connected >= 1 && swarm.known >= swarm.connected,
        "{swarm:?}"
    );
    assert_eq!(engine.swarm(&"1".repeat(40)), None);
}

#[test]
fn an_unknown_torrent_is_an_error_not_a_hang() {
    let seeder = seeder();
    let root = tempfile::tempdir().unwrap();
    let engine = engine(&seeder, root.path());
    let missing = "0".repeat(40);
    assert!(matches!(
        engine.read(&missing, 0, 0, None, WAIT),
        Err(Error::Unknown)
    ));
    assert!(matches!(
        engine.probe("../etc", &[], WAIT),
        Err(Error::Unknown)
    ));
}

#[test]
fn a_new_engine_starts_with_an_empty_work_dir() {
    let seeder = seeder();
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("work/stale")).unwrap();
    std::fs::write(root.path().join("work/stale/x.flac"), b"x").unwrap();
    let _engine = engine(&seeder, root.path());
    assert!(!root.path().join("work/stale").exists());
}

#[test]
fn a_read_right_after_a_discarded_probe_gets_the_bytes() {
    let seeder = seeder();
    let root = tempfile::tempdir().unwrap();
    let engine = engine(&seeder, root.path());
    let meta = engine
        .resolve(Input::File(seeder.torrent.clone()), WAIT)
        .unwrap();
    let first = index_of(&meta, "01.flac");
    let source = &seeder.files[0].1;
    for round in 0..5u64 {
        let probe = engine
            .probe(
                &meta.info_hash,
                &[Want {
                    file: first,
                    start: 0,
                    end: 1000,
                }],
                WAIT,
            )
            .unwrap();
        drop(probe);
        let start = round * 300_000;
        let mut body = engine
            .read(&meta.info_hash, first, start, Some(start + 200_000), WAIT)
            .unwrap();
        let mut got = Vec::new();
        body.read_to_end(&mut got).unwrap();
        assert_eq!(
            got,
            source[start as usize..start as usize + 200_000],
            "{round}"
        );
    }
}

#[test]
fn switching_uploads_off_keeps_a_running_session_reading() {
    let seeder = seeder();
    let root = tempfile::tempdir().unwrap();
    let engine = engine(&seeder, root.path());
    let meta = engine
        .resolve(Input::File(seeder.torrent.clone()), WAIT)
        .unwrap();
    let second = index_of(&meta, "02.flac");
    let source = &seeder.files[1].1;
    engine.set_upload(crate::Upload::Off);
    let mut got = Vec::new();
    engine
        .read(&meta.info_hash, second, 0, None, WAIT)
        .unwrap()
        .read_to_end(&mut got)
        .unwrap();
    assert_eq!(&got, source);
    engine.set_upload(crate::Upload::WhileActive);
}

#[test]
fn a_probe_without_progress_gives_up_after_the_stall_time() {
    let seeder = seeder();
    let root = tempfile::tempdir().unwrap();
    let engine = Engine::new(crate::Config {
        work_dir: root.path().join("work"),
        state_dir: root.path().join("state"),
        upload: crate::Upload::WhileActive,
        idle_unload: Duration::from_secs(600),
        work_limit_bytes: u64::MAX,
        network: crate::Network::Local {
            listen_port: crate::testing::free_port(),
            peers: vec![([127, 0, 0, 1], crate::testing::free_port()).into()],
        },
    })
    .unwrap();
    let meta = engine
        .resolve(Input::File(seeder.torrent.clone()), WAIT)
        .unwrap();
    let started = Instant::now();
    let probe = engine.probe(
        &meta.info_hash,
        &[Want {
            file: index_of(&meta, "01.flac"),
            start: 0,
            end: 1000,
        }],
        Duration::from_secs(2),
    );
    assert!(matches!(probe, Err(Error::Timeout)));
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn parallel_first_calls_share_one_session() {
    let seeder = seeder();
    let root = tempfile::tempdir().unwrap();
    let engine = engine(&seeder, root.path());
    let results: Vec<_> = std::thread::scope(|scope| {
        let calls: Vec<_> = (0..4)
            .map(|_| {
                let engine = engine.clone();
                let torrent = seeder.torrent.clone();
                scope.spawn(move || engine.resolve(Input::File(torrent), WAIT))
            })
            .collect();
        calls.into_iter().map(|call| call.join().unwrap()).collect()
    });
    for result in results {
        result.unwrap();
    }
}

#[test]
fn a_probe_of_many_files_at_once_does_not_stall() {
    let dir = tempfile::tempdir().unwrap();
    let album = dir.path().join("many");
    std::fs::create_dir_all(&album).unwrap();
    for i in 0..12u64 {
        std::fs::write(
            album.join(format!("{i:02}.flac")),
            content(i + 10, 600_000 + i as usize),
        )
        .unwrap();
    }
    let seeding = crate::testing::Seeder::new(&album, 64 * 1024);
    let root = tempfile::tempdir().unwrap();
    let engine = seeding.engine(root.path());
    let meta = engine
        .resolve(Input::File(seeding.torrent.clone()), WAIT)
        .unwrap();
    let wants: Vec<Want> = meta
        .files
        .iter()
        .map(|file| Want {
            file: file.index,
            start: 0,
            end: 1000,
        })
        .collect();
    engine.probe(&meta.info_hash, &wants, WAIT).unwrap();
    let mut bodies: Vec<_> = std::thread::scope(|scope| {
        let opens: Vec<_> = meta
            .files
            .iter()
            .map(|file| {
                let engine = &engine;
                let hash = &meta.info_hash;
                scope.spawn(move || engine.read(hash, file.index, 300_000, Some(300_010), WAIT))
            })
            .collect();
        opens
            .into_iter()
            .map(|open| open.join().unwrap().unwrap())
            .collect()
    });
    let mut got = Vec::new();
    bodies[11].read_to_end(&mut got).unwrap();
    assert_eq!(got.len(), 10);
}

#[test]
fn big_pieces_get_helper_streams_small_ones_do_not() {
    let mib = 1024 * 1024;
    assert_eq!(crate::engine::helper_count(256 * 1024), 0);
    assert_eq!(crate::engine::helper_count(mib), 0);
    assert_eq!(crate::engine::helper_count(4 * mib), 0);
    assert_eq!(crate::engine::helper_count(8 * mib), 1);
    assert_eq!(crate::engine::helper_count(16 * mib), 3);
    assert_eq!(crate::engine::helper_count(64 * mib), 3);
}

#[test]
fn a_read_without_any_incoming_bytes_gives_up_after_the_quiet_time() {
    let seeder = seeder();
    let root = tempfile::tempdir().unwrap();
    let engine = Engine::new(crate::Config {
        work_dir: root.path().join("work"),
        state_dir: root.path().join("state"),
        upload: crate::Upload::WhileActive,
        idle_unload: Duration::from_secs(600),
        work_limit_bytes: u64::MAX,
        network: crate::Network::Local {
            listen_port: crate::testing::free_port(),
            peers: vec![([127, 0, 0, 1], crate::testing::free_port()).into()],
        },
    })
    .unwrap();
    let meta = engine
        .resolve(Input::File(seeder.torrent.clone()), WAIT)
        .unwrap();
    let started = Instant::now();
    let read = engine.read(
        &meta.info_hash,
        index_of(&meta, "01.flac"),
        0,
        Some(100),
        Duration::from_secs(2),
    );
    assert!(matches!(read, Err(Error::Timeout)));
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn a_read_in_a_torrent_with_big_pieces_returns_the_right_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let album = dir.path().join("big");
    std::fs::create_dir_all(&album).unwrap();
    let source = content(77, 80 * 1024 * 1024 + 123);
    std::fs::write(album.join("long.flac"), &source).unwrap();
    let seeding = crate::testing::Seeder::new(&album, 16 * 1024 * 1024);
    let root = tempfile::tempdir().unwrap();
    let engine = seeding.engine(root.path());
    let meta = engine
        .resolve(Input::File(seeding.torrent.clone()), WAIT)
        .unwrap();
    let file = index_of(&meta, "long.flac");
    let start = 20 * 1024 * 1024 + 5;
    let mut got = Vec::new();
    engine
        .read(&meta.info_hash, file, start, Some(start + 300_000), WAIT)
        .unwrap()
        .read_to_end(&mut got)
        .unwrap();
    assert_eq!(got, source[start as usize..start as usize + 300_000]);
}

#[test]
fn a_torrent_over_the_work_limit_is_unloaded_as_soon_as_reads_stop() {
    let seeder = seeder();
    let root = tempfile::tempdir().unwrap();
    let engine = Engine::new(crate::Config {
        work_dir: root.path().join("work"),
        state_dir: root.path().join("state"),
        upload: crate::Upload::WhileActive,
        idle_unload: Duration::from_secs(600),
        work_limit_bytes: 1,
        network: crate::Network::Local {
            listen_port: crate::testing::free_port(),
            peers: vec![seeder.seeding.addr],
        },
    })
    .unwrap();
    let meta = engine
        .resolve(Input::File(seeder.torrent.clone()), WAIT)
        .unwrap();
    let mut got = Vec::new();
    engine
        .read(
            &meta.info_hash,
            index_of(&meta, "02.flac"),
            0,
            Some(1000),
            WAIT,
        )
        .unwrap()
        .read_to_end(&mut got)
        .unwrap();
    assert_eq!(got.len(), 1000);
    assert!(wait_until(|| engine.loaded().is_empty()));
}

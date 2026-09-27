use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use torrent::{FileEntry, Meta, Want};

use super::*;

const MIB: u64 = 1024 * 1024;

fn meta(files: &[(&str, u64)]) -> Meta {
    Meta {
        info_hash: "0".repeat(40),
        name: "t".into(),
        files: files
            .iter()
            .enumerate()
            .map(|(index, (path, len))| FileEntry {
                index,
                path: PathBuf::from(path),
                len: *len,
            })
            .collect(),
    }
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("../../fixtures")
        .join(name)
}

#[test]
fn a_plan_takes_heads_tails_sidecars_and_one_cover_per_album() {
    let meta = meta(&[
        ("A/01.flac", 50 * MIB),
        ("A/02.mp3", 10 * MIB),
        ("A/a.cue", 2_000),
        ("A/folder.jpg", 100_000),
        ("A/back.jpg", 200_000),
        ("A/Scans/front.jpg", 3 * MIB),
        ("A/Scans/booklet 01.jpg", 3 * MIB),
        ("A/log.txt", 1_000),
        ("B/CD1/x.flac", 30 * MIB),
        ("B/cover.png", 500_000),
        ("C/huge.flac", 30 * MIB),
        ("C/cover.jpg", 20 * MIB),
        ("D/small.opus", 300_000),
    ]);
    let plan = plan(&meta);
    let mut wants = plan.wants.clone();
    wants.sort_by_key(|w| (w.file, w.start));
    let want = |file, start, end| Want { file, start, end };
    assert_eq!(
        wants,
        vec![
            want(0, 0, MIB),
            want(1, 0, MIB),
            want(1, 10 * MIB - 256 * 1024, 10 * MIB),
            want(2, 0, 2_000),
            want(3, 0, 100_000),
            want(8, 0, MIB),
            want(9, 0, 500_000),
            want(10, 0, MIB),
            want(12, 0, 300_000),
        ]
    );
    let mut files = plan.files.clone();
    files.sort_unstable();
    assert_eq!(files, vec![0, 1, 2, 3, 8, 9, 10, 12]);
}

#[test]
fn an_artwork_folder_is_used_when_the_album_folder_has_no_cover() {
    let meta = meta(&[
        ("A/01.flac", 5 * MIB),
        ("A/Scans/Front.jpg", MIB),
        ("A/Scans/Back.jpg", MIB),
    ]);
    assert!(plan(&meta).files.contains(&1));
    assert!(!plan(&meta).files.contains(&2));
}

fn sparse(dir: &Path, name: &str, len: u64, parts: &[(u64, &[u8])]) -> PathBuf {
    use std::io::{Seek, SeekFrom, Write};
    let path = dir.join(name);
    let mut file = std::fs::File::create(&path).unwrap();
    file.set_len(len).unwrap();
    for (at, bytes) in parts {
        file.seek(SeekFrom::Start(*at)).unwrap();
        file.write_all(bytes).unwrap();
    }
    path
}

fn rounds(path: &Path, len: u64, mut have: Vec<(u64, u64)>) -> Vec<(u64, u64)> {
    let mut asked = Vec::new();
    for _ in 0..8 {
        let Some(need) = metadata_needs(path, len, &have) else {
            return asked;
        };
        asked.push(need);
        have.push(need);
    }
    panic!("no end after {asked:?}");
}

#[test]
fn flac_metadata_past_the_head_is_fetched_block_by_block() {
    let fixture_len = std::fs::metadata(fixture("tagged_with_cover.flac"))
        .unwrap()
        .len();
    assert_eq!(
        metadata_needs(
            &fixture("tagged_with_cover.flac"),
            fixture_len,
            &[(0, fixture_len)]
        ),
        None
    );
    assert_eq!(
        metadata_needs(&fixture("tagged_with_cover.flac"), fixture_len, &[(0, 4)]),
        Some((4, 8))
    );

    let dir = tempfile::tempdir().unwrap();
    let picture = 3 * MIB;
    let len = 20 * MIB;
    let picture_at = 42u64;
    let padding_at = picture_at + 4 + picture;
    let mut streaminfo = vec![0x00, 0x00, 0x00, 34];
    streaminfo.extend([0u8; 34]);
    let picture_header = [
        0x06,
        (picture >> 16) as u8,
        (picture >> 8) as u8,
        picture as u8,
    ];
    let padding_header = [0x81, 0x00, 0x03, 0xe8];
    let path = sparse(
        dir.path(),
        "a.flac",
        len,
        &[
            (0, b"fLaC"),
            (4, &streaminfo),
            (picture_at, &picture_header),
            (padding_at, &padding_header),
        ],
    );
    assert_eq!(
        rounds(&path, len, vec![(0, MIB)]),
        vec![(picture_at + 4, padding_at), (padding_at, padding_at + 4)]
    );
}

#[test]
fn an_mp4_with_moov_at_the_end_gets_the_whole_moov() {
    let dir = tempfile::tempdir().unwrap();
    let mdat = 4 * MIB;
    let moov = 600 * 1024;
    let len = 24 + mdat + moov;
    let mut ftyp = 24u32.to_be_bytes().to_vec();
    ftyp.extend(b"ftypM4A ");
    let mut mdat_header = (mdat as u32).to_be_bytes().to_vec();
    mdat_header.extend(b"mdat");
    let mut moov_header = (moov as u32).to_be_bytes().to_vec();
    moov_header.extend(b"moov");
    let moov_at = 24 + mdat;
    let path = sparse(
        dir.path(),
        "a.m4a",
        len,
        &[(0, &ftyp), (24, &mdat_header), (moov_at, &moov_header)],
    );
    assert_eq!(
        rounds(&path, len, vec![(0, MIB), (len - 256 * 1024, len)]),
        vec![(moov_at, moov_at + 8), (moov_at, len)]
    );
}

#[test]
fn an_id3_tag_longer_than_the_head_is_fetched_whole() {
    let dir = tempfile::tempdir().unwrap();
    let path = sparse(
        dir.path(),
        "a.mp3",
        5 * MIB,
        &[(0, b"ID3\x04\x00\x10\x00\x00\x02\x01")],
    );
    assert_eq!(
        metadata_needs(&path, 5 * MIB, &[(0, 10)]),
        Some((0, 10 + 257 + 10))
    );
    assert_eq!(metadata_needs(&path, 5 * MIB, &[(0, MIB)]), None);
    let plain = sparse(dir.path(), "b.mp3", 100, &[(0, &[0xff, 0xfb, 0x90, 0x00])]);
    assert_eq!(metadata_needs(&plain, 100, &[(0, 100)]), None);
}

#[test]
fn a_track_becomes_a_song_keyed_by_its_file_with_cue_offsets() {
    let file = FileEntry {
        index: 7,
        path: PathBuf::from("Album/image.FLAC"),
        len: 1234,
    };
    let track = |cue: bool, offset: Option<u64>| music_indexer::PreparedTrack {
        path: PathBuf::from("/view/Album/image.FLAC"),
        title: None,
        artist_names: vec!["[Unknown Artist]".into()],
        album_artist_names: Vec::new(),
        album_title: Some("Album".into()),
        track_number: Some(1997),
        disc_number: Some(1),
        year: None,
        genres: vec!["Rock".into(), "Pop".into()],
        duration_ms: Some(60_000),
        cover_hash: Some("ab".into()),
        start_offset_ms: offset,
        bitrate: Some(900),
        is_cue: cue,
        lyrics: None,
    };
    let whole = song(track(false, None), &file);
    assert_eq!(whole.key, "7");
    assert_eq!(whole.title, "image");
    assert_eq!(whole.artist, None);
    assert_eq!(whole.track_number, None);
    assert_eq!(whole.suffix.as_deref(), Some("flac"));
    assert_eq!(whole.size, Some(1234));
    assert_eq!(whole.genre.as_deref(), Some("Rock"));
    assert_eq!(whole.cover_key.as_deref(), Some("ab"));
    assert_eq!(whole.start_offset_ms, None);
    assert_eq!(song(track(true, None), &file).start_offset_ms, Some(0));
    assert_eq!(
        song(track(true, Some(200)), &file).start_offset_ms,
        Some(200)
    );
}

fn jpeg() -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbImage::from_pixel(8, 8, image::Rgb([10, 200, 10]))
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Jpeg,
        )
        .unwrap();
    bytes
}

fn wait_until(check: impl Fn() -> bool) -> bool {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(30) {
        if check() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

#[test]
fn a_torrent_is_indexed_from_its_heads_and_the_index_is_reused() {
    let content = tempfile::tempdir().unwrap();
    let album = content.path().join("Album");
    std::fs::create_dir_all(album.join("Scans")).unwrap();
    std::fs::copy(
        fixture("tagged_with_cover.flac"),
        album.join("01 Cover.flac"),
    )
    .unwrap();
    std::fs::copy(fixture("tagless.flac"), album.join("image.flac")).unwrap();
    std::fs::write(
        album.join("image.cue"),
        "PERFORMER \"Band\"\nTITLE \"Live\"\nFILE \"image.flac\" WAVE\n  TRACK 01 AUDIO\n    TITLE \"Intro\"\n    INDEX 01 00:00:00\n  TRACK 02 AUDIO\n    TITLE \"Song\"\n    INDEX 01 00:00:15\n",
    )
    .unwrap();
    std::fs::write(album.join("folder.jpg"), jpeg()).unwrap();
    std::fs::write(album.join("Scans/back.jpg"), vec![7u8; 300_000]).unwrap();
    let seeder = torrent::testing::Seeder::new(&album, 16 * 1024);
    let root = tempfile::tempdir().unwrap();
    let engine = seeder.engine(root.path());
    let meta = engine
        .resolve(
            torrent::Input::File(seeder.torrent.clone()),
            Duration::from_secs(30),
        )
        .unwrap();
    let index_of = |path: &str| {
        meta.files
            .iter()
            .find(|f| f.path == Path::new(path))
            .unwrap()
            .index
            .to_string()
    };

    let listed = songs(&engine, &meta.info_hash, &std::sync::Mutex::new(())).unwrap();
    let mut shape: Vec<(String, String, Option<i64>, Option<String>)> = listed
        .iter()
        .map(|s| {
            (
                s.title.clone(),
                s.key.clone(),
                s.start_offset_ms,
                s.artist.clone(),
            )
        })
        .collect();
    shape.sort();
    assert_eq!(
        shape,
        vec![
            (
                "Cover Track".to_string(),
                index_of("01 Cover.flac"),
                None,
                shape[0].3.clone()
            ),
            (
                "Intro".to_string(),
                index_of("image.flac"),
                Some(0),
                Some("Band".into())
            ),
            (
                "Song".to_string(),
                index_of("image.flac"),
                Some(200),
                Some("Band".into())
            ),
        ]
    );
    for song in &listed {
        let key = song.cover_key.as_deref().expect("every song has a cover");
        let bytes = cover(&engine, &meta.info_hash, key).unwrap();
        assert!(image::load_from_memory(&bytes).is_ok());
    }
    let work = root.path().join("work");
    assert!(wait_until(|| {
        std::fs::read_dir(&work).map_or(true, |mut entries| entries.next().is_none())
    }));

    drop(seeder);
    assert_eq!(
        songs(&engine, &meta.info_hash, &std::sync::Mutex::new(())).unwrap(),
        listed
    );
    engine.forget(&meta.info_hash);
    assert!(
        std::fs::read_dir(root.path().join("state"))
            .unwrap()
            .flatten()
            .all(|entry| !entry
                .file_name()
                .to_string_lossy()
                .starts_with(&meta.info_hash))
    );
}

#[test]
fn a_file_the_engine_has_not_grown_to_full_length_is_viewed_at_full_length() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("probe");
    let view = dir.path().join("probe.view");
    std::fs::create_dir_all(root.join("a")).unwrap();
    let head = b"fLaC-head".to_vec();
    std::fs::write(root.join("a/disc.flac"), &head).unwrap();
    let full = 300 * MIB;
    let meta = meta(&[("a/disc.flac", full)]);
    let plan = Plan {
        wants: vec![Want {
            file: 0,
            start: 0,
            end: head.len() as u64,
        }],
        files: vec![0],
    };
    let fetched = fetched_of(&plan.wants);

    link_view(&meta, &plan, &fetched, &root, &view).unwrap();

    let viewed = view.join("a/disc.flac");
    assert_eq!(std::fs::metadata(&viewed).unwrap().len(), full);
    let mut start = vec![0u8; head.len()];
    std::io::Read::read_exact(&mut std::fs::File::open(&viewed).unwrap(), &mut start).unwrap();
    assert_eq!(start, head);
    assert_eq!(
        std::fs::metadata(root.join("a/disc.flac")).unwrap().len(),
        head.len() as u64
    );
}

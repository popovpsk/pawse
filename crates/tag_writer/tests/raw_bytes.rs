//! What the bytes on disk actually say, decoded without lofty.
//!
//! The unit tests write with lofty and read back with lofty, so a mistake lofty
//! makes symmetrically — writing something it alone understands — round-trips
//! clean and still leaves every other player showing the wrong thing. These tests
//! close that loop with a hand-written parser: FLAC metadata blocks and ID3v2
//! frames are simple enough to decode from the spec in a page of code, and the
//! result owes nothing to the library under test.
//!
//! Only the containers with a fixture are covered. Ogg carries the same Vorbis
//! comments as FLAC but wraps them in Ogg pages; that framing is the only thing a
//! parser here would add, so it is left to the round-trip tests.

use std::path::{Path, PathBuf};

mod flac {
    pub const VORBIS_COMMENT: u8 = 4;
    pub const PICTURE: u8 = 6;

    /// `(block type, payload)` for every metadata block, in file order.
    pub fn blocks(bytes: &[u8]) -> Vec<(u8, Vec<u8>)> {
        assert_eq!(&bytes[..4], b"fLaC", "not a FLAC stream");
        let mut out = Vec::new();
        let mut at = 4;
        loop {
            let header = bytes[at];
            let len = u32::from_be_bytes([0, bytes[at + 1], bytes[at + 2], bytes[at + 3]]) as usize;
            at += 4;
            out.push((header & 0x7f, bytes[at..at + len].to_vec()));
            at += len;
            if header & 0x80 != 0 {
                break;
            }
        }
        out
    }

    /// Every `NAME=value` pair of the comment block, name upper-cased. Repeated
    /// names stay repeated — that is the whole point of reading them raw.
    pub fn fields(bytes: &[u8]) -> Vec<(String, String)> {
        let Some((_, block)) = blocks(bytes)
            .into_iter()
            .find(|(k, _)| *k == VORBIS_COMMENT)
        else {
            return Vec::new();
        };
        let le = |at: usize| {
            u32::from_le_bytes([block[at], block[at + 1], block[at + 2], block[at + 3]]) as usize
        };
        let mut at = 4 + le(0);
        let count = le(at);
        at += 4;
        let mut out = Vec::new();
        for _ in 0..count {
            let len = le(at);
            at += 4;
            let text = std::str::from_utf8(&block[at..at + len]).expect("comment must be UTF-8");
            at += len;
            let (name, value) = text.split_once('=').expect("comment must be NAME=value");
            out.push((name.to_uppercase(), value.to_string()));
        }
        out
    }

    pub fn values(bytes: &[u8], name: &str) -> Vec<String> {
        fields(bytes)
            .into_iter()
            .filter(|(key, _)| key == name)
            .map(|(_, value)| value)
            .collect()
    }

    pub fn pictures(bytes: &[u8]) -> Vec<Vec<u8>> {
        blocks(bytes)
            .into_iter()
            .filter(|(kind, _)| *kind == PICTURE)
            .map(|(_, block)| block)
            .collect()
    }
}

mod id3 {
    pub struct Tag {
        pub major: u8,
        pub frames: Vec<Frame>,
    }

    pub struct Frame {
        pub id: String,
        pub body: Vec<u8>,
    }

    fn syncsafe(bytes: &[u8]) -> usize {
        bytes
            .iter()
            .fold(0usize, |acc, byte| (acc << 7) | (*byte as usize & 0x7f))
    }

    pub fn parse(bytes: &[u8]) -> Tag {
        assert_eq!(&bytes[..3], b"ID3", "not an ID3v2 tag");
        let major = bytes[3];
        let flags = bytes[5];
        assert_eq!(flags & 0x80, 0, "unsynchronisation is not decoded here");
        let end = 10 + syncsafe(&bytes[6..10]);
        let mut at = 10;
        if flags & 0x40 != 0 {
            at += syncsafe(&bytes[at..at + 4]);
        }
        let mut frames = Vec::new();
        while at + 10 <= end {
            // A run of zeroes is the padding that follows the last frame.
            if bytes[at] == 0 {
                break;
            }
            let id = String::from_utf8_lossy(&bytes[at..at + 4]).into_owned();
            let raw = &bytes[at + 4..at + 8];
            let len = if major >= 4 {
                syncsafe(raw)
            } else {
                u32::from_be_bytes(raw.try_into().unwrap()) as usize
            };
            at += 10;
            frames.push(Frame {
                id,
                body: bytes[at..at + len].to_vec(),
            });
            at += len;
        }
        Tag { major, frames }
    }

    impl Tag {
        pub fn frame(&self, id: &str) -> Option<&Frame> {
            self.frames.iter().find(|frame| frame.id == id)
        }

        pub fn frames_named(&self, id: &str) -> Vec<&Frame> {
            self.frames.iter().filter(|frame| frame.id == id).collect()
        }

        /// A `TXXX` frame's values by description. TXXX is ID3v2's slot for a name
        /// the spec has no frame for: `<encoding><description>\0<value>`.
        pub fn user_text(&self, description: &str) -> Option<Vec<String>> {
            self.frames_named("TXXX").into_iter().find_map(|frame| {
                let text = frame.text();
                let (found, values) = text.split_once('\0')?;
                (found == description).then(|| {
                    values
                        .trim_end_matches('\0')
                        .split('\0')
                        .map(str::to_owned)
                        .collect()
                })
            })
        }
    }

    impl Frame {
        /// A text frame's values. ID3v2.4 puts several of them in one frame,
        /// separated by NUL — the representation `set_multi` is betting on.
        pub fn text_values(&self) -> Vec<String> {
            self.text()
                .trim_end_matches('\0')
                .split('\0')
                .map(str::to_owned)
                .collect()
        }

        pub fn text(&self) -> String {
            let (encoding, rest) = self.body.split_first().expect("empty frame");
            match encoding {
                0 => rest.iter().map(|b| *b as char).collect(),
                3 => String::from_utf8(rest.to_vec()).expect("declared UTF-8"),
                other => panic!("frame {} uses encoding {other}, not decoded here", self.id),
            }
        }
    }
}

struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("pawse_raw_{}_{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self { dir }
    }

    fn copy(&self, fixture: &str) -> PathBuf {
        let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let src = PathBuf::from(manifest).join("../../fixtures").join(fixture);
        let dst = self.dir.join(fixture);
        std::fs::copy(&src, &dst).unwrap();
        dst
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap()
}

fn edits() -> tag_writer::TrackTagEdits {
    tag_writer::TrackTagEdits {
        title: Some("Raw Title".into()),
        artists: vec!["First".into(), "Second".into()],
        album: Some("Raw Album".into()),
        album_artists: vec!["Album Artist".into()],
        track_number: Some(3),
        disc_number: Some(1),
        year: Some(1994),
        genres: vec!["Ambient".into(), "Drone".into()],
        ..Default::default()
    }
}

#[test]
fn flac_gives_every_artist_its_own_comment_field() {
    let scratch = Scratch::new("flac_multi");
    let path = scratch.copy("tagged_basic.flac");
    tag_writer::write_metadata(&path, &edits()).unwrap();

    let bytes = read(&path);
    assert_eq!(flac::values(&bytes, "ARTIST"), ["First", "Second"]);
    assert_eq!(flac::values(&bytes, "ARTISTS"), ["First", "Second"]);
    assert_eq!(flac::values(&bytes, "GENRE"), ["Ambient", "Drone"]);
    assert_eq!(flac::values(&bytes, "ALBUM"), ["Raw Album"]);
    assert!(
        flac::fields(&bytes).iter().all(|(_, v)| !v.contains('\0')),
        "Vorbis holds repeats as separate fields; a NUL inside one is the ID3v2 trick leaking"
    );
}

/// The multi-value bet `set_multi` makes for MP3, checked against the bytes rather
/// than against lofty reading its own output back: several values in one frame
/// separated by NUL only *means* several values from ID3v2.4 on. Were the tag
/// written as 2.3, every other player would read one artist called "First\0Second".
#[test]
fn mp3_packs_the_artists_into_one_nul_separated_v24_frame() {
    let scratch = Scratch::new("mp3_multi");
    let path = scratch.copy("tagged_mp3.mp3");
    tag_writer::write_metadata(&path, &edits()).unwrap();

    let bytes = read(&path);
    let tag = id3::parse(&bytes);
    assert!(
        tag.major >= 4,
        "NUL-separated values need ID3v2.4; this tag is v2.{}",
        tag.major
    );
    assert_eq!(tag.frames_named("TPE1").len(), 1, "one key is one frame");
    assert_eq!(
        tag.frame("TPE1").unwrap().text_values(),
        ["First", "Second"]
    );
    assert_eq!(
        tag.frame("TCON").unwrap().text_values(),
        ["Ambient", "Drone"]
    );
    assert_eq!(tag.frame("TIT2").unwrap().text_values(), ["Raw Title"]);
    assert_eq!(tag.frame("TDRC").unwrap().text_values(), ["1994"]);
}

#[test]
fn a_cleared_field_leaves_nothing_behind_rather_than_an_empty_one() {
    let scratch = Scratch::new("cleared");
    let flac = scratch.copy("tagged_basic.flac");
    let mp3 = scratch.copy("tagged_mp3.mp3");
    for path in [&flac, &mp3] {
        tag_writer::write_metadata(path, &edits()).unwrap();
        tag_writer::write_metadata(
            path,
            &tag_writer::TrackTagEdits {
                title: Some("Only A Title".into()),
                ..Default::default()
            },
        )
        .unwrap();
    }

    let bytes = read(&flac);
    assert!(
        flac::values(&bytes, "ARTIST").is_empty(),
        "cleared means the field is gone: {:?}",
        flac::fields(&bytes)
    );
    assert!(flac::values(&bytes, "GENRE").is_empty());

    let tag = id3::parse(&read(&mp3));
    assert!(
        tag.frame("TPE1").is_none(),
        "an empty TPE1 frame is not a clear"
    );
    assert!(tag.frame("TCON").is_none());
}

#[test]
fn an_embedded_picture_comes_through_a_write_byte_for_byte() {
    let scratch = Scratch::new("picture");
    let path = scratch.copy("tagged_with_cover.flac");
    let before = flac::pictures(&read(&path));
    assert_eq!(before.len(), 1, "the fixture is supposed to carry art");

    let mut with_custom = edits();
    with_custom.added_tags = vec![tag_writer::RawTag {
        key: "MOOD".into(),
        value: "calm".into(),
    }];
    tag_writer::write_metadata(&path, &with_custom).unwrap();

    assert_eq!(
        flac::pictures(&read(&path)),
        before,
        "the custom-row path is the one that used to rebuild the tag"
    );
}

/// Where a hand-typed row actually ends up. For Vorbis the answer is boring — a
/// field like any other — and that is worth pinning: it means another tag editor
/// will show it under the name the user typed.
#[test]
fn a_custom_row_becomes_a_plain_vorbis_field() {
    let scratch = Scratch::new("custom_flac");
    let path = scratch.copy("tagged_basic.flac");
    let mut with_custom = edits();
    with_custom.added_tags = vec![
        tag_writer::RawTag {
            key: "MOOD".into(),
            value: "calm".into(),
        },
        tag_writer::RawTag {
            key: "ENGINEER".into(),
            value: "Someone".into(),
        },
    ];
    tag_writer::write_metadata(&path, &with_custom).unwrap();

    let bytes = read(&path);
    assert_eq!(flac::values(&bytes, "MOOD"), ["calm"]);
    assert_eq!(flac::values(&bytes, "ENGINEER"), ["Someone"]);
}

/// The MP3 answer, which is not obvious and matters: ID3v2 has no frame for an
/// arbitrary name, and lofty resolves one to a `TXXX` user-text frame keyed by
/// description. That is the portable spelling — every other editor shows TXXX rows
/// under their description — and it is also *why* the four-character rule exists: a
/// name that long is the width of a frame id, so it is taken for one instead of
/// being wrapped. `ARTISTS`, which the writer always emits, rides the same slot.
#[test]
fn a_custom_row_becomes_a_txxx_frame_on_mp3() {
    let scratch = Scratch::new("custom_mp3");
    let path = scratch.copy("tagged_mp3.mp3");
    let mut with_custom = edits();
    with_custom.added_tags = vec![
        tag_writer::RawTag {
            key: "ENGINEER".into(),
            value: "Someone".into(),
        },
        tag_writer::RawTag {
            key: "MYTAG".into(),
            value: "v".into(),
        },
    ];
    tag_writer::write_metadata(&path, &with_custom).unwrap();

    let tag = id3::parse(&read(&path));
    assert_eq!(
        tag.user_text("ENGINEER").as_deref(),
        Some(&["Someone".to_string()][..])
    );
    assert_eq!(
        tag.user_text("MYTAG").as_deref(),
        Some(&["v".to_string()][..])
    );
    assert_eq!(
        tag.user_text("ARTISTS").as_deref(),
        Some(&["First".to_string(), "Second".to_string()][..]),
        "the reader prefers ARTISTS, so it has to survive as a readable frame"
    );
}

/// The four-character refusal, from the outside. It has to happen before the save,
/// not during it: a write that dies half-way would take every other edit in the same
/// dialog with it, so the file must come out untouched.
#[test]
fn a_four_character_custom_name_is_refused_without_touching_the_file() {
    let scratch = Scratch::new("four_char");
    let path = scratch.copy("tagged_mp3.mp3");
    tag_writer::write_metadata(&path, &edits()).unwrap();
    let before = read(&path);

    let mut doomed = edits();
    doomed.title = Some("Never Written".into());
    doomed.added_tags = vec![tag_writer::RawTag {
        key: "MOOD".into(),
        value: "calm".into(),
    }];
    assert!(tag_writer::write_metadata(&path, &doomed).is_err());

    assert_eq!(
        read(&path),
        before,
        "a refused save must not be a partial one"
    );
    assert_eq!(
        id3::parse(&before).frame("TIT2").unwrap().text_values(),
        ["Raw Title"]
    );
}

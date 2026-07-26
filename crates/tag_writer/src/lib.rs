use std::path::Path;

use lofty::config::WriteOptions;
use lofty::file::AudioFile;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::prelude::{ItemKey, TaggedFileExt};
use lofty::tag::{ItemValue, Tag, TagItem, TagType};

const ID3V2_MULTI_SEPARATOR: &str = "\u{0}";
const ID3V2_FRAME_ID_LEN: usize = 4;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrackTagEdits {
    pub title: Option<String>,
    pub artists: Vec<String>,
    pub album: Option<String>,
    pub album_artists: Vec<String>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub year: Option<i32>,
    pub genres: Vec<String>,
    /// Rows handed out by [`read_raw_tags`] to drop from the file entirely.
    pub removed_tags: Vec<RawTag>,
    /// Free-form rows to add. Keys already covered by a field above are ignored.
    pub added_tags: Vec<RawTag>,
    /// What to do with the embedded cover. Defaults to leaving it alone, so a caller
    /// that only edits text never has to think about it.
    pub cover: CoverEdit,
}

/// A cover lives in the tag as the image itself — there is no "path to the artwork"
/// tag in any container — and the reader prefers it over anything found on disk, so
/// replacing it here is what overrides the external-file heuristic.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum CoverEdit {
    #[default]
    Keep,
    Replace {
        bytes: Vec<u8>,
    },
    Remove,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawTag {
    pub key: String,
    pub value: String,
}

pub fn write_metadata(path: &Path, edits: &TrackTagEdits) -> anyhow::Result<()> {
    let mut tagged = lofty::read_from_path(path)?;

    let mut tag = match tagged.primary_tag().or_else(|| tagged.first_tag()) {
        Some(tag) => tag.clone(),
        None => Tag::new(target_tag_type(&tagged)),
    };

    apply(&mut tag, edits);
    let mut tag = with_custom(tag, &edits.added_tags)?;
    apply_cover(&mut tag, &edits.cover)?;

    tagged.insert_tag(tag);
    tagged.save_to_path(path, WriteOptions::default())?;
    Ok(())
}

/// Add the free-form rows.
///
/// `Tag::push` refuses an [`ItemKey::Unknown`] outright — it returns `false` and
/// drops the item — because it checks the key against the container's vocabulary
/// first. `push_unchecked` is lofty's documented way in for exactly this case; the
/// key is verified again at write time, so an out-of-spec one is dropped rather
/// than corrupting the file. Everything else, multi-value handling included, is the
/// same [`set_multi`] the form fields go through.
fn with_custom(mut tag: Tag, added: &[RawTag]) -> anyhow::Result<Tag> {
    let tag_type = tag.tag_type();
    let wanted: Vec<&RawTag> = added
        .iter()
        .filter(|row| !row.key.trim().is_empty() && !row.value.trim().is_empty())
        .filter(|row| !shown_as_field(&ItemKey::from_key(tag_type, row.key.trim())))
        .collect();
    if wanted.is_empty() {
        return Ok(tag);
    }
    let support = CustomTags::of(tag_type);
    if !support.supported() {
        anyhow::bail!("custom tags are not supported for {:?} tags", tag_type);
    }

    for (key, values) in group_by_key(&wanted) {
        match support.check_key(&key) {
            None => {}
            Some(KeyProblem::Invalid) => anyhow::bail!("'{}' is not a usable tag name", key),
            Some(KeyProblem::FrameIdLength) => anyhow::bail!(
                "ID3v2 cannot store '{}': custom names may not be 4 characters",
                key
            ),
        }

        let item_key = ItemKey::from_key(tag_type, &key);
        let mut merged: Vec<String> = tag.get_strings(&item_key).map(str::to_owned).collect();
        for value in values {
            if !merged.contains(&value) {
                merged.push(value);
            }
        }
        set_multi(&mut tag, item_key, &merged);
    }

    Ok(tag)
}

/// Put the chosen cover in, or take every cover out.
///
/// Both branches clear *all* pictures first. `music_indexer`'s reader takes
/// `CoverFront` and falls back to the first picture of any type, so leaving a back
/// cover or a booklet scan behind would promote it to album art — a removal that
/// silently swaps the image rather than clearing it.
fn apply_cover(tag: &mut Tag, edit: &CoverEdit) -> anyhow::Result<()> {
    let bytes = match edit {
        CoverEdit::Keep => return Ok(()),
        CoverEdit::Remove => {
            clear_pictures(tag);
            return Ok(());
        }
        CoverEdit::Replace { bytes } => bytes,
    };
    let picture = read_cover(bytes)?;
    clear_pictures(tag);
    tag.push_picture(picture);
    Ok(())
}

/// lofty offers removal by index or by type, neither of which empties the list, and a
/// file can carry any mix of types.
fn clear_pictures(tag: &mut Tag) {
    while !tag.pictures().is_empty() {
        tag.remove_picture(0);
    }
}

/// Decode cover bytes into a front-cover picture, or say why they are unusable.
///
/// `Picture::from_reader` sniffs the format and rejects a non-image, but it also
/// accepts GIF, BMP and TIFF and always reports [`PictureType::Other`]. Narrowing to
/// JPEG and PNG is ours: those are the only two MP4's `covr` atom takes, and the only
/// two every player renders, so accepting more would make the result depend on the
/// container.
fn read_cover(bytes: &[u8]) -> anyhow::Result<Picture> {
    let mut picture = Picture::from_reader(&mut std::io::Cursor::new(bytes))
        .map_err(|_| anyhow::anyhow!("this file is not an image"))?;
    if !matches!(
        picture.mime_type(),
        Some(MimeType::Jpeg) | Some(MimeType::Png)
    ) {
        anyhow::bail!("a cover has to be a JPEG or a PNG");
    }
    picture.set_pic_type(PictureType::CoverFront);
    Ok(picture)
}

/// Whether [`CoverEdit::Replace`] would accept these bytes. The UI asks this the
/// moment a file is picked so the refusal lands on the picker rather than on a save
/// that then discards every other edit.
pub fn cover_bytes_ok(bytes: &[u8]) -> bool {
    read_cover(bytes).is_ok()
}

/// The tag every entry point works on: what the reader reads, and what a file with
/// no tag at all would get.
fn target_tag_type(tagged: &lofty::file::TaggedFile) -> TagType {
    tagged
        .primary_tag()
        .or_else(|| tagged.first_tag())
        .map(|tag| tag.tag_type())
        .unwrap_or_else(|| tagged.primary_tag_type())
}

fn group_by_key(rows: &[&RawTag]) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for row in rows {
        let key = row.key.trim().to_string();
        let value = row.value.trim().to_string();
        match out.iter_mut().find(|(seen, _)| *seen == key) {
            Some((_, values)) => values.push(value),
            None => out.push((key, vec![value])),
        }
    }
    out
}

/// What changed between the file's own rows and the list the user is looking at,
/// as `(removed, added)`. Diffing beats tracking each click: a row added and then
/// deleted cancels out on its own, and repeated key/value pairs are matched one for
/// one instead of collapsing.
pub fn diff_raw_tags(original: &[RawTag], current: &[RawTag]) -> (Vec<RawTag>, Vec<RawTag>) {
    let mut survivors: Vec<&RawTag> = current.iter().collect();
    let mut removed = Vec::new();
    for row in original {
        match survivors.iter().position(|kept| *kept == row) {
            Some(ix) => {
                survivors.remove(ix);
            }
            None => removed.push(row.clone()),
        }
    }
    (removed, survivors.into_iter().cloned().collect())
}

pub fn read_raw_tags(path: &Path) -> anyhow::Result<Vec<RawTag>> {
    let tagged = lofty::read_from_path(path)?;
    let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) else {
        return Ok(Vec::new());
    };

    let tag_type = tag.tag_type();
    let mut out: Vec<RawTag> = Vec::new();
    for item in tag.items() {
        if shown_as_field(item.key()) {
            continue;
        }
        let Some(value) = item.value().text() else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        let Some(key) = item.key().map_key(tag_type, true) else {
            continue;
        };
        out.push(RawTag {
            key: key.to_string(),
            value: value.to_string(),
        });
    }

    out.sort_by(|a, b| a.key.cmp(&b.key).then_with(|| a.value.cmp(&b.value)));
    Ok(out)
}

/// What a file's container accepts as a hand-typed key. Resolved once when the
/// editor opens so a row can be refused as it is typed — the writer runs long after
/// the dialog is gone, and a refusal there costs the user every other edit in the
/// same save.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CustomTags {
    /// Vorbis Comments: any name the charset allows.
    Free,
    /// ID3v2: the same, except a name of [`ID3V2_FRAME_ID_LEN`] characters.
    NoFrameIdLength,
    /// Everything else: custom rows cannot be written at all.
    #[default]
    Unsupported,
}

/// Why a hand-typed key cannot be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyProblem {
    /// Empty, built from characters no container stores, or a name the form's own
    /// fields already own.
    Invalid,
    /// ID3v2 only: 4 characters is a frame id, not a description.
    FrameIdLength,
}

impl CustomTags {
    fn of(tag_type: TagType) -> Self {
        match tag_type {
            TagType::VorbisComments => Self::Free,
            TagType::Id3v2 => Self::NoFrameIdLength,
            _ => Self::Unsupported,
        }
    }

    pub fn supported(self) -> bool {
        !matches!(self, Self::Unsupported)
    }

    /// Judges the *name* only — whether this container takes custom rows at all is
    /// [`CustomTags::supported`], so `Unsupported.check_key("ANYTHING")` is `None`.
    /// Both questions have to be asked.
    ///
    /// Vorbis field names are ASCII `0x20..=0x7D` minus `=`, and
    /// `VorbisComments::push` drops anything else without a word, so that rule is
    /// applied to every container. The collision check runs against *both* container
    /// vocabularies, so `TITLE` and `TIT2` are equally refused however the file
    /// happens to be tagged.
    pub fn check_key(self, key: &str) -> Option<KeyProblem> {
        let key = key.trim();
        let storable = !key.is_empty()
            && key.bytes().all(|b| (0x20..=0x7d).contains(&b) && b != b'=')
            && ![TagType::VorbisComments, TagType::Id3v2]
                .into_iter()
                .any(|tag_type| shown_as_field(&ItemKey::from_key(tag_type, key)));
        if !storable {
            return Some(KeyProblem::Invalid);
        }
        if self == Self::NoFrameIdLength && key.len() == ID3V2_FRAME_ID_LEN {
            return Some(KeyProblem::FrameIdLength);
        }
        None
    }
}

pub fn custom_tags_for(path: &Path) -> CustomTags {
    match lofty::read_from_path(path) {
        Ok(tagged) => CustomTags::of(target_tag_type(&tagged)),
        Err(_) => CustomTags::Unsupported,
    }
}

fn shown_as_field(key: &ItemKey) -> bool {
    matches!(
        key,
        ItemKey::TrackTitle
            | ItemKey::AlbumTitle
            | ItemKey::TrackArtist
            | ItemKey::TrackArtists
            | ItemKey::AlbumArtist
            | ItemKey::TrackNumber
            | ItemKey::DiscNumber
            | ItemKey::RecordingDate
            | ItemKey::Year
            | ItemKey::OriginalReleaseDate
            | ItemKey::ReleaseDate
            | ItemKey::Genre
            | ItemKey::Lyrics
    )
}

fn apply(tag: &mut Tag, edits: &TrackTagEdits) {
    set_text(tag, ItemKey::TrackTitle, edits.title.as_deref());
    set_text(tag, ItemKey::AlbumTitle, edits.album.as_deref());

    set_multi(tag, ItemKey::TrackArtists, &edits.artists);
    set_multi(tag, ItemKey::TrackArtist, &edits.artists);
    set_multi(tag, ItemKey::AlbumArtist, &edits.album_artists);

    set_number(tag, ItemKey::TrackNumber, edits.track_number);
    set_number(tag, ItemKey::DiscNumber, edits.disc_number);

    set_year(tag, edits.year);

    set_multi(tag, ItemKey::Genre, &edits.genres);

    let tag_type = tag.tag_type();
    for removed in &edits.removed_tags {
        let key = ItemKey::from_key(tag_type, &removed.key);
        tag.retain(|item| {
            *item.key() != key || item.value().text().map(str::trim) != Some(removed.value.as_str())
        });
    }
}

/// A number field the form shows as a plain integer but the file may spell more
/// richly — `5/12`, say. Rewriting an untouched field would throw the rest of that
/// spelling away, so the file keeps its own as long as it still reads back the same.
fn set_number(tag: &mut Tag, key: ItemKey, value: Option<u32>) {
    if current_number(tag, &key) == value {
        return;
    }
    set_text(tag, key, value.map(|n| n.to_string()).as_deref());
}

/// Mirrors the reader: the number is whatever precedes the first `/`.
fn current_number(tag: &Tag, key: &ItemKey) -> Option<u32> {
    tag.get(key)
        .and_then(|item| item.value().text())
        .and_then(|text| text.split('/').next()?.trim().parse().ok())
}

fn set_year(tag: &mut Tag, year: Option<i32>) {
    if current_year(tag) == year {
        return;
    }
    tag.remove_key(&ItemKey::Year);
    tag.remove_key(&ItemKey::OriginalReleaseDate);
    tag.remove_key(&ItemKey::ReleaseDate);
    set_text(
        tag,
        ItemKey::RecordingDate,
        year.map(|y| y.to_string()).as_deref(),
    );
}

/// Mirrors `read_year`: the same key order, and the same "first four digits" rule.
fn current_year(tag: &Tag) -> Option<i32> {
    [
        ItemKey::RecordingDate,
        ItemKey::Year,
        ItemKey::OriginalReleaseDate,
        ItemKey::ReleaseDate,
    ]
    .iter()
    .find_map(|key| {
        let text = tag.get(key).and_then(|item| item.value().text())?;
        let digits: String = text
            .trim_start()
            .chars()
            .take_while(char::is_ascii_digit)
            .take(4)
            .collect();
        (digits.len() == 4).then(|| digits.parse().ok())?
    })
}

fn set_text(tag: &mut Tag, key: ItemKey, value: Option<&str>) {
    match value.map(str::trim).filter(|v| !v.is_empty()) {
        Some(value) => {
            tag.insert_text(key, value.to_string());
        }
        None => tag.remove_key(&key),
    }
}

fn set_multi(tag: &mut Tag, key: ItemKey, values: &[String]) {
    let cleaned = clean(values);
    if cleaned.is_empty() {
        tag.remove_key(&key);
        return;
    }

    if tag.tag_type() == TagType::Id3v2 {
        tag.insert_unchecked(TagItem::new(
            key,
            ItemValue::Text(cleaned.join(ID3V2_MULTI_SEPARATOR)),
        ));
        return;
    }

    tag.remove_key(&key);
    for value in cleaned {
        tag.push_unchecked(TagItem::new(key.clone(), ItemValue::Text(value)));
    }
}

fn clean(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new() -> Self {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("pawse_tag_writer_{}_{}", std::process::id(), n));
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn copy_fixture(dir: &TempDir, name: &str) -> PathBuf {
        let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let src = PathBuf::from(manifest).join("../../fixtures").join(name);
        let dst = dir.path.join(name);
        std::fs::copy(&src, &dst).unwrap();
        dst
    }

    fn raw_value(path: &PathBuf, key: ItemKey) -> Option<String> {
        let tagged = lofty::read_from_path(path).unwrap();
        tagged
            .primary_tag()
            .or_else(|| tagged.first_tag())?
            .get(&key)
            .and_then(|item| item.value().text())
            .map(str::to_owned)
    }

    /// A custom row for containers that take one, and nothing for those that do not —
    /// so a test covering "everything at once" can still run against MP4.
    fn custom_row_if_supported(path: &Path, value: &str) -> Vec<RawTag> {
        if custom_tags_for(path).supported() {
            vec![RawTag {
                key: "MYCUSTOM".into(),
                value: value.into(),
            }]
        } else {
            Vec::new()
        }
    }

    fn full_edits() -> TrackTagEdits {
        TrackTagEdits {
            title: Some("New Title".into()),
            artists: vec!["First Artist".into(), "Second Artist".into()],
            album: Some("New Album".into()),
            album_artists: vec!["Album Artist".into()],
            track_number: Some(7),
            disc_number: Some(2),
            year: Some(1994),
            genres: vec!["Drum & Bass".into(), "Ambient".into()],
            removed_tags: Vec::new(),
            added_tags: Vec::new(),
            cover: CoverEdit::Keep,
        }
    }

    fn roundtrip(fixture: &str) {
        let dir = TempDir::new();
        let path = copy_fixture(&dir, fixture);
        let edits = full_edits();

        write_metadata(&path, &edits).unwrap();
        let read = music_indexer::metadata::read_metadata(&path).unwrap();

        assert_eq!(read.title.as_deref(), Some("New Title"), "{fixture} title");
        assert_eq!(
            read.artist_names,
            vec!["First Artist".to_string(), "Second Artist".to_string()],
            "{fixture} artists"
        );
        assert_eq!(
            read.album_title.as_deref(),
            Some("New Album"),
            "{fixture} album"
        );
        assert_eq!(
            read.album_artist_names,
            vec!["Album Artist".to_string()],
            "{fixture} album artists"
        );
        assert_eq!(read.track_number, Some(7), "{fixture} track number");
        assert_eq!(read.disc_number, Some(2), "{fixture} disc number");
        assert_eq!(read.year, Some(1994), "{fixture} year");
        assert_eq!(
            read.genres,
            vec!["Drum & Bass".to_string(), "Ambient".to_string()],
            "{fixture} genres"
        );
    }

    #[test]
    fn roundtrip_flac_tagless() {
        roundtrip("tagless.flac");
    }

    #[test]
    fn roundtrip_flac_tagged() {
        roundtrip("tagged_basic.flac");
    }

    #[test]
    fn roundtrip_mp3() {
        roundtrip("tagged_mp3.mp3");
    }

    #[test]
    fn roundtrip_ogg() {
        roundtrip("tagged_ogg.ogg");
    }

    #[test]
    fn roundtrip_m4a() {
        roundtrip("tagged_m4a.m4a");
    }

    #[test]
    fn empty_values_clear_keys() {
        let dir = TempDir::new();
        let path = copy_fixture(&dir, "tagged_basic.flac");

        write_metadata(&path, &full_edits()).unwrap();
        write_metadata(&path, &TrackTagEdits::default()).unwrap();

        let read = music_indexer::metadata::read_metadata(&path).unwrap();
        assert_eq!(read.title, None);
        assert!(read.artist_names.is_empty());
        assert_eq!(read.album_title, None);
        assert!(read.album_artist_names.is_empty());
        assert_eq!(read.track_number, None);
        assert_eq!(read.disc_number, None);
        assert_eq!(read.year, None);
        assert!(read.genres.is_empty());
    }

    #[test]
    fn stale_year_fallback_does_not_resurrect() {
        let dir = TempDir::new();
        let path = copy_fixture(&dir, "tagged_basic.flac");

        {
            let mut tagged = lofty::read_from_path(&path).unwrap();
            let tag = tagged.primary_tag_mut().unwrap();
            tag.insert_text(ItemKey::Year, "1977".into());
            tagged.save_to_path(&path, WriteOptions::default()).unwrap();
        }

        let mut edits = full_edits();
        edits.year = None;
        write_metadata(&path, &edits).unwrap();

        assert_eq!(
            music_indexer::metadata::read_metadata(&path).unwrap().year,
            None
        );
    }

    #[test]
    fn raw_tags_exclude_edited_fields() {
        let dir = TempDir::new();
        let path = copy_fixture(&dir, "tagged_basic.flac");

        {
            let mut tagged = lofty::read_from_path(&path).unwrap();
            let tag = tagged.primary_tag_mut().unwrap();
            tag.insert_text(ItemKey::Composer, "A Composer".into());
            tag.insert_text(ItemKey::Comment, "A Comment".into());
            tagged.save_to_path(&path, WriteOptions::default()).unwrap();
        }

        write_metadata(&path, &full_edits()).unwrap();
        let raw = read_raw_tags(&path).unwrap();

        assert!(raw.iter().any(|t| t.value == "A Composer"));
        assert!(raw.iter().any(|t| t.value == "A Comment"));
        assert!(!raw.iter().any(|t| t.value == "New Title"));
        assert!(!raw.iter().any(|t| t.value == "New Album"));
        assert!(!raw.iter().any(|t| t.value == "1994"));
    }

    #[test]
    fn removed_tags_are_dropped_from_the_file() {
        for fixture in ["tagged_basic.flac", "tagged_mp3.mp3"] {
            let dir = TempDir::new();
            let path = copy_fixture(&dir, fixture);

            {
                let mut tagged = lofty::read_from_path(&path).unwrap();
                let tag = tagged.primary_tag_mut().unwrap();
                tag.insert_text(ItemKey::Composer, "A Composer".into());
                tagged.save_to_path(&path, WriteOptions::default()).unwrap();
            }

            let composer = read_raw_tags(&path)
                .unwrap()
                .into_iter()
                .find(|t| t.value == "A Composer")
                .unwrap_or_else(|| panic!("{fixture}: composer not listed"));

            let edits = TrackTagEdits {
                title: Some("Kept".into()),
                removed_tags: vec![composer],
                ..TrackTagEdits::default()
            };
            write_metadata(&path, &edits).unwrap();

            let raw = read_raw_tags(&path).unwrap();
            assert!(
                !raw.iter().any(|t| t.value == "A Composer"),
                "{fixture}: removed key survived"
            );
            assert_eq!(
                music_indexer::metadata::read_metadata(&path)
                    .unwrap()
                    .title
                    .as_deref(),
                Some("Kept"),
                "{fixture}: removal must not disturb the edited fields"
            );
        }
    }

    #[test]
    fn removing_one_row_keeps_its_siblings_under_the_same_key() {
        let dir = TempDir::new();
        let path = copy_fixture(&dir, "tagged_basic.flac");

        {
            let mut tagged = lofty::read_from_path(&path).unwrap();
            let tag = tagged.primary_tag_mut().unwrap();
            tag.push(TagItem::new(
                ItemKey::Comment,
                ItemValue::Text("ripped by X".into()),
            ));
            tag.push(TagItem::new(
                ItemKey::Comment,
                ItemValue::Text("disc 1 of 2".into()),
            ));
            tagged.save_to_path(&path, WriteOptions::default()).unwrap();
        }

        let doomed = read_raw_tags(&path)
            .unwrap()
            .into_iter()
            .find(|t| t.value == "ripped by X")
            .expect("comment not listed");

        let edits = TrackTagEdits {
            title: Some("Kept".into()),
            removed_tags: vec![doomed],
            ..TrackTagEdits::default()
        };
        write_metadata(&path, &edits).unwrap();

        let raw = read_raw_tags(&path).unwrap();
        assert!(!raw.iter().any(|t| t.value == "ripped by X"));
        assert!(raw.iter().any(|t| t.value == "disc 1 of 2"));
    }

    #[test]
    fn custom_tags_round_trip_per_container() {
        for fixture in ["tagged_basic.flac", "tagged_ogg.ogg", "tagged_mp3.mp3"] {
            let dir = TempDir::new();
            let path = copy_fixture(&dir, fixture);

            let edits = TrackTagEdits {
                added_tags: vec![
                    RawTag {
                        key: "MYCUSTOM".into(),
                        value: "hello".into(),
                    },
                    RawTag {
                        key: "MY MOOD".into(),
                        value: "calm".into(),
                    },
                ],
                ..full_edits()
            };
            write_metadata(&path, &edits).unwrap();

            let raw = read_raw_tags(&path).unwrap();
            let has = |k: &str, v: &str| raw.iter().any(|t| t.key == k && t.value == v);
            assert!(has("MYCUSTOM", "hello"), "{fixture}: {raw:?}");
            assert!(has("MY MOOD", "calm"), "{fixture}: {raw:?}");

            let read = music_indexer::metadata::read_metadata(&path).unwrap();
            assert_eq!(
                read.title.as_deref(),
                Some("New Title"),
                "{fixture}: custom tags must not disturb the fields"
            );
        }
    }

    /// MP4 takes no custom rows, and the refusal has to come before the file is
    /// touched. Not because the container cannot hold one — `----` freeform atoms
    /// exist — but because lofty's generic-tag conversion does not emit them: an
    /// ilst atom name is exactly four bytes, so a 4-character key is written as a
    /// literal atom (which no player recognises) and every other length is dropped
    /// with no error at all. Silence is the one outcome the UI cannot report, so the
    /// whole container is refused up front instead.
    #[test]
    fn mp4_refuses_custom_rows_before_writing_anything() {
        let dir = TempDir::new();
        let path = copy_fixture(&dir, "tagged_m4a.m4a");
        write_metadata(&path, &full_edits()).unwrap();
        let before = std::fs::read(&path).unwrap();

        assert!(!custom_tags_for(&path).supported());
        let edits = TrackTagEdits {
            title: Some("Never Written".into()),
            added_tags: vec![RawTag {
                key: "MYCUSTOM".into(),
                value: "hello".into(),
            }],
            ..full_edits()
        };
        assert!(write_metadata(&path, &edits).is_err());
        assert_eq!(
            std::fs::read(&path).unwrap(),
            before,
            "a refusal must not be a partial write"
        );
    }

    #[test]
    fn two_custom_rows_can_share_a_key() {
        for fixture in ["tagged_basic.flac", "tagged_ogg.ogg", "tagged_mp3.mp3"] {
            let dir = TempDir::new();
            let path = copy_fixture(&dir, fixture);

            let edits = TrackTagEdits {
                added_tags: vec![
                    RawTag {
                        key: "MOODX".into(),
                        value: "happy".into(),
                    },
                    RawTag {
                        key: "MOODX".into(),
                        value: "calm".into(),
                    },
                ],
                ..full_edits()
            };
            write_metadata(&path, &edits).unwrap();

            let raw = read_raw_tags(&path).unwrap();
            let values: Vec<&str> = raw
                .iter()
                .filter(|t| t.key == "MOODX")
                .map(|t| t.value.as_str())
                .collect();
            assert_eq!(values, vec!["calm", "happy"], "{fixture}: {raw:?}");
        }
    }

    #[test]
    fn a_custom_row_joins_a_key_the_file_already_has() {
        for fixture in ["tagged_basic.flac", "tagged_mp3.mp3"] {
            let dir = TempDir::new();
            let path = copy_fixture(&dir, fixture);

            write_metadata(
                &path,
                &TrackTagEdits {
                    added_tags: vec![RawTag {
                        key: "MOODX".into(),
                        value: "happy".into(),
                    }],
                    ..full_edits()
                },
            )
            .unwrap();

            let original = read_raw_tags(&path).unwrap();
            let mut current = original.clone();
            current.push(RawTag {
                key: "MOODX".into(),
                value: "calm".into(),
            });
            let (removed_tags, added_tags) = diff_raw_tags(&original, &current);

            write_metadata(
                &path,
                &TrackTagEdits {
                    removed_tags,
                    added_tags,
                    ..full_edits()
                },
            )
            .unwrap();

            let raw = read_raw_tags(&path).unwrap();
            let values: Vec<&str> = raw
                .iter()
                .filter(|t| t.key == "MOODX")
                .map(|t| t.value.as_str())
                .collect();
            assert_eq!(values, vec!["calm", "happy"], "{fixture}: {raw:?}");
        }
    }

    #[test]
    fn embedded_cover_survives_a_custom_row() {
        let dir = TempDir::new();
        let path = copy_fixture(&dir, "tagged_with_cover.flac");

        let before = lofty::read_from_path(&path).unwrap();
        let pictures = before.primary_tag().unwrap().picture_count();
        assert!(pictures > 0, "fixture must carry a cover");

        write_metadata(
            &path,
            &TrackTagEdits {
                added_tags: vec![RawTag {
                    key: "MYCUSTOM".into(),
                    value: "hello".into(),
                }],
                ..full_edits()
            },
        )
        .unwrap();

        let after = lofty::read_from_path(&path).unwrap();
        assert_eq!(after.primary_tag().unwrap().picture_count(), pictures);
    }

    #[test]
    fn id3v2_refuses_a_four_character_custom_name() {
        let dir = TempDir::new();
        let path = copy_fixture(&dir, "tagged_mp3.mp3");
        let edits = TrackTagEdits {
            added_tags: vec![RawTag {
                key: "MOOD".into(),
                value: "calm".into(),
            }],
            ..full_edits()
        };

        let err = write_metadata(&path, &edits).unwrap_err().to_string();
        assert!(err.contains("4 characters"), "{err}");
        assert_eq!(
            music_indexer::metadata::read_metadata(&path)
                .unwrap()
                .title
                .as_deref(),
            Some("MP3 Track"),
            "a refused custom tag must abort the whole write"
        );

        let flac = copy_fixture(&dir, "tagged_basic.flac");
        write_metadata(&flac, &edits).unwrap();
        assert!(
            read_raw_tags(&flac)
                .unwrap()
                .iter()
                .any(|t| t.key == "MOOD" && t.value == "calm"),
            "the limit is ID3v2's, not ours"
        );
    }

    #[test]
    fn diffing_raw_tags_cancels_an_add_then_delete() {
        let row = |k: &str, v: &str| RawTag {
            key: k.into(),
            value: v.into(),
        };
        let original = vec![
            row("COMPOSER", "A"),
            row("COMMENT", "x"),
            row("COMMENT", "x"),
        ];

        let (removed, added) = diff_raw_tags(&original, &original);
        assert!(removed.is_empty() && added.is_empty(), "no edits, no work");

        let mut current = original.clone();
        current.push(row("MYCUSTOM", "new"));
        current.remove(0);
        let (removed, added) = diff_raw_tags(&original, &current);
        assert_eq!(removed, vec![row("COMPOSER", "A")]);
        assert_eq!(added, vec![row("MYCUSTOM", "new")]);

        let mut current = original.clone();
        current.push(row("MYCUSTOM", "new"));
        current.pop();
        let (removed, added) = diff_raw_tags(&original, &current);
        assert!(removed.is_empty() && added.is_empty(), "added then deleted");

        let mut current = original.clone();
        current.pop();
        let (removed, added) = diff_raw_tags(&original, &current);
        assert_eq!(
            removed,
            vec![row("COMMENT", "x")],
            "one of two identical rows, not both"
        );
        assert!(added.is_empty());
    }

    #[test]
    fn custom_tags_cannot_shadow_a_form_field() {
        let dir = TempDir::new();
        let path = copy_fixture(&dir, "tagged_basic.flac");

        let edits = TrackTagEdits {
            added_tags: vec![RawTag {
                key: "TITLE".into(),
                value: "Sneaky".into(),
            }],
            ..full_edits()
        };
        write_metadata(&path, &edits).unwrap();

        for support in [CustomTags::Free, CustomTags::NoFrameIdLength] {
            assert_eq!(support.check_key("TITLE"), Some(KeyProblem::Invalid));
            assert_eq!(support.check_key("TIT2"), Some(KeyProblem::Invalid));
            assert_eq!(support.check_key("WITH=EQUALS"), Some(KeyProblem::Invalid));
            assert_eq!(support.check_key("  "), Some(KeyProblem::Invalid));
            assert_eq!(support.check_key("MYCUSTOM"), None);
        }
        assert_eq!(CustomTags::Free.check_key("MOOD"), None);
        assert_eq!(
            CustomTags::NoFrameIdLength.check_key("MOOD"),
            Some(KeyProblem::FrameIdLength),
            "the UI must refuse this before the dialog closes"
        );
        assert_eq!(
            custom_tags_for(&copy_fixture(&dir, "tagged_mp3.mp3")),
            CustomTags::NoFrameIdLength
        );
        assert_eq!(custom_tags_for(&path), CustomTags::Free);
        assert_eq!(
            music_indexer::metadata::read_metadata(&path)
                .unwrap()
                .title
                .as_deref(),
            Some("New Title")
        );
    }

    #[test]
    fn raw_tags_on_minimal_file_expose_only_encoder() {
        let dir = TempDir::new();
        let path = copy_fixture(&dir, "tagless.flac");
        let raw = read_raw_tags(&path).unwrap();
        assert_eq!(raw.len(), 1);
        assert_eq!(raw[0].value, "Lavf61.7.100");
    }

    #[test]
    fn multi_values_survive_per_container() {
        for fixture in [
            "tagged_basic.flac",
            "tagged_ogg.ogg",
            "tagged_mp3.mp3",
            "tagged_m4a.m4a",
        ] {
            let dir = TempDir::new();
            let path = copy_fixture(&dir, fixture);

            let edits = TrackTagEdits {
                artists: vec!["A One".into(), "B Two".into(), "C Three".into()],
                album_artists: vec!["AA".into(), "BB".into()],
                genres: vec!["Ambient".into(), "Drum & Bass".into()],
                ..TrackTagEdits::default()
            };
            write_metadata(&path, &edits).unwrap();
            let read = music_indexer::metadata::read_metadata(&path).unwrap();

            assert_eq!(read.artist_names, edits.artists, "{fixture} artists");
            assert_eq!(
                read.album_artist_names, edits.album_artists,
                "{fixture} album artists"
            );
            assert_eq!(read.genres, edits.genres, "{fixture} genres");
        }
    }

    #[test]
    fn raw_tags_come_back_sorted_by_key_then_value() {
        let dir = TempDir::new();
        let path = copy_fixture(&dir, "tagged_basic.flac");

        {
            let mut tagged = lofty::read_from_path(&path).unwrap();
            let tag = tagged.primary_tag_mut().unwrap();
            tag.insert_text(ItemKey::Composer, "Zed".into());
            tag.push(TagItem::new(
                ItemKey::Comment,
                ItemValue::Text("second".into()),
            ));
            tag.push(TagItem::new(
                ItemKey::Comment,
                ItemValue::Text("first".into()),
            ));
            tagged.save_to_path(&path, WriteOptions::default()).unwrap();
        }

        let raw = read_raw_tags(&path).unwrap();
        let pairs: Vec<(&str, &str)> = raw
            .iter()
            .map(|t| (t.key.as_str(), t.value.as_str()))
            .collect();
        let mut expected = pairs.clone();
        expected.sort();
        assert_eq!(pairs, expected, "{raw:?}");
        assert!(pairs.contains(&("COMMENT", "first")));
        assert!(pairs.contains(&("COMMENT", "second")));
    }

    #[test]
    fn raw_tags_skip_pictures_and_binary_values() {
        let dir = TempDir::new();
        let path = copy_fixture(&dir, "tagged_with_cover.flac");

        {
            let mut tagged = lofty::read_from_path(&path).unwrap();
            let tag = tagged.primary_tag_mut().unwrap();
            tag.push(TagItem::new(
                ItemKey::Comment,
                ItemValue::Binary(vec![0xde, 0xad, 0xbe, 0xef]),
            ));
            tagged.save_to_path(&path, WriteOptions::default()).unwrap();
        }

        assert!(
            lofty::read_from_path(&path)
                .unwrap()
                .primary_tag()
                .unwrap()
                .picture_count()
                > 0
        );
        let raw = read_raw_tags(&path).unwrap();
        assert_eq!(raw.len(), 1, "only the encoder is text: {raw:?}");
        assert_eq!(raw[0].key, "ENCODER");
    }

    #[test]
    fn raw_tags_trim_values_and_drop_blank_ones() {
        let dir = TempDir::new();
        let path = copy_fixture(&dir, "tagged_basic.flac");

        {
            let mut tagged = lofty::read_from_path(&path).unwrap();
            let tag = tagged.primary_tag_mut().unwrap();
            tag.insert_text(ItemKey::Composer, "  padded  ".into());
            tag.insert_text(ItemKey::Comment, "   ".into());
            tagged.save_to_path(&path, WriteOptions::default()).unwrap();
        }

        let raw = read_raw_tags(&path).unwrap();
        assert!(raw.iter().any(|t| t.value == "padded"), "{raw:?}");
        assert!(!raw.iter().any(|t| t.key == "COMMENT"), "{raw:?}");
    }

    #[test]
    fn raw_tags_on_a_missing_file_are_an_error() {
        let dir = TempDir::new();
        assert!(read_raw_tags(&dir.path.join("nope.flac")).is_err());
    }

    #[test]
    fn an_untouched_slash_form_number_keeps_its_of_n_half() {
        let dir = TempDir::new();
        let path = copy_fixture(&dir, "tagged_track_disc_slash.flac");

        let before = music_indexer::metadata::read_metadata(&path).unwrap();
        assert_eq!(before.track_number, Some(5));
        assert_eq!(before.disc_number, Some(2));

        let mut edits = full_edits();
        edits.track_number = before.track_number;
        edits.disc_number = before.disc_number;
        write_metadata(&path, &edits).unwrap();

        let after = music_indexer::metadata::read_metadata(&path).unwrap();
        assert_eq!(after.track_number, Some(5));
        assert_eq!(after.disc_number, Some(2));
        assert_eq!(
            raw_value(&path, ItemKey::DiscNumber),
            Some("2/3".to_string()),
            "the form has no `of N` field, so an unchanged number must not be respelled"
        );
    }

    #[test]
    fn a_changed_number_is_written_plainly() {
        let dir = TempDir::new();
        let path = copy_fixture(&dir, "tagged_track_disc_slash.flac");

        let mut edits = full_edits();
        edits.track_number = Some(5);
        edits.disc_number = Some(1);
        write_metadata(&path, &edits).unwrap();

        assert_eq!(raw_value(&path, ItemKey::DiscNumber), Some("1".to_string()));
        assert_eq!(
            raw_value(&path, ItemKey::TrackTotal),
            Some("12".to_string()),
            "lofty splits TRACKNUMBER's slash form on the way in, so its `of N` is a row of its \
             own and survives regardless — only DISCNUMBER's stays a single string"
        );
    }

    #[test]
    fn an_untouched_year_keeps_the_rest_of_its_date() {
        let dir = TempDir::new();
        let path = copy_fixture(&dir, "tagged_track_disc_slash.flac");

        let before = music_indexer::metadata::read_metadata(&path).unwrap();
        assert_eq!(before.year, Some(2023));
        assert_eq!(raw_value(&path, ItemKey::Year), Some("2023-06-15".into()));

        let mut edits = full_edits();
        edits.year = before.year;
        write_metadata(&path, &edits).unwrap();

        assert_eq!(
            raw_value(&path, ItemKey::Year),
            Some("2023-06-15".to_string()),
            "month and day are not recoverable once dropped"
        );

        edits.year = Some(2024);
        write_metadata(&path, &edits).unwrap();
        assert_eq!(raw_value(&path, ItemKey::Year), None);
        assert_eq!(
            raw_value(&path, ItemKey::RecordingDate),
            Some("2024".to_string()),
            "a real change still goes through the four-key clear"
        );
    }

    #[test]
    fn unrelated_raw_rows_survive_a_field_write() {
        let dir = TempDir::new();
        let path = copy_fixture(&dir, "tagged_track_disc_slash.flac");

        assert!(
            read_raw_tags(&path)
                .unwrap()
                .iter()
                .any(|t| t.key == "TRACKTOTAL" && t.value == "12")
        );

        write_metadata(&path, &full_edits()).unwrap();

        assert!(
            read_raw_tags(&path)
                .unwrap()
                .iter()
                .any(|t| t.key == "TRACKTOTAL" && t.value == "12"),
            "a field write must not touch rows it does not own"
        );
    }

    #[test]
    fn every_year_fallback_key_is_cleared() {
        let dir = TempDir::new();
        let path = copy_fixture(&dir, "tagged_basic.flac");

        {
            let mut tagged = lofty::read_from_path(&path).unwrap();
            let tag = tagged.primary_tag_mut().unwrap();
            tag.insert_text(ItemKey::Year, "1977".into());
            tag.insert_text(ItemKey::OriginalReleaseDate, "1978".into());
            tag.insert_text(ItemKey::ReleaseDate, "1979".into());
            tag.insert_text(ItemKey::RecordingDate, "1980".into());
            tagged.save_to_path(&path, WriteOptions::default()).unwrap();
        }

        let mut edits = full_edits();
        edits.year = None;
        write_metadata(&path, &edits).unwrap();

        assert_eq!(
            music_indexer::metadata::read_metadata(&path).unwrap().year,
            None,
            "read_year walks four keys, so all four have to go"
        );
    }

    #[test]
    fn artists_are_written_to_both_keys() {
        for fixture in ["tagged_basic.flac", "tagged_mp3.mp3"] {
            let dir = TempDir::new();
            let path = copy_fixture(&dir, fixture);
            write_metadata(&path, &full_edits()).unwrap();

            let tagged = lofty::read_from_path(&path).unwrap();
            let tag = tagged.primary_tag().unwrap();
            let plain: Vec<&str> = tag.get_strings(&ItemKey::TrackArtist).collect();
            let all: Vec<&str> = tag.get_strings(&ItemKey::TrackArtists).collect();

            assert_eq!(all, vec!["First Artist", "Second Artist"], "{fixture}");
            assert_eq!(
                plain,
                vec!["First Artist", "Second Artist"],
                "{fixture}: players that only know the plain key must not see a stale artist"
            );
        }
    }

    #[test]
    fn id3v1_is_left_untouched() {
        let dir = TempDir::new();
        let path = copy_fixture(&dir, "tagged_mp3.mp3");

        {
            let mut tagged = lofty::read_from_path(&path).unwrap();
            let mut v1 = Tag::new(TagType::Id3v1);
            v1.insert_text(ItemKey::TrackTitle, "Old V1 Title".into());
            tagged.insert_tag(v1);
            tagged.save_to_path(&path, WriteOptions::default()).unwrap();
        }

        write_metadata(&path, &full_edits()).unwrap();

        let tagged = lofty::read_from_path(&path).unwrap();
        let v1 = tagged
            .tags()
            .iter()
            .find(|t| t.tag_type() == TagType::Id3v1)
            .expect("the id3v1 tag must still be there");
        assert_eq!(v1.get_string(&ItemKey::TrackTitle), Some("Old V1 Title"));
        assert_eq!(
            music_indexer::metadata::read_metadata(&path)
                .unwrap()
                .title
                .as_deref(),
            Some("New Title"),
            "the indexer reads the primary tag, so the library never sees the stale one"
        );
    }

    #[test]
    fn writing_the_same_edits_twice_changes_nothing() {
        for fixture in [
            "tagged_basic.flac",
            "tagged_ogg.ogg",
            "tagged_mp3.mp3",
            "tagged_m4a.m4a",
        ] {
            let dir = TempDir::new();
            let path = copy_fixture(&dir, fixture);
            let edits = TrackTagEdits {
                added_tags: custom_row_if_supported(&path, "hello"),
                ..full_edits()
            };

            write_metadata(&path, &edits).unwrap();
            let once = read_raw_tags(&path).unwrap();
            let once_read = music_indexer::metadata::read_metadata(&path).unwrap();

            write_metadata(&path, &edits).unwrap();
            let twice = read_raw_tags(&path).unwrap();
            let twice_read = music_indexer::metadata::read_metadata(&path).unwrap();

            assert_eq!(once, twice, "{fixture} raw rows");
            assert_eq!(once_read.title, twice_read.title, "{fixture} title");
            assert_eq!(
                once_read.artist_names, twice_read.artist_names,
                "{fixture} artists"
            );
            assert_eq!(once_read.genres, twice_read.genres, "{fixture} genres");
            assert_eq!(once_read.year, twice_read.year, "{fixture} year");
        }
    }

    #[test]
    fn unicode_values_round_trip_per_container() {
        for fixture in [
            "tagged_basic.flac",
            "tagged_ogg.ogg",
            "tagged_mp3.mp3",
            "tagged_m4a.m4a",
        ] {
            let dir = TempDir::new();
            let path = copy_fixture(&dir, fixture);

            let edits = TrackTagEdits {
                title: Some("Прогулка 🎵".into()),
                artists: vec!["Кино".into(), "Аквариум".into()],
                album: Some("Группа крови".into()),
                genres: vec!["Пост-панк".into()],
                added_tags: custom_row_if_supported(&path, "значение 🎧"),
                ..TrackTagEdits::default()
            };
            write_metadata(&path, &edits).unwrap();

            let read = music_indexer::metadata::read_metadata(&path).unwrap();
            assert_eq!(read.title.as_deref(), Some("Прогулка 🎵"), "{fixture}");
            assert_eq!(read.artist_names, edits.artists, "{fixture}");
            assert_eq!(
                read.album_title.as_deref(),
                Some("Группа крови"),
                "{fixture}"
            );
            assert_eq!(read.genres, edits.genres, "{fixture}");
            if custom_tags_for(&path).supported() {
                assert!(
                    read_raw_tags(&path)
                        .unwrap()
                        .iter()
                        .any(|t| t.value == "значение 🎧"),
                    "{fixture}: a unicode custom value"
                );
            }
        }
    }

    #[test]
    fn a_custom_row_can_be_added_to_a_file_without_tags() {
        let dir = TempDir::new();
        let path = copy_fixture(&dir, "tagless.flac");

        write_metadata(
            &path,
            &TrackTagEdits {
                title: Some("From Nothing".into()),
                added_tags: vec![RawTag {
                    key: "MYCUSTOM".into(),
                    value: "hello".into(),
                }],
                ..TrackTagEdits::default()
            },
        )
        .unwrap();

        assert_eq!(
            music_indexer::metadata::read_metadata(&path)
                .unwrap()
                .title
                .as_deref(),
            Some("From Nothing")
        );
        assert!(
            read_raw_tags(&path)
                .unwrap()
                .iter()
                .any(|t| t.key == "MYCUSTOM" && t.value == "hello")
        );
    }

    #[test]
    fn a_riff_info_container_takes_field_edits_but_refuses_custom_rows() {
        let dir = TempDir::new();
        let path = copy_fixture(&dir, "1khz_16_44_1.wav");
        assert_eq!(custom_tags_for(&path), CustomTags::Unsupported);

        let edits = TrackTagEdits {
            title: Some("W Title".into()),
            artists: vec!["W Artist".into()],
            album: Some("W Album".into()),
            year: Some(1999),
            genres: vec!["Ambient".into()],
            ..TrackTagEdits::default()
        };
        write_metadata(&path, &edits).unwrap();

        let read = music_indexer::metadata::read_metadata(&path).unwrap();
        assert_eq!(read.title.as_deref(), Some("W Title"));
        assert_eq!(read.artist_names, vec!["W Artist".to_string()]);
        assert_eq!(read.year, Some(1999));

        let err = write_metadata(
            &path,
            &TrackTagEdits {
                added_tags: vec![RawTag {
                    key: "MYCUSTOM".into(),
                    value: "x".into(),
                }],
                ..edits
            },
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("RiffInfo"), "{err}");
    }

    #[test]
    fn custom_tags_for_reports_unsupported_when_the_file_cannot_be_read() {
        let dir = TempDir::new();
        assert_eq!(
            custom_tags_for(&dir.path.join("nope.flac")),
            CustomTags::Unsupported
        );
    }

    #[test]
    fn an_unsupported_container_is_gated_by_supported_not_by_check_key() {
        assert!(!CustomTags::Unsupported.supported());
        assert!(CustomTags::Free.supported());
        assert!(CustomTags::NoFrameIdLength.supported());
        assert_eq!(
            CustomTags::Unsupported.check_key("MYCUSTOM"),
            None,
            "check_key only judges the name — `supported` is what refuses the container"
        );
    }

    #[test]
    fn custom_keys_must_be_printable_ascii() {
        for key in [
            "",
            "   ",
            "has\ttab",
            "line\nbreak",
            "with=equals",
            "Настроение",
            "emoji 🎵",
            "~tilde",
        ] {
            assert_eq!(
                CustomTags::Free.check_key(key),
                Some(KeyProblem::Invalid),
                "{key:?} must be refused"
            );
        }
        for key in ["MY MOOD", "  PADDED  ", "\ttrimmed\n"] {
            assert_eq!(
                CustomTags::Free.check_key(key),
                None,
                "{key:?} is fine once trimmed, and the writer trims it the same way"
            );
        }
    }

    #[test]
    fn only_a_four_character_key_is_special_to_id3v2() {
        let long = "X".repeat(200);
        for key in ["ABC", "ABCDE", long.as_str()] {
            assert_eq!(
                CustomTags::NoFrameIdLength.check_key(key),
                None,
                "{key} is not a frame id"
            );
        }
        assert_eq!(
            CustomTags::NoFrameIdLength.check_key("ABCD"),
            Some(KeyProblem::FrameIdLength)
        );

        let dir = TempDir::new();
        let path = copy_fixture(&dir, "tagged_mp3.mp3");
        write_metadata(
            &path,
            &TrackTagEdits {
                added_tags: vec![
                    RawTag {
                        key: "ABC".into(),
                        value: "three".into(),
                    },
                    RawTag {
                        key: "ABCDE".into(),
                        value: "five".into(),
                    },
                ],
                ..full_edits()
            },
        )
        .unwrap();

        let raw = read_raw_tags(&path).unwrap();
        assert!(
            raw.iter().any(|t| t.key == "ABC" && t.value == "three"),
            "{raw:?}"
        );
        assert!(
            raw.iter().any(|t| t.key == "ABCDE" && t.value == "five"),
            "{raw:?}"
        );
    }

    #[test]
    fn three_values_under_one_key_survive_on_id3v2() {
        let dir = TempDir::new();
        let path = copy_fixture(&dir, "tagged_mp3.mp3");

        write_metadata(
            &path,
            &TrackTagEdits {
                added_tags: ["alpha", "beta", "gamma"]
                    .into_iter()
                    .map(|v| RawTag {
                        key: "MOODX".into(),
                        value: v.into(),
                    })
                    .collect(),
                ..full_edits()
            },
        )
        .unwrap();

        let raw = read_raw_tags(&path).unwrap();
        let values: Vec<&str> = raw
            .iter()
            .filter(|t| t.key == "MOODX")
            .map(|t| t.value.as_str())
            .collect();
        assert_eq!(values, vec!["alpha", "beta", "gamma"], "{raw:?}");
    }

    #[test]
    fn a_row_removed_and_re_added_in_one_save_keeps_the_new_value() {
        for fixture in ["tagged_basic.flac", "tagged_mp3.mp3"] {
            let dir = TempDir::new();
            let path = copy_fixture(&dir, fixture);

            write_metadata(
                &path,
                &TrackTagEdits {
                    added_tags: vec![RawTag {
                        key: "MOODX".into(),
                        value: "happy".into(),
                    }],
                    ..full_edits()
                },
            )
            .unwrap();

            write_metadata(
                &path,
                &TrackTagEdits {
                    removed_tags: vec![RawTag {
                        key: "MOODX".into(),
                        value: "happy".into(),
                    }],
                    added_tags: vec![RawTag {
                        key: "MOODX".into(),
                        value: "calm".into(),
                    }],
                    ..full_edits()
                },
            )
            .unwrap();

            let raw = read_raw_tags(&path).unwrap();
            let values: Vec<&str> = raw
                .iter()
                .filter(|t| t.key == "MOODX")
                .map(|t| t.value.as_str())
                .collect();
            assert_eq!(values, vec!["calm"], "{fixture}: {raw:?}");
        }
    }

    #[test]
    fn a_custom_row_can_be_removed_again() {
        for fixture in ["tagged_basic.flac", "tagged_mp3.mp3"] {
            let dir = TempDir::new();
            let path = copy_fixture(&dir, fixture);

            let row = RawTag {
                key: "MYCUSTOM".into(),
                value: "hello".into(),
            };
            write_metadata(
                &path,
                &TrackTagEdits {
                    added_tags: vec![row.clone()],
                    ..full_edits()
                },
            )
            .unwrap();
            assert!(read_raw_tags(&path).unwrap().contains(&row), "{fixture}");

            write_metadata(
                &path,
                &TrackTagEdits {
                    removed_tags: vec![row.clone()],
                    ..full_edits()
                },
            )
            .unwrap();
            assert!(
                !read_raw_tags(&path).unwrap().contains(&row),
                "{fixture}: an unknown key must be removable too"
            );
        }
    }

    #[test]
    fn diffing_reports_nothing_for_a_pure_reorder() {
        let row = |k: &str, v: &str| RawTag {
            key: k.into(),
            value: v.into(),
        };
        let original = vec![row("A", "1"), row("B", "2"), row("C", "3")];
        let current = vec![row("C", "3"), row("A", "1"), row("B", "2")];

        let (removed, added) = diff_raw_tags(&original, &current);
        assert!(removed.is_empty() && added.is_empty());
    }

    #[test]
    fn diffing_an_empty_side_reports_everything_on_the_other() {
        let row = |k: &str, v: &str| RawTag {
            key: k.into(),
            value: v.into(),
        };
        let rows = vec![row("A", "1"), row("B", "2")];

        let (removed, added) = diff_raw_tags(&rows, &[]);
        assert_eq!(removed, rows);
        assert!(added.is_empty());

        let (removed, added) = diff_raw_tags(&[], &rows);
        assert!(removed.is_empty());
        assert_eq!(added, rows);

        let (removed, added) = diff_raw_tags(&[], &[]);
        assert!(removed.is_empty() && added.is_empty());
    }

    #[test]
    fn diffing_reports_several_adds_and_removes_at_once() {
        let row = |k: &str, v: &str| RawTag {
            key: k.into(),
            value: v.into(),
        };
        let original = vec![row("A", "1"), row("B", "2"), row("C", "3")];
        let current = vec![row("B", "2"), row("D", "4"), row("E", "5")];

        let (removed, added) = diff_raw_tags(&original, &current);
        assert_eq!(removed, vec![row("A", "1"), row("C", "3")]);
        assert_eq!(added, vec![row("D", "4"), row("E", "5")]);
    }

    #[test]
    fn a_changed_value_under_one_key_is_a_remove_plus_an_add() {
        let row = |k: &str, v: &str| RawTag {
            key: k.into(),
            value: v.into(),
        };
        let original = vec![row("COMPOSER", "Old")];
        let current = vec![row("COMPOSER", "New")];

        let (removed, added) = diff_raw_tags(&original, &current);
        assert_eq!(removed, vec![row("COMPOSER", "Old")]);
        assert_eq!(added, vec![row("COMPOSER", "New")]);
    }

    #[test]
    fn writing_to_a_missing_file_is_an_error() {
        let dir = TempDir::new();
        assert!(write_metadata(&dir.path.join("nope.flac"), &full_edits()).is_err());
    }

    #[test]
    fn writing_to_a_file_that_is_not_audio_is_an_error() {
        let dir = TempDir::new();
        let path = dir.path.join("fake.flac");
        std::fs::write(&path, b"this is not a flac file").unwrap();
        assert!(write_metadata(&path, &full_edits()).is_err());
    }
}

# tag_writer

The write side of audio-file metadata: `write_metadata` applies a `TrackTagEdits` (form
fields, free-form rows, cover) to the file's tag, and the crate lists, diffs and validates
the rows the tag editor shows. GPUI-free, no SQLite. Its contract: what it writes reads
back unchanged through `music_indexer`.

Not here: reading (`music_indexer/src/doc.md`); re-indexing the DB after a write and
refusing cue tracks (`library_service.rs`) and the modal (`tag_editor_view.rs`), both in
`pawse/src/doc.md`.

## Files

- `lib.rs` — the whole crate: `TrackTagEdits`, `write_metadata`, `CoverEdit` +
  `cover_bytes_ok`, `RawTag` + `read_raw_tags` + `diff_raw_tags`, `CustomTags` +
  `KeyProblem` + `custom_tags_for`, and the round-trip tests (read back through
  `music_indexer`, a dev-dependency).
- `tests/raw_bytes.rs` — the same writes checked against the bytes with a hand-written
  FLAC and ID3v2 parser. The `lib.rs` tests write and read through the same `lofty`, so a
  mistake `lofty` makes symmetrically passes them and still breaks other players; only
  this file catches it. Keep it dependency-free: no external tagger is installed anywhere
  this repo builds, and one would make the suite platform-dependent. Ogg is not covered
  (same Vorbis comments as FLAC, only the page framing differs).

## Round-trip contract

Each field must read back equal per container, because a rescan re-derives the library
from the files.

| Field | Written to | Read by `music_indexer` as |
|---|---|---|
| title, album | `TrackTitle`, `AlbumTitle` | `title()`, `album()` |
| artists | `TrackArtists` and `TrackArtist` | `TrackArtists`, else `artist()` |
| album_artists | `AlbumArtist` | `AlbumArtist` |
| track / disc number | `TrackNumber` / `DiscNumber` | text before `/` |
| year | `RecordingDate` | `read_year`: `RecordingDate`, then three fallbacks |
| genres | `Genre`, multi-value | `normalize_genres` |

- Holds only for values the reader leaves alone: a genre containing `,` `;` or `/`, or a
  junk name, does not survive `normalize_genres`.
- Artists go to both keys: the reader prefers `TrackArtists`, third-party players show
  `TrackArtist` (TPE1 / ARTIST / ©ART).
- The target is `primary_tag`, else `first_tag`, else a fresh tag of `primary_tag_type()` —
  the reader's choice, or edits would look discarded. The tag is saved through
  `AudioFile::save_to_path` on the whole file so its other tags survive; ID3v1 is not
  touched (the indexer reads the primary tag, so the library never diverges).
- DSF/DFF are refused (`lofty` has no support): `write_metadata` errors, `read_raw_tags`
  returns nothing, `custom_tags_for` says `Unsupported`.
- Order in `write_metadata`: fields, removals, added rows, cover. Everything that can
  refuse (custom rows, cover format, non-audio file) fails before the save and leaves the
  file byte-identical.
- Empty means remove: `set_text` / `set_multi` trim, drop blanks and remove the key when
  nothing is left, so a cleared field reads back as `None`, not `Some("")`.

## Fields

- An unchanged value is not rewritten. The form shows year and disc as integers, but the
  file may say `2023-06-15` or `2/3`, and the rest is lost once dropped. `set_year` and
  `set_number` read the file's value through the reader's rules (`current_year`,
  `current_number`) and do nothing on a match; they duplicate `read_year` and the `/` split
  in `music_indexer`, so change them together. Track `5/12` would survive anyway (`lofty`
  splits the total into its own `TRACKTOTAL` row); `DISCNUMBER=2/3` stays one string.
- A changed year removes `Year`, `OriginalReleaseDate`, `ReleaseDate` and writes
  `RecordingDate`; clearing it empties all four keys, or an old value returns from a
  fallback.

## Multi-value

- `lofty`'s ID3v2 writer keeps one frame per key, so separate items collapse to the last
  value. `set_multi` NUL-joins for `TagType::Id3v2` (one ID3v2.4 frame that `lofty` reads
  back as several values) and pushes separate items elsewhere: Vorbis comments repeat
  fields, MP4 `ilst` holds several data atoms. `raw_bytes` asserts the tag is v2.4; in
  v2.3 a NUL-joined value would read as one artist `First\0Second`.
- Verified for flac, ogg, mp3, m4a. APE and other containers have no fixture.
- `set_multi` uses `push_unchecked` / `insert_unchecked` for every key: `Tag::push` checks
  the container's vocabulary and silently drops an `ItemKey::Unknown`. Keys are verified
  again at write time, so an out-of-spec one is dropped, not written corrupt. This is what
  lets free-form rows reuse `set_multi`.

## Free-form rows

- Rows with the same key are written in one call, merged with what the tag holds under it:
  `set_multi` replaces a key wholesale and on ID3v2 one key is one frame. Blank rows and
  keys that name a form field are ignored.
- On ID3v2 an unknown key becomes a `TXXX` frame (as does `ARTISTS`), on Vorbis a plain
  field. A 4-character ID3v2 name is taken by `lofty` for a literal frame id and refused
  at save time, which would abort the whole save.
- Key validity is decided by the container before the dialog closes: the writer runs on
  the background executor long after, and a refusal there costs every other edit.
  `custom_tags_for(path)` gives a `CustomTags`; `CustomTags::check_key` is the one rule the
  UI and `with_custom` both ask: trimmed, non-empty, ASCII `0x20..=0x7D` without `=`
  (`VorbisComments::push` drops anything else silently, so it applies to every container);
  not a form-field name in either vocabulary (`TITLE` and `TIT2` both); not 4 characters
  on ID3v2. `supported()` is a separate question (`Unsupported.check_key` returns `None`);
  the UI hides the add row when it is false.
- Only Vorbis comments and ID3v2 take custom rows. MP4 is refused up front: `lofty`'s
  generic-tag conversion never emits `----` freeform atoms, a 4-character key becomes a
  literal atom no player knows and any other length is dropped with no error, and a silent
  drop is what the UI cannot report. RIFF INFO takes field edits but not custom rows.
  Every other tag type is `Unsupported` for custom rows and has no fixture for field edits.

## Raw rows

- `read_raw_tags` lists what the form does not show: it skips form-field keys (year
  fallbacks and `Lyrics` included), non-text and blank values, and keys the container has
  no name for. Keys are the container's own (`TPE1`, `ARTIST`), values trimmed, the list
  sorted by key then value; encoder fields show up.
- `removed_tags` carries whole rows as `read_raw_tags` returned them. `apply` maps the key
  back with `ItemKey::from_key` (unmapped names become `Unknown`, so they stay removable)
  and keeps everything except that exact (key, value). Not by key alone: Vorbis repeats
  fields, so removing one `COMMENT` would delete both. Two identical rows are
  indistinguishable and go together.
- `diff_raw_tags(original, current)` returns `(removed, added)`, so the caller keeps no
  bookkeeping: add-then-delete cancels, repeats match one for one, a reorder is no change,
  an edited value is a removal plus an add. Removal then re-add in one save keeps the new
  value.

## Cover

- The cover is the picture itself; no container has a "path to the artwork" tag. Embedding
  overrides the external-file heuristic because the indexer reads the embedded picture
  first. Embedded art survives every other edit, the free-form path included.
- `Remove` clears every picture, not just the front: the reader falls back to the first
  picture of any type, so a leftover back cover would become the album art. `Replace`
  clears the same way, then pushes one `CoverFront`.
- Pictures never reach the raw rows; `Remove` is the only way to delete one. Do not
  surface `APIC` as a row — the value is binary and the list is text.

## Decisions

Owner's calls; ask before changing.

- JPEG and PNG only. `Picture::from_reader` also accepts GIF, BMP and TIFF, but MP4's
  `covr` takes only the first two and they are the only two every player renders, so more
  would make the result depend on the container. `cover_bytes_ok` is asked when a file is
  picked, not at save.

## Tests

- Fixtures (`make generate`): flac (tagless, tagged, with cover, `5/12` form), ogg, mp3,
  m4a, wav.
- Cover tests use `cover_alternate.png`: `tagged_with_cover.flac` is built from
  `cover_front.png`, so replacing a cover with that one looks like not replacing it. The
  removal test plants a `CoverBack` first, since a lone front cover cannot tell "remove the
  front" from "empty the list".

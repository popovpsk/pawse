# tag_writer

The write side of audio-file metadata, and the symmetric twin of `music_indexer`'s
read side. `music_indexer` is read-only by contract and `music_library` is
DB-only, so writing into the user's own files lives here — isolated, GPUI-free,
with its own round-trip tests. It knows nothing about SQLite or the UI.

`pawse::library_service` is the only consumer.

## Files

- `lib.rs` — the whole crate. `TrackTagEdits` (the editable field set),
  `write_metadata` (durable write), `RawTag` + `read_raw_tags` (the read-only
  "everything else in this file" list the tag-editor modal shows), and the
  round-trip tests.

## Non-obvious behavior

- **Round-trip alignment is the entire correctness contract.** Whatever this
  crate writes must come back out of `music_indexer::metadata::read_metadata`
  byte-identical, because a later full rescan re-derives the library from the
  files. The tests assert exactly that, per container, and are the reason to
  touch this crate carefully:

  | Field | Written to | Read by `metadata.rs` |
  |---|---|---|
  | title | `TrackTitle` | `tag.title()` |
  | album | `AlbumTitle` | `tag.album()` |
  | artists | `TrackArtists` **and** `TrackArtist` | `get_strings(TrackArtists)`, falls back to `artist()` |
  | album_artists | `AlbumArtist` | `get_strings(AlbumArtist)` |
  | track_number / disc_number | `TrackNumber` / `DiscNumber` | `get(…)`, split on `/` |
  | year | `RecordingDate` | `read_year`, which tries `RecordingDate` first |
  | genres | `Genre` (multi) | `normalize_genres(get_strings(Genre))` |

- **The target tag mirrors the reader's choice.** `primary_tag` first, then
  `first_tag`, and a fresh `Tag::new(primary_tag_type())` only when the file has
  no tag at all. Writing to a *different* tag than the reader reads would make
  every edit appear to be silently discarded. The tag is edited as an owned copy
  and put back with `insert_tag` — the custom-row path has to rebuild it through
  the container's own type anyway, and one copy keeps the whole function free of
  the borrow dance that reading the tag type while holding `&mut` would need.
  Saving goes through `AudioFile::save_to_path` on the whole `TaggedFile` (not
  `TagExt::save_to_path` on the one tag) so the file's other containers survive.

- **Multi-value writing is container-dependent — this is the sharp edge.** lofty's
  ID3v2 writer keeps one frame per key, so separately pushed items collapse to the
  *last* value (observed: three artists in, one artist out). ID3v2.4's own
  representation is a single NUL-separated frame, which lofty reads back as
  distinct items — so `set_multi` NUL-joins for `TagType::Id3v2` and pushes
  separate items for everything else. Vorbis Comments (flac, ogg) hold separate
  fields natively. `multi_values_survive_per_container` locks this down for all
  three; **MP4/m4a and APE are unverified** — there is no fixture for them, so add
  one to the test loop before trusting multi-value edits there.

- **Embedded art survives an edit**, including the custom-row path, which is the
  only one that ever rebuilt the tag. `embedded_cover_survives_a_custom_row` pins it
  against `fixtures/tagged_with_cover.flac`: the failure mode would be silent.

- **Custom rows need `push_unchecked`, not `push`.** `TrackTagEdits::added_tags`
  carries hand-typed key/value pairs, which arrive as `ItemKey::Unknown`. `Tag::push`
  *silently* refuses those — it checks `re_map` (does this key exist in the
  container's vocabulary?) and returns `false`, dropping the item. `push_unchecked` /
  `insert_unchecked` are lofty's documented way in for exactly this case, and keys
  are verified again at write time, so an out-of-spec one is dropped rather than
  corrupting the file. `set_multi` therefore uses the unchecked calls for *every*
  key: for one that maps, `re_map` was only ever a predicate, so nothing changes; for
  one that doesn't, the outcome is the same drop either way. That is what lets the
  custom path reuse `set_multi` — multi-value handling included — instead of
  rebuilding the tag through `VorbisComments` / `Id3v2Tag`, which is what an earlier
  version did.

- **Rows sharing a key are written in one call.** `set_multi` replaces a key
  wholesale, so `with_custom` groups by key and folds in whatever the tag already
  holds under it (`get_strings`) before writing. Doing otherwise loses values on
  ID3v2, where one key is one frame: `two_custom_rows_can_share_a_key` and
  `a_custom_row_joins_a_key_the_file_already_has` both failed that way.

- **ID3v2 cannot take a 4-character custom name.** Frame ids are exactly four
  characters, so lofty treats a 4-character unknown key as a literal frame id and
  refuses it while writing (`Attempted to write an invalid frame. ID: "MOOD"`), which
  would abort the save and take every other edit in it down too. `CustomTags` catches
  it first, and the UI asks the same question when the row is typed — see the note
  below. Vorbis has no such limit, which
  `id3v2_refuses_a_four_character_custom_name` asserts from both sides.

- **Key validity is decided by the container, before the dialog closes.**
  `custom_tags_for(path)` resolves a file to a `CustomTags`, and
  `CustomTags::check_key` is the single rule both the UI and `with_custom` ask. This
  matters for *when*, not just what: the writer runs on the background executor long
  after the dialog is gone, so a refusal there costs the user every other edit in the
  same save. The rule itself: `VorbisComments::push` drops a field whose name is
  outside ASCII `0x20..=0x7D` or contains `=` — without a word — so that is applied
  to every container; names the form already owns are refused too (checked against
  *both* vocabularies, so `TITLE` and `TIT2` go equally, whatever the file is tagged
  with); and `NoFrameIdLength` adds the ID3v2 rule above. `CustomTags::Unsupported`
  (MP4, APE, RIFF — no fixture to verify against) makes the UI hide the add row
  entirely rather than offer an input that only ever produces an error.

- **The edit lists are diffed, not tracked.** `diff_raw_tags(original, current)`
  turns "the rows the file had" and "the rows on screen" into
  `(removed_tags, added_tags)`. The caller therefore keeps no bookkeeping: a row
  added and then deleted in the same dialog cancels out by construction, and
  repeated key/value pairs are matched one for one, so deleting one of two identical
  rows reports exactly one removal. (The *writer* still cannot tell those two apart
  — see the deletion note above — but the arithmetic stays honest.)

- **Artists are written twice on purpose.** The reader prefers `TrackArtists`, so
  that key carries the exact list. `TrackArtist` (TPE1 / ARTIST / ©ART) gets the
  same values so third-party players — which only know that key — don't show a
  stale artist.

- **Clearing a year has to clear four keys.** `read_year` walks `RecordingDate` →
  `Year` → `OriginalReleaseDate` → `ReleaseDate`, so `set_year` removes the latter
  three unconditionally. Otherwise clearing the year would silently resurrect an
  old value from a fallback key.

- **A field the user did not change is not rewritten.** The form shows a year and a
  disc number as plain integers, but the file may spell them more richly —
  `2023-06-15`, `2/3`. Writing the parsed value back would quietly throw the rest
  away, and the month, day or disc total are not recoverable afterwards. So
  `set_number` and `set_year` first read the file's own value *through the reader's
  own rules* (`current_number` splits on `/`, `current_year` walks the same four keys
  and takes the same leading four digits) and do nothing when it already matches.
  A real edit still goes through the full path, four-key clear included.

  The rule only applies to fields whose display form loses information. Track
  numbers do not need it: lofty splits `TRACKNUMBER=5/12` into `TrackNumber` plus a
  `TrackTotal` row on the way in, so the total is a row of its own and survives
  anyway — `DISCNUMBER=2/3` stays one string, which is the asymmetry
  `a_changed_number_is_written_plainly` records.

- **Empty means remove, not empty string.** `set_text`/`set_multi` trim, drop
  blanks, and `remove_key` when nothing is left — so a cleared field reads back as
  `None`/empty rather than as `Some("")`.

- **Deletion round-trips through the displayed row, key *and* value.**
  `TrackTagEdits::removed_tags` holds whole `RawTag`s as `read_raw_tags` handed them
  out — the *container* key name (`TPE1`, `ARTIST`, …) plus the trimmed value. `apply`
  maps the key back with `ItemKey::from_key(tag_type, key)`, which falls back to
  `ItemKey::Unknown(key)` so a tag lofty has no generic name for is still removable,
  then `retain`s everything that is not that exact (key, value) pair. Matching on the
  key alone would be wrong: Vorbis Comments hold repeated fields natively (that is what
  `set_multi` relies on), so a file with two `COMMENT`s lists two rows under one key and
  `remove_key` would delete both. Two rows with an identical key *and* value are
  indistinguishable to the user and go together — the only case where one click removes
  more than one item. Removals run last, after the field writes, so deleting a key can
  never clobber a field the same save just set.

- **`read_raw_tags` deliberately skips what the form already shows** (the table
  above, plus `Lyrics`, which the lyrics view owns) and skips non-text values, so
  pictures and binary blobs never reach the UI. Keys are rendered with
  `ItemKey::map_key(tag_type, …)`, i.e. the container's own name (`TPE1`,
  `ARTIST`), which is what a user comparing against another tag editor expects to
  see. Encoder fields do show up — they are genuinely part of the file's tags.

- **ID3v1 is not touched.** On an mp3 carrying both ID3v2 and ID3v1, only the
  primary (ID3v2) tag is edited, so a strictly-ID3v1 reader keeps seeing the old
  values. The indexer reads the primary tag, so the library never diverges.

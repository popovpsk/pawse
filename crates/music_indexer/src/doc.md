# music_indexer

Turns a set of music folders into a stream of `ScanEvent`s (cover thumbnails,
prepared tracks, progress, errors) for the DB writer in `pawse::library_service`.
It owns directory walking, metadata/tag reading (`lofty`), CUE-sheet expansion,
external cover-art discovery, and change-detection fingerprinting. It does **not**
touch SQLite — it only emits events; the caller persists them.

The split is deliberate: `pipeline.rs` owns *concurrency*, `metadata.rs` + `cue.rs`
own *parsing rules*. Either can change without touching the other.

## Files

- `lib.rs` — crate root. Re-exports the public surface (`collect_sources`, `run`,
  `DirectoryScanner`, and the `types`). Holds the integration tests (fixture-based,
  GUI-less) that drive a real scan and assert on emitted `ScanEvent`s.
- `types.rs` — the data types crossing module/crate boundaries: `ScannedTrack`
  (raw parser output, carries cover *bytes*), `PreparedTrack` (DB-ready, cover
  bytes replaced by a content *hash*), `SourceSet` (walk result + fingerprint),
  and the `ScanEvent` enum.
- `pipeline.rs` — the only place with threading. `collect_sources` (cheap
  stat-only walk + fingerprint) and `run` (worker pool → events). Contains the
  `AUDIO_EXTENSIONS`/`CUE_EXTENSIONS` lists and `INDEXER_FORMAT_VERSION`.
- `metadata.rs` — `read_metadata` (tags → `ScannedTrack` for one standalone audio
  file) and external cover-art discovery (`find_external_cover_art` + helpers).
- `cue.rs` — CUE-sheet business logic: `process_cue_file` (one `.cue` → many
  `ScannedTrack`), audio-file resolution, multi-disc folder inference, and
  `read_cue_text` (encoding-tolerant CUE reader).
- `scanner.rs` — `DirectoryScanner::scan`, a one-shot convenience wrapper
  (`collect_sources` + `run` with no prior cover knowledge) for tests/ad-hoc use.
  Production rescans call `collect_sources` + `run` directly to take the fast path
  and reuse existing cover thumbnails.

## Non-obvious behavior

- **Fingerprint is filesystem-state only, salted by version.** `collect_sources`
  hashes every audio/cue/image file's `(path, mtime, size)`. The caller skips all
  DB work when the stored fingerprint matches (the "fast path"). Because the hash
  reflects on-disk bytes — not how the indexer interprets them — a behavior change
  on unchanged files would otherwise be invisible. `INDEXER_FORMAT_VERSION` is
  mixed into the hash input for exactly this reason: **bump it whenever a fix
  changes the tracks produced from the same files**, and every existing library
  reindexes once. (Image files are in the fingerprint so swapping cover art alone
  still triggers a rescan; stray non-media files are not, to avoid noise.)

- **CUE files may not be UTF-8.** EAC and similar rippers write Windows-1252
  (e.g. a `0x92` curly apostrophe in "I'm Alive"), which `std::fs::read_to_string`
  rejects. Always read CUE text via `cue::read_cue_text` (UTF-8 first, then a
  Windows-1252 fallback that never fails). It is used in **both** `process_cue_file`
  and `collect_sources` — they must agree, or the CUE's audio file is dropped from
  the skip set and gets double-indexed.

- **CUE de-duplication.** Audio referenced by a `.cue` is expanded via the cue, so
  `collect_sources` removes it from the standalone audio set (compared by
  `canonicalize`d path) to avoid indexing the whole-album file twice.

- **CUE track durations / offsets.** Each track's start is its `INDEX 01`; its
  duration is the next track's `INDEX 01` minus its own, and the last track runs to
  the audio file's full duration (read once via `lofty`). CUE tracks share one
  `path`, distinguished by `start_offset_ms`.

- **Multi-disc CUE albums.** The CUE format has no disc field. When a cue sits in a
  `CD1`/`Disc 2`/etc. folder (`parse_disc_folder`), the disc number comes from the
  folder and the album title is anchored to the shared parent folder
  (`clean_album_folder_title`, strips a leading `YYYY -` date and a trailing
  `[catalog]`) so both discs merge into one album. Otherwise the CUE's own title is
  used and disc defaults to 1.

- **Audio-file resolution is stem-tolerant.** `resolve_audio_file` first tries the
  exact `FILE` name, then any sibling with the same stem and a supported audio
  extension — rippers often leave the original `.wav` name in the cue after
  encoding to FLAC.

- **Cover art: many fallbacks, one dedupe.** Order: embedded `CoverFront` picture →
  first embedded picture → external file. External search (`find_external_cover_art`)
  walks the track dir, named artwork subdirs (`ARTWORK_DIR_NAMES`), then the album
  parent and its artwork subdirs. File ranking prefers `cover`/`folder`/`front`/…
  prefixes, demotes `back`/`disc`/… via **word-boundary** matching (so "cd" inside a
  catalog number like `WIGCD188J` is not a false positive — see `contains_word`),
  and treats RED/OPS `*_01.jpg` as the front cover. In `pipeline::emit_track`, each
  unique cover (by SHA-256) is thumbnailed exactly once across all workers; peers
  reference the hash and let the writer resolve the id. Hashes already in the DB
  (`known_hashes`) skip thumbnail generation entirely.

- **Graceful degradation.** A failed file (bad tags, unreadable cue) emits a
  `ScanEvent::Error` and the scan continues. Every `tx.send` is checked: a dropped
  receiver makes workers stop pulling work.

- **Event ordering.** `run` feeds cue files to the workers before standalone audio
  so cue tracks aren't starved behind a large standalone backlog. `Progress` is
  emitted every `PROGRESS_INTERVAL` tracks; a final `Complete` always closes the
  stream.

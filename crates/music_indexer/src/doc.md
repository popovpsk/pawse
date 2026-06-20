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
  the `ScanEvent` enum, and `IndexedLyrics`/`LyricsSource` (a track's words plus
  whether they are timestamped and where they came from — `lrc` sidecar or
  `embedded` tag). Both `ScannedTrack` and `PreparedTrack` carry an
  `Option<IndexedLyrics>` that rides straight through `into_prepared`.
- `pipeline.rs` — the only place with threading. `collect_sources` (cheap
  stat-only walk + fingerprint) and `run` (cue-dedup + worker pool → events).
  Contains the `AUDIO_EXTENSIONS`/`CUE_EXTENSIONS`/`FINGERPRINT_IMAGE_EXTENSIONS`/
  `FINGERPRINT_LYRICS_EXTENSIONS` lists and `INDEXER_FORMAT_VERSION`.
- `metadata.rs` — `read_metadata` (tags → `ScannedTrack` for one standalone audio
  file), date/genre normalization (`read_year`, `normalize_genres`), lyrics
  resolution (`read_sidecar_lrc` + `read_lyrics`), and external cover-art
  discovery (`find_external_cover_art` + helpers).
- `cue.rs` — CUE-sheet business logic: `process_cue_file` (one `.cue` → many
  `ScannedTrack`), audio-file resolution, multi-disc folder inference, and
  `read_cue_text` (encoding-tolerant CUE reader).
- `scanner.rs` — `DirectoryScanner::scan`, a one-shot convenience wrapper
  (`collect_sources` + `run` with no prior cover knowledge) for tests/ad-hoc use.
  Production rescans call `collect_sources` + `run` directly to take the fast path
  and reuse existing cover thumbnails.

## Non-obvious behavior

- **Fingerprint is filesystem-state only, salted by version.** `collect_sources`
  hashes every audio/cue/image/lyric (`.lrc`) file's `(path, mtime, size)`. It is *truly*
  stat-only — no decoding, no cue parsing — so the fast path stays cheap; the
  per-file stat runs on the walk's worker threads (`process_read_dir`) and only
  for files that feed the fingerprint. The caller skips all DB work when the
  stored fingerprint matches (the "fast path"). Because the hash reflects on-disk
  bytes — not how the indexer interprets them — a behavior change on unchanged
  files would otherwise be invisible. `INDEXER_FORMAT_VERSION` is mixed into the
  hash input for exactly this reason: **bump it whenever a fix changes the tracks
  produced from the same files**, and every existing library reindexes once.
  (Image files are in the fingerprint so swapping cover art alone still triggers a
  rescan; `.lrc` sidecars are in it too so adding or editing words alone triggers
  one; stray non-media files are not, to avoid noise.) The current version is **3**
  (v3 added lyrics reading — `.lrc` sidecar then embedded tag); a `.lrc` is a
  fingerprint input only, never a track, so it stays out of `audio_files`/`cue_files`.

- **CUE files may not be UTF-8.** EAC and similar rippers write Windows-1252
  (e.g. a `0x92` curly apostrophe in "I'm Alive"), which `std::fs::read_to_string`
  rejects. Always read CUE text via `cue::read_cue_text` (UTF-8 first, then a
  Windows-1252 fallback that never fails). It is used in **both** `process_cue_file`
  and `run`'s de-dup pass — they must agree, or the CUE's audio file is dropped
  from the skip set and gets double-indexed.

- **CUE de-duplication.** Audio referenced by a `.cue` is expanded via the cue, so
  `run` (not the stat-only `collect_sources`) removes it from the standalone audio
  set (compared by `canonicalize`d path) to avoid indexing the whole-album file
  twice. Keeping this in `run` means the fast path never parses cues or
  canonicalizes; only a real scan pays for it.

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

- **Year & genre tags.** `read_year` tries `RecordingDate` (the modern `DATE`/`TDRC`
  field most taggers write) first, then `Year` / `OriginalReleaseDate` / `ReleaseDate`,
  and extracts the leading 4-digit year (so `1994-05-12` → `1994`; a bare 2-digit year
  is dropped). `normalize_genres` splits each value on `,` `;` `/` — but **not** `&`, so
  `Drum & Bass` / `R&B` survive — then trims/collapses whitespace, drops junk
  (`Album` / `Unknown` / numeric-only), and dedups case-insensitively. The resulting
  `Vec<String>` becomes `genres` + `track_genres` rows in the writer. Changing either
  reading is exactly what an `INDEXER_FORMAT_VERSION` bump is for.

- **Cover art: many fallbacks, one dedupe.** Order: embedded `CoverFront` picture →
  first embedded picture → external file. External search (`find_external_cover_art`)
  walks the track dir, named artwork subdirs (`ARTWORK_DIR_NAMES`), then the album
  parent and its artwork subdirs. File ranking prefers `cover`/`folder`/`front`/…
  prefixes, demotes `back`/`disc`/… via **word-boundary** matching (so "cd" inside a
  catalog number like `WIGCD188J` is not a false positive — see `contains_word`),
  and treats RED/OPS `*_01.jpg` as the front cover. A track carries its cover as
  `CoverArt::Bytes` (embedded, or an external cover read for the first time) or
  `CoverArt::Cached(hash)` (an external cover an earlier track in the same dir
  already resolved). The per-scan `CoverCache` (keyed by parent dir, storing only
  hashes — never bytes, so memory stays bounded) means each album's external cover
  is enumerated/read/hashed **once**, not once per track. In `pipeline::emit_track`,
  each unique `Bytes` cover (by SHA-256) is thumbnailed exactly once across all
  workers; peers reference the hash and let the writer resolve the id. Hashes
  already in the DB (`known_hashes`) skip thumbnail generation entirely.

- **Lyrics: sidecar wins, CUE gets none.** `read_lyrics` prefers a `.lrc` sidecar
  (same stem as the audio, same dir — `read_sidecar_lrc`, UTF-8 with a lossy
  fallback) over the embedded `ItemKey::Lyrics` tag (USLT / `©lyr`). `synced` comes
  from `lyrics::has_timestamps`; empty/whitespace-only text yields no lyrics. CUE
  tracks always get `lyrics: None` — they share one whole-album audio file, so its
  embedded text would be the wrong words for every track.

- **Graceful degradation.** A failed file (bad tags, unreadable cue) emits a
  `ScanEvent::Error` and the scan continues. Every `tx.send` is checked: a dropped
  receiver makes workers stop pulling work.

- **Event ordering.** `run` feeds cue files to the workers before standalone audio
  so cue tracks aren't starved behind a large standalone backlog. `Progress` is
  emitted every `PROGRESS_INTERVAL` tracks; a final `Complete` always closes the
  stream.

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
  `embedded` tag). Both `ScannedTrack` and `PreparedTrack` carry a
  `Vec<IndexedLyrics>` (at most one per source) that rides straight through
  `into_prepared`.
- `pipeline.rs` — the only place with threading. `collect_sources` (cheap
  stat-only walk + fingerprint) and `run` (cue-dedup + worker pool → events).
  Contains the `AUDIO_EXTENSIONS`/`CUE_EXTENSIONS` (both public, so other
  sources classify files the same way)/`FINGERPRINT_IMAGE_EXTENSIONS`/
  `FINGERPRINT_LYRICS_EXTENSIONS` lists and `INDEXER_FORMAT_VERSION`.
- `metadata.rs` — `read_metadata` (tags → `ScannedTrack` for one standalone audio
  file), year normalization (`read_year`; genres go through
  `music_library::normalize_genres`, shared with server listings), lyrics
  resolution (`read_sidecar_lrc` + `read_lyrics`), and external cover-art
  discovery (`find_external_cover_art` + helpers). The name ranking behind it is
  a pure function, `best_cover_name` (plus `is_cover_image_name`,
  `is_artwork_dir_name`), so a caller that only has a file list — the torrent
  source in `pawse` — picks the same cover the scan would, without reading any
  image.
- `cue.rs` — CUE-sheet business logic: `read_cue` (parse, split-rip check and
  resolution of every audio file the tracks live in — the one decision both `run`'s
  de-dup pass and `process_cue_file` take), `process_cue_file` (one `.cue` → many
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
  one; stray non-media files are not, to avoid noise.) The current version is **6**
  (v3 added lyrics reading — `.lrc` sidecar then embedded tag; v6 stores each
  track's own album-artist tag and derives every album's artist from those, so the
  artist grouping no longer depends on scan order). Note what this crate does *not*
  do: it reports tags verbatim, placeholder credits (`[no artist]`, `Unknown Artist`)
  included, because `read_metadata` also pre-fills the tag editor — dropping them
  here would make saving any other field write the emptied artist back to the file.
  The library filters them on the way in instead (`library_service::clean_artist_names`); a `.lrc` is a
  fingerprint input only, never a track, so it stays out of `audio_files`/`cue_files`.

- **CUE files may not be UTF-8.** EAC and similar rippers write Windows-1252
  (e.g. a `0x92` curly apostrophe in "I'm Alive"), which `std::fs::read_to_string`
  rejects. Always read CUE text via `cue::read_cue_text` (UTF-8 first, then a
  Windows-1252 fallback that never fails). Both `process_cue_file` and `run`'s
  de-dup pass reach it through `cue::read_cue` — they must agree, or the CUE's
  audio file is dropped from the skip set and gets double-indexed.

- **CUE de-duplication.** Audio a `.cue` expands (`read_cue`'s resolved files) is
  removed by `run` (not the stat-only `collect_sources`) from the standalone audio
  set (compared by `canonicalize`d path) to avoid indexing the whole-album file
  twice. Keeping this in `run` means the fast path never parses cues or
  canonicalizes; only a real scan pays for it. A cue that fails (parse error, a
  referenced file missing) expands nothing and drops nothing, so its audio is still
  indexed standalone next to the `ScanEvent::Error`.

- **Split-rip cues are skipped, silently.** EAC and friends write a cue next to a
  rip with one file per track (`FILE` per `TRACK`, the gap before a track often
  written as an `INDEX 00` at the end of the previous file). When more than one
  track exists and no file holds two of them (a `FILE` line repeated with the same
  name is the same file — some tools write one before every track of an image),
  `read_cue` returns `None`: the cue adds nothing over the files' own tags, and
  indexing the files standalone keeps them first-class (lyrics, tag edits, casting
  the original). No tracks, no error, files not dropped, and no file resolution —
  so missing files in such a cue are not reported either. A one-track single-file
  cue still expands via the cue, as before.

- **Cues over several files.** Once any file holds two or more tracks (one cue for
  `CD1.flac` + `CD2.flac`, vinyl sides, a hidden track inside the last split file)
  the cue is authoritative for every file it names: all of them expand via the cue
  and all are dropped from the standalone set, so the album is not split between
  cue tracks and tag tracks. Every referenced file must resolve, otherwise the
  whole cue is an error. Disc number still comes only from the folder; the cue's
  track numbers run on across files.

- **Data tracks.** `TRACK nn MODE1/2352` and other non-`AUDIO` tracks (enhanced
  CDs) are skipped by `cue_parser`; their `INDEX` lines never attach to the
  preceding audio track. Before this they failed the whole sheet with
  "no AUDIO tracks found". The multi-file and data-track handling did not bump
  `INDEXER_FORMAT_VERSION` (by decision): it only changes cues that used to fail
  outright, and those pick up the new expansion on the next real rescan.

- **CUE track durations / offsets.** Each track's start is its `INDEX 01` (in the
  file that `INDEX 01` is written under); its duration is the next track's
  `INDEX 01` minus its own when the next track is in the same file, otherwise the
  track runs to the end of its own file (each file's duration read once via
  `lofty`). CUE tracks of one file share its `path`, distinguished by
  `start_offset_ms`; the cover is read per file. A track with no `INDEX 01` (only
  an `INDEX 00`, e.g. a truncated split-rip cue) fails the whole sheet in
  `cue_parser` instead of landing on offset 0 next to track 1, where it would hit
  the `(path, start_offset_ms)` unique index. The start rule lives in one place,
  `CueTrack::start`.

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

- **Lyrics: every disk source, CUE gets none.** `read_lyrics` returns both the
  `.lrc` sidecar (same stem as the audio, same dir — `read_sidecar_lrc`, UTF-8 with
  a lossy fallback) and the embedded `ItemKey::Lyrics` tag (USLT / `©lyr`), sidecar
  first; empty/whitespace-only text is dropped. Which one is shown is not decided
  here: the library keeps one row per source and the lyrics panel picks by
  priority (`music_library::lyrics_source::pick`) and lets the user switch —
  embedded text is often junk. The raw text is stored as-is (timed vs. plain is
  re-derived at display time by `lyrics::parse_lrc`). CUE tracks always get no
  lyrics — they share one whole-album audio file, so its embedded text would be the
  wrong words for every track. Emitting both sources did not bump
  `INDEXER_FORMAT_VERSION` (by decision, to spare a full reindex): a track that has
  both a sidecar and a tag gains the second row the next time its folder is
  rescanned for real.

- **Graceful degradation.** A failed file (bad tags, unreadable cue) emits a
  `ScanEvent::Error` and the scan continues. Every `tx.send` is checked: a dropped
  receiver makes workers stop pulling work.

- **Event ordering.** `run` feeds cue files to the workers before standalone audio
  so cue tracks aren't starved behind a large standalone backlog. `Progress` is
  emitted every `PROGRESS_INTERVAL` tracks; a final `Complete` always closes the
  stream.

# music_indexer

Turns music folders into a stream of `ScanEvent`s (deduplicated cover thumbnails,
`PreparedTrack`s, progress, errors) and computes the change-detection fingerprint. It
owns directory walking, tag reading (`lofty`; DSF/DFF through the `dsd` crate), CUE
expansion, external cover discovery and `INDEXER_FORMAT_VERSION`. `read_metadata` is
also the tag editor's prefill and the app's single-file re-read after a tag edit, so its
output can end up written back into the user's file.

Not here: SQLite (`music_library/src/doc.md`); the fast-path decision, persisting events
and dropping placeholder credits (`library_service.rs` in `pawse/src/doc.md`); writing
tags (`tag_writer/src/doc.md`); CUE syntax (`cue_parser`, no doc); `normalize_genres`
(`music_library/src/doc.md`).

## Files

- `pipeline.rs` — the only file with threads: `collect_sources`, `run`, the extension
  lists (`AUDIO_EXTENSIONS`, `CUE_EXTENSIONS` are public so the torrent index classifies
  files the same way), the version constant.
- `metadata.rs` — `read_metadata`, year/lyrics reading, external cover discovery,
  `CoverCache`. `best_cover_name`, `is_cover_image_name`, `is_artwork_dir_name` are pure so
  callers with only a file list (torrent index, Covers tool, album export) pick the cover
  a scan would. `extract_embedded_cover` and `load_cover_from_source` are the app's
  cover re-reads.
- `cue.rs` — `read_cue`, `process_cue_file`, audio-file resolution, disc-folder rules.
- `types.rs` — `ScannedTrack` (cover as bytes) → `PreparedTrack` (cover as hash), `SourceSet`,
  `ScanEvent`, `CoverArt`, `IndexedLyrics`.
- `scanner.rs` — `DirectoryScanner`, a one-shot `collect_sources` + `run`; only tests use it.
- `lib.rs` — re-exports and the fixture-based, GUI-less tests that run a real scan.

## Pipeline

- `pipeline.rs` owns concurrency only; parsing rules live in `metadata.rs` / `cue.rs`.
- `run` blocks until the workers finish: call it on its own thread, read the receiver
  elsewhere. Cue files are queued first so cue tracks do not wait behind a big backlog.
- A failing file emits `ScanEvent::Error` and the scan goes on. Every send is checked; a
  dropped receiver stops the workers. `Complete` always closes the stream.
- Each unique cover (SHA-256) is thumbnailed once across workers (`claimed`) and emitted
  as one `Cover` event; tracks carry the hash and the writer resolves the id. `known_hashes`
  (already in the DB) skip thumbnailing. A failed thumbnail releases its claim so another
  track can retry — otherwise every track sharing that cover is written art-less.

## Fingerprint and `INDEXER_FORMAT_VERSION`

- The fingerprint hashes the version plus the sorted `(path, mtime, size)` of every audio,
  `.cue`, jpg/jpeg/png and `.lrc` file. Images are in so swapping a cover alone triggers a
  rescan, `.lrc` so editing words alone does; other stray files stay out to avoid noise.
  A `.lrc` is a fingerprint input only, never a track.
- `collect_sources` stays stat-only: no decoding, no cue parsing, no canonicalizing. That
  is the whole cost of the fast path, so cue-referenced audio is dropped later, in `run`.
- The hash sees only on-disk state. Bump `INDEXER_FORMAT_VERSION` whenever the same
  unchanged files would produce different tracks (tag, year, genre, lyrics, cover or cue
  rules); every library reindexes once and the reindex keeps playlists and likes.

## Reading tags

- The tag is `primary_tag`, else `first_tag`; `tag_writer` writes to the same one.
- Report tags as the file has them; do not add cleaning to `read_metadata`. Placeholder
  credits (`[no artist]`, `Unknown Artist`) stay in `artist_names` and the library drops
  them at ingest (`clean_artist_names`): dropping them here would make saving any other
  field in the tag editor write the emptied artist back to the file.
- What is reduced today: genres go through `normalize_genres` (an editor save writes them
  back normalized); year is the leading four digits of the first of `RecordingDate`,
  `Year`, `OriginalReleaseDate`, `ReleaseDate` that yields one; track and disc number are
  the part before `/`. Year and numbers survive an editor save only because `tag_writer`
  re-reads the file's value through the same rules and skips an unchanged one — change
  both sides together.
- `album_artist_names` is only the file's own `AlbumArtist` tag, empty when absent; the
  library derives an album's artist from those, so grouping does not depend on scan order.
- `release_id` is the raw `MusicBrainzReleaseId` (`MUSICBRAINZ_ALBUMID`,
  `TXXX:MusicBrainz Album Id`, the iTunes freeform atom); `release_year` is the leading
  four digits of `ReleaseDate` (`RELEASEDATE`, `TDRL`) only, never the recording date
  `year` prefers. Both key the album (`music_library/src/doc.md`, "Album identity");
  CUE and DSD tracks have neither.
- DSF/DFF skip `lofty`: `dsd::read_tags` supplies the tags, there is no bitrate, lyrics
  come from the sidecar only.
- Lyrics: the `.lrc` sidecar (same stem and directory) and the embedded tag are both
  returned, sidecar first, blank text dropped, text stored verbatim. Which one is shown is
  not decided here: the library keeps a row per source and the lyrics panel picks. CUE
  tracks get none — they share one whole-album file, so its embedded text would be wrong
  for every track.

## Cover art

- Order: embedded `CoverFront`, else the first embedded picture, else an external file in
  the track's directory, its artwork subdirectories, the parent, the parent's artwork
  subdirectories (`find_external_cover_art`). Negative ranking words (`back`, `cd`, …)
  match on word boundaries, so `cd` inside a catalog number such as `WIGCD188J` does not
  demote a file.
- `CoverArt::embedded` is true only for a picture inside the file, and
  `extract_embedded_cover` returns only that: the tag editor must show exactly what Remove
  would remove, not a neighbouring `cover.jpg`.
- `CoverCache` keys the external lookup by parent directory and stores hashes, never bytes,
  so memory stays bounded: the first track of a directory returns `Bytes`, its siblings
  `Cached(hash)`. Workers racing on a directory's first track may both read the cover;
  thumbnails are still made once. CUE tracks bypass the cache and read per audio file.

## CUE

- Read cue text only through `read_cue_text`: UTF-8, else a Windows-1252 fallback that
  cannot fail (EAC writes Windows-1252).
- `read_cue` is the single decision about a cue: parse, split-rip check, resolve every
  audio file. `run`'s dedupe pass and `process_cue_file` both call it and must agree, or an
  expanded image is also indexed standalone.
- Audio a cue resolves is removed from the standalone set in `run`, by canonical path. A
  cue that fails (parse error, missing file) expands and drops nothing: its audio is
  indexed standalone next to the `ScanEvent::Error`.
- Split-rip cues are skipped silently: more than one track and each in its own file (a
  repeated `FILE` with the same name is the same file). `read_cue` returns `None`: no
  tracks, no error, no file resolution, so missing files in such a cue go unreported. The
  files stay first-class standalone tracks (lyrics, tag edits, casting the original).
- Once any file holds two or more tracks, the cue is authoritative for every file it names:
  all expand via the cue, none is indexed standalone, and every referenced file must
  resolve or the whole cue is an error.
- `cue_parser` skips non-`AUDIO` tracks (enhanced-CD data) with their `INDEX` lines and
  fails a sheet whose track has no `INDEX 01`: it would land on offset 0 next to track 1
  and hit the `(path, start_offset_ms)` unique index. `CueTrack::start` is the only start
  rule. A track's duration is the next track's start minus its own when that track is in
  the same file, else the rest of its file; tracks of one file share its `path`.
- Year is `REM DATE` parsed as a plain integer (a full date gives none).
- The sheet has no disc field. A cue inside a folder named exactly `CD n` / `Disc n` takes
  that disc number and anchors the album title to the cleaned parent folder name, so the
  discs merge into one album; otherwise the sheet title and disc 1.
- `resolve_audio_file`: the exact `FILE` name, else a sibling with the same stem
  (case-insensitive) and a supported extension — rippers leave `.wav` in the cue after
  encoding to FLAC.

## Decisions

Owner's calls; ask before changing.

- A change that only affects input that used to fail (multi-file cues, data tracks) or only
  adds rows (the second lyrics source) does not bump `INDEXER_FORMAT_VERSION`, to spare a
  full reindex. It shows up when the folder is next rescanned for real.

# tools/covers

The Covers page of the Tools screen: finds covers for albums that have none and
saves them as `cover.jpg` next to the album's files. The music files are never
touched; the next scan picks the image up through
`music_indexer::metadata::find_external_cover_art` (image files are part of the
scan fingerprint, so a new `cover.jpg` is enough to trigger it).

## Files

- `mod.rs` — `CoversState` (phase, progress, found rows with their checkboxes
  and thumbnails, skipped albums with reasons, precomputed status texts), the
  `find` / `apply` actions and the page. `ToolsView` rebuilds the page when
  `CoversState::layout()` changes (the results and skipped items only exist when
  there is something to show) and calls `relabel` on a language change.
- `plan.rs` — `plan`, a pure function from `albums()` + `all_tracks()` to the
  albums worth searching (`Job` = album + target folder) and the ones skipped
  with a `Skip` reason. Tests live here.
- `folder.rs` — everything that touches the album folder: `check` (is there
  already a cover / an image the scan would use), `validate` (downloaded bytes
  are a JPEG/PNG of at least 200 px) and `write`.

## Which albums

`cover_art_id IS NULL`, not the no-metadata bucket, with at least one local,
available track (server-only and offline albums are silently left out — there
is no folder to write to). No artist or no title → skipped, nothing to search by.

## Where the file goes

- All tracks in one folder → that folder. A folder plus its own subfolders
  (`X/` and `X/Bonus/`) → `X/`. Tracks in sibling folders (`CD1/`, `CD2/`) →
  their common parent, which the scan checks after the track folder. Anything
  else → skipped as scattered.
- **Shared folders are skipped.** The scan looks for an external cover in the
  track's folder and its parent, so a `cover.jpg` in a folder that tracks of
  another album (or tracks with no album) see through either level would become
  *their* cover too. `plan` records, for every folder, which albums see it and
  skips the target if any other album does. That covers flat "Singles" folders
  and artist folders with loose tracks next to album subfolders. A target with
  an artwork-folder name (`Images/`, `Art/`, `Covers/` — `is_artwork_dir_name`)
  is also seen from its parent, because the scan searches such subfolders too.
- **Never overwrite.** `check` refuses a folder with any `cover.*` image, or with
  an image `best_cover_name` would pick (the album has no cover although the scan
  should have found one — something is off, leave it alone). Back / CD / booklet
  scans alone don't block. `check` runs before the search (no requests for
  albums that can't be written) and again right before writing.
- **Write** goes to a hidden temp file (`.cover.jpg.pawse-tmp`, `create_new`),
  then `hard_link` to the final name, which fails instead of replacing an
  existing file; filesystems without hard links fall back to `rename` after an
  existence check. PNG downloads are saved as `cover.png`.
- A read-only folder shows up as `PermissionDenied` / `ReadOnlyFilesystem` on
  the temp file and becomes the "read-only" reason. There is no up-front write
  probe: it would create and delete files in every music folder just for looking.

## Flow

- **Find** runs `plan` + `check` on a background task, then searches album by
  album, each search its own `background_spawn` (the `Finder` is moved in and
  back out, it carries the rate-limit clocks). Progress and rows update after
  every album; **Stop** takes effect between albums. With iTunes' 3 s gap a
  library with hundreds of coverless albums takes many minutes, so the state
  lives in `ToolsView` and survives leaving the Tools screen.
- Rows: exact hits start checked, uncertain ones unchecked with a note. The
  second line shows what the service called the album, so the user can see why
  it is uncertain. Checkboxes work while the search is still running (rows are
  only appended then) and are disabled while saving, which removes rows and
  would shift the indices the checkboxes were rendered with.
- The skipped list's text is appended line by line as albums are skipped and
  rebuilt in full only on a new search or a language change. Thumbnails are `RenderImage`s released with
  `drop_atlas_tile` when a row goes away or a new search starts.
- **Save** downloads the full-size image for each checked row, validates and
  writes it. Written rows leave the list, failed ones move to the skipped list
  with the reason. One `request_rescan` at the end if anything was written.
- Summary: "found X of Y" after the search (Y = albums actually searched),
  "found X of Y, saved Z" after saving.

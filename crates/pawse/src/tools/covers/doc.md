# tools/covers

The Covers page of the Tools screen: finds covers for albums that have none and saves them as `cover.jpg` (`cover.png` for a PNG download) next to the album's files. The music files are never touched; the next scan picks the image up through `music_indexer::metadata::find_external_cover_art` (image files are part of the scan fingerprint, so a new file is enough).

Not here: the web lookup and rate limits (`crates/cover_search/src/doc.md`); hosting and rebuilding the page (`tools/doc.md`).

## Files

- `mod.rs` — `CoversState`, the `find` / `apply` / `stop` actions and the page. `layout()` tells `ToolsView` when to rebuild the page.
- `plan.rs` — `plan`: pure function from `albums()` + `all_tracks()` to the albums worth searching (`Job` = album + target folder) and the ones skipped with a `Skip` reason. Tests live here.
- `folder.rs` — everything that touches the album folder: `check`, `validate` (a JPEG/PNG with a minimum side) and `write`.

## Which albums

No cover (`cover_art_id` empty), not the no-metadata bucket, at least one local available track: server-only and offline albums are silently left out, there is no folder to write to. No artist or no title: skipped.

## Where the file goes

- All tracks in one folder: that folder. A folder plus its own subfolders (`X/`, `X/Bonus/`): `X/`. Sibling folders (`CD1/`, `CD2/`): their common parent, which the scan checks after the track folder. Anything else: skipped as scattered.
- **Shared folders are skipped.** The scan looks for an external cover in the track's folder and its parent, so a `cover.jpg` in a folder that tracks of another album (or tracks with no album) see through either level would become *their* cover too. `plan` records which albums see each folder and skips the target if any other album does. A target with an artwork-folder name (`is_artwork_dir_name`) is also seen from its parent, because the scan searches such subfolders too.
- **Never overwrite.** `check` refuses a folder with any `cover.*` image, or with an image `best_cover_name` would pick (the album has no cover although the scan should have found one: something is off, leave it alone). Back / CD / booklet scans alone don't block. It runs before the search (no requests for albums that can't be written) and again right before writing.
- **Write** goes to a hidden temp file (`create_new`), then `hard_link` to the final name, which fails instead of replacing; filesystems without hard links fall back to `rename` after an existence check.
- A read-only folder shows up as `PermissionDenied` / `ReadOnlyFilesystem` on the temp file and becomes the "read-only" reason.

## Flow

- **Find** runs `plan` + `check` on a background task, then searches album by album, each search its own `net_runtime::unblock`: the `Finder` is moved in and back out because it carries the rate-limit clocks. **Stop** takes effect between albums. The state lives in `ToolsView` and survives leaving the screen.
- Exact hits start checked, uncertain ones unchecked with a note. Checkboxes work while searching (rows are only appended then) and are disabled while saving, which removes rows and would shift the indices they were rendered with. Save is available only when idle.
- **Save** downloads the full-size image for each checked row, validates and writes it. Written rows leave the list, failed ones move to the skipped list with the reason. If anything was written, one ordinary (debounced, neither forced nor manual) `request_rescan` of the music folders follows.
- The skipped text is appended line by line during a phase and rebuilt in full only by `relabel`. Thumbnails are `RenderImage`s released with `drop_atlas_tile` when a row goes away or a new search starts.

## Decisions

- No up-front write probe for read-only folders: it would create and delete files in every music folder just for looking.

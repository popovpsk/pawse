# track_list

The shared pieces of a track row: the row data base, formatting, the per-row and
album-header action buttons, the click that starts playback, and the current-row style.
Used by the `library_views` track pages and by `queue_view.rs`; one namespace,
`crate::track_list::`, re-exported from `mod.rs`.

Not here: the columns and layout of each page (`library_views/doc.md`); the queue, tag
editor, playlist popup, `cache_fill.rs` and `album_export.rs` (`pawse/src/doc.md`).

## Files

- `mod.rs` — `TrackRowBase` (views embed it and add only the fields they render), duration /
  track-number formatting, `RowButtonColors`, and the private `row_icon_button` every button
  is built on.
- `like_button.rs` — the heart; `LIKE_ROW_GROUP`.
- `queue_button.rs` — add a track / an album to the queue, `play_replacing_queue`.
- `playlist_buttons.rs` — add to / remove from a playlist.
- `edit_tags_button.rs` — the row pencil and the album-header pencil.
- `cache_button.rs` — the album / artist header buttons for saving to the cache and moving
  an album to local folders.
- `row_style.rs` — `current_row`.

## Rules

- Everything per row is built when the data changes. Buttons take a `&RowButtonColors`
  resolved once per render, and callers read flags such as `tag_editor_enabled` once per
  frame, so the `v_virtual_list` closure does no per-row theme or settings reads.
- Row buttons are built `opacity(0)` and show on `group_hover(LIKE_ROW_GROUP)`: a row must call
  `.group(LIKE_ROW_GROUP)` or they stay hidden. A liked heart is always visible; the
  album-header buttons never hide.
- `play_replacing_queue` is every row's click; with a custom queue it asks first (add the
  clicked track, cancel, replace).
- `remove_from_playlist_button`: if the queue is backed by that playlist and the removed track
  is playing, it moves one position forward (or pauses at the end) before the DB change. Not
  `next_track()`: under repeat-one that stays on the removed track. Saving is left to
  `sync_queue_with_playlist` after `PlaylistTracksChanged`; saving earlier would persist a
  queue that still holds the removed track.
- Pages show the row pencil only for rows with `TrackRowBase::local`. The album pencil writes
  the album-shared fields to every file of the album.
- A cache button shows a static stop icon while its job runs (an animation would repaint the
  window every frame) and saved / total bytes in the tooltip (bytes, not files: a cue image
  is a whole disc). The owning view decides visibility: "something is missing" is computed on
  load and when `CacheFill`'s revision changes, never in render, with the stat-only
  `RemoteMedia::is_cached` so opening a page does not touch cache mtimes. LRU eviction is not
  noticed until the view is opened again.
- `current_row` assumes rows with `pl_4` and a 1 px bottom border: the left accent border
  replaces 1 px of left padding and the bottom border, so content stays aligned.

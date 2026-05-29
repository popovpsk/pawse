# track_list

Shared building blocks for every track-list row — the album/artist/liked/playlist
views in `library_views` and the `queue_view`. One namespace (`crate::track_list::`)
so call sites have a single import; `mod.rs` re-exports the submodule items.

## Files

- `mod.rs` — the row data base and formatting helpers, plus submodule declarations
  and re-exports:
  - `TrackRowBase { id, title, duration, liked }` + `from_track` — the fields every
    row needs. Views embed it by composition (`base: TrackRowBase`) and add only the
    extra fields they use, so a view never allocates a field it doesn't render.
  - `fmt_duration` / `fmt_track_num` — `mm:ss` and `N.` as `SharedString`.
  - `track_duration` — the fixed-width duration cell.
- `like_button.rs` — `like_button` (heart toggle → `LibraryService::set_liked`) and
  `LIKE_ROW_GROUP`, the hover-group name rows apply so the heart/queue/playlist
  buttons fade in on row hover.
- `queue_button.rs` — `add_to_queue_button` (append one track) and
  `add_album_to_queue_button` (append a whole album); both emit `QueueChanged`.
- `playlist_buttons.rs` — `add_to_playlist_button` (opens the global playlist popup)
  and `remove_from_playlist_button`.
- `row_style.rs` — `current_row`, the styling applied to the currently-playing row.

## Conventions & non-obvious behavior

- **Why precompute**: the `from_track` constructors run when the queue/list changes,
  not per frame. The `v_virtual_list` render closure runs at render rate (~120fps)
  for every visible row, so it must not format strings, join artists, or hit the
  cover cache — all of that is baked into `TrackRowBase` / the view's row struct up
  front.
- **Group reveal**: action buttons start at `opacity(0.)` and rely on the row setting
  `.group(LIKE_ROW_GROUP)` plus `.group_hover(LIKE_ROW_GROUP, …)`. A row that forgets
  the group will render its action buttons permanently hidden.
- `LIKE_BUTTON_SIZE` is internal to `like_button.rs`; `LIKE_ROW_GROUP` is the only
  cross-module constant and is reached from sibling submodules via `super::like_button`.

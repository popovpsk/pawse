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
  - `RowButtonColors { icon_hover, icon, accent }` + `from_cx` — theme colors the
    per-row buttons need. Resolve once per render (views stash it in their row
    `*Params`) and pass `&RowButtonColors` into the button builders, so they don't
    re-read the theme for every visible row each frame.
- `like_button.rs` — `like_button(track_id, liked, &RowButtonColors)` (heart toggle →
  `LibraryService::set_liked`) and `LIKE_ROW_GROUP`, the hover-group name rows apply
  so the heart/queue/playlist buttons fade in on row hover.
- `queue_button.rs` — `add_to_queue_button(Rc<Track>, …, &RowButtonColors)` (append one
  track; takes an `Rc` so the row clone is a refcount bump) and
  `add_album_to_queue_button` (append a whole album); both emit `QueueChanged`. Also
  `play_replacing_queue(tracks, index, source, window, cx)` — the shared click handler
  for every track-list row: replaces the queue and plays, but when the queue is custom
  (`PlaybackQueue::is_custom`) it first opens a three-button confirm dialog
  (add the clicked track to the queue / cancel / replace). The dialog builder runs per
  frame, so the track list is captured behind an `Rc`.
- `playlist_buttons.rs` — `add_to_playlist_button` (opens the global playlist popup)
  and `remove_from_playlist_button`.
- `row_style.rs` — `current_row`, the styling applied to the currently-playing row.

## Conventions & non-obvious behavior

- **Why precompute**: the `from_track` constructors run when the queue/list changes,
  not per frame. The `v_virtual_list` render closure runs at render rate (~120fps)
  for every visible row, so it must not format strings, join artists, hit the cover
  cache, or re-read theme colors — all of that is baked into `TrackRowBase` /
  `RowButtonColors` / the view's row struct up front.
- **Rc tracks**: the views keep `tracks_all: Vec<Rc<Track>>` and the queue stores
  `Vec<Rc<Track>>` too, so the per-row "add to queue" clone and the whole-list clone
  on click are pointer/refcount copies, not deep `Track` clones. Persistence still
  needs owned `Track`, so `Services::snapshot_playback` deep-clones out of the `Rc`s
  on save (rare).
- **Group reveal**: action buttons start at `opacity(0.)` and rely on the row setting
  `.group(LIKE_ROW_GROUP)` plus `.group_hover(LIKE_ROW_GROUP, …)`. A row that forgets
  the group will render its action buttons permanently hidden.
- `LIKE_BUTTON_SIZE` is internal to `like_button.rs`; `LIKE_ROW_GROUP` is the only
  cross-module constant and is reached from sibling submodules via `super::like_button`.

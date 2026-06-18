# library_views

The library browsing UI: the root tab container and every screen reachable from
it (albums, artists, liked, playlists) plus the drill-down track lists. All views
are GPUI entities that read from `LibraryService`, subscribe to `LibraryEventsBus`
(scan/like/playlist changes) and `EngineEventsBus` (current track / playing), and
drive the `PlaybackQueue` on click.

## Files

- `mod.rs` — module declarations only.
- `library_view.rs` — root container. Holds the four root-tab views as long-lived
  entities and the drill-down views (`tracks`/`artist_tracks`/`playlist_tracks`)
  as `Option`s created on navigation. Owns `LibraryRootTab` / `LibraryViewState`,
  routes the header search query to the active child, and re-emits navigation events.
- `albums_view.rs` — Albums tab: virtualized vertical list of albums.
- `artists_view.rs` — Artists tab: virtualized list of artists.
- `tracks_view.rs` — tracks of one album (drill-down). Multi-disc aware.
- `artist_tracks_view.rs` — all tracks of one artist, grouped by album.
- `liked_view.rs` — the liked-tracks screen. Rows are drag-reorderable (only with
  an empty filter) via `LibraryService::move_liked_track`.
- `playlists_view.rs` — list of playlists (create / delete / rename, fuzzy filter).
- `playlist_tracks_view.rs` — tracks of one playlist. Rows are drag-reorderable
  (only with an empty filter), persisted via `LibraryService::move_track_in_playlist`.
- `album_info.rs` — the album header element (cover + title/artist/year + genres +
  add-album button) rendered as the first row inside `tracks_view`. Album genres are
  aggregated from the album's tracks (most-common first), capped at 3 inline with a
  trailing `…` and the full set on hover when there are more.

## Conventions & non-obvious behavior

- **Row model**: track-list views keep `tracks_all: Vec<Rc<Track>>` (the full
  unfiltered source) and a derived `Vec<TrackRow>` (`row_data`) of *precomputed*
  render data — formatted strings, cover `Arc<Image>`, liked flag. `TrackRow` embeds
  the shared `TrackRowBase` from `crate::track_list`; building it once keeps the
  `v_virtual_list` render closures allocation-free (see `track_list/doc.md`). The
  `Rc` lets the per-row "add to queue" clone and the on-click whole-list hand-off to
  the queue be refcount bumps rather than deep `Track` clones.
- **Filtering**: search keeps only `(index, score)` pairs (never clones the `Track`),
  sorts, then rebuilds `row_data` from `&tracks_all[ix]`; `tracks_all` is never
  reordered. Each `TrackRow` stores `track_all_ix` so a click maps back to the
  unfiltered index — clicking a track replaces the queue with the *whole* source
  list (not the filtered subset) starting at that index.
- **Like updates** arrive as `LibraryEvent::TrackLikedChanged` and are applied by
  mutating the matching `TrackRow` in place (no full rebuild); the `tracks_all`
  entry is updated via `Rc::make_mut` (copy-on-write only if shared). `liked_view`
  instead re-fetches, since unliking removes the row.
- **Liked ordering**: likes are backed by a hidden playlist in `music_library`, so
  the liked set has a persisted manual order (newest like appended last). The
  `tracks.liked` boolean stays the source of truth for the heart icon; the hidden
  playlist only carries order and is filtered out of `playlists()` /
  `playlists_containing_track`. `liked_view` reorder calls `move_liked_track` then
  reloads itself (no event round-trip, and the queue is never backed by liked).
- **Item sizing**: virtual lists use an `items` enum (`TopPadding` / `AlbumInfo` /
  `DiscHeader` / `Track`) with a parallel `item_sizes` vec; heights are fixed
  constants, width is `px(0.)` (unused by the vertical list — kept zero on purpose).
- Shared row controls (like / queue / playlist buttons, `current_row` styling) live
  in `crate::track_list`, not here.

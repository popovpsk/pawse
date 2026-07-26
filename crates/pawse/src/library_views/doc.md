# library_views

The library browsing UI: the root tab container and every screen reachable from
it (albums, artists, liked, playlists) plus the drill-down track lists. All views
are GPUI entities that read from `LibraryService`, subscribe to `LibraryEventsBus`
(scan/like/playlist changes) and `EngineEventsBus` (current track / playing), and
drive the `PlaybackQueue` on click.

## Files

- `mod.rs` — module declarations only.
- `library_view.rs` — root container. Holds the four root-tab views as long-lived
  entities. Navigation is a back-stack `Vec<NavEntry>` (not a flat state machine):
  `stack[0]` is always the current `Root(LibraryRootTab)`; drill-downs
  (`AlbumTracks`/`ArtistTracks`/`PlaylistTracks`) push a frame on top that *owns* the
  live drill view, and the album/artist cross-nav `Subscription` lives in the frame
  (so it dies with its view). `go_back` pops one frame; picking a tab resets the
  stack to `[Root(tab)]`; jumps from footer/now-playing/cover-mode push frames that
  unwind on back. Only `stack.last()` renders and receives the header search query —
  buried frames stay live (their like/track-change subscriptions keep them current)
  but unmounted, so they cost nothing per frame. `is_drilled_in() = stack.len() > 1`;
  `current_tab()` is `None` while drilled in (`MainView` keeps the prior tab lit).
  Disabling Liked/Playlists in settings purges those frames, resetting to
  `[Root(Albums)]` if that breaks the `stack[0]`-is-`Root` invariant.
- `albums_view.rs` — Albums tab: virtualized vertical list of albums. Genre and year
  are fixed-width trailing columns (reserve their slot even when empty so rows don't
  flex), each toggleable in Settings → Interface → Albums view (`albums_show_year` /
  `albums_show_genre`, default on; the view observes `SettingsStore` so a toggle
  re-renders); row order is SQL-side (`artist, year, title`), not derived from the
  text — independent of how the artist is shown. The artist has a tri-state display
  (`albums_artist_display`: `Inline` "artist - title" in the title cell, default;
  `Column` a separate fixed-width column left of year; `Hidden` title only).
  Genre shows the most-common one + `…` when there are more, full list on hover.
  Album genres are batch-fetched once (`album_genres_map`) and cached, not queried
  per row — `recompute_visible` runs on every keystroke.
- `artists_view.rs` — Artists tab: virtualized list of artists.
- `tracks_view.rs` — tracks of one album (drill-down). Multi-disc aware.
- `artist_tracks_view.rs` — all tracks of one artist, grouped by album. An album
  the artist only partly appears on (their track count < the album's total) is
  "partial"; its queue button offers artist-tracks-only vs. the full album. When the
  artist has any partial album, the header shows a "Full albums" toggle (top-right):
  flipping it on re-fetches the source so partial albums expand to every track
  (`tracks_for_album`) — `tracks_all` is the playback/queue source, so it stays in
  sync — and suppresses the per-album queue menu (the displayed album is already full).
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
- **Tag editor**: the per-row pencil is wired only into `tracks_view` and
  `artist_tracks_view` (the album and artist screens) — deliberately *not* into
  `liked_view`, `playlist_tracks_view` or the queue, which are playback-ordering
  screens. `album_info` carries the album-level pencil next to the add-album-to-queue
  button. All of them are gated on `tag_editor_enabled`, read once per render into the
  row `*Params` struct alongside `liked_enabled` / `playlists_enabled`. No
  `observe_global::<SettingsStore>` is needed here: `MainView` already observes it and
  re-rendering the parent re-renders these entities. Album title, album artist and
  **year** are locked in the per-track modal: `albums` is keyed on `(title, year)`
  (`ScanSession::resolve_album`), so editing any of the three from a track that has
  siblings re-keys the row and splits the album in two. They unlock only in the album
  editor, or for a track that belongs to no album at all — nothing shared to break,
  and no album editor it could be reached from.
- **An album's cover is chosen deterministically, and the choice is re-made after
  every write.** A cover is derived, never typed: the scanner takes the embedded
  picture, or an image file found next to the track, and hashes it into `cover_art`
  with the id hung off the track. The *album*'s cover is then resolved by
  `LibraryRepository::resolve_album_covers` — the cover of its lowest
  `(disc, track, path)` track that has one, and `NULL` when none does. It used to be
  "whichever track the scan finished first", which is not a defined order in a parallel
  pipeline: harmless while every track of an album shares its art, but once the tag
  editor can set art per track, that album's cover would change on every rescan.
  `settle_derived_rows` is the single place both the scan and the point-update paths
  call, so they cannot drift; `an_album_edit_lands_exactly_where_a_full_rescan_would`
  fails if either one skips it. This is also what keeps a *renamed* album's cover: the
  rename lands its tracks on a new `albums` row born with `cover_art_id NULL`, and the
  re-resolve fills it from the tracks that moved.
- **The cover row shows the file's own picture, never the library's cover.** This is
  the one place the two must not be conflated: `tracks.cover_art_id` may have been
  derived from a `cover.jpg` next to the file, and a *tag* editor showing that would
  claim a tag the file does not have — Remove would then look like it did something and
  change nothing. So the preview comes from the embedded picture only
  (`read_metadata`'s `CoverArt::Bytes { embedded: true }`, or
  `extract_embedded_cover` for a cue track), the row is empty when there is none, and
  the Remove button is hidden in that state. Setting one is still offered, and the tag
  then outranks the folder image by the reader's own precedence —
  `a_cover_set_in_the_tag_wins_over_the_image_beside_the_file` pins that, and
  `an_external_cover_is_reported_as_art_but_not_as_an_embedded_picture` pins the
  distinction it rests on.
- **Cover editing is offered in every tag modal**, track and album alike, unlike
  album/album-artist/year. It is safe because the album's cover is derived from its
  tracks rather than stored per-album: setting art on one track cannot re-key anything.
  There is no album-level cover *tag* — the album editor simply writes the same file tag
  into all of the album's files, exactly as it already does for album/year/genre, and
  says so under the row. Costs to know: the image is embedded verbatim into each file
  (no resizing — deliberate, so the modal shows the file size instead), and
  `reindex_one` must set `tracks.cover_art_id` explicitly because `upsert_track`
  `COALESCE`s that column and can only ever keep the old value.
- **The point-update path is checked against a real rescan, not field by field.**
  `library_service`'s tests index a temp folder with the actual pipeline
  (`music_indexer::run` + `open_scan_session`, no GPUI), apply a tag edit through the
  same functions the spawned task calls (`apply_track_tags` / `apply_album_tags`),
  then scan again and compare a snapshot of the whole library. The point update is
  only an optimisation over that rescan, so anything it forgets shows up as a diff —
  including columns nobody thought to assert, which is how the missing cover would
  have been caught.

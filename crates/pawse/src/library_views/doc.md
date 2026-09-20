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
  unwind on back. `navigate_to_artist` (those jumps, plus the album header's artist
  link) resolves the id with `artist_summary` in the configured grouping and falls
  back to `TrackArtist` — a featured performer with no album of their own is not in
  the album-artist list, and the click must still land somewhere. Only `stack.last()` renders and receives the header search query —
  buried frames stay live (their like/track-change subscriptions keep them current)
  but unmounted, so they cost nothing per frame. `is_drilled_in() = stack.len() > 1`;
  `current_tab()` is `None` while drilled in (`MainView` keeps the prior tab lit).
  Disabling Liked/Playlists in settings purges those frames, resetting to
  `[Root(Albums)]` if that breaks the `stack[0]`-is-`Root` invariant.
- `albums_view.rs` — Albums tab. One entity, two layouts (`albums_layout`, Settings →
  Interface → Albums view): `List` renders here, `Grid` (default) delegates to
  `albums_grid.rs`. Data, filter, subscriptions and `row_data` are shared; only the
  render branches. Row order is SQL-side (`artist, year, title`), not derived from the
  text — independent of how the artist is shown.
  **List**: virtualized vertical list of 48 px rows with a 32 px cover. Genre and year
  are fixed-width trailing columns (reserve their slot even when empty so rows don't
  flex), each toggleable in Settings (`albums_show_year` / `albums_show_genre`, default
  on). The artist has a tri-state display (`albums_artist_display`: `Inline`
  "artist - title" in the title cell, default; `Column` a separate fixed-width column
  left of year; `Hidden` title only). Genre shows the most-common one + `…` when there
  are more, full list on hover. Album genres are batch-fetched once
  (`album_genres_map`) and cached, not queried per row — `recompute_visible` runs on
  every keystroke.
  **The layout picks the cover size**, so it is a data change, not just a repaint:
  `AlbumRowData::from_album` takes the `AlbumsLayout` and loads `get_small` (128 px) for
  the list, `get_large` (320 px) for the grid. The `observe_global::<SettingsStore>`
  handler therefore compares before acting — it fires on *any* settings write, and an
  unconditional `recompute_visible` would re-read every cover blob on each one. A layout
  switch rebuilds `row_data` and scrolls back to the top (the old scroll offset means
  nothing in the other geometry).
  `AlbumRowData` precomputes *both* subtitle forms (`artist`, and `subtitle_year` =
  "artist · year"); render only picks one. Formatting in the virtual-list closure is
  banned — see `track_list/doc.md`.
- `albums_grid.rs` — the `Grid` layout: a tile wall of album covers, the alternative
  most popular players offer. Virtualized on the same `v_virtual_list`, where each item
  is one `h_flex` strip of N tiles (the pattern gpui-component's own `virtual_list`
  story uses); item 0 stays the shared top spacer, so `set_filter`'s
  `scroll_to_item(0)` needs no special case. `items` is left empty in this mode — the
  strip's slice is arithmetic on `columns`, so `AlbumItem` needs no grid variant.
  **Geometry.** `grid_metrics(width)` is a pure function (unit-tested, no GPUI): columns
  = how many `TILE_MIN_WIDTH` tiles fit, then the tiles *stretch* to divide the width
  exactly, so there is no ragged right edge. Because the tile is square, its width sets
  the row height — `item_sizes` is rebuilt whenever the measured width changes, not just
  when the column count does. Below one minimum-width tile the single column *shrinks*
  past `TILE_MIN_WIDTH` instead of clamping to it: the panel can be squeezed under 182 px
  (window minimum 900 px against queue and lyrics at up to 560 px each), and a tile held
  at 150 px there would overflow the content box and be silently clipped by the list's
  `overflow_x_hidden`, with a row height computed for a width the tile never got.
  **Caption geometry is derived, not a constant.** The virtual list needs the row height
  up front, so the caption boxes carry a fixed `h(..)` — and a fixed box is only safe if
  the line inside it is fixed too. gpui's default `line_height` is `phi()`, 1.618 × font
  size (`style.rs`), so a 20 px box around `text_sm` clips at *every* font scale and loses
  11 px at `FontScale::Large`. `caption_heights(rem)` therefore sizes both boxes from the
  rem, and the tile sets `line_height` to that same number, so box and line are one value
  by construction and gpui's default stops mattering. The rem comes from
  `AlbumsView::rem_size` (`FontScale::px()`, what `Root::render` feeds the window), kept
  current by the settings observer — a font-scale change rebuilds `item_sizes` exactly
  like a width change. The coupling that remains is `TEXT_SM_REMS` / `TEXT_XS_REMS`
  mirroring what `text_sm()` / `text_xs()` mean in gpui.
  **Measuring.** The albums panel is narrower than the window (queue and lyrics panels
  resize beside it), so `window.viewport_size()` is wrong here. The width comes from an
  absolute `canvas` overlay in the grid container that compares `bounds.size.width`
  against `measured_width` and, on a change, calls `set_grid_width` + schedules a
  `cx.notify()` for the next frame — the same shape `cover_mode_view` uses. Until the
  first measurement the container renders alone (one frame), which avoids laying the
  grid out at a made-up width.
  **Everything geometric is snapshotted into `TileParams`, `columns` included.** The two
  halves of a frame do not see the same state: `render_grid` runs at render time, while
  the `v_virtual_list` item closure runs in *prepaint* (`virtual_list.rs`), after the
  measuring canvas — the container's first child — has already had its prepaint callback
  run `set_grid_width`. Reading `view.columns` live inside `grid_row` therefore paired a
  freshly updated column count with the tile width, row height and `item_sizes` captured
  a moment earlier, on every frame where the width moved — i.e. for the whole duration of
  a splitter drag or the queue/lyrics slide, not just once. Snapshotting `columns`
  alongside the rest makes each frame internally consistent; the frame after the
  `on_next_frame` notify is the one that shows the new geometry.
  **Settings.** The three list options are about *columns*, so `albums_view_group` only
  offers them when the layout is `List`: `albums_artist_display` (Inline/Column/Hidden is
  meaningless on a tile — the artist is simply the caption's second line) and
  `albums_show_genre` (no room on a tile) are hidden in `Grid`, and the grid ignores both.
  `albums_show_year` survives into `Grid` and appends `· year` to the caption, but under a
  wording that fits — `album_year` / `album_year_desc` instead of `year_column*`. Because
  the group's *items* depend on the layout, `build_settings_pages` takes the current
  `AlbumsLayout`; `MainView` already rebuilds the pages from `observe_global::<SettingsStore>`,
  so flipping the layout re-renders the group with the right rows.
  **Covers are bounded and lazy, because `Grid` is the default layout.** `img(Arc<Image>)`
  lets gpui own the decode: it keeps the `RenderImage` in `App.loading_assets` and its
  atlas tile forever, and neither is reachable for release. A wall of 320 px tiles made
  that unaffordable — roughly 1.5 GB for a fully browsed 2000-album library — and eagerly
  building `row_data` in grid mode read every album's blob on the main thread before the
  window appeared. So `CoverArtCache.large` is an LRU of *decoded* `Arc<RenderImage>`
  that hands each eviction to `drop_atlas_tile`, and the
  grid never fills `AlbumRowData::cover` at all: it keeps `cover_art_id`, reads the cache
  at render time through `peek_large`, and `ensure_grid_covers` loads what the visible
  range is missing — one row of margin either side — on the background executor, decoding
  there too, then inserting and notifying. `covers_in_flight` keeps a scroll from queueing
  the same id twice.
  **The capacity follows the viewport, it is not a constant.** A number big enough for an
  unscaled 4K wall (~220 tiles on screen) never evicts anything on a laptop, where about
  24 fit — and on a library smaller than the constant the bound is pure decoration.
  `capacity_for_visible` instead takes the span `ensure_grid_covers` was asked for, which
  *is* the visible tile count plus the margin, and keeps `LARGE_COVER_SCREENS` of them,
  clamped to `LARGE_COVER_MIN_CAPACITY..=LARGE_COVER_MAX_CAPACITY`. The floor is what
  cover mode and `album_info` live on when the grid is small or the layout is `List`. The
  invariant that matters is capacity > visible: below that the cache evicts what is on
  screen and thrashes reload → decode → evict, which is why it has its own test.
  **Capacity only ever grows** (`capacity_for_peak_visible` against
  `AlbumsView::visible_span_peak`), because the span the closure is handed is not always
  the viewport. `VirtualList::measure_item` calls the same closure with `0..1` every frame
  to size an item, and taking that literally dropped the capacity to the floor, evicted
  everything above it, and made the grid flicker between cover and placeholder once per
  frame — with only the most-recently-used entries surviving, which is exactly what it
  looked like. A high-water mark ignores the measuring pass by construction. It is never
  reset: re-learning on every width change would put the same thrash inside a splitter
  drag, and the cost of holding the largest viewport the session ever had is bounded by
  `LARGE_COVER_MAX_CAPACITY` anyway.
  The same measuring pass is also why the *load* is gated on `visible_range.end > 1`: with
  `0..1` the row arithmetic resolves to albums `0..columns` whatever the scroll offset, so
  the first row of the library would be re-requested every frame — and once a long scroll
  pushed it out of the LRU, each request meant another blob read, decode and `cx.notify()`
  for tiles nowhere near the viewport. A range that short carries no tile rows at all
  (item 0 is the spacer), so skipping it loses nothing.
  A cover whose blob is missing or fails to decode goes into `covers_unavailable`.
  Without it the id is in neither the cache nor `covers_in_flight`, so every frame would
  queue another background load for a cover that will never arrive. It is cleared on tag
  changes as well as rescans, since re-tagging is how a missing cover gets filled in.
  `insert_large` keeps an entry that is already there rather than replacing it: the only
  way to insert twice is a race between the grid's background load and a synchronous
  `get_large` from `album_info` or cover mode, and the loser's copy would otherwise have
  its atlas tile dropped while the winner is still painting it.
  **Eviction skips covers another view still holds** (`Arc::strong_count == 1` is the test
  for "only the cache has this"), which is what makes one cache safe to share: cover mode
  holds `large_cover` for a whole track while the grid scrolls past hundreds of tiles.
  Recency covers the other case — anything on screen is touched every frame, so the
  visible set is never the coldest. If every entry is held, nothing is evicted and the
  cache runs over capacity until someone lets go, which is the right failure direction.
  Sharing also means the common path is a hit: clicking a tile opens `album_info`, whose
  `get_large` finds the cover the grid just decoded.
  `small` stays an unbounded `HashMap<i64, Arc<Image>>` — a 128 px cover is ~6× cheaper,
  every list view uses it, and `cover_backdrop::from_thumbnail` needs the undecoded bytes.
  Note `decode_cover_tile` swaps R and B: gpui's `RenderImage` is BGRA, `to_rgba8` is not.
- `artists_view.rs` — Artists tab: virtualized list of artists. Which relation the
  list is built on is a setting (`artists_grouping`, Settings → Interface → Artists
  view): `AlbumArtist` (default) attributes each track to its own album-artist tag;
  a track without one follows its album's artist when that is *known*
  (`albums.artist_known`, see the derived-artists note below), and only on a true
  compilation falls back to its own track artists — so an untagged compilation
  still shows each performer as a partial album; `TrackArtist` lists everyone
  credited on a track. In `AlbumArtist` mode the *list* is who heads an album, but
  a listed artist's page, count and avatar covers take the union with their
  `track_artists` credits (the `m` / `u` CTE pair in `membership_ctes`, where `u`
  selects from `m` rather than repeating it, so the union is evaluated once) — so a
  "Various Artists" soundtrack still shows up (partial, with the Full-albums toggle)
  on the page of a performer who has an album of their own, while a performer credited only on compilations stays out of
  the list. The tab's filter matches each listed artist's name *plus* the names of
  everyone credited on the tracks of their page (`artist_search_haystacks`, fetched
  with the list and refreshed with it) — so "mick gordon" also surfaces "Various
  Artists", and "xzibit" surfaces Limp Bizkit, without either guest becoming a row.
  That expansion is `AlbumArtist`-only: in `TrackArtist` mode every performer is a row
  of their own, so there is nobody to reach through somebody else's name and the
  haystack is just the name. The buttons are labelled with the raw tag names
  (`artist` / `album artist`) on purpose, untranslated. The view observes
  `SettingsStore` and, when the grouping changes, re-fetches `artists` /
  `artist_album_covers` (a data change, not just a repaint — unlike `albums_view`).
  It re-fetches on `TrackTagsChanged` / `AlbumTagsChanged` too: a tag edit re-derives
  every album's artist, so rows and counts here move with no scan.
- `tracks_view.rs` — tracks of one album (drill-down). Multi-disc aware.
- `artist_tracks_view.rs` — all tracks of one artist, grouped by album. It is
  constructed with the `ArtistGrouping` it should use and keeps it for its lifetime
  (`rebuild_source` re-queries with the same one); flipping the setting does not
  rebuild an already open page, the list behind it reloads instead. An album
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
- **Like updates** arrive as `LibraryEvent::TrackLikedChanged`, or as one
  `LibraryEvent::LikesImported` carrying a whole batch (the Last.fm loved-tracks
  import). `LibraryEvent::liked_update` normalizes both into an id set, which is
  applied by mutating the matching `TrackRow`s in place (no full rebuild); the
  `tracks_all` entries are updated via `Rc::make_mut` (copy-on-write only if
  shared). `liked_view` instead re-fetches, since unliking removes the row and an
  import adds many.
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
- **An album's artists are derived, like its cover.** The scan and `reindex_one`
  only write each track's own album-artist tag (`track_album_artists`);
  `resolve_album_artists` in `settle_derived_rows` then runs
  `music_library::album_artists::derive_album_artists` over the album's tracks in
  `(disc, track, path)` order: the first track carrying a tag names the album; with
  no tag anywhere, an artist that appears *standalone* on some track and is, on every
  other track, followed by a *join marker* — punctuation (`/ & , ; + ( [ -`) or a
  guest word (feat/ft/with/vs/and/x) — names it: "Limp Bizkit" over "Limp Bizkit
  Feat. Xzibit", "Two Feathers" over "Two Feathers/Mikael Stanne". Matching is
  case-insensitive. A bare space is deliberately not a marker, so "Queen" never
  swallows "Queen Latifah" nor "Pink" "Pink Floyd", and a dash counts only when it is
  spaced off ("Band - Guest"), so "Jay-Z", "Wu-Tang Clan" and "T-Pain" stay whole
  names. Identical multi-value credits on every track are kept whole. Those cases set `albums.artist_known = 1`, and every untagged track
  of the album follows it in the album-artist grouping. Anything else (two standalone
  artists, no artists at all) leaves `artist_known = 0`: the album row still gets
  the first track's artists for the Albums tab, but tracks keep their own artists.
  `set_album_artists` sets the same flag, so the credited rows and the flag cannot
  disagree whichever of the two wrote them.
  The standalone requirement is what keeps "AC/DC" from being cut at the slash and
  "Sonic Youth"/"Sonic Boom" from collapsing into "Sonic". Order-independent, so a
  scan and a point update agree; the rescan-equivalence tests pin it through
  `snapshot()`, which includes the flag.
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

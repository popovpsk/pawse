# library_views

The library browsing UI: the root tab container and every screen reachable from
it (albums, artists, genres, liked, playlists) plus the drill-down track lists. All views
are GPUI entities that read from `LibraryService`, subscribe to `LibraryEventsBus`
(scan/like/playlist changes) and `EngineEventsBus` (current track / playing), and
drive the `PlaybackQueue` on click.

## Files

- `mod.rs` — module declarations only.
- `library_view.rs` — root container. Holds the root-tab views as long-lived
  entities — except Genres, which is optional (Settings → Appearance, off by
  default) and so is built on the first `select_tab(Genres)` and dropped when the
  setting is turned off: nobody who never opens it pays for its queries and cover
  loads at startup. Navigation is a back-stack `Vec<NavEntry>` (not a flat state machine):
  `stack[0]` is always the current `Root(LibraryRootTab)`; drill-downs
  (`AlbumTracks`/`ArtistTracks`/`GenreTracks`/`PlaylistTracks`) push a frame on top that *owns* the
  live drill view, and the album/artist cross-nav `Subscription`s live in the frame
  (so they die with their view). `ArtistTracks` and `GenreTracks` hold the same view
  type; they are separate variants only so the settings observer can tell genre
  frames apart. `go_back` pops one frame; picking a tab resets the
  stack to `[Root(tab)]`; jumps from footer/now-playing/cover-mode push frames that
  unwind on back. `navigate_to_artist` (those jumps, plus the album headers' artist
  links) goes through `resolve_artist_page` (pure, unit-tested) and opens the first
  page that exists, lazily: the clicked artist in the configured grouping; else the
  artist the *track* is listed under in that grouping, but only when the clicked
  credit is that artist plus a join marker (`listed_artist_for_credit`, the same
  `is_credit_of` rule album-artist derivation uses; only when the event carries
  `track_id`); else the clicked artist in the other grouping. Now-playing and cover
  mode send the id of the track whose artists they show (`artists_track_id`, set where
  the artist list is filled), not `PlaybackStatus`'s track: after a launch that
  restores an uncached server track the status is still empty while the queue's
  track is on screen. The second step is for a credit like "Samurai feat. Refused" on
  a track tagged album artist "Samurai": in the album-artist grouping the credit is
  not a listed artist, and landing on its one-track `TrackArtist` page felt broken,
  so the click goes to "Samurai", which holds that track. The join-marker condition
  is what keeps a compilation guest ("Martin Stig Andersen" on a "Various Artists"
  soundtrack) going to their own `TrackArtist` page as before instead of to "Various
  Artists". The other-grouping step covers both old fallbacks: a featured performer
  with no album of their own (album-artist grouping → `TrackArtist`), and an album
  artist nobody is credited as on a track ("Various Artists" on a soundtrack) while
  the grouping is `TrackArtist` — without it the genre page's album-artist link did
  nothing. Only `stack.last()` renders and receives the header search query —
  buried frames stay live (their like/track-change subscriptions keep them current)
  but unmounted, so they cost nothing per frame. `is_drilled_in() = stack.len() > 1`;
  `current_tab()` is `None` while drilled in (`MainView` keeps the prior tab lit).
  Disabling Liked/Playlists/Genres in settings purges those frames, resetting to
  `[Root(Albums)]` if that breaks the `stack[0]`-is-`Root` invariant.
- `albums_view.rs` — Albums tab. One entity, two layouts (`albums_layout`, chosen in
  the view menu, see `view_menu.rs`): `List` rows render here, `Grid` strips come from
  `albums_grid.rs`. Data, filter, subscriptions and `row_data` are shared, and both
  layouts are one `v_virtual_list` over `cover_grid::LibraryItem`s (top padding,
  section headers, list rows or grid strips). Row order is computed in Rust by
  `view_order::order_albums` from `AlbumKey`s built once per catalog load
  (`albums_sort` / `albums_sort_desc`), not taken from SQL; with `albums_grouped` the
  rows are cut into `view_order::sections`. While a search query is typed the order is
  the fuzzy score's and there are no sections — the menu's sort and grouping come back
  when the query is cleared.
  **List**: virtualized vertical list of 48 px rows with a 32 px cover. Genre and year
  are fixed-width trailing columns (reserve their slot even when empty so rows don't
  flex), each toggleable from the menu's Show chips (`albums_show_year` /
  `albums_show_genre`, default on). The artist is shown or not (`albums_show_artist`)
  and, when shown, placed by `albums_artist_display`: `Inline` "artist - title" in the
  title cell (default) or `Column`, a separate fixed-width column left of year. The
  enum still has a `Hidden` variant only so old `settings.json` files deserialize;
  `migrate_albums_artist` turns it into `Inline` on load, with `albums_show_artist`
  false only when the layout was `List` — the old `Hidden` never reached grid tiles, so
  a grid user keeps seeing the artist. The separate flag is what lets the menu remember
  the placement while the artist is hidden.
  Genre shows the most-common one + `…` when there are more, full list on hover. Album
  genres are batch-fetched once (`album_genres_map`) and cached, not queried per row —
  `recompute_visible` runs on every keystroke.
  **The layout picks the cover size**, so it is a data change, not just a repaint:
  `AlbumRowData::from_album` takes the `LibraryLayout` and loads `get_small` (128 px) for
  the list; for the grid it loads nothing — tiles read the 320 px LRU at render time (see
  `albums_grid.rs` below). The `observe_global::<SettingsStore>`
  handler therefore compares an `AlbumsPrefs` snapshot before acting — it fires on *any*
  settings write, and an unconditional `recompute_visible` would re-read every cover
  blob on each one. A layout, sort or grouping change rebuilds `row_data` and scrolls
  back to the top; a Show change only rebuilds the item sizes (the grid row loses its
  subtitle line when neither artist nor year is shown).
  `AlbumRowData` precomputes every subtitle form (`artist`, `year`, and `subtitle_year`
  = "artist · year"); render only picks one. A year ≤ 0 (a `0000` tag) counts as no year everywhere:
  `AlbumKey` files it under "No year" and `from_album` leaves the year empty, so a row
  never shows "0" inside the undated section. Formatting in the virtual-list closure is
  banned — see `track_list/doc.md`.
- `albums_grid.rs` — the album tile of the `Grid` layout (`grid_strip` / `grid_tile`,
  `TileParams`, `TileSubtitle`): a tile wall of album covers, the alternative most
  popular players offer. The geometry, the strip layout and the lazy cover loading it
  relies on live in `cover_grid.rs`, next to the section machinery both lists share;
  the notes below describe them here because the album wall is what they serve.
- `cover_grid.rs` — the item machinery behind the Albums tab (both layouts) and the
  Artists list: grid geometry, `LibraryItem` / `ItemLayout` (`list_layout`, `grid_layout`),
  `strip_span` / `cover_span`, `header_metrics`, `section_header`, the measuring
  `width_probe` and the `GridCovers` lazy loader.
  **Sticky section header = a docked slot, not an overlay.** The virtual list has no
  sticky support, so while sections exist (grouping on, no search query) the view puts
  a fixed `section_slot` (`TOP_PADDING` + header height, no background) *above* the
  list and the list scrolls only in the area below it. The first section's header is
  never a list item — it starts docked in the slot — and every later header is an
  inline item. `docked_labels` (pure, unit-tested) works out from the header offsets
  and the scroll offset (read in `render`) which label is docked and where an incoming
  one is: the slot draws the incoming header at exactly the position the inline one
  would have if the list did not clip it, so a header scrolling up crosses the
  list's top edge into the slot in one piece, pushes the docked label out, and settles
  in its place. The overlay this replaced floated over the list, so covers scrolled
  *under* the letter: with the blur backdrop a translucent band let them show through
  the label, and an opaque one was a flat plate on the blur — the user rejected both.
  With the slot nothing ever passes under the label, so it needs no background at all.
  The slot clamps the scroll offset itself, to the layout height (`ItemLayout.height`)
  minus the viewport: gpui's wheel handler adds the delta unclamped and notifies, and
  the clamp only happens later in the list's prepaint, so at the end of the list
  `render` saw an overscrolled offset and docked a header the list still showed
  inline. The slot forwards the mouse wheel to the list's scroll handle under the
  same clamp, so scrolling over the band still scrolls, as it did over the overlay;
  a mostly sideways trackpad swipe is ignored there, as the list's axis lock does.
  In a grouped list the row right before an inline header draws no bottom rule
  (`closes_section`, checked in the item closure against the next item): the header's
  own rule comes right after, and the two read as a double line.
  Virtualized on one `v_virtual_list`, where each grid item
  is one `h_flex` strip of N tiles (the pattern gpui-component's own `virtual_list`
  story uses). Item 0 is always the `TopPadding` spacer, zero-high while the slot is
  shown (the slot carries the top gap then): `VirtualList` measures by calling the item
  closure with `0..1` every frame, and if item 0 were the first strip that pass would
  keep requesting its covers whatever the scroll offset (see "the load only follows the
  strips" below). Going back to the top (a new query,
  sort, layout or grouping) is `cover_grid::scroll_to_top`, which sets the offset to zero
  at once rather than `scroll_to_item(0)`: that one is deferred to the list's prepaint,
  after `render` has already computed the docked label from the old offset against
  the new header positions, so for a frame (or until something else repainted) the
  slot showed a section from the middle of the new order over the top of the list. Strips are cut *inside* each section, so
  a section always starts on a fresh row; an ungrouped wall is one implicit section
  without a header.
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
  **Everything geometric is snapshotted at render time, the items included.** The two
  halves of a frame do not see the same state: `render` runs first, while the
  `v_virtual_list` item closure runs in *prepaint* (`virtual_list.rs`), after the
  measuring canvas — the container's first child — has already had its prepaint callback
  run `set_grid_width`, which rebuilds `layout_items`. Reading the column count (or now
  the strip list) live inside the closure therefore paired freshly cut strips with the
  tile width, row height and `item_sizes` captured a moment earlier, on every frame where
  the width moved — i.e. for the whole duration of a splitter drag or the queue/lyrics
  slide, not just once; with strips that is also an item count that no longer matches
  the sizes. So `ItemLayout.items` is an `Rc` like `sizes`, `render` clones both, and
  the closure only ever indexes that snapshot; the frame after the `on_next_frame` notify
  is the one that shows the new geometry. The closure does read `row_data` and
  `section_labels` live, which is safe only because `set_grid_width` — the one thing
  that runs between render and the closure — touches nothing but `layout_items`; never
  rebuild rows or labels from there.
  **Which options apply.** The column options make no sense on a tile, so the menu only
  offers them in `List`: the artist placement (the artist is simply the caption's second
  line) and `albums_show_genre` (no room on a tile); the grid ignores both.
  `albums_show_year` and `albums_show_artist` both shape the grid caption (`TileSubtitle`)
  and, when both are off, the caption loses its second line and the row height shrinks
  (`row_height(.., subtitle)`).
  **Covers are bounded and lazy, because `Grid` is the default layout.** `img(Arc<Image>)`
  lets gpui own the decode: it keeps the `RenderImage` in `App.loading_assets` and its
  atlas tile forever, and neither is reachable for release. A wall of 320 px tiles made
  that unaffordable — roughly 1.5 GB for a fully browsed 2000-album library — and eagerly
  building `row_data` in grid mode read every album's blob on the main thread before the
  window appeared. So `CoverArtCache.large` is an LRU of *decoded* `Arc<RenderImage>`
  that hands each eviction to `drop_atlas_tile`, and the
  grid never fills `AlbumRowData::cover` at all: it keeps `cover_art_id`, reads the cache
  at render time through `peek_large`, and `GridCovers::ensure` loads what the visible
  range is missing — one row of margin either side — on the background executor, decoding
  there too, then inserting and notifying. `in_flight` keeps a scroll from queueing
  the same id twice.
  **The capacity follows the viewport, it is not a constant.** A number big enough for an
  unscaled 4K wall (~220 tiles on screen) never evicts anything on a laptop, where about
  24 fit — and on a library smaller than the constant the bound is pure decoration.
  `capacity_for_visible` instead takes the span `GridCovers::ensure` was asked for, which
  *is* the visible tile count plus the margin, and keeps `LARGE_COVER_SCREENS` of them,
  clamped to `LARGE_COVER_MIN_CAPACITY..=LARGE_COVER_MAX_CAPACITY`. The floor is what
  cover mode and `album_info` live on when the grid is small or the layout is `List`. The
  invariant that matters is capacity > visible: below that the cache evicts what is on
  screen and thrashes reload → decode → evict, which is why it has its own test.
  **Capacity only ever grows** (`CoverArtCache::fit_large_capacity`, which keeps the
  high-water mark in the cache itself, next to the capacity it bounds), because the span
  the closure is handed is not always
  the viewport. `VirtualList::measure_item` calls the same closure with `0..1` every frame
  to size an item, and taking that literally dropped the capacity to the floor, evicted
  everything above it, and made the grid flicker between cover and placeholder once per
  frame — with only the most-recently-used entries surviving, which is exactly what it
  looked like. A high-water mark ignores the measuring pass by construction. It is never
  reset: re-learning on every width change would put the same thrash inside a splitter
  drag, and the cost of holding the largest viewport the session ever had is bounded by
  `LARGE_COVER_MAX_CAPACITY` anyway.
  The same measuring pass is also why the *load* only follows the strips in the range
  (`strip_span`): with `0..1` a row-arithmetic version resolved to albums `0..columns`
  whatever the scroll offset, so the first row of the library was re-requested every
  frame — and once a long scroll pushed it out of the LRU, each request meant another
  blob read, decode and `cx.notify()` for tiles nowhere near the viewport. Item 0 is the
  spacer, so that range carries no strip and loads nothing.
  A cover whose blob is missing or fails to decode goes into `GridCovers::unavailable`.
  Without it the id is in neither the cache nor `in_flight`, so every frame would
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
- `artists_view.rs` — Artists tab, a list only: a `Grid` of round avatars was built and
  removed at the user's request, so don't bring it back without asking. Same item machinery as the Albums list (`cover_grid::list_layout`, docked
  headers); the order comes from
  `view_order::order_artists` over `ArtistKey`s (`artists_sort`: Name, keyed on
  `ArtistSummary.sort_name`, or Track count, most first by default), and letter
  sections only exist for the Name sort. Which relation the
  list is built on is a setting (`artists_grouping`, Settings → Appearance → Artists
  view, deliberately not in the view menu): `AlbumArtist` (default) attributes each track to its own album-artist tag;
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
  It re-fetches on every `CatalogChanged`, which a tag edit sends too: a tag edit
  re-derives every album's artist, so rows and counts here move with no scan.
- `view_order.rs` — pure, unit-tested ordering and sectioning for the Albums and Artists
  tabs: `AlbumKey` / `ArtistKey`, `order_albums` / `order_artists`, `sections`,
  `SectionKey` and its label. Keys go through `music_library::compute_sort_name` (the
  rule behind `artists.sort_name`) and are lowercased, so "The Smile" sorts and groups
  under S and "A Perfect Circle" under P, for album titles too. A section letter is the
  first character when it is a *cased* letter (Latin, Cyrillic, Greek, …) whose single
  uppercase form lowercases back to it; everything else (including `ı` and `ß`, which
  would otherwise open a second "I"/"S" section after Z, since they sort there) — digits, symbols, uncased scripts such as CJK, Thai or Devanagari —
  is `#`. **`#`, undated albums and the "No metadata" rows always go last**, in both
  directions and whether or not grouping is on (the user's choice, Apple Music's
  convention): keeping one rule for grouped and ungrouped order means a name never
  moves just because headers were switched on, and a byte order would otherwise put
  digits first and CJK after Z, splitting `#` in two. Descending flips only the primary
  key: an artist's albums stay chronological under Z–A. Order is byte order of the
  lowercased key, not a collation, so accented capitals get their own section after Z
  (É, Å, Ö) — the same limitation the SQL `NOCASE` order had. Year sections are decades.
- `view_menu.rs` — the view menu: one ghost round button next to the header search
  (`icons/s1-view.svg`, two sliders — Lucide `settings-2` redrawn at the app's 1.7
  stroke). It is the same on both tabs and in both layouts: a layout glyph that
  followed Grid/List was tried and dropped, the button means "view options", not the
  current mode. It opens a `gpui_component::Popover` (`Anchor::TopLeft`, so it
  opens to the right over the queue instead of over the list it changes). `MainView`
  reserves a button-wide slot on *both* sides of the 200 px search box (fuzzy search
  needs two or three letters, a wider field bought nothing) and puts the
  button in the right one, so the search stays centred and the side groups give way in
  a narrow window instead of being overlapped. Which menu, if any, is
  `LibraryView::view_menu_tab` (top of the nav stack, re-read by `MainView` on every
  `StateChanged`): `Albums` and `Artists` on those root tabs, `Genre` on a genre page
  (sort only), `Tracks` on the Liked tab, a playlist and All tracks (columns and
  unavailable tracks, see `liked_view.rs` below); nothing on the album and artist pages,
  the Genres and Playlists lists, cover mode or Settings/Tools. Every control writes straight to `SettingsStore`
  (persisted like any other setting); the popover stays open and the tab views redraw
  behind it through their own settings observers. Clicking the active sort again flips
  its direction; picking another sort resets to that sort's default direction (Track
  count starts most-first). The grouping `Switch` is display-only (no handler, no tab
  stop) and the whole row owns the click: an enabled gpui-component `Switch` does not
  stop propagation, so a handler on both wrote the setting twice per click. Row labels
  are `min_w_0` and wrap, the switch and the sort hint never shrink: "group by decade"
  in Russian and Ukrainian is wider than the menu and used to push the switch past its
  edge. Each menu
  keeps its own settings; the Grid/List switch is Albums-only. These used to be the Settings → Appearance → Albums view group, which is
  gone; only `artists_grouping` (which tag defines an artist) stays in Settings, since
  it changes the data rather than how it looks. Strings live in their own table,
  `ui_resources::i18n::view_menu_strings`.
- `tracks_view.rs` — tracks of one album (drill-down). Multi-disc aware.
- `genres_view.rs` — Genres tab: virtualized list of every genre that still has a
  track (`LibraryService::genres`, ordered by `genres.key`, i.e. the Rust-lowercased
  name, which sorts non-ASCII correctly where SQLite's `NOCASE` would not), with the
  same row shape as the Artists tab: a collage of up to three album covers
  (`genre_album_covers` — the albums with the most tracks in that genre first, through
  the same `artist_avatar`), the name and the track count. Filter matches the name.
  Reloads on `CatalogChanged` (tag edits included). No "No genre" pseudo-row by
  decision: in a poorly tagged library it would be the biggest row and a copy of All
  tracks. When the library is empty it offers the library settings like the other
  tabs; when it has music but no genre tags it only says so — adding folders would not
  help.
- `grouped_tracks_view.rs` — tracks grouped by album, for one artist or one genre
  (`Scope::Artist { id, grouping }` / `Scope::Genre { key, sort }`). Everything
  except the source query, the header and the partial-album machinery is shared.
  **Artist page**: constructed with the `ArtistGrouping` it should use and keeps it for its lifetime
  (`rebuild_source` re-queries with the same one); flipping the setting does not
  rebuild an already open page, the list behind it reloads instead. An album
  the artist only partly appears on (their track count < the album's total) is
  "partial"; its queue button offers artist-tracks-only vs. the full album. When the
  artist has any partial album, the header shows a "Full albums" toggle (top-right):
  flipping it on re-fetches the source so partial albums expand to every track
  (`tracks_for_album`) — `tracks_all` is the playback/queue source, so it stays in
  sync — and suppresses the per-album queue menu (the displayed album is already full).
  The save-to-cache button is artist-only too.
  **Genre page**: holds the genre by `genres.key`, never by id — genre ids are not
  kept across scans (see `music_library/src/doc.md`, "Stable album and artist ids"),
  the key is. Its sort lives in the view menu (`ViewMenuTab::Genre`): Artist or Year,
  each with a direction — clicking the active one flips it, like the Albums tab — and
  is one global preference (`genres_sort` + `genres_sort_desc` in settings.json), so
  every genre opens with it. The page follows the setting itself
  (`observe_global::<SettingsStore>` → `follow_genre_sort`), re-queries and scrolls to
  the top. These used to be `[Artist | Year]` buttons in the page header, without a
  direction. The order is SQL-side (`tracks_by_genre`) and always keeps an
  album's tracks contiguous, because grouping is by consecutive `album_id` runs: both
  sorts key on the *album's* year and its position-0 album artist — never `t.year` or
  a track artist — so they ignore `artists_grouping`, exactly like the Albums tab
  (a per-track artist would scatter a compilation across the page). The direction
  flips only the primary key (`genre_track_order`): an artist's albums stay
  chronological under Z–A. Undated albums go after the dated ones — within each
  artist under Artist, at the very end under Year — and tracks with no album go last
  in every sort and direction. Album
  headers here add the album artist before the year ("Artist · 1970"), the name
  linking to that artist's page (`NavigateToArtistRequested`). No partial albums and
  no Full-albums toggle — expanding an album to its non-genre tracks would contradict
  the page — so the album queue button just appends the group. No save-to-cache either:
  `FillTarget` is keyed by stable i64 ids, and a whole-genre fill would mostly hit the
  "too big" dialog; a `FillTarget::Genre` keyed by the genre key is the follow-up if
  wanted. Search matches title, album and album artist (the artist page: title only),
  from haystacks built once per source load.
  **Shared**: album titles and artists come from an `AlbumMeta` map built once per
  source load from `albums()`, so regrouping on a filter keystroke does no database
  reads (it used to query `album_title` per group per keystroke). Each album header
  mirrors `album_info` at list scale (60 px cover, title over a muted subtitle); the
  year string and artist are precomputed in `AlbumGroup` so the virtual-list closure
  doesn't format.
- `liked_view.rs` — the liked-tracks screen. Rows are drag-reorderable (only with
  an empty filter) via `LibraryService::move_liked_track`.
  **View options (shared with `playlist_tracks_view`).** The `Tracks` view menu sets one
  preference for Liked, every playlist and All tracks (`TrackListPrefs` in
  `track_row.rs`: `playlists_show_artist` (default on), `playlists_show_album`,
  `playlists_show_year`, `playlists_show_unavailable` (default on)). Columns are
  `track_row::track_columns`, widths in rems so they follow the font scale. In a narrow
  window artist and album shrink, the year does not, and the title keeps at least
  `TITLE_MIN_WIDTH`: as a `flex_1` item it starts at zero width, so without a floor it
  vanished before the columns gave up a pixel. Album names come from
  `track_albums_map(track ids)` (`TrackNames`), a batch lookup shaped like
  `track_artists_map`, which also names *unavailable* entries from the item snapshot
  (`media_items.album`) — `album_id` is NULL for those. Not `albums()`: a reload runs
  on every reorder drop. The year is the track's own (≤ 0 shown empty). There is no
  sort: these are playback-ordering screens with drag reorder. Hiding unavailable
  tracks drops them from `row_data` (reorder still works — rows carry `track_all_ix`),
  the header label turns into "Hidden unavailable: N", and when nothing is left the
  label is the empty state instead of "no matches". The switch changes only the list:
  unavailable tracks never enter the queue either way (`PlaybackQueue` keeps only
  `playable` ones), so the click path is unchanged. Each view has its own settings
  observer; only a change of the unavailable switch rebuilds rows.
- `playlists_view.rs` — list of playlists (create / delete / rename, fuzzy filter).
  Creating is a list row, not a button: the first row ("New playlist") turns into an
  inline name input in place with ✓ / ✕ icon buttons (✓ stays dimmed until a name is
  typed). Enter creates, Esc or blurring it empty cancels; a typed name survives blur
  so clicking ✓ still works. With
  no playlists at all the row is replaced by a centered empty state (hint + primary
  button) until creation starts. "New playlist" and All tracks are the first items of
  the playlists' `v_virtual_list` (then a 12 px `Gap`), so they scroll away with the
  list; only when no playlist row is shown (empty state, no filter matches) are they
  plain rows above the body. Their slots are exactly the row height, so they don't
  shift when a filter flips between the two branches. Scrolling an open "New
  playlist" input out of view blurs it (an empty one cancels), same as renaming. Renaming reuses the pattern: the hover pencil next to
  the trash swaps that row for an inline input prefilled with the name (✓ / ✕,
  Enter / Esc). Blur cancels only when the text is empty or unchanged, so ✓ still
  works after typing; clicking another row while renaming just closes the editor.
  The rename goes through `LibraryService::rename_playlist` → `PlaylistsChanged`;
  the SQL refuses the hidden liked playlist.
  Above everything (the empty state too) sits a pinned strip of right-aligned ghost
  buttons, its own row so the "New playlist" row keeps one hover and one click:
  **Import** (only while a server whose `ServerKind::imports_playlists` is
  configured; `can_import` is refreshed from a `SettingsStore` observer, never
  computed in render) opens the import dialog, **AI** (icon only, the name lives in
  the tooltip) dispatches `tools::OpenAiPlaylist`, which `MainView` turns into the
  Tools screen on the AI prompt page in a playlist mode. AI is there even when the
  Tools button is hidden in Settings.
- `playlist_import.rs` — the server playlist import dialog. `PlaylistImport` is a
  long-lived entity owned by `PlaylistsView` and rendered as the dialog's body, so
  closing the dialog mid-import loses nothing: the busy set and the last result per
  server live there, filled from `LibraryEvent::RemotePlaylistsImported`. Opening it
  snapshots the importable servers (`refresh_servers`) and which of them are syncing
  (`LibraryService::remote_syncing`), kept current from `RemoteSyncStarted` /
  `RemoteSyncFinished` like the Library tab's rows. A syncing server's Import is
  disabled and its row shows "still syncing" instead of the last result: mid-sync
  (the first one above all) its songs have no bindings yet, so an import would map
  nothing and create nothing. The service checks too, for a click that beats the
  `RemoteSyncStarted` event: `import_playlists_unless_syncing` refuses with
  `RemoteError::Syncing` while the server's key is in `RemoteSyncState::active`, and
  the dialog records no result for that refusal (the syncing note already says it).
  A server only queued behind another server's sync is not in `active` yet, so it
  is not caught. The scope ("Only my
  playlists" / "All available") is one choice for every server, default Mine; the
  hint under "All available" warns that a server admin can get other users'
  private playlists too (Navidrome lists every user's playlists to an admin, only
  marked `readonly`). The
  work itself is `LibraryService::import_remote_playlists` →
  `remote_sync::import_playlists`: a one-time copy, nothing is ever sent back and
  no sync touches the result again. Song keys become library items through their
  bindings (`items_by_remote_key`), so a song that is also a local file is that
  file's item and plays from disk. A server playlist goes into the local playlist
  with the same name (the oldest one if several), or a new one, through
  `add_tracks_to_playlist`, which only appends what is missing — importing again
  adds the server's new tracks and anything removed locally, and never removes or
  reorders. Two server playlists with one name (Navidrome: two discs' `.m3u` with
  the same file name) end up in one local playlist. A track repeated inside a
  playlist is kept once (the `(playlist_id, track_id)` unique index). A playlist
  none of whose songs is in the library yet (not synced, or every entry a broken
  `.m3u` path) creates nothing. The result line counts local playlists that were
  created or got at least one track (so same-name server playlists count once and a
  re-import that changes nothing says 0), the tracks actually added (what
  `add_tracks_to_playlist` inserted), and the tracks found out of all tracks, each
  playlist's tracks counted once (a track in two playlists counts twice).
  `PlaylistTracksChanged` goes out for those same changed playlists.
  `RemoteError::NotSynced` ("not synced yet") means the source has no enabled row
  to map keys through. Imports run one at a time
  (`RemoteSyncState::playlist_imports`): two servers importing a playlist with the
  same new name at once would otherwise each create it.
- `playlist_tracks_view.rs` — tracks of one playlist. Rows are drag-reorderable
  (only with an empty filter), persisted via `LibraryService::move_track_in_playlist`.
  Liked and playlist screens show "Unavailable: N" when entries have no file right
  now (folder offline, file deleted); the label is computed where `tracks_all`
  changes (`track_row::unavailable_label`), never in render.
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
  `grouped_tracks_view` (the album, artist and genre screens) — deliberately *not* into
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

## Library sources in Settings

Sources are managed on Settings → Library, one group per kind (local
folders, Subsonic, Jellyfin, torrents, the network cache). `crate::library_sources::
LibrarySources` is the entity those groups read: the folder list comes from
`settings.json` (`music_folders`), status and track count from the `sources`
table (`LibraryService::sources`), joined by the pure, unit-tested
`source_rows`. A folder is **Unavailable** when its source row says so, even
mid-scan; otherwise **Scanning** while a scan runs, else **Online**. The count is
the source's *present* bindings, not catalog rows, so an offline folder still
shows how many tracks it holds.

Settings rows render every frame, so nothing there touches the database: the
entity caches rows with their labels. The scanning state is
`LibraryService::is_scanning()`, re-read whenever rows are rebuilt. The worker
clears that flag only after its last result event, so it also sends
`LibraryEvent::ScanIdle` once the flag is down (after any queued follow-up scan);
the entity rebuilds on `ScanStarted` and `ScanIdle`, and reloads the summaries
after `CatalogChanged` or `ScanFailed` (a failed scan may already
have committed new availability flags) and when the folder list changes.

A database migrated to v9 has every binding on the disabled placeholder source
until a real scan re-points them; `run_scan` refuses the fast path while
`has_unplaced_media()` is true, otherwise an unchanged disk would keep the folders
looking empty (and their missing files never retired) indefinitely.

Folder-watcher events go through `LibraryService::watched_change`, not the 2 s
debounce of `request_rescan`: the scan waits until the folders have been quiet
for `WATCH_QUIET` (10 s) and at least `WATCH_COOLDOWN` (60 s) have passed since
the previous scan finished. A torrent client or a copy writing into a music
folder therefore yields one scan when it stops, or at most one a minute while it
trickles — not one per file. Explicit requests (settings, window activation,
album moves) keep the short debounce, and `hold_rescans()` suppresses the
non-forced ones.

### Servers (Subsonic, Jellyfin)

The Subsonic and Jellyfin groups on the same page share their rendering and
wiring (`remote_settings.rs`: server list, connect form, remove dialog,
`apply_remote_sources`, `sync_all`, the offline watcher); `subsonic_settings.rs`
and `jellyfin_settings.rs` only hold each kind's connect. They list configured
servers (`settings.json` → `subsonic_servers` with the password in plain text by
decision, next to the scrobbling keys; `jellyfin_servers` with an access token,
user id and device id — the password is only sent once, to log in) with status,
track count, Sync, a "Likes & scrobbling" link and Remove, plus the connect
form. The link dispatches `settings_view::OpenScrobblingSettings`, which
`MainView` turns into `open_settings(scrobbling)`; importing a server's
favorites lives on that tab, next to the Last.fm/ListenBrainz imports, under
one label ("Import likes" — the same word as "Send likes" and the servers'
"Likes" switch). Connect
pings (Jellyfin: logs in, then pings) and saves only a server that answered.
Everything downstream is kind-agnostic: protocols live behind
`crate::servers::ServerClient` (see `servers/doc.md`), bytes behind
`crate::remote_media::SourceMedia` (see `remote_media/doc.md`), and servers are
keyed `kind:uri` (`RemoteServer::key`) in `source_ids`, the sync queue, the
events and the per-row messages, so a Subsonic and a Jellyfin account at the
same address stay apart. The rows come from the same `LibrarySources` entity:
`RemoteSyncStarted`/`Finished` events drive the Syncing state and the
per-server result line. `RemoteStarsImported` is read by the Scrobbling tab
instead (`scrobble_settings::watch_server_imports`), which shows the import's
result under that server's row.

Sync (`remote_sync.rs`, run on its own thread by `LibraryService::sync_remote`,
one at a time — requests arriving mid-sync are queued): ping → full listing →
`apply_remote_listing` with the covers already known → covers not seen before →
`apply_remote_listing` again, with them. The listing goes in first so a big
first sync shows its songs right away (a listing that changed something is
rescanned before the covers start) instead of after every picture has come in.
A song whose key has no picture yet is written with `cover_key = NULL` and the
cover its row already shows (`remote_song_cover_hashes`): an album the server
re-keyed keeps its art for the length of the sync, and a sync cut short (quit,
server gone) leaves the key unknown, so the next one simply asks again — nothing
about an unfinished cover phase is kept between sessions, and a key that gave no
usable picture is asked again on every sync, as before. `fetch_covers` runs 4
`cover-fetch` threads that only wait on the network (`ServerClient::cover_art`
with the large thumbnail's size, so the server scales the picture); hashing and
thumbnailing stay on the sync thread, which reports
`LibraryEvent::RemoteSyncProgress { done, total }` about once a second — the
server row shows it instead of "Syncing…" ("Covers 340/1358"). The new covers
stay in memory and go into the second `apply_remote_listing`, in its
transaction, as they always did. Eight transport or auth failures in a row end
the cover phase (the rest waits for the next sync instead of a timeout per
cover). A sync with new songs and new covers therefore rescans twice. The adapters in `servers/` clean the listing on the way in: server
placeholders (`[Unknown Artist]`, `[Unknown Album]`) become empty, and a track
number above 999 is dropped — Navidrome takes one from a leading number in an
untagged file's name, so a whole-disc image `1997 - Around The Fur.flac` arrives
as track 1997. A Jellyfin item's suffix comes from its file path, else from
its container list (`mov,mp4,m4a,…` → `m4a`), since the decoder picks a backend by
extension; its bitrate arrives in bit/s and is stored in kbit/s like Subsonic's.
Only when something changed (songs, tags, availability)
is the scan fingerprint dropped and a scan run so the catalog re-projects;
otherwise a launch with a server keeps the local fast path. A server that is
added, removed or re-added turns its `sources` row on or off, which no listing
reports (re-adding a server whose songs did not change syncs as "nothing
changed"), so `apply_remote_sources` rescans itself whenever
`reconcile_remote_sources` says a row flipped. At launch `main.rs` calls
`reconcile_sources` instead: it only drops the fingerprint, because the launch
scan that follows has the folder list and a scan started earlier would run
without it. A failed ping or
listing, or a listing the library refuses (empty while it had songs), marks the
source unavailable and changes nothing else. Failing to *store* a listing is our
own database being busy, not the server being down: it is retried a few times
2 s apart and never marks the source unavailable. Servers sync at
launch and on demand. A server marked unavailable is retried by itself: every
60 s (`remote_settings::watch_offline_servers`) and on window activation
(`sync_offline`), skipped while a sync is already running. Online servers are not
re-listed on activation, so a healthy server costs no network until the next
launch or Sync. The launch
scan also runs when only servers are configured, so a server-only library is
re-projected from the cache at every start.

Playback of a server-only track: `Track.path` is a locator.
Opening a track lives in `crate::playback_opener` — no GPUI, unit-tested
against a fake backend (`OpenerBackend`: copies from the library, the cache,
download, open a stream, "is the server down?") with the engine commands
collected from a `Sink`. `Services::start_track` only resets the position and
calls `Player::start` (`crate::cast`), which hands the track to the active
target's `PlaybackOpener` (or to a cast session when a renderer is selected). A local file or a cached track is set at once;
anything else gets `Command::Prepare` and is opened on a `track-opener`
thread: `RemoteMedia::open_stream` joins or starts a `media_stream` download of
the file into `<cache>/pawse/media/<source>/…`, and the engine gets a
`StreamingSource` through an `EngineCommander` (a `Send` handle on the engine's
command channel). The engine thread never waits on the network. A generation
counter drops the result if another track was requested meanwhile (or `stop`),
the stream being opened is aborted at once (its error is neither shown nor
answered with a ping), and the final send happens under the
same slot lock a newer `start` takes, so an old track can never overtake a newer
one. A failure reaches the user through
`Command::Fail` → the usual playback-error toast. The message is the download's
own reason (`AbortHandle::failure`): symphonia turns a failed read during format
probing into "no suitable format reader", which says nothing. When a server track
fails, the server is pinged; if that fails too the source is marked unavailable
right away (`mark_source_offline`, same as a failed sync) and the toast says the
server is unreachable; the offline retry brings it back. A download that has not
received a byte gives up after 2 retries (under a second), one that was already
streaming keeps the 6 retries for network hiccups. APE and DSD are downloaded whole
first. The opener always walks `playback_locators` (local first, then servers
by source id), so a vanished local file or a server that is down falls through
to the item's next copy, and only the last failure is shown. At launch a server track that is
not cached is not restored; Play then loads it and the saved position is applied
once it is loaded (`resume_at`). The next remote track in the queue is fetched
60 s ahead (`REMOTE_PREFETCH_LEAD`; 30 s was too little on slow servers and
torrents) so gapless playback works. While a track that is not cached yet is
opening, the engine sends `EngineEvent::Preparing` with the track's catalog
duration: now-playing, the progress slider (at 0, disabled) and the "current
row" highlights switch to the new track at once instead of showing the old one
until `Loaded`. The web remote gets `buffering` in its state (published on every
`Buffering` event) and stops advancing its slider and shows a spinner while it
is set. Next on the last track without repeat does nothing
(`PlaybackQueue::skip_to_next`); only a track that ends on its own empties the
queue position. The cache is trimmed to the user's
limit (`network_cache_gb`: 1, 2, 4, 8, 16 or 32 GB or Unlimited, default 8),
oldest first; lowering the limit trims at once, on the background executor. It is one cache for every network source, so it has its own group
under Settings → Library (`cache_settings.rs`), not a row inside a server group:
size (recounted each time settings open), Clear, and the limit. Clearing and
trimming leave downloads in progress (`.partial` younger than a day) alone. Tag editing (track and album) and lyrics export skip server tracks; a server
track's own lyrics are asked from the server each time the lyrics panel shows
it (`ServerClient::lyrics`, see `servers/doc.md`) and take the place of the
`.lrc`/tag segments, ranked against LRCLIB by the same "Prefer LRCLIB" setting.
The request starts with the track (alongside the `library.db` read); for up to
`SERVER_GRACE` (1.5 s) the LRCLIB auto-search waits for it and a stored source
ranked below the server is not shown yet, so the text does not swap under the
user. A slower server (unreachable while the track plays from the cache waits
out a 10 s connect timeout) stops being waited for: what is stored shows, the
search may start, and a late server answer still takes over. A failed request
counts as "the server has none" for that play. Turning "Fetch lyrics from the
internet" off forgets everything LRCLIB gave (`LyricsAccess::forget_fetched`:
the words and the "not found" markers, so a later search starts over), and so
does every launch while it stays off; until then the view drops LRCLIB variants
itself. A search already in flight cannot write after that: it carries the
`fetch_epoch` it started under, the forget bumps it at once on the UI thread
(the `DELETE` follows on the background executor), and a write checks the epoch
under the same lock the `DELETE` takes. The request itself is not cancelled; only
its result is dropped. Backing vocals
are a smaller, dimmer line under their row (see "Lyrics karaoke fill"). The
pencil is not shown on a server track's row (`TrackRowBase::local`, from
`Track::local_file`) nor on an
album header whose album has no local file.

## Lyrics karaoke fill

`lyrics_fill.rs` (pure, unit-tested) + `lyrics_view.rs`. The active row is
filled, by drawing the text again in the primary color, clipped to the filled
part (4 alpha bands make the soft edge; gpui has no text masks). A line is
shaped once (`shape_line`, cached by text/width/size/line end) into its wrapped
rows; "filled" is a distance along those rows laid end to end, which equals the
x of a byte index in the unwrapped layout — that is how word ranges map to
pixels.

- **Line fill** (no word timing): the whole interval to the next line (less
  `FILL_LEAD`) sweeps the line evenly.
- **Word fill** (`Word`s from Enhanced LRC, OpenSubsonic cues, Jellyfin cues):
  each word sweeps from the end of the word before it (or the line start) to
  its own end, from its start to its end time; a word without an end runs to
  the next word's start, or to the line's end for the last one. Between words
  the fill holds. The fill is the furthest point any started word has reached,
  so overlapping singers never pull it back; once every word is over, the whole
  line is filled (trailing untimed punctuation included).
- **Backing line**: filled the same way, by its own words, in its own smaller
  normal-weight shape (`FillSlot::Backing`, measured by its own canvas); without
  words it stays unfilled.
- **Lingering row.** Word timings may outlast the next line's start: a held
  note, a duet partner coming in early, or a backing line still answering
  (`Viva La Vida`'s "oh"s run ~2 s into the next line). So the row before the
  active one stays lit and keeps filling while any of its main or backing words
  is unfinished (`sung_until`, same word ends as the fill), then dims. Only that
  one row: real files never overrun two lines. Fill state is kept per lit row
  (`LitRow`, keyed by row index), so when the active row moves on, the old one
  keeps its shapes and measurements and lingers without a blank frame. Rows
  with no word timing on either line never linger: their fill ends before the
  next line starts. Dimming drops the semibold weight, which can change how a
  long line wraps, so the frame after a row stops lingering re-runs autoscroll
  (`resettle`) against the new layout; a no-op when nothing moved.
- **Open words.** A word with no end runs to the next word's start, else to the
  line's end; when the line ends before it even starts (a backing word sung
  over the next line) or the end is unknown, it gets `OPEN_WORD_MS` so it still
  shows filling instead of popping at its start.
- **Frames.** A repaint is scheduled only while something moves: during a word
  at its pixel rate (never faster than `FRAME_MIN_MS`), in a gap exactly when
  the next word starts, and not at all once every lit line is done. The repaint
  after a lingering row's last word is the one that dims it.

## Playback status: what is playing now

`crate::playback_status::PlaybackStatus` (one entity, `Services::playback_status`)
is the only UI-side reader of the engine's track lifecycle: `Preparing`,
`Loaded`, `TrackEnded`, `Stopped`, `Error`. It keeps the current track id (from
the queue), the phase (`Idle` / `Preparing` / `Ready` with the stream format)
and the duration (catalog duration while preparing, the decoder's once ready),
and emits `StatusChanged { track_changed }`. Now-playing, the progress slider,
lyrics, the cover backdrop, cover mode, the cover skin, the queue and the
current-row highlight in every track list subscribe to it instead of each
matching engine events and re-reading the queue — a new engine phase is handled
in one place (`transition`, unit-tested). Views still take `Playing`, `Paused`,
`PositionChanged` and `Buffering` straight from the engine bus; the
integrations (scrobbling, OS media controls, Discord) keep `Loaded`, since they
need the moment audio is really there. Lyrics load once per track, when the
phase is `Preparing` or `Ready` (never on the `Idle` in between, whose duration
is unknown).

## Library events: catalog vs scan

`LibraryEvent::CatalogChanged` is the one "re-read what you display" signal: the
tracks, albums, artists, covers or disk lyrics in the database changed. A scan
that did work sends it, and so does saving tags (track or album) — anything that
rewrites catalog rows. Views, the queue (which then rebuilds every row even when
the ids are unchanged, since a cover or an artist may be new), now-playing, the
cover skins, the web remote's `library_rev` and the queue re-mapping all listen
to it. `ScanStarted` / `ScanComplete` are only the scan's lifecycle (the
"scanning" indicators); `ScanComplete` also fires on the fast path, when nothing
changed. `TagsSaved` is only for the "tags saved" toast and dropping the cover
cache, and is sent before `CatalogChanged` so views reload against the cleared
cache.

The title-bar corner indicator (`library_scan_indicator.rs`) is one global
entity created at startup, so it sees scans and syncs that began before the
window existed (onboarding, a macOS window reopened from the Dock). It shows a
refresh icon while a scan runs or any server syncs (`RemoteSyncStarted` /
`Finished`, keyed by server) and a check for 30 s after `ScanSucceeded` or the
manual `ScanUpToDate`; `ScanFailed`, an unavailable folder and `TagsSaved` stay
toasts. A sync shows the icon only once it has run for 1 s: every launch
re-syncs every server, and an unchanged one finishes in milliseconds, which
would flash the icon at each start. The icon goes away with the last
`RemoteSyncFinished`; a sync that changed something is followed by a rescan
whose `ScanStarted` only comes after the folder walk, so on a big library the
icon may blink off for that moment (accepted, a hold-over delay at launch was
worse).
It is right-aligned on macOS and left-aligned elsewhere (opposite the window
buttons) and hidden in fullscreen.

The drill-down views (`tracks_view`, `grouped_tracks_view`) are built once for an
album, artist or genre and do not listen to either; they show what they were opened
with until the user navigates. The ids they hold stay valid across rescans:
the scan gives an album back its previous id (same title and year) and an
artist too (same name), so a stale view never links to a different album (see
`music_library/src/doc.md`, "Stable album and artist ids"). A genre page holds the
genre's key instead, for the same reason.

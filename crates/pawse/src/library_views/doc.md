# library_views

The library screens: the root tabs (Albums, Artists, Genres, Liked, Playlists), the
pages reached from them (album, artist, genre, playlist, All tracks), the view-options
menu next to the header search, and the server playlist import dialog. Views read
`LibraryService` synchronously, follow `LibraryEventsBus`, `PlaybackStatus` (current
row) and the engine bus (play/pause icon), and start playback through `crate::track_list`.

Not here: row buttons and row styling (`track_list/doc.md`), the cover cache and artist
cards/photos (`pawse/src/doc.md`), the queries behind the views
(`music_library/src/doc.md`), the header that hosts the search field and the menu
trigger (`main_view/doc.md`).

## Files

- `library_view.rs` — `LibraryView`: root container, navigation stack, search routing,
  `resolve_artist_page`.
- `albums_view.rs` — Albums tab, List or Grid; also `empty_library` /
  `no_music_message`, the empty state the tabs share.
- `albums_grid.rs` — album tile and strip of the Grid layout.
- `artists_view.rs` — Artists tab, List or Grid, picture Photo or Covers.
- `artists_grid.rs` — artist tile: photo, or a cover flow of the first three albums
  (`flow`, pure).
- `cover_grid.rs` — what Albums and Artists share: the item list (`LibraryItem`,
  `list_layout` / `grid_layout`), grid geometry, the docked section header,
  `width_probe`, `GridCovers` (lazy large covers).
- `view_order.rs` — order and sections for Albums and Artists (pure). The web remote's
  `/api/albums` and `/api/artists` sort with it too (`library_service.rs`); the remote
  keeps its own sort choice per browser and never touches the desktop's.
- `view_menu.rs` — the view-options popover. `menu_surface`, `MenuColors`,
  `section_label`, `separator` are reused by `similar_tracks::menu`.
- `genres_view.rs` — Genres tab.
- `tracks_view.rs` + `album_info.rs` — album page and its header.
- `grouped_tracks_view.rs` — artist page and genre page (`Scope`), tracks grouped by album.
- `liked_view.rs` — Liked tab.
- `playlist_tracks_view.rs` — one playlist, or All tracks.
- `playlists_view.rs` — Playlists tab: list, create / rename / delete, Import and AI.
- `playlist_import.rs` — server playlist import dialog.
- `track_row.rs` — shared by the track pages: Liked/playlist columns, the guest-credit
  rule, search haystacks, the unavailable label. `build_artist_map` is used by the queue.
- `fuzzy.rs` — the fuzzy filter (nucleo) with a minimum score per typed character.

## Navigation

- `stack[0]` is always `Root(tab)`; a page pushes a frame that owns its view and the
  view's subscriptions. Only the top frame renders and gets the search query; frames
  below stay live but unmounted. A tab click resets to `[Root(tab)]`, back pops one
  frame; jumps from now-playing, the footer and cover mode push frames too. On a page
  `current_tab()` is `None` and `MainView` keeps the previous tab lit.
- Genres is optional (Settings, off by default): its view is built on first visit and
  dropped when the setting goes off. Turning Liked / Playlists / Genres off removes
  their frames; if `stack[0]` is no longer a root, the stack becomes `[Root(Albums)]`.
- Root tabs reload on `CatalogChanged` (tag edits send it too); pages do not. Pages
  rely on album and artist ids surviving rescans (`music_library/src/doc.md`, "Stable
  album and artist ids"); a genre page holds the genre key, not its id, for the same
  reason.
- `SoundGenresChanged` (sent while the sound analysis saves) is acted on only while
  genres by sound are shown: the Genres tab, the album list and an open album header
  reload on it; a genre page follows only the switch.
- An artist link opens the first page that exists (`resolve_artist_page`, unit-tested):
  the artist in the current grouping; else, given the track, the listed artist the
  credit belongs to ("Samurai" for "Samurai feat. Refused", `listed_artist_for_credit`
  — join markers only, so a compilation guest keeps their own page instead of "Various
  Artists"); else the artist in the other grouping. Now-playing and cover mode pass the
  id of the track whose artists they show, not `PlaybackStatus`'s, which is empty while
  a restored server track is not loaded.

## Rows and rendering

- Each view turns its data into display rows (strings, covers, flags) when the data
  changes. The `v_virtual_list` closure only indexes them: no formatting, allocation or
  database reads there.
- Track pages keep `tracks_all` (the source, `Rc<Track>`) and `row_data` (the filtered
  rows, each with `track_all_ix`). A click plays the whole `tracks_all` from that track,
  not the filtered subset.
- Likes are patched into rows in place (`LibraryEvent::liked_update`); Liked itself
  reloads or drops the row.
- Draw-only settings (liked / playlists / tag editor enabled) are read once per render
  into the row `*Params`; they reach the screen because `LibraryView` is a cached block
  that every settings write notifies (`pawse/src/doc.md`, "Cached blocks"). Any other
  render input of a view here must end in a notify of some view inside `LibraryView`, or
  it stays stale while the block is replayed. Settings that change data are followed by
  the view's own `observe_global::<SettingsStore>`, which compares a snapshot first: it
  fires on every settings write.
- Search: the trimmed query is fuzzy-matched against haystacks built at load, best
  first; the list scrolls to the top.

## Albums and Artists tabs

- Order is computed in Rust by `view_order` from keys built once per load, not in SQL.
  While a query is typed the order is the fuzzy score's and there are no sections.
- `view_order`: names go through `compute_sort_name` (articles dropped) and compare
  lowercased, byte-wise (accented capitals land after Z). A section letter is a cased
  letter whose single uppercase lowercases back to it; anything else is `#`. `#`,
  undated albums and "No metadata" are last in both directions. Descending flips only
  the primary key, so an artist's albums stay chronological. Year sections are decades;
  a year ≤ 0 is no year everywhere.
- Both layouts are one `v_virtual_list` over `LibraryItem`s: rows in List, strips of N
  tiles in Grid. A section always starts on a new strip.
- The section header sticks as a slot docked above the list, not an overlay: nothing
  scrolls under the label, so it needs no background. `docked_labels` (pure) places the
  docked and the incoming label. The slot clamps the scroll offset itself (gpui clamps
  only in prepaint, so `render` can see an overscrolled offset) and forwards the wheel.
- Item 0 is a spacer: `VirtualList` measures with the range `0..1` every frame, and
  cover loads follow only the strips in the range (`cover_span`), so that pass loads
  nothing.
- Back to the top is `cover_grid::scroll_to_top`, which sets the offset at once;
  `scroll_to_item` waits for prepaint, after `render` has placed the docked label.
- Geometry: `grid_metrics` (pure) fits as many `TILE_MIN_WIDTH` columns as possible and
  stretches the tiles to the full width; a panel narrower than one tile shrinks the
  single column. Caption boxes take fixed heights from `caption_heights(rem)` and set
  the same `line_height` (gpui's default 1.618 would clip); `TEXT_SM_REMS` /
  `TEXT_XS_REMS` mirror gpui's `text_sm` / `text_xs`. The width comes from
  `width_probe`, since the panel is narrower than the window; no grid is drawn before
  the first measurement.
- `render` snapshots items and sizes (`Rc`), and the list closure indexes only that
  snapshot. `set_grid_width` runs between the two (from the probe's prepaint) and may
  touch only `layout_items`, never rows or section labels.
- Covers: List rows hold small covers (`get_small`). Grid rows hold none: tiles read
  the large LRU at render (`peek_large`, which also keeps them recent) and
  `GridCovers::ensure` decodes what the visible strips ±1 row miss on the background
  executor. A cover that fails goes to `unavailable` so it is not requested every
  frame; both sets reset on `CatalogChanged` (re-tagging can add a cover).
- Albums: `AlbumsPrefs` decides what a settings change costs. Layout, sort or grouping
  → new rows and back to the top (the layout picks the cover source, so it is a data
  change); the genres-by-sound switch → reload of that map on the background executor
  (the old one stays until the new arrives); Show chips or font scale → item sizes only.
  Genre column: first genre + `…`, tags then heard genres in the tooltip; an album with
  no tag genre shows its first heard one, dimmed.
- Artists: the grouping (`artists_grouping`, album artist / track artist) is a data
  change; `MainView` mirrors it into `LibraryService` and republishes the web remote's
  library, whose Artists list follows. In the album-artist grouping the search also matches everyone credited on
  the artist's page (`artist_search_haystacks`: "xzibit" finds Limp Bizkit) without
  making guests rows. Letter sections exist only for the Name sort.
- Artist pictures: Photo applies only while `artist_info_from_internet` is on, and an
  artist without a photo gets the person placeholder, never covers. Photos come from
  `ArtistCards` (80 px list, 300 px grid), asked for by the list closure: visible rows,
  then the screen below, then above. Rows keep no copy; the view repaints on
  `ArtistCards` notify. In Covers the grid tile is a cover flow of the first three
  albums: sides dimmed by opacity, not a dark overlay (no hardcoded black), caption
  centred.
- On-screen pictures stay recent: covers through the tile's own `peek_large`, photos
  through `ensure_*`, so photo tiles read with the non-touching getter.

## Album, artist and genre pages

- `album_info`: up to three tag genres inline + all of them in a tooltip, heard genres
  dimmed after them; tracks and genres reload on `CatalogChanged`. Buttons: tag editor
  (the album has a local file), save to cache, move to the local folder, add to queue.
- Guest column (`track_row::guest_credits`): a track whose credited artists differ, as
  a set, from its album's *known* artists — and on an artist page from the page artist
  — shows them in an artist column; when the album's artist is not known, every track
  shows its credit. Artist pages show it only in the album-artist grouping. The column
  is reserved per album group so titles align; the full credit is in a tooltip.
- An artist page keeps the grouping it was opened with. An album the artist is only
  partly on gets a queue menu (the artist's tracks / the whole album), and the page a
  "Full albums" toggle that re-fetches with whole albums (`tracks_all` stays the queue
  source). Save-to-cache and the card header are artist-only, no card for "No
  metadata". When the card changes the header height, the scroll offset moves by the
  same amount.
- Genre page: sort from `genres_sort` (Artist / Year + direction), one preference for
  every genre, followed live together with the genres-by-sound filter. Groups are runs
  of consecutive `album_id`, so `tracks_by_genre` must keep an album's tracks together:
  both sorts key on the album's year and its first album artist, never the track's, and
  ignore `artists_grouping`; undated albums after dated, tracks without an album last.
  Album headers add the album artist as a link. No partial albums, no Full albums, no
  save-to-cache (`FillTarget` is keyed by ids).
- Search haystacks: album page — title + guest credit; artist page — the same; genre
  page — title + credit + album + album artist.
- The tag-editor pencil is on album, artist and genre pages only, for local tracks; not
  on Liked, playlists or the queue (playback-ordering screens).

## Liked, playlists, All tracks

- Playback-ordering screens: no sort, drag reorder only with an empty filter (Liked →
  `move_liked_track` + reload; a playlist → `move_track_in_playlist`, reloaded by
  `PlaylistTracksChanged`). All tracks is a `PlaylistTracksView` without reorder or
  remove.
- Columns (`TrackListPrefs`, the `Tracks` view menu) are one preference for Liked, every
  playlist and All tracks; widths in rems; the title keeps `TITLE_MIN_WIDTH` (a `flex_1`
  item would give way first). Album names come from `track_albums_map`, which also
  names unavailable tracks; not from `albums()`, since a reload runs on every drop.
- Unavailable tracks are dimmed; hiding them only drops rows (the queue skips
  unplayable tracks anyway). The "Unavailable: N" label is computed when tracks change.
- Playlists tab: "New playlist" and All tracks are list items that scroll away (plain
  rows when no playlist row is shown). Create and rename are inline inputs; blur cancels
  only an empty or unchanged name, so ✓ still works; delete confirms inline. Import
  shows while a server that imports playlists is configured (`can_import`, refreshed by
  the settings observer); AI dispatches `tools::OpenAiPlaylist`.
- Import dialog: `PlaylistImport` is owned by `PlaylistsView`, so closing the dialog
  mid-import loses nothing. A syncing server's import is disabled (its songs have no
  bindings yet); the service refuses too (`RemoteError::Syncing`), and the dialog shows
  no result for that. The scope (only mine / all available) is one choice for every
  server, default mine; the hint under "all" warns that a server admin also gets other
  users' private playlists. What an import does: `pawse/src/doc.md`, "Server playlist
  import".

## View menu

- One trigger with the same icon everywhere: "view options", not the current mode. It
  opens to the right, over the queue rather than over the list it changes. Which menu:
  `LibraryView::view_menu_tab` — Albums, Artists, Genre (genre page), Tracks (Liked, a
  playlist, All tracks); none on album and artist pages or the Genres and Playlists lists.
- Controls write `SettingsStore` directly; the popover stays open and the views follow
  through their observers.
- Clicking the active sort flips its direction; another sort starts in its default
  direction (Track count: most first).
- In a switch row the row owns the click and the `Switch` only shows state: an enabled
  gpui-component `Switch` does not stop propagation, so a handler on both writes twice.
- Labels wrap (`min_w_0`), switches and hints never shrink: translations run long.
- Controls are ordered by use, most used nearest the trigger: Grid/List on top, Tag at
  the bottom of the Artists menu. The Tag buttons are the raw tag names `artist` /
  `album artist`, untranslated.

## Decisions

Owner's calls; ask before changing.

- Artists picture defaults to Covers: users updating from 0.8.7 see the look they had.
- `#`, undated and "No metadata" go last in both directions (Apple Music's convention),
  grouped or not, so a name never moves when headers are switched on.
- Sticky header as an overlay was rejected: a translucent band shows covers through the
  label on the blur backdrop, an opaque one is a flat plate on it.
- Artist grid: cover flow, not a 2×2 mosaic or round stacks. Two covers are centred as a
  group; the caption is centred under Covers and left-aligned under Photo.
- A missing photo shows the placeholder, not covers: one setting, one look.
- No "No genre" row: in a poorly tagged library it would be the biggest row and a copy
  of All tracks. A genres tab with no tags only says so (adding folders wouldn't help).
- Guest credits go into a tooltip, not shortened; no guest column under the track-artist
  grouping, where every row is the page artist's.
- No sort on Liked and playlists.
- No layout glyph on the menu trigger.

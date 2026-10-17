# pawse (crate root)

The GPUI app: the files directly in `crates/pawse/src` that have no folder of their own.
Folder modules (`library_views`, `track_list`, `servers`, `remote_media`, `cast`,
`similar_tracks`, `tools`, `sleep_timer`, `cover_skin`, `main_view`) have their own
`doc.md`; the library crates have theirs.

## Files

App shell:
- `main.rs` — startup, window creation, app-level actions, launch restore, the quit hook.
- `main_view.rs` — `MainView`, the root view (header: `main_view/header.rs`).
  `window_title_bar.rs` — `WindowTitleBar`. `onboarding_view.rs` — the first-run window.
- `services.rs` — the `Services` global, transport functions (`play_next`, `seek_to_ms`,
  `set_volume`, …), `run_engine_events_bus`, prefetch, queue persistence, and the web
  remote's command/state half (`apply_remote_command`, `build_remote_state`).
- `keyboard_shortcuts.rs` — key bindings. `app_menu.rs` — localized application menu.
  `localization.rs` — saved language to `tr()`. `pickers.rs` — theme and language
  dropdowns. `theme_colors.rs` — `Colors`, the theme lookups views use.
- `single_instance.rs` — second-launch guard (Windows, Linux).
- `settings_store.rs` — `SettingsStore` over `UserSettings` (`settings.json`).
  `settings_view.rs` — `build_settings_pages` (Appearance, General, Scrobbling, Library).

Playback and now playing:
- `playback_opener.rs` — `PlaybackOpener`: `TrackRequest` in, engine `Command`s out via a
  `Sink`; `OpenerBackend` is its seam to library and cache. `playback_opener/tests.rs` —
  fake backend, no GPUI.
- `playback_queue.rs` — `PlaybackQueue`: order, current index, shuffle, repeat, source,
  `custom`, session-only mix state. Pure, UI thread (`Rc<Track>`).
- `playback_status.rs` — `PlaybackStatus`: current track id, phase, duration.
- `queue_view.rs` — the queue panel (rows from `track_list`). `panel_header.rs` — the
  header row of the side panels. `playlist_popup.rs` — the add-to-playlist popup.
- `lyrics_view.rs` — lyrics panel. `lyrics_fill.rs` — its pure half: rows, active row,
  fill geometry.
- `now_playing.rs` — footer's left block. `footer.rs` — footer layout and toggles.
  `track_progress_slider.rs` — progress slider, labels, keyboard seek.
  `play_button.rs`, `prev_button.rs`, `next_button.rs`, `shuffle_button.rs`,
  `repeat_button.rs` — the transport buttons.
- `cover_mode_view.rs` — fullscreen cover. `cover_backdrop.rs` — blurred cover behind the
  window, translucent chrome colors. `cover_volume.rs` / `volume.rs` — cover-mode / footer
  volume.
- `audio_settings.rs` — header output controls; `bit_perfect_info.rs` — their tooltip and
  dialog. `pipewire_alsa_gate.rs` — Linux window shown instead of the app.

Library and sources:
- `library_service.rs` — `LibraryService`, the app's door to `LibraryRepository`: scans,
  tag-edit point updates, server sync and imports, playlist/like writes; `LibraryEvent`;
  `LibraryAccess` (web remote reader); `LyricsAccess`.
- `library_sources.rs` — `LibrarySources`, the entity behind Settings → Library.
- `library_watcher.rs` — `notify` watcher on the music folders.
- `library_scan_indicator.rs` — global entity, the icon left of the search field.
- `remote_settings.rs` — what the server groups share (list, connect form, remove
  dialog) and the source/sync entry points.
- `remote_sync.rs` — one server's sync (listing, covers), star and playlist import.
- `subsonic_settings.rs`, `jellyfin_settings.rs`, `dlna_settings.rs`,
  `torrent_settings.rs` — each kind's connect/add flow and group.
- `cache_settings.rs` — the network-cache group; `cache_fill.rs` — saving server tracks
  into the cache; `album_export.rs` — moving an album into a music folder.
- `tag_editor_view.rs` — the track and album tag modal.
- `cover_art_cache.rs` — small/large cover caches, `TileLru`, `drop_atlas_tile`.
- `artist_card.rs`, `artist_photos.rs` — artist info indexing, card, photo stores.

Integrations:
- `scrobble_bridge.rs` — events to plays and loves, targets, failure notices, old-queue
  import. `scrobble_store.rs` — `ScrobbleStore` over `LibraryRepository`.
  `server_scrobble.rs` — a server as a target. `scrobble_import.rs` — loved-tracks
  import.
- `scrobble_settings.rs` — the Scrobbling page; `scrobble_rules_settings.rs` — its rule
  groups.
- `scrobble_flow_tests.rs` — whole-flow tests; a `#[path]` child module of
  `scrobble_bridge`, so `main.rs` does not list it.
- `discord_bridge.rs`, `media_bridge.rs` — engine events to Discord / the OS now-playing
  surface.
- `error_bridge.rs` — `diagnostics` notices and background errors as notifications.

## Startup and quit (`main.rs`)

- `diagnostics::init` is the first call in `app.run`; its notice receiver goes to
  `error_bridge`. Then the soft open-file limit is raised to the hard one (failure only
  logged): the torrent engine keeps a descriptor open per file of every loaded torrent,
  and an app started from Finder has a soft limit of 256, which breaks library scans
  next to a torrent of a few hundred files.
- Settings load, theme, background writer and language sync come before any view exists.
  A named theme is applied again when the bundled themes finish registering: the
  registry may not hold it yet at the first application.
- `open_initial_window(cx, run_startup_tasks)`: only the launch window passes `true`.
  The startup tasks run once per process: `reconcile_sources`, then
  `scrobble_bridge::apply_settings` (server targets need the `sources` ids reconcile
  creates), `restore_engine_state`, and on the first frame the rescan,
  `library_watcher::rebuild` and `sync_all`. A macOS reopen, a second-launch raise and
  the window after onboarding pass `false`; without `onboarding_complete` the first
  window is the onboarding one.
- Closing the last window quits on Windows and Linux. On macOS the app stays (audio
  keeps playing, `on_reopen` opens a window); only Quit leaves.
- The quit hook finalizes the scrobble, shuts Discord and the similar-tracks pipeline
  down, saves the playback state with `save_playback_blocking` (synchronous; it also
  detaches the background writer so nothing races it), shuts the services down and
  flushes the log. Only the scrobble commit is awaited.
- `run_engine_events_bus` is spawned last, after every subscriber exists; earlier engine
  events wait in its channel. `Services` exist only after the Linux gate, which returns
  early after opening its window and installing the single-instance listener.

## Single instance (`single_instance.rs`, `Info.plist`)

- Windows and Linux only. `acquire()` runs before the GPUI application exists: a
  successful connection to the socket `dev.pawse.app.sock` means another instance runs
  and this one exits. A failed bind is retried as a connect (two simultaneous launches);
  if both fail the app runs unguarded with a warning.
- `install` runs the blocking `accept` on a thread (`single-instance`). Each connection
  raises the app on the UI thread: activate, first window, or open a window when none
  exists (`run_startup_tasks` false). After 16 consecutive accept errors the thread
  stops.
- macOS has no socket: `LSMultipleInstancesProhibited` in `crates/pawse/Info.plist` is
  enforced by LaunchServices, so it covers Finder and `open` but not a binary started
  from a shell (CLAUDE.md: no single-instance lock when running `target/*/pawse`). With
  the key `open -n` starts no second instance (`updater/src/install/doc.md`).

## Main view (`main_view.rs`, `window_title_bar.rs`)

- The content area shows one of: cover mode, Settings, Tools, the library. Settings and
  Tools are exclusive overlays; opening either calls `leave_overlays` (clear search,
  leave cover mode) and closes the other. `LibraryView` keeps its navigation stack
  underneath.
- `toggle_cover_mode` (header button and footer cover) closes both screens and clears
  the search first, so leaving cover mode lands in the library. Escape only leaves cover
  mode, else it propagates.
- Tools: `ToolsView` is created once with `MainView`, so tool state survives closing the
  screen. Entry points: the header button (drawn only with `tools_enabled` and no screen
  open), `tools::OpenAiPlaylist` and the footer's `OpenSleepTimerEvent`
  (`tools::TIMER_PAGE`); the last two work with the button hidden because `open_tools`
  never reads `tools_enabled`. Back closes either screen (`tools/doc.md` for the
  module).
- Settings: `open_settings_at(page, anchor)` bumps `settings_page_request`, so the
  widget switches page even when already open. `OpenScrobblingSettings` (the link on a
  server row) opens Scrobbling at `MEDIA_SERVERS_ANCHOR`; `OpenSleepTimerSettings` opens
  General at the sleep-timer anchor.
- Settings pages are built with labels baked in and rebuilt on every `SettingsStore`
  change and every `ThemeRegistry` change (a language change reaches them this way: the
  language is saved in the store). State that must survive a rebuild (`ScrobbleUiState`,
  `ScrobbleRules`, `ScrobbleInputs`, the sliders, the remote-port input) lives in
  entities owned by `MainView`. The same store observer mirrors a changed
  `artists_grouping` into `LibraryService` (`library_views/doc.md`).
- Focus: the `MainView` key context and focus handle sit on `#main_content`, never on
  `#main_view` or anything spanning the title bar. `track_focus` calls `prevent_default`
  on every left mouse-down in its bounds, which on Windows kills the caption buttons and
  window dragging. `on_action` handlers are on `#main_view`, so actions dispatched from
  the header still arrive. A blur of the search field reclaims focus only when nothing
  else holds it, so popups and pickers are not robbed.
- `WindowTitleBar` is `gpui_component::TitleBar` on macOS and Windows (in fullscreen a
  thin inset; the header then moves below it, `main_view/doc.md` "Placement"). On Linux
  it is ours: window buttons, double-click zoom, drag by `start_window_move`, window
  menu on right click when client-decorated; it does not collapse in fullscreen.
- macOS traffic lights: `HEIGHT` and `TRAFFIC_LIGHT_X/Y` mirror `gpui_component`'s 34 px
  bar and the (9, 9) its `title_bar_options()` sets. A taller bar
  (`header_in_title_bar`) needs them recentered: `sync_traffic_lights` does it on the
  next frame when the target changes, checked every render because the font scale
  changes the bar height. Windows caption buttons are `gpui_component`'s and follow the
  bar height (not checked on a real Windows machine).

## Shortcuts and app menu (`keyboard_shortcuts.rs`, `app_menu.rs`)

- Seek, next / previous, volume, play / pause and Escape are bound to
  `MainView && !Input`. `!Input` relies on `gpui_component` giving every text field the
  context `Input`, so typing never triggers them; they also do not fire while focus is
  in the header. Next / previous are Cmd+arrows on macOS, Ctrl+arrows elsewhere. Cmd-Q,
  H, Alt-H, M are bound app-wide on macOS.
- The menu is built from `tr()`, so `pickers::choose_lang` rebuilds it after a language
  change. Hide / Services / Window items are macOS only; "Check for updates" only when
  `updater::is_supported()`.

## Localization (`localization.rs`)

- The tables and the active-language atomic live in `ui_resources`; the dependency stays
  one-way, `pawse -> ui_resources`. `tr()` is an atomic load and a match, safe inside
  `render`. The saved choice (`System` via the OS locale, `Named(code)` via
  `Lang::from_code`, English if unknown) is resolved only at sync points, never per
  render: `sync_active_lang` at startup, `notify_lang_changed` after a user change
  (`pickers.rs`), which also emits `LangChanged` on `LangEventBus`. Views that cache
  localized strings off the render path subscribe and rebuild; views calling `tr()` in
  `render` need not, the picker follows with `cx.refresh_windows()`.

## Settings persistence (`settings_store.rs`)

- `settings.json` is `UserSettings` in `config_dir/pawse`. Every field has a serde
  default, so a file from an older build loads and a new key takes its default; a file
  that does not parse loads as all defaults and the next save overwrites it. An older
  binary drops keys it does not know on its next save (CLAUDE.md, "Runtime data").
- `save()` sends a copy to the background writer, which keeps only the latest and writes
  a temp file and renames it; before the writer starts it writes synchronously. Failures
  surface through `notify_save_error`. The playback state (queue, position, shuffle,
  repeat) lives in the same file.
- Any `cx.global_mut::<SettingsStore>()` notifies the global's observers; that is how
  views follow a setting.
- Upgrades run in `load_from` on every load and stay idempotent: `migrate_scrobble`
  moves the old top-level `lastfm_enabled` / `lastfm_session` into `scrobble.lastfm` (an
  existing session there wins; the legacy keys are read, never written back);
  `migrate_torrents` and `migrate_albums_artist` belong to the library sources and the
  Albums tab.
- The `scrobble` section holds the per-service state (session or token, `enabled`,
  `send_loves`), `first_artist_only` (on by default), `servers` (both switches off by
  default; entries are not pruned when a server is removed) and `rewrite`.

## Starting a track (`services.rs`)

- `play_track` (with fade-in), `play_track_gapless` (no fade-in; used after `TrackEnded`)
  and `load_track` (load, stay paused) are the only ways to start a track. Each resets the
  position atomic and calls `Player::start`, which routes to the active target
  (`cast/doc.md`).
- The transport functions are the one implementation of each action (buttons, shortcuts,
  cover mode, media keys, web remote): change behaviour there.
- A user "next" (`skip_to_next`) does nothing on the last track without repeat; only a
  track that ends on its own clears the queue position. "Previous" restarts the track
  after 3 s or on the first track.
- `advance_on_track_end` runs before `TrackEnded` is re-emitted, so subscribers see the
  advanced queue. With the sleep timer's "stop at the end of the track" armed the next
  track is only loaded and no continuation is built; with no next track and no stop,
  `similar_tracks::continue_ended_queue` extends the queue.
- Play with nothing loaded (`current_duration_ms == 0`: an uncached server track after
  launch, `Stopped`, `Error`) starts the track instead of resuming.
- Queue edits end with `queue_mutated` (`QueueChanged`, save, remote publish).

## Opening a track (`playback_opener.rs`)

- A local file that exists or a cached server track is set at once (`SetLocalTrack`,
  `prepared: false`) and played unless `AfterLoad::Stay`. Anything else sends
  `Command::Prepare` and opens on a `track-opener` thread; the engine thread never waits
  on the network (`audio_engine/src/doc.md`). One opener per target (local, AirPlay).
- Candidates come from `playback_locators` (local first, then servers by source id:
  `music_library/src/doc.md`). The first that opens wins: a vanished file or a server
  that is down falls through to the item's next copy, and only the last failure is shown.
  With no known copy the catalog `path` is tried as is, so the engine reports it.
- A copy that cannot stream (`can_stream`: APE, DSD) is downloaded whole first; the rest
  become a `StreamingSource` over a `media_stream` download.
- Newest request wins. `start`, `stop` and `cancel` `supersede`: bump the generation and
  abort the stream being opened (its error is neither shown nor pinged). The opener
  thread sends its final command under the slot lock `supersede` also takes, and only if
  its generation is current, so an old track never overtakes a newer one. `start` must
  `supersede` before it sends anything.
- A failure is `Command::Fail` (the playback-error toast) with the stream's own reason
  (`StreamControl::failure`); a failed read while probing otherwise reads "no suitable
  format reader". The server is then pinged: if that fails too, the source is marked
  unavailable at once (`mark_source_offline`), the toast says the server is unreachable,
  and the offline watcher brings it back.

## Queue (`playback_queue.rs`)

- Only available tracks enter the queue (`playable`). Callers pass lists and indexes in
  natural order; `set_tracks_and_play_at` maps the clicked index past dropped tracks and,
  with shuffle on, anchors the clicked track at slot 0.
- Shuffle keeps `original_order` (restored around the current track when turned off);
  `move_track` leaves it alone.
- `custom` = hand-edited (append, remove, move; not shuffle, a new queue or a playlist
  refresh): replacing a custom queue asks first (`track_list/doc.md`).
- `QueueSource::Playlist(id)` follows its playlist (`sync_queue_with_playlist` →
  `refresh_keeping_current`). Mix and continuation set `Unknown`: a playlist-backed queue
  is rebuilt on every edit and would lose them.
- After a rescan (`CatalogChanged`) entries are re-matched by (path, cue offset), not id
  (`remap_to_fresh_tracks`); the current track survives a vanished file, an empty result
  is ignored. Likes are patched in place in both orders.
- Saved on track changes and queue edits, and at quit with the position. `heard` and
  `mixed` are not saved.

## Mix and continuation state (`playback_queue.rs`)

- `heard` — ids of the last `HEARD_KEPT` tracks that became current (`note_current`, on
  every way a track becomes current, `restore` included); consecutive repeats skipped;
  cleared by a new queue and `restore`. Ids, so moving, removing and shuffling cannot
  shift it. `QueueSnapshot::played_tail(n)`: last `n` distinct, oldest first.
- `mixed` — ids `mix_in` added; cleared by a new queue, `restore` and
  `refresh_keeping_current`; removing a track forgets its id. `mix_plan` leaves upcoming
  mixed tracks out of `keep` and all mixed out of `basis`; `mix_in` first drops the
  upcoming mixed tracks (from `original_order` too), so mixing again re-rolls.
- `mix_in` and `continue_with` apply only if the `QueueSnapshot` (ids, current index)
  still equals the queue, else change nothing. Neither touches `custom`. With shuffle on,
  a mixed block goes after its anchor in `original_order`, a continuation at its end, so
  turning shuffle off keeps each mixed track after the one it was mixed for.
- A queue that ended has no current index; `continue_ended_queue` snapshots after that.
  `QueueView::new` registers its refresh with `similar_tracks::set_queue_refresh`; the
  similar-tracks code runs it before `queue_mutated`.

## Services and engine events (`services.rs`)

- `run_engine_events_bus` is the only reader of `Player::events()`. Per event it updates
  the shared atomics (`current_position_ms`, `current_duration_ms`, `current_dsd_rate`,
  `is_playing`, `is_buffering`), publishes to the web remote, then re-emits on
  `EngineEventsBus`, so a subscriber reading an atomic sees the state of the event it
  handles. `Rc` fields of `Services` are UI-thread only.
- Views rebuilt without a track reload (a macOS Dock reopen builds a new `MainView`) take
  initial state from these atomics, not events; the DSD rate is an atomic only for that.
- `Stopped` and `Error` zero the duration, which is what makes the next Play reload;
  `Preparing` sets the catalog duration, zeroes the position and clears the DSD rate.
- Prefetch runs once per loaded track (re-armed on `Loaded`, needs a known duration):
  a remote next track goes to `RemoteMedia::prefetch` `REMOTE_PREFETCH_LEAD` before the
  end so gapless works (`remote_media/doc.md`); a local one only has its first 64 KiB
  read 2 s before the end, to warm the page cache.

## Launch restore (`main.rs`)

- After `Services::initialize`, `main` sets the saved volume and restores the queue. The
  window's startup tasks (not run on a macOS Dock reopen) then call
  `restore_engine_state`.
- A current track that is a local file or cached is loaded paused into the local engine
  and seeked. A server track not in the cache is not restored: its position goes into
  `resume_at`, Play loads it, and the seek happens on `Loaded`
  (`resume_restored_position`: that track id only, position inside the duration).
  Switching cast targets uses the same pair (`cast/doc.md`).

## Playback status (`playback_status.rs`)

- The only UI-side reader of the track lifecycle (`Preparing`, `Loaded`, `TrackEnded`,
  `Stopped`, `Error`): track id, phase (`Idle`, `Preparing`, `Ready` with the stream
  format), duration (catalog while preparing, the decoder's once ready); emits
  `StatusChanged { track_changed }`. A new phase is handled in `transition`
  (unit-tested), nowhere else. `Preparing` lets the UI switch to the new track at once
  instead of keeping the old one until `Loaded`.
- The track id is the queue's current track when the event arrives, and `track_changed`
  compares it with the previous one: the queue must already point at the new track. After
  `TrackEnded` it is the advanced queue, hence `Idle` with `track_changed`.
- Views (now-playing, progress, lyrics, backdrop, cover mode and skin, queue, row
  highlights) subscribe to it; `Playing`, `Paused`, `PositionChanged` and `Buffering`
  still come from the engine bus. Scrobbling, OS media controls and Discord keep
  `Loaded`: they need the moment audio is really there ("Engine-event bridges").

## Lyrics (`lyrics_view.rs`)

- Four sources in fixed tab order: server, `.lrc`, embedded tag, LRCLIB. Which wins is
  `lyrics_source::pick` (`music_library/src/doc.md`, "Lyrics"). Tabs show only with more
  than one source; an unavailable LRCLIB tab starts the search.
- Loaded once per track, on a `PlaybackStatus` change of track id in `Preparing` or
  `Ready`, never on `Idle`. `Stopped` clears; `LyricsChanged` for the current track
  reloads. Neither the server request nor the LRCLIB search starts while the panel is
  hidden; showing it starts them.
- For a track played from a server, its lyrics are asked each time it is shown
  (`RemoteMedia::lyrics`), never stored (`servers/doc.md`, Decisions), and take the place
  of `.lrc`/tag in the ranking against LRCLIB ("Prefer LRCLIB"). For `SERVER_GRACE` the
  LRCLIB auto-search waits and a stored source ranked below the server is not shown, so
  the text does not swap under the user; after it the server is no longer waited for and
  a late answer still takes over. A failed request counts as "none" for that play.
- The auto-search needs "Fetch lyrics from the internet" on, nothing searched yet, and
  no own lyrics (or "Prefer LRCLIB"). Turning the switch off forgets everything LRCLIB
  gave (`LyricsAccess::forget_fetched`: words and "not found" markers; a search in
  flight cannot write after it, see "Web remote reader and lyrics access"), as does
  every launch while it stays off; until the delete lands the view drops LRCLIB rows
  itself.
- Export writes `current_raw` as `.lrc` beside the file, only for LRCLIB text of a local
  non-cue file without a sidecar. Synthetic ♪ rows live only in `rows`, so they never
  reach the file; keep it so. Server tracks never export.
- Time is interpolated between `PositionChanged` ticks (frozen while not playing,
  including `Preparing` and buffering); a row goes active `ACTIVE_TOLERANCE_MS` early.
  Autoscroll centers the active row; the wheel disengages it, the locate button
  re-engages. A hidden panel does not repaint on ticks.

## Lyrics karaoke fill (`lyrics_fill.rs`, `lyrics_view.rs`)

- The active row is filled by drawing its text again in the primary color, clipped to the
  filled part; 4 alpha bands make the soft edge (gpui has no text masks). The unfilled
  text is foreground, or muted when foreground is too close to primary. A layer within
  `BLEED_SNAP_PX` of a row's start or end reaches `FILL_BLEED` font sizes past it, so
  slanted italic glyphs that overhang their advance are filled too (the snap absorbs the
  float error of a full row); `fill_rects` itself stays inside the row.
- A line is shaped once (`shape_line`, cached by text, width, `LineFont`, line end) into
  its wrapped rows. "Filled" is a distance along those rows laid end to end, which equals the
  x of a byte index in the unwrapped layout: how word ranges map to pixels.
- A fill is drawn only when the measured box agrees with the shaped rows (row pitch
  0.9–2.2 font sizes), else it waits for the next measure. The measuring canvas defers its
  notify with `on_next_frame`: gpui drops a notify issued during paint.
- **Line fill** (no word timing): the interval to the next line sweeps the line evenly,
  done `FILL_LEAD` of the way. A blank timed line with a gap of at least
  `INTERLUDE_MIN_MS` shows as "♪ ♪ ♪" and fills its whole interval; consecutive ones
  collapse.
- **Word fill** (Enhanced LRC, OpenSubsonic cues, Jellyfin cues): each word sweeps from
  the end of the previous word (or the line start) to its own end, between its start and
  end time; with no end it runs to the next word's start, for the last word to the line's
  end. Between words the fill holds. The fill is the furthest point any started word
  reached, so overlapping singers never pull it back; when all words are over the whole
  line is filled (trailing untimed punctuation included). If the line ends before the word
  starts, or the end is unknown, it gets `OPEN_WORD_MS` so it still shows filling.
- **Backing line**: filled the same way by its own words, smaller, always normal weight
  but italic with the main line (`backing_font`, `FillSlot::Backing`, own measuring
  canvas); without words it stays unfilled.
- **Lingering row**: word timings may outlast the next line's start (held note, duet
  partner early, backing "oh"s). The row before the active one stays lit and filling while
  any main or backing word is unfinished (`sung_until`), then dims; only that one row,
  and never rows without word timing. Fill state is per lit row (`LitRow`, by index), so
  the old row lingers without a blank frame.
- A line's face never depends on it being lit: weight and slant come only from the Bold /
  Italic settings (`lyrics_bold`, `lyrics_italic`; bold is `SEMIBOLD`, backing lines stay
  normal weight). Lighting or dimming changes only the color, so a row never re-wraps or
  changes height when it goes active. A changed `LineFont` (size or style setting)
  re-wraps every line, so the next frame, once layout has the new bounds, re-centers the
  active row (`recenter`).
- **Frames**: repaints are scheduled only while something moves — during a word at its
  pixel rate (at most every `FRAME_MIN_MS`), in a gap exactly when the next word starts,
  never once every lit line is done. With `lyrics_karaoke_fill` off the panel repaints
  only when the active row changes.

## Now playing, footer and progress (`now_playing.rs`, `footer.rs`, `track_progress_slider.rs`)

- `NowPlaying` follows `PlaybackStatus`: `Ready` fills the specs line from the stream
  format (DSD as `DSDn→rate`, no bitrate); `Preparing` or a track change shows the track
  without specs; no track clears it. `CatalogChanged` and a freshly built view read
  `Output::source_format()` and the DSD atomic instead.
- Its text widths come from the viewport, the transport button count and
  `track_progress_slider::row_visible_left`, so text never runs under the transport or
  progress row. `FOOTER_FIXED_W` hard-codes the footer's fixed widths (two 200 px side
  columns, paddings, gaps): change the footer layout and it has to follow.
- The footer cover emits `ToggleCoverModeRequested` only for the first click of a click
  sequence (`click_count() == 1`); otherwise a double click would enter and at once leave
  cover mode. It travels `NowPlaying` → `Footer` → `MainView::toggle_cover_mode`
  (`main_view/doc.md`).
- While a sleep timer is armed the footer draws a moon badge in the volume row, left of
  the volume icon; a click emits `OpenSleepTimerEvent` and `MainView` opens the Tools
  Timer page. It goes with the rest of the footer in cover mode; only countdown mode shows
  text (`sleep_timer/doc.md`). `set_show_queue` / `set_show_lyrics` mirror panel state
  toggled elsewhere (cover mode).
- `TrackProgressSlider` wraps `ui_components::Slider` with `live_update(false)`: the thumb
  follows the pointer while dragging and the single `Change` on release becomes
  `Player::seek`. `PositionChanged` is ignored while no track is ready or
  `Slider::is_interacting()`, so a drag is not fought by playback.
- It follows `PlaybackStatus`: enabled only in `Ready` with a duration; `Preparing`
  shows it at 0, disabled; anything else resets it. Labels are `mm:ss`, no hours.
- `seek_step` (keyboard) accumulates: 5 s for the first three presses in one direction,
  10 s for the fourth, 15 s after. A run ends after `SEEK_RESET` without a press or on a
  direction change, and continues from its own accumulated target, not the live position.

## Cover mode and backdrop (`cover_mode_view.rs`, `cover_backdrop.rs`, `cover_volume.rs`)

- `CoverModeView` loads nothing while cover mode is off (`set_active`). It owns a second
  `TrackProgressSlider`; only its inner slider is drawn, under the caption.
- The cover is two layers: the 320 px thumbnail (`CoverArtCache::get_large`) under the
  full-size image once decoded. Never swap one for the other: the thumbnail keeps the
  square filled during the decode. Full-size images are `RenderImage`s released with
  `drop_atlas_tile`. The box takes the picture's real aspect ratio (square until known).
- Full-size source: a local cover is read from `cover_art.source_path`, falling back to
  the playing track's file when that yields nothing (`load_cover_from_source`). A server
  cover (`<kind>-cover://<source_id>/<key>`) is asked as
  `RemoteMedia::cover(source_id, key, REMOTE_COVER_MAX)`: 2048 px fills a Retina screen
  without a Jellyfin original of several thousand pixels arriving whole. Nothing is
  cached; the DB dedupes covers, so an album is one request.
- Only one server cover request runs at a time (it blocks an executor thread up to the
  HTTP timeouts). A cover asked meanwhile is only remembered (`remote_cover_wanted`) and
  the current one loads when the request ends; covers skipped past are never fetched. If
  the server gives nothing (or `HttpMedia::cover` turned its placeholder into `NotFound`,
  `remote_media/doc.md`), the file fallback runs, else the thumbnail stays with a logged
  warning. With no full layer the view asks again at the next track of the same album.
- With the chrome hidden the view shows the caption (title · artists, clickable), the
  optional progress bar and a control bar that slides in on mouse move and hides after
  `CONTROLS_HIDE_DELAY`, held open while the volume popup is open (`cover_show_*` gate
  artist, progress, controls). The slide honours `App::reduce_motion`. The view's size is
  measured by a canvas whose notify is deferred with `on_next_frame`.
- `CoverBackdrop` bakes the current track's small cover (128 px bytes from
  `CoverArtCache::get_small`, no full decode) into a 96 px blurred, saturated raster on
  the background executor (gpui has no blur). It rebuilds on a track change,
  `CatalogChanged` and a blur setting change (intensity reloads without a fade). `Off`
  removes it; `CoverView` paints it in cover mode, `AllViews` on every screen but
  Settings and Tools. Rasters are released with `drop_atlas_tile`; a crossfade keeps the
  total weight constant (`blend_opacity`).
- `MainView` paints it at the window root; chrome over it gets translucent colors from
  `chrome_bg`, `panel_bg`, `inset_bg`, `field_bg`, `popover_bg`, scaled by the
  interface-opacity setting. `MainView::render` publishes the backdrop's presence
  (`set_active`) because popovers cannot be handed the flag; `veil_factor` is `None`
  without a backdrop. Every new popover or dropdown must use
  `popover_bg(color, veil_factor(cx))`, which never drops below `POPOVER_MIN_OPACITY`: a
  list of devices or playlists must stay readable.
- `CoverVolume` drives the footer's `Volume` entity through a vertical popup slider; the
  wheel nudges by `VOLUME_STEP`, a click outside collapses it.

## Volume (`volume.rs`)

- `Services::volume_locked` (exclusive mode, or casting to a renderer that keeps its own
  volume: `cast/doc.md`, Volume) disables the slider and icon, and `set_volume` returns
  early. While locked the slider is parked at unity (exclusive mode takes the app gain
  out of the signal path); the user's level is kept and returns afterwards.
- `set_volume` goes to the cast device when it owns the volume (not saved), else to
  `Output` and the app gain, and is saved. `effective_volume` is what the slider shows.

## Output picker and devices (`audio_settings.rs`, `bit_perfect_info.rs`, `main.rs`)

- Header controls, left to right: bit-perfect indicator (only in exclusive/native-rate
  mode), exclusive toggle (needs `native_mode_available()` and `show_hog_button`), device
  popover (`show_device_picker`). While casting the first two are hidden: they describe
  the local output. On Linux the toggle is "native sample rate" (`NATIVE_RATE_WORDING`),
  also in the bit-perfect texts. Toggling republishes the remote state (volume lock).
- `AudioSettings::render` is the only place `OutputEvent`s are drained: `Recovered` is a
  warning toast, `Failure` an error toast.
- `show_device_picker` (off by default) decides whether the picker exists. `main` calls
  `Output::set_follow_default(!show_device_picker)` at startup and the Settings switch on
  every toggle, adding `follow_system_default()` when hiding it (drops any pinned
  device). With the picker hidden the output follows the system default, and leaving
  exclusive drops the pin entering it placed (`audio_output/src/doc.md`).
- A device row is checked if its UID is the pinned one, or it is the default and nothing
  is pinned; not while casting. A local row calls `cast::disconnect`, then
  `select_device`; a failure shows on the next render. Devices are enumerated once per
  popup build (on Linux that shells out).
- The "Streaming" section lists `CastState::receivers` while `cast_enabled`. Opening the
  popup calls `cast::start_discovery`, a receiver row `cast::connect`. The active receiver
  has the check and the trigger shows its icon with "Playing on …" (`casting_to`, rebuilt
  on `CastState` change, not in `render`). What happens once chosen: `cast/doc.md`.
- `bit_perfect_info`: tooltip (cached per status) and dialog come from
  `BitPerfectStatus::issues`; `NoSource` alone means "nothing is playing yet", not a
  problem.

## Linux startup gate (`pipewire_alsa_gate.rs`, `main.rs`)

- When `audio_output::pipewire_alsa_missing()` is true (`audio_output/src/doc.md`, "Linux
  and the PipeWire ALSA plugin"), `main` does not start the app: `should_block_startup()`
  is checked before `Services::initialize`, so no engine or output exists, and the gate
  window opens instead.
- It lists install commands per distro (`INSTALL_COMMANDS`, with copy) and a Quit button;
  closing the last window quits. Flatpak is exempt (`/.flatpak-info` exists): the sandbox
  has its own ALSA config and the user cannot fix the host from inside it.
## Scans (`library_service.rs`, `library_watcher.rs`)

- One scan at a time; a request meanwhile sets `pending` and the loop re-runs with the
  latest folders. `ScanIdle` goes out once the `scanning` flag is down, after any
  follow-up scan: the flag drops only after the last result event.
- `request_rescan` is debounced for window activation, the Covers tool and album moves;
  `force` (folder add/remove, launch) starts at once; `manual` (Settings rescan) also
  reports offline folders and answers an unchanged library with `ScanUpToDate`.
  `hold_rescans()` drops the non-forced, non-manual requests.
- Watcher events go through `watched_change`: the scan waits for `WATCH_QUIET` of silence
  and `WATCH_COOLDOWN` since the last scan, so a torrent client or copy yields one scan
  when it stops. A hold is checked on arrival and again when the timer fires.
- `spawn_scan` reads `ScanState.folders`, set only by `request_rescan`, `watched_change`
  and `force_rescan`; scans started by a sync or source change reuse that list. So launch
  calls `reconcile_sources` (drops the fingerprint only), not `apply_remote_sources`:
  the launch scan that follows brings the folder list.
- Offline folders: `ScanScope::probe` marks a folder unavailable when `read_dir` fails,
  its first entry is an error, or it lists empty while `has_media_under` says the
  library has present bindings under it (an unmounted mount point). Others scan as
  usual; the scope reaches `reconcile_local_sources` as `LocalFolder::available`
  (`music_library/src/doc.md`, "Scans"). With every folder offline the scan still runs
  (clear, `finish`): offline means hidden; likes and playlist entries stay, unavailable.
  `ScanFolderUnavailable` (toast) fires on a manual scan or the first time after all
  folders were online (`warned_unavailable`).
- Fast path: no DB work when the available folders' fingerprint equals
  `scan_fingerprint()`, `scan_folders()` equals `ScanScope::folders_key` and
  `has_unplaced_media()` is false. `folders_key` is the sorted list with `?` before
  offline folders, so going offline and coming back each force a real scan. The unplaced
  check heals a migrated database whose bindings sit on the placeholder source: an
  unchanged disk would leave folders empty and missing files unretired.
- `flush` runs whenever the indexer channel is empty, so the scan lock is not held while
  it parses. An indexer that stops before `Complete` drops the session without `finish`
  and sends `ScanFailed`; the fingerprint is stored only by `finalize_rescan` after a
  successful `finish`, so a failed scan is redone.
- App writes to files (tag edits, `.lrc` export) bracket themselves with `ScanBaseline`;
  `rebaseline` advances the fingerprint only if the library matched the disk before the
  write, or it would absorb an unrelated unindexed change and the fast path skip it.

## Library events and the scan indicator (`library_service.rs`, `library_scan_indicator.rs`)

- `CatalogChanged` is the one "re-read what you display" signal: tracks, albums,
  artists, covers or disk lyrics changed. A scan that did work sends it, and so do tag
  saves. Views, the queue (rebuilds every row even for unchanged ids: a cover or artist
  may be new), now-playing, cover skins, the web remote's `library_rev` and queue
  re-mapping listen to it.
- `ScanStarted` / `ScanComplete` are only the lifecycle (`ScanComplete` also fires on
  the fast path); `ScanSucceeded` / `ScanFailed` / `ScanUpToDate` are outcomes.
  `TagsSaved` drives the toast and the cover-cache drop and goes out before
  `CatalogChanged`, so views reload against the cleared cache.
- The indicator is created at startup to see scans and syncs begun before the window
  existed (onboarding, macOS reopen). `main_view/header.rs` places it absolutely left of
  the search field; it is not drawn where search is not (Settings, Tools, cover mode).
- One muted icon: sync arrows while a scan runs or a server syncs, a check for
  `DONE_VISIBLE` after `ScanSucceeded` or manual `ScanUpToDate`; failures stay toasts.
  A sync shows the icon only after `SYNC_SHOW_DELAY`: every launch re-syncs every server
  and an unchanged one ends in milliseconds.

## Library sources in Settings (`library_sources.rs`, `remote_settings.rs`, `*_settings.rs`)

- Folder list from `settings.json` (`music_folders`), status and count from the `sources`
  table, joined by the pure `source_rows` / `server_status`. A folder is Offline (label
  "Unavailable") when its source row says so, even mid-scan; else Scanning during a
  scan, else Online. The count is present bindings, not catalog rows, so an offline
  folder still shows its tracks.
- Rows render every frame, so nothing there touches the DB: the entity caches rows and
  labels and reloads summaries on `CatalogChanged`, `ScanFailed`, `RemoteSyncFinished`
  and any change of the folder or server lists.
- The shared list/form/remove code is in `remote_settings.rs`; each `*_settings.rs`
  holds only its connect. What differs per kind is asked of `ServerKind`
  (`servers/doc.md`). Connect pings first (Jellyfin logs in, a DLNA address is
  described) and saves only a server that answered. Removing calls `ServerClient::forget`.
- Subsonic and Jellyfin rows link "Likes & scrobbling" (`OpenScrobblingSettings`): the
  favorites import lives on that tab and reports through `RemoteStarsImported`.
- Jellyfin: `new_device_id` mints a fresh id per login, i.e. per added server: tokens
  are tied to the device id and a new login may revoke the older ones. It is stored
  with the token and user id; the password is sent once, to log in.
- DLNA: "Search the network" lists discovered devices not added yet (`dlna_unadded`, by
  UDN); an address field adds a server discovery cannot reach (VPN, another subnet,
  filtered multicast) through `servers::describe_dlna`.
- Torrents: until `torrents_enabled` the group is only an "Enable torrents" switch whose
  dialog warns that sharing copyrighted music is illegal in some countries. The flag is
  a one-time acknowledgement with no way back; a settings file already listing torrent
  sources loads as enabled (`migrate_torrents`). With it off no engine exists.
  Idle torrents unload after `IDLE_UNLOAD`: a track downloads far faster than it plays,
  so the clock starts early, and the next-track prefetch reloads the torrent.
- A torrent row shows `connected/known` peers while loaded: `LibrarySources` polls
  `ServerClient::peers` every `PEERS_POLL`, only while a source whose kind has peers
  is configured. Torrents have no manual Sync button.
- The network cache has its own group (`cache_settings.rs`), not a row in a server
  group, because one cache serves every network source: size (recounted whenever
  Settings opens), Clear, limit (1–32 GB or Unlimited, default 8). The limit is applied
  at launch (`Services::initialize`) and by `apply_cache_limit` on change; Clear and
  trimming act on `RemoteMedia` (`remote_media/doc.md`); `CacheFill::cache_changed`
  makes track rows re-check.

## Server sync (`remote_settings.rs`, `remote_sync.rs`, `library_service.rs`)

- Ping → full listing → `apply_remote_listing` with the covers already known → covers
  not seen before → `apply_remote_listing` again with them. The listing goes in first
  so a big first sync shows its songs at once; a changing listing is rescanned before
  the covers start, so new songs plus covers rescan twice. Adapters clean the listing
  on the way in (`servers/doc.md`, "RemoteSong").
- A song whose cover key has no picture yet is written with `cover_key = NULL` and the
  cover its row already shows (`remote_song_cover_hashes`): a re-keyed album keeps its
  art meanwhile. Nothing of an unfinished cover phase survives the session; a key that
  gave no usable picture is asked again every sync.
- `fetch_covers`: `cover-fetch` threads only wait on the network (`cover_art` at the
  large thumbnail size, so the server scales it); hashing and thumbnailing stay on the
  sync thread, which sends `RemoteSyncProgress` about once a second. Eight
  (`COVER_UNREACHABLE_LIMIT`) transport or auth failures in a row end the phase; any
  other answer resets the count.
- A failed ping or listing, or one the library refuses (empty while it had songs), marks
  the source unavailable and changes nothing else. Failing to *store* a listing is our
  own busy database: retried, never marks the source unavailable.
- Requests arriving mid-sync are queued one per server (a probe request is dropped when
  the server is already queued, any other replaces the entry); one thread drains the
  queue and re-checks it after clearing `running`, so a late request is not stranded.
  `syncs_alone` kinds (torrents) skip the queue: one thread per key. The "running" marks
  are `Claim` drop guards, so a panic in an index leaves nothing stuck as syncing.
- Only a sync that changed something (songs, tags, availability) drops the scan
  fingerprint and rescans; otherwise a launch keeps the local fast path. The launch
  scan also runs when only servers are configured, re-projecting a server-only library
  from the cache.
- Adding, removing or re-adding a server flips its `sources` row, which no listing
  reports (a re-added server with unchanged songs syncs as "nothing changed"), so
  `apply_remote_sources` rescans whenever `reconcile_remote_sources` says a row flipped.
- Servers sync at launch and on demand. An unavailable one is retried every
  `OFFLINE_RETRY` and on window activation (`sync_offline`, skipped while a sync runs),
  pinging first so a dead server shows no "Syncing" flicker. Online servers are not
  re-listed on activation.

## Server playlist import (`remote_sync::import_playlists`)

- A one-time copy: nothing is sent back and no sync touches the result again. Song keys
  become library items through their bindings (`items_by_remote_key`), so a song that is
  also a local file is that file's item and plays from disk.
- A server playlist goes into the oldest local playlist of the same name, or a new one,
  through `add_tracks_to_playlist`, which only appends what is missing: re-importing
  adds new server tracks and anything removed locally, never removes or reorders. Equal
  names (two discs' `.m3u`) land in one playlist; a repeated track is kept once.
- A playlist none of whose songs is in the library creates nothing. The result counts
  playlists created or given a track (a no-op re-import says 0), tracks actually added,
  and tracks found out of all (once per playlist). `PlaylistTracksChanged` goes out for
  the changed playlists.
- `RemoteError::NotSynced`: the source has no enabled row to map keys through. Imports
  run one at a time (`playlist_imports`), or two servers would each create the same new
  name. `import_playlists_unless_syncing` refuses with `Syncing` while the server's key
  is in `active`.

## Tag editing (`tag_editor_view.rs`, `library_service.rs`)

- Album and album artist are locked in the per-track modal: `albums` is keyed on the
  title and split on the album artist, so editing them from a track with siblings
  re-keys or splits the album. They unlock in the album editor, or for a track in no
  album. Year is a track field (the album shows its newest track year) and is editable
  in both. The album editor shows an album whose tracks disagree on the year with an
  empty field and the range as placeholder, and writes the year only if the field was
  changed (`AlbumTagEdits::year` `None`): otherwise one save would stamp the first
  track's year onto every song of a compilation.
- Read-only: a cue track, a file that is the current queue track (checked on open and
  on save), an album with no own files. Server tracks get no editor; the album save
  skips server and cue tracks (`Track::own_file`).
- Point update (`update_track_tags` / `update_album_tags`): `tag_writer::write_metadata`
  → `reindex_one` (re-reads through `read_metadata`, so normalization stays in one
  place) → `settle_derived_rows` → `TagsSaved`, `CatalogChanged`. A refusal (cue or
  unknown track) or failed write leaves file and DB untouched (a write failure toasts);
  a failed re-index, or an album write that stops midway, forces a rescan. The album
  save skips files already equal (an untouched mtime keeps the watcher quiet) and
  overwrites every track's genres.
- `settle_derived_rows` (album regroup, covers, years, album artists, item snapshots,
  orphan sweep) is the one place scan and point update both call: the point update is
  only an optimisation over a rescan, so a step one skips shows up as a library that
  changes when re-indexed. A renamed album's new row is born coverless; the re-resolve fills
  it. Derivation rules: `music_library/src/doc.md`. Tests apply an edit through
  `apply_track_tags` / `apply_album_tags`, rescan with the real pipeline and compare
  `snapshot()` (every column, so forgotten ones show up).
- Placeholder credits are dropped at ingest: `to_scan_track` and `reindex_one` pass both
  name lists through `clean_artist_names` (trim, drop empty and `is_placeholder_artist`).
  Never move that into `read_metadata`: it prefills this modal, and saving any other
  field would write the emptied artist back.
- The cover row shows the file's own embedded picture, never the library's cover (which
  may come from a `cover.jpg` beside the file), or Remove would look like it did
  something. Empty when none, and Remove is hidden. Setting one is still offered and
  outranks the folder image.
- Cover editing is offered in both modals: the album's cover is derived from its tracks,
  so it cannot re-key anything. The album editor writes the same picture into every
  file, verbatim and unresized (the modal shows its size). `reindex_one` sets
  `tracks.cover_art_id` through `set_track_cover`, because `upsert_track` COALESCEs it.
  The format is checked at pick time (`cover_bytes_ok`): the writer runs after the
  dialog closed, and a refusal there would drop every other edit. The preview is a
  `RenderImage` freed through `drop_atlas_tile`.
- Raw tags and custom rows are track-only; Save diffs them against the rows as loaded
  (`diff_raw_tags`); adding needs `CustomTags::supported`.

## Cover cache (`cover_art_cache.rs`)

- `CoverArtCache` (`Services::cover_art_cache`): `small` holds the stored thumbnail's
  JPEG bytes as `Arc<Image>`, unbounded (list rows; `cover_backdrop::from_thumbnail`
  needs the undecoded bytes). `large` is a `TileLru<i64>` of decoded `Arc<RenderImage>`
  for the grids, album header and cover mode.
- Large covers are decoded by us and bounded because `img(Arc<Image>)` makes gpui keep
  the decode and its atlas tile forever. Eviction frees the tile via `drop_atlas_tile`,
  which must defer: `drop_image` inside a window update skips the window being updated.
- `TileLru<K>` is generic: artist photos (keyed by name) get the same bound.
- `fit_capacity(visible)` keeps `LARGE_COVER_SCREENS` screens of the largest span ever
  asked, clamped and always above the visible count (below it the cache evicts what is
  on screen and thrashes). It only grows: the virtual list's measuring pass asks for one
  item every frame.
- Eviction takes the least recently used entry nobody else holds (`strong_count == 1`);
  with all held the cache stays over capacity. `insert` keeps an existing entry, or a
  racing decode would drop a tile still painted. `touch` (`get_large`, `peek_large`)
  updates recency; `get` and `holds` don't. `render_tile` swaps R and B: BGRA.
- Cleared on `TagsSaved` (`services.rs`) and `CatalogChanged` (the Albums tab).

## Artist cards and photos (`artist_card.rs`, `artist_photos.rs`)

`ArtistCards` (`Services::artist_cards`) indexes artists into the `artist_info` table
(`artist_info` crate: MusicBrainz facts + Deezer photo), builds the artist-page card and
keeps the Artists tab's bounded photo stores.

- Indexing runs at launch, on `ScanIdle` and `CatalogChanged`: every artist from
  `artists_without_info()`, then every row from `artists_pending_photo()`, one at a time
  on the background executor. A trigger during a run sets `index_again`.
- Never during a scan: mid-scan an artist has part of their tracks and a "not found"
  saved then would not be retried. `index` checks `is_scanning()`, and every step checks
  again *after* reading the titles (the flag is set before `clear()`, so a read that saw
  a partial catalog always sees it). `ScanIdle` resumes indexing.
- Rows are keyed by artist name and written once: a found row is never refreshed; a
  "not found" row is looked up again only after one of the artist's tracks changed
  since its `fetched_at` (when the titles were read).
- A transport error ends the run (offline: one request per trigger); other errors skip
  the artist unsaved. A failed photo download saves the facts with `photo_pending`;
  photo steps retry Deezer until a photo arrives, Deezer has none, or it answers a 4xx
  other than 429.
- Privacy switch `artist_info_from_internet` (default on): off means no runs (checked
  before each step; a request in flight finishes), `card` / `thumb` / `photo` return
  `None` and the photo stores are cleared; rows and card slots stay, so turning it on
  shows cards at once. What leaves the machine: artist names (MusicBrainz search; album
  and track titles are compared locally) and the Deezer photo download.
- Card: `load(name)` reads the row and decodes the photo off-thread; slots live for the
  session. After the indexer saves an artist, `refresh` re-reads the slot in place (the
  old card stays until the new one arrives) and frees the replaced photo's tile. A
  language change only rebuilds text (`relabel`). Lines: years · places, top three
  genres, members or member-of (MusicBrainz's English names). `card_header` draws it.
- Photo stores `thumbs` (list) and `photos` (grid) are `PhotoTiles`: a `TileLru<String>`
  plus `in_flight` / `missing` / `stale` / `generation`, separate from the cards because
  cards live all session while list photos must stay bounded. `ensure_*` touches held
  names, skips loading or known-missing ones, loads the rest one by one. `refresh`
  forgets a missing name; one in flight is marked stale so a read from before the save
  cannot mark it missing. Privacy off clears both and bumps `generation`; a running
  load stops at the first mismatch.

## Saving albums to the cache (`cache_fill.rs`)

- `plan` lists the remote files not yet cached (`missing_files`; a cue image once) with
  sizes from the library (unknown counts as 0). `CacheFill` fetches them one at a time
  through `RemoteMedia::resolve`, the path playback uses, so one download never starves
  the playing track. Targets are an album or an artist; a second request for a running
  target cancels it.
- Over `network_cache_bytes`, a dialog offers to save only the files that fit, in album
  order (`fitting_prefix`; disabled when none fits); otherwise the LRU makes room
  silently. Saved tracks are ordinary cache entries, evictable: no pinning. "Unlimited"
  is a 100 TB limit (`UNLIMITED_CACHE_GB`), so no code special-cases it.

## Moving an album to a music folder (`album_export.rs`)

- Moves an album's server tracks out of the cache into a music folder: one folder is
  used, several ask, none hides the button (a file outside the library folders would
  leave the cache unscanned and the album, with its likes, would vanish). Only the album
  header has it; per artist it would depend on the grouping.
- Names come from tags (`Album Artist/Year - Album/NN - Title.ext`, `D-NN - …` for
  several discs): cache names are digests and the server's paths are not stored. A cue
  image gets a generated `.cue`; `cue_time` rounds back to the exact frames the scan
  derived, so local tracks start at the same offsets. A taken name gets ` (n)`; a target
  of the file's size counts as done, so a stopped move restarts. `cover.jpg` is written
  only when the folder has no image.
- Each file goes through `RemoteMedia::resolve` and leaves the cache at once (rename,
  else copy + delete across volumes), so the LRU cannot evict one before it moves. The
  plan is built on the background executor, not in the click handler.
- Then a full rescan (the catalog has no per-folder refresh): the moved file has the
  server binding's size and offset, so adoption joins it to the same item by the `file`
  tier (`music_library/src/doc.md`); likes, playlists and history stay and the local
  copy plays first. While moving it holds `hold_rescans()` (each landing file would
  trigger a rescan), released right before the move's own rescan.
- Right after a move `AlbumExport::is_done` hides the album header's cache and move
  buttons until the next `CatalogChanged`. Cancel ends quietly, without an error toast.

## Web remote reader and lyrics access (`library_service.rs`)

- `LibraryAccess` implements `pawse_remote::LibraryReader`. Albums and artists sort with
  `view_order`, like the desktop tabs. The artist grouping is a shared `AtomicU8`
  (`set_artists_grouping`), so flipping the setting re-points the remote without
  rebuilding it. `artist_detail` marks partial albums and `full` expands them
  (`artist_display_tracks`, as on the desktop artist page). Covers over `WEB_COVER_MAX`
  are re-encoded as JPEG.
- `LyricsAccess` gates LRCLIB writes by epoch: `forget_fetched` bumps `fetch_epoch` at
  once (UI thread) and deletes on the executor; `save_fetched` / `mark_not_found` check
  the epoch under the lock the delete takes, so a lookup in flight cannot write after a
  forget. The request is not cancelled, only its result.

## Engine-event bridges (`scrobble_bridge.rs`, `discord_bridge.rs`, `media_bridge.rs`)

- All three subscribe to `EngineEventsBus` and read the `Services` atomics
  (`is_playing`, `current_position_ms`, `current_duration_ms`). `run_engine_events_bus`
  stores them before it re-emits an event, so they are never behind the event being
  handled.
- `Loaded` is also what a gapless advance emits (no `Playing` follows) and what a paused
  load or the launch restore emits. Scrobbling and Discord therefore start on `Loaded`
  only when `is_playing` is true, else they wait for `Playing`; dropping the `Loaded`
  start would stop scrobbling every auto-advanced track. Discord and the media bridge
  treat `TrackEnded` / `Stopped` as the end only when the queue has no current track.
  The media bridge does not look at `is_playing`: its `Loaded` publishes Playing at 0.

## Scrobbling

Engine, retry, rewrite rules: `scrobble/src/doc.md`; tables: `music_library/src/doc.md`.

- One `ScrobbleHandle` exists from launch whatever is configured, so the history is
  recorded with no service set up; only `qualified` (hence any delivery row) needs a
  configured target (`active`). `HISTORY_MIN_SECS` keeps click-throughs out of the
  history and is deliberately far below the scrobble threshold: a track started and
  skipped is signal worth keeping.
- Metadata is captured at `Loaded` from the queue's current track: artist through
  `scrobble::primary_artist` (first credited artist or all joined, per
  `first_artist_only`, default on), album, first album artist, track number and id. A
  track without artist or title is ignored. The timestamp is the first `Playing`; a
  track that never played records nothing.
- Commits: `Paused` commits only if the play already qualifies (an earlier pause leaves
  the track able to qualify later; a scrobble survives a crash while paused). `Loaded`
  of the next track, `TrackEnded`, `Stopped` and quit (`finalize_on_quit`: `persist`
  without a flush, delivery waits for the next launch) commit what is left. Both commits
  hit the same row; the store raises `played_secs`.
- DB writes never run on the UI thread: `commit_play` spawns on the background executor,
  and likes go over a channel to one writer task (`spawn_love_writer`); a task per click
  could commit a quick like then unlike in reverse and leave the server starred while
  the track is unliked here.
- Only `LibraryEvent::TrackLikedChanged`, and only while a target is active, becomes a
  love. `LikesImported` (the imports below, all through `LibraryService::like_many`) is
  ignored, so an import never bounces back as a `track.love`.
- `build_targets` makes the list from settings: each service switched on with its
  credential, plus a `ServerTarget` per Subsonic / Jellyfin server with plays or likes
  on. `apply_settings` rebuilds it, calls `ScrobbleHandle::configure` and clears the
  failure memory and `auth_failed`. It runs after every Scrobbling edit
  (`update_scrobble`), after a server is added or removed (`remote_settings`), and once
  more in `open_main_window` after `reconcile_sources`: at `setup` a server whose
  `sources` row does not exist yet has no id. `apply_rewrite` swaps only the `Rewriter`;
  a custom rule that fails to compile is logged and skipped.
- `StatusEvent`s become the `ScrobbleStatus` entity (`auth_failed`; Settings rows say
  "sign in again") and a notification via `error_bridge::push_background_error`, titled
  by target (a server: kind and address). The text is remembered per target, so a
  rejection repeating every backoff window notifies once; `apply_settings` clears the
  memory.
- Old queue: `setup` imports `<config>/pawse/scrobble_queue.json` once (v1 bare array of
  Last.fm plays, v2 `{items: [{event, targets}]}`). The file is renamed to
  `.json.imported` before anything is inserted, and the import gives up if the rename
  fails: inserting first would re-import everything on each launch. A non-empty file
  that parses to nothing becomes `.json.unreadable`, so "imported" never names a dropped
  file; a malformed entry costs only itself. Imported plays have NULL `played_secs`:
  guessing would poison the history.
- `scrobble_store.rs` maps types between `scrobble` and `LibraryRepository` and nothing
  else; `TargetId::key()` strings are the `target` values.

### Media servers as targets (`server_scrobble.rs`)

- A Subsonic / Jellyfin server switched on in Settings → Scrobbling → Media servers
  (`ScrobbleSettings::servers`, keyed `kind:uri`; plays and likes separately) is
  `TargetId::Server(source_id)`, stored as `server:<id>`. `ServerTarget` lives in the
  app because it needs the library and `ServerClient`.
- The server wants its own song id: the target resolves the item's present binding
  (`LibraryRepository::remote_key_for_item`). No binding (a local-only file, a song that
  is gone) is `Unsupported`, settled as dropped. A local file the server also has is
  reported: same library item.
- `ServerKind::reports_plays` (Subsonic only) and `sends_favorites` (Subsonic, Jellyfin)
  decide the row's switches and what the target accepts; a server with only likes on
  gets no `play_deliveries` rows. DLNA and torrents get no target. With plays on,
  Subsonic also gets Now Playing.
- Errors keep the usual split, except `NotFound` (Subsonic error 70, Jellyfin favorite
  404): `Unsupported`, dropped quietly, the next sync refreshes the binding.

### Loved-tracks import (`scrobble_import.rs`)

- `start` pages the loved tracks of Last.fm / Libre.fm or ListenBrainz feedback (200 per
  page, at most 100 pages) on a background task, matches them by `normalize_tag(artist)`
  + `normalize_tag(title)` over every credited artist of every track, and likes the
  unliked matches through `like_many`.
- The index holds each track under its own pair and, when the current rules change them,
  under the rewritten pair too (`Rewriter::artist_title`), so a love that went out as
  `Let It Be` finds `Let It Be (Remastered 2009)`. A server's stars import is another
  path (`LibraryService::import_remote_stars`);
  `scrobble_settings::watch_server_imports` shows its result.

### Scrobbling tab (`scrobble_settings.rs`, `scrobble_rules_settings.rs`)

- One `SettingGroup` per target. The rule groups come first, because rules are what
  people come back to adjust while an account is connected once; then Last.fm,
  ListenBrainz, Libre.fm, the CSV log, Media servers (`MEDIA_SERVERS_ANCHOR`, which the
  server rows in Library link to). "First artist only" lives in the rules group, as one
  more way of shaping what gets sent.
- Every edit goes through `update_scrobble` (save, then `apply_settings`). A target's
  Enabled switch is disabled, and shows off, until its credential exists; signing in or
  choosing a file turns it on. Last.fm shows "unavailable" without an API key.
- Sign-in (`start_web_sign_in`): the cancel flag is checked again right before the
  session is applied. Cancel cannot abort a `get_session` in flight, and a session
  arriving after it must be discarded rather than silently sign the account in.
  `TokenExpired`, `TimedOut`, `NotAuthorized` and `Denied` show one localized "link
  expired, sign in again". ListenBrainz validates the token before saving it; the CSV
  log takes a new file or one passing `scrobble::is_pawse_log`.
- Rule previews: one `Sample` per track (the artist as the bridge would send it) is
  loaded on a background task the first time the page renders, and dropped on
  `CatalogChanged` / `TagsSaved` and when "first artist only" is toggled. The draft
  rule's preview is debounced.
- A preview line is `before → after` with a character-level diff (LCS; prefix / suffix
  only past 250k cells) as `theme.danger` / `theme.success` backgrounds: a one-character
  fix (`’` → `'`, a dropped double space) is invisible in a small proportional font
  otherwise. Zero-width characters and NBSP show as `·` on the before side so there is
  something to highlight. Diffs are built with the previews, not per frame.
- In regex mode the form shows a fixed example (`^(.+), (.+)$` → `$2 $1`), not one from
  the library, so it works with an empty library; a test runs it through the engine so
  it cannot drift from the rule.

### Flow tests (`scrobble_flow_tests.rs`)

- Drives the bridge's own `captured_meta` / `love_meta` / `pending_play` / `deliver`
  over real parts: a temp `SqliteLibrary`, `LibraryScrobbleStore`, `ScrobbleHandle` with
  its worker thread, `CsvLog`, `ServerTarget` over a fake `ServerClient`. Only the
  internet services are stand-ins; settings are parsed from JSON as `settings.json`
  would be.
- The scenarios pin bugs at the seams: rules missing from the startup flush after a
  restart, a like importing back under the wrong name, the settings preview disagreeing
  with delivery, rules switched on while items are queued.

## Discord presence (`discord_bridge.rs`)

- `DiscordHandle` (`discord/src/doc.md`) is spawned at launch only when a client id
  exists; everything is gated on `discord_enabled` (`set_enabled(true)` seeds from the
  current track and position, `false` clears). A `Presence` is published only on real
  changes: `Loaded` (when playing), `Playing` (anchored: `started_at` = now − position)
  and a position drifting from the anchored expectation by more than `SEEK_THRESHOLD`,
  i.e. a seek. Position ticks are not forwarded.
- `Paused`, `Error` and `TrackEnded` / `Stopped` with no current track clear the status.
  The artist is the track's artists joined with ", " (also the art-cache key). Quit
  sends `Shutdown`. The Settings → General switch is built only when
  `discord::is_available()`.

## OS media controls (`media_bridge.rs`)

- Wires `media_integration` (its `doc.md`) to the engine bus. On macOS `setup` runs once
  at launch and the integration is app-lived (audio keeps running with no window). On
  Windows and Linux a `MediaBridge` entity owned by `MainView` creates it (with the
  window's `hwnd` for SMTC) and it dies with the window, which is when those platforms
  quit.
- Inbound `MediaCommand`s arrive on a `flume` channel and a GPUI task maps them to
  `services::*` playback calls. Outbound, the bridge seeds itself from `Services`;
  `Loaded` publishes the track (artists joined with ", ", cover path); `TrackEnded` /
  `Stopped` set Stopped only with no current track; `PositionChanged` is ignored while
  Stopped.
- The cover path comes from `LibraryService::get_cover_art_path_for_media`, which writes
  `<cache_dir>/pawse/artwork/<cover id>.jpg` (and deletes the other `.jpg` files there),
  never the temp dir: the OS reads the file from outside our process, and inside a
  Flatpak `/tmp` is the sandbox's private tmpfs.

## Error notices (`error_bridge.rs`)

- `diagnostics::notify_error` / `notify_warning` (any crate, any thread) log and push a
  `Notice`; `spawn_notice_forwarder` (started in `main` before the settings load) drains
  it on the foreground executor into an error or warning notification on the active
  window. No active window: the notice is dropped, its log line is already in
  `pawse.log`.
- `push_background_error` is for errors that do not come through `diagnostics` (today
  `scrobble_bridge::report_failure`): active window or else the first, no auto-hide, and
  it returns `false` with no window, so the caller can try again later instead of
  recording the message as shown.

## Decisions

Owner's calls; ask before changing.

- Credentials sit in plain text in `settings.json`: the Subsonic password next to the
  scrobbling sessions and tokens; Jellyfin keeps only a token.
- Torrents stay off until the user confirms the warning; the flag is a one-time
  acknowledgement, not a switch with an off position.
- Artist info: no time-based refresh of found rows, no Wikipedia text, no on-demand
  loading on the card.
- Playlist import is a one-time copy that never removes or reorders. A server queued
  behind another server's sync is not in `active`, so its import is not refused (a
  "0 of N" result); accepted, do not fix unasked.
- The scan indicator may blink off between a sync and its rescan; a hold-over delay at
  launch was worse.
- An unfinished cover phase is not persisted (no table or migration for resuming it).
- A missing `pipewire-alsa` plugin blocks startup with install commands, not a fallback
  to raw ALSA cards (`audio_output/src/doc.md`, Decisions).
- The sleep-timer badge lives in the footer volume row in every layout, not in the title
  bar or beside the search field.
- The bit-perfect check/warning button and the exclusive-mode button keep their icons;
  redesigns of them were tried and rejected.
- No `App::reduce_motion` guard on the cover colour crossfade or the backdrop crossfade
  (`cover_skin/doc.md`, Decisions); the cover-mode controls slide does honour it.
- Double counting of server plays is the user's call: a server that forwards plays to
  Last.fm / ListenBrainz itself (Navidrome can) plus the same service on here counts
  every play twice, and neither API tells us. Both server switches are off by default
  and the settings text is one sentence, no warning.
- Space as play / pause stays bound broadly, not gated to root focus, so a focused
  `gpui_component::Button` also gets a synthesized click on the same press. Accepted; do
  not "fix" it without asking.
- No Telegram now-playing presence: Telegram has no presence API, and the only route is
  a userbot holding a full-account session, which we will not ask users for. Do not
  prototype it.
- Lyrics lines all share one face: the active line is not made bolder (it re-wrapped
  lines that only just fit). Bold and italic are toggle buttons left of the size slider;
  bold is on by default, italic off.

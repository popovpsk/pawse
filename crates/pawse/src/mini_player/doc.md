# mini_player

The mini player: a small window that replaces the main one, with the cover, title and
artists, lyrics, ⏮ ⏯ ⏭, like, volume, progress and the lyrics and queue toggles. Its own
window rather than a narrow main window, because gpui fixes a window's minimum size when
it creates the window and the main layout needs 900 px. While it is open the main window
does not exist; going back builds a new `MainView` on the last screen.

Not here: keeping a window above others and its maximum size (`crates/window_level`),
window placement (`window_placement.rs`, `pawse/src/doc.md`), the last screen a new
`MainView` restores (`main_view/doc.md`), the transport buttons, `TrackProgressSlider`,
`Volume`, `CoverVolume`, `QueueView` and `PlaylistPopup` it hosts (`pawse/src/doc.md`).

## Files

- `mod.rs` — `toggle` / `enter` / `leave` / `open`, the window options and size limits,
  `MiniState` (the open mini window, if any, whether the mini player is the current mode,
  the lyrics toggle); `main_window` in `main.rs` is every other window.
- `view.rs` — `MiniPlayer`: the two layouts, the queue layer, the hover chrome, dragging,
  the pin, the shortcuts.
- `lyrics_line.rs` — `LyricsLine`: the current track's synced lyrics, the active row and
  the rows around it.

## Switching windows

- Every entry point (footer button, `ToggleMiniPlayer` from the macOS Window menu and the
  shortcut, the chrome's "full player") goes through `cx.defer`: an action handler runs
  inside a window update, and gpui takes that window out of its map until the update
  ends, so it could not be removed or read from there.
- The new window opens first and the old one is removed after, in a spawned task. With
  zero windows in between, Windows and Linux would quit or park in the tray. The task is
  queued behind the platform's focus-change callback for the old window, so that callback
  still finds the window (removed at once, gpui logs "window not found").
- Refused until onboarding is complete.
- `MiniState::current` says which mode the user is in: set on the way into the mini player,
  cleared on the way back. Closing the mini window (its ×, the OS) is closing the last
  window: the app stays on macOS and parks in the tray or quits elsewhere, and the window
  that comes back (Dock, tray Show, second launch, all through `open_initial_window`) is
  the mini one while `current` holds. Memory only, like the last screen: a new launch
  opens the main window. The tray's Show and a second launch raise whichever window
  exists.
- On Windows the media controls (SMTC) are bound to a window handle; `MiniPlayer::new`
  rebinds them, as `MainView::new` does on the way back, before the old window goes.
- Toasts are for the main window only (`main_window`, `active_main_window`): in the mini
  player a notice is only logged, it would not fit.
- The first open sits in the bottom-right corner of the main window's display; after
  that `window_placement` restores the mini window's own slot (`mini_window` in
  `settings.json`), its size clamped to `max_size`. A maximized mini window is not
  recorded.

## Size

- Minimum `MIN_W`×`MIN_H` (the strip's controls need the width), maximum 520×520: past
  that the layouts are only empty space. gpui has no maximum, so `size_limits` goes to
  `window_level::set_size_limits` on the first frame; Wayland has none.
- Sizes come from the viewport minus `window_paddings`: with client-side decorations
  (Linux) the viewport includes `Root`'s shadow and border. Before the window exists the
  options add a fixed `FRAME` for it (minimum, first size, `max_size` for a restored
  size); `size_limits` measures the real frame, which is zero when an X11 desktop without
  a compositor gives server decorations.

## Layout

- Under `COVER_LAYOUT_MIN_H` tall: the strip. Cover on the left (the 320 px tile, as tall
  as the strip unless the controls need the width), then title, artists, the lyric line,
  the controls row (transport, like, volume, lyrics and queue toggles) and progress.
  On the cover the hover row is like on the left, transport in the middle, lyrics and
  queue toggles and volume on the right.
  Taller: the cover fills the window (full-size image over the tile, as in cover mode,
  loaded only while this layout shows; letterboxed by the theme background), with three
  lyric rows and the progress bar always visible and the rest on hover.
- When space runs out, detail goes rather than getting clipped: the lyric line in a
  short strip, the volume in a narrow one.
- Title and artists `truncate` as in the footer. Lyric rows are sentences in any script,
  so they wrap and are clipped at their line count instead of taking an ellipsis.
- The three lyric rows (previous, current, next) slide up and fade in on every row
  change, keyed by the active row. Rows can wrap, so the slide distance is one line, not
  a measured row. With the hover panel up in a window near `COVER_LAYOUT_MIN_H` only the
  current row stays, or the stack would push the title off the top.
- The queue toggle lays `QueueView` over the whole window (slide and fade both ways).
  `QueueView::for_mini_player` drops Save to playlist and Clear queue from its header and
  puts a lit queue icon where the main panel has none; an × there would sit where the
  chrome's × closes the window. When the window leaves room under the queue header for
  fewer than `MINI_ARTIST_ROWS` two-line rows, the rows drop the artist line even if the
  queue setting shows it; the room is the window height, since the layer fills it. The
  view is created on the
  first open. A `PlaylistPopup` of its own serves the queue's add-to-playlist buttons; its
  list shrinks to the window height (`LIST_ROOM`), so in a short strip it scrolls instead of
  running off the bottom.
- The lyrics toggle hides the lyrics in both layouts and stops `LyricsLine` loading
  (no web search while off). Kept in `MiniState` for the session.
- Controls are `occlude`d so a press on them is not a window drag. That also takes hover
  away from everything behind them, so hover is read from an empty full-size layer
  painted on top, which blocks nothing. The layer's hover is set by mouse moves outside a
  drag and by a left mouse-up, and cleared by `on_hover(false)`; gpui's `on_hover(true)`
  is ignored. After the pointer leaves, gpui keeps its last position inside, so the next
  paint finds the full-size layer hovered again; only an activation change re-reads the
  real position. gpui reports hover lost when a drag (the progress slider, the queue
  rows) starts and sends nothing more if the drag ends outside the window, so moves
  during one do not count and a mouse-up inside brings the hover back. A pointer resting
  on a window that just opened counts once it moves. The overlay stays while the volume popup is open
  or the progress is dragged, since a slider drag makes gpui report hover lost. Windows
  sends no mouse-exit to elements, so there hover also needs `is_window_hovered`.
- Hover goes through a delay (`SHOW_DELAY` in, `HIDE_DELAY` out): while the window is
  resized the pointer sits on its edge and crosses in and out every frame.
- The open volume popup sits outside its 34 px slot; an occluding block behind it keeps
  that area out of the Windows drag region, or a slider drag would move the window.

## Window

- Drag from any empty spot: press then move calls `start_window_move` on macOS and
  Linux; on Windows the root is a `WindowControlArea::Drag`, which gpui answers as the
  caption, so a double-click there maximizes the window (up to the maximum size).
- The macOS traffic lights are moved off-window at creation; the chrome's ×, "full
  player" and pin replace them. The chrome is hidden while the queue is open.
- Keyboard: the root has the `MiniPlayer` key context and a zero-size element holds
  focus. A focused root would `prevent_default` every mouse-down, which kills the drag
  area on Windows. Focus is set again whenever the window activates and whenever the
  focused element leaves the tree (a closed add-to-playlist popup), or key dispatch falls
  back to the window root, outside the `MiniPlayer` context. The shared actions
  (space, arrows, ⌘/Ctrl+arrows) are bound for `MiniPlayer && !Input` too.
- The pin and the maximum size call `window_level` on the first frame (X11 ignores a
  window that is not mapped yet); the pin again on click. It is hidden where
  `window_level::supported` is false (Wayland).

## Lyrics

- Reads what the web remote reads: `LibraryReader::lyrics` with the remote media attached
  (best stored variant or the server's, on `net_runtime` because a server answer is a
  network call). Only synced lyrics give rows. When that answer says a web search could
  help, it runs the same search the remote's Find button runs (`services::find_lyrics`);
  a source the user picked in the lyrics panel does not carry over.
- Reloads on a track change, `LyricsChanged` for the current track and a change of the
  two lyrics settings; repaints only when the active row changes.

## Decisions

- Instead of the main window, not next to it.
- Adaptive layout (strip / cover) chosen by the window's height, no setting.
- Like, volume, lyrics and queue besides the transport; no shuffle / repeat. The queue is
  a full-window layer, the lyrics three rows on the cover and one in the strip.
- In a short window the queue rows go without the artist line so more of them fit.
- Entry: a footer button left of the lyrics toggle, ⌥⌘M / Ctrl+Shift+M, the macOS Window
  menu.
- The window can't be stretched past 520×520.
- Closing and reopening the window keeps the mini player within a session; a new launch
  opens the main window.
- Keep on top is on by default and remembered (`mini_player_on_top`); it only changes the
  window level, so on macOS it does not follow into other apps' full-screen Spaces.

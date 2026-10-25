# main_view

Child modules of `main_view.rs`, the root view: the header (tabs, search field, view menu
and the right-hand buttons), the Full screen stage and the screen a reopened window
returns to. The header renders with a
`Context<MainView>`, because its buttons call `MainView`'s private methods and fields
(`open_settings`, `leave_overlays`, `toggle_cover_mode`, `library_view`, …); `HeaderHost`
is only the entity it is drawn under, so the header can be a cached block.

Not here: the rest of the root view — screens, side panels (`side_panel`), footer, key
actions (`main_view.rs`, `pawse/src/doc.md`, "Cover mode and backdrop" for what Full
screen looks like); the title bar widget (`window_title_bar.rs`);
the view-options menu (`library_views/doc.md`); the library indicator (`pawse/src/doc.md`,
"Library events"); the sleep-timer badge, which is in the footer (`sleep_timer/doc.md`).

## Files

- `header.rs` — `HeaderState`, `HeaderParts`, `HeaderHost`, `MainView::header_block`,
  `render_header`, `Placement`, `HEIGHT` (standalone bar) and `TITLE_BAR_HEIGHT` (bar when
  the header lives in the title bar), both scaled by the UI font scale, and the button
  helpers only the header uses.
- `last_screen.rs` — `LastScreen` (global), `MainView::remember_screen` /
  `restore_screen`.
- `stage.rs` — `Stage`: everything Full screen cover mode changes in the root view,
  computed once per render from cover mode, the cover view and the backdrop (title bar
  background, the bare lyrics panel, the queue reaching up to the window top, the
  silhouette `Strip` and where it runs, the
  lyrics' `LyricsPresentation`, the traffic lights, which stay put in native fullscreen
  where AppKit shows its own bar); `Strip::layer`. `MainView::render` only applies these
  fields; a new Full screen difference goes into `Stage`, not into render as another
  condition.

## Caching

- `MainView::render` builds a `HeaderState` with everything the header takes from
  `MainView` (screens, cover mode, tab, drill-in, view-menu tab, staged update) and from
  its own render (placement, colours, veil, scale). `render_header` is a free function of
  that state and `HeaderParts` (the child views: audio settings, search field, scan
  indicator, whose notifies reach the block on their own), so it cannot read `MainView`
  at render time; a value read there would stay stale while the block is replayed. A new
  header input goes into `HeaderState`; only click handlers reach `MainView`.
- `header_block` hands the state to `HeaderHost` and draws it uncached in the frame the
  state changed, cached otherwise; settings and app events reach it through
  `follow_app_events`. The slot style carries the placement's size (`HEIGHT` bar or
  `flex_1` in the title bar) and the header root fills it. The contract for cached blocks
  is in `pawse/src/doc.md`, "Cached blocks".

## Behaviour

- In cover mode the left side always shows the tabs (none lit) and the lit cover-mode button,
  never Back, even when the library is drilled in; the search and view menu are hidden.
  Leaving by the cover-mode button, the footer cover or Escape returns to the drilled-in page;
  a tab click switches to that tab. Escape only leaves cover mode. The button and the footer
  cover share `MainView::toggle_cover_mode`, which also closes Settings / Tools and clears the
  search, so leaving cover mode lands in the library.
- Tab, Back, settings and tools clicks go through `leave_overlays` (clear the search, focus
  `#main_content`, leave cover mode); the update button does neither.
- An empty spacer mirroring the view-menu slot sits left of the search field, so the field
  stays centered. The library indicator is an absolutely positioned child of the search box
  and takes no layout space: with a menu it lies in that spacer, without one it reaches over
  the left group, which matters only at the minimum window width with all tabs on.

## Placement

- `Placement::Below(bg)`: a bar of its own under the window title bar, inside `#main_content`.
- `Placement::TitleBar` (`header_in_title_bar`, Appearance, on by default): the header is the
  content of `WindowTitleBar`, which takes `TITLE_BAR_HEIGHT`.
- Interactive pieces (button groups, search field, menu button) are wrapped in `guard`:
  `occlude` plus a left mouse-down that stops propagation. Without it the title bar reads a
  press on a button as a window drag (macOS, Linux) or a caption hit (Windows
  `WindowControlArea::Drag`) and the click never arrives. Gaps, the strips above and below the
  buttons and the spacer next to the search field still drag the window.
- `main_view.rs` falls back to `Below` in fullscreen on macOS and Windows, where
  `WindowTitleBar` collapses to a thin inset; on Linux the bar does not collapse. In cover
  mode with hidden chrome the bar keeps its height and stays empty, so the layout does not
  jump.
- The header sits outside `#main_content`, which owns the `MainView` key context and focus
  handle; focus tracking must stay off the title-bar region. So arrow and space shortcuts do
  not fire while focus is in the header (the binding is `MainView && !Input`). The `on_action`
  handlers are on `#main_view`, so actions dispatched from the header, for example by a
  notification button, still reach `MainView`.

## Last screen

- A closed window loses its `MainView` (macOS Dock reopen, the tray's Show, a second
  launch on Windows / Linux all build a new one). `remember_screen` runs in `MainView`'s
  `on_release` and stores `LastScreen`; `restore_screen` ends `MainView::new`. Memory
  only: a new launch starts on Albums.
- Kept: the screen (library, Settings page, Tools page, cover mode with Full screen),
  the library's whole navigation stack as `NavKey`s, so Back still leads where it led
  (`library_views/doc.md`, "Navigation"), and both side panels (open, width). Not kept:
  scroll positions, the search text, open popovers.
- The library is restored first, then the panels, then the screen: opening Settings or
  Tools goes through `leave_overlays`, and cover mode must be entered last.
- Settings and Tools report tab clicks through `Settings::on_page_change`, so `page_ix`
  is the shown tab and not only the last requested one.

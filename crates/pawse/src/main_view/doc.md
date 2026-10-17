# main_view

Child modules of `main_view.rs`, the root view. Today that is the header: tabs, search
field, view menu and the right-hand buttons. It is an `impl MainView` in a child module, not
a standalone entity, because its buttons call `MainView`'s private methods and fields
(`open_settings`, `leave_overlays`, `toggle_cover_mode`, `library_view`, …).

Not here: the rest of the root view — screens, panels, footer, cover-mode chrome, key
actions (`main_view.rs`, `pawse/src/doc.md`); the title bar widget (`window_title_bar.rs`);
the view-options menu (`library_views/doc.md`); the library indicator (`pawse/src/doc.md`,
"Library events"); the sleep-timer badge, which is in the footer (`sleep_timer/doc.md`).

## Files

- `header.rs` — `MainView::render_header(placement, …)`, `Placement`, `HEIGHT` (standalone
  bar) and `TITLE_BAR_HEIGHT` (bar when the header lives in the title bar), both scaled by
  the UI font scale, and the button helpers only the header uses.

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

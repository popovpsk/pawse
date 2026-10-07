# main_view

Child modules of `main_view.rs` (the root view that composes title bar, header, library/settings/tools screens, queue and lyrics panels, footer). Children see the private fields and methods of `MainView`, which is why the header lives here instead of being a standalone entity: its buttons call `open_settings`, `leave_overlays`, `set_cover_mode`, `library_view`, … directly.

## Files

- `header.rs` — `MainView::render_header(placement, …)`: back button or library tabs plus the cover-mode button on the left, search field with the view menu in the middle, update / tools / settings / audio buttons on the right. Also holds the button helpers only the header uses and `HEIGHT` (44 px, the standalone bar) and `TITLE_BAR_HEIGHT` (52 px, the bar when the header lives in the title bar; both before the font scale).

## Placement

`Placement::Below(bg)` is the classic layout: the header is its own bar under the window title bar, inside `#main_content`.

`Placement::TitleBar` is the `header_in_title_bar` setting (Appearance, on by default). The header is passed to `WindowTitleBar::content` and the title bar takes `TITLE_BAR_HEIGHT`, so the two bars become one. It is taller than the standalone header so the search field and the round buttons keep some air above and below.

Non-obvious behavior of the title-bar placement:

- Interactive pieces (the tab / button groups, the search field and the view-menu button, each only as big as its content) are wrapped by `guard` (`occlude` plus a left mouse-down that stops propagation). Without it the title bar treats a press on a button as the start of a window drag (macOS, Linux) or as a caption hit (Windows `WindowControlArea::Drag`), and the click never arrives. Everything else in the bar still drags the window: the gaps between the groups, the strips above and below the buttons and the spacer next to the search field. Clicks on the tab, back and cover-mode buttons return focus to `#main_content` through `clear_search`, the gear / tools / update buttons never take focus on click, so focus behaves as in the standalone layout.
- macOS traffic lights are re-centered for the taller bar through `Window::set_traffic_light_position` (`MainView::sync_traffic_lights`, applied on the next frame and only when the target changes). The font scale changes the bar height, so it is recomputed on every render. The Windows and Linux window buttons come from `gpui_component::TitleBar` and size themselves to the bar, so with the taller bar they are taller than native (34 px wide on Windows); that is untested on a real Windows machine.
- Fullscreen on macOS and Windows has no title bar (`WindowTitleBar` collapses to a thin inset), so the header falls back to `Below` while fullscreen. On Linux the title bar does not collapse, so the header stays in it.
- In cover mode with hidden chrome the bar keeps the header height but stays empty, so the layout does not jump when the chrome toggles.
- The library indicator (`library_scan_indicator.rs`, see `library_views/doc.md`) sits just left of the search field in both placements. It is one muted icon (books with an arrows or check badge), an absolutely positioned child of the search box, so it takes no layout space and the search field stays centered. When the view menu is present it falls into the empty spacer that mirrors the menu button; without a menu it extends over the left group, which only matters at the minimum window width with all tabs enabled. The sleep-timer badge is not part of the header or the title bar: it lives in the footer (see `sleep_timer/doc.md`), so it is hidden together with the chrome in cover mode. Scans and syncs are also shown in Settings → Library (sources list, `library_sources.rs`).
- The header sits outside `#main_content`, which owns the `MainView` key context and the focus handle (focus tracking must stay off the title-bar region). Arrow / space shortcuts therefore do not fire while focus is inside the header; typing in the search field is unaffected (`!Input` excludes it anyway). The `on_action` handlers live on `#main_view`, so actions dispatched while focus is in the header (for example from a notification button) still reach `MainView`.

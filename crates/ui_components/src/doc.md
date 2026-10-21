# ui_components

Reusable GPUI widgets with no app dependencies (only `gpui` and `gpui-component`): the tabbed
Settings widget, the Slider, cover and artist image tiles with placeholders, `NativeTooltip`
and the scrollbar gutter constant. Colors come from `cx.theme()` only.

Not here: what settings rows do and say (`pawse/src/doc.md`, `pawse/src/tools/doc.md`), how
the footer sliders and the cover cache use these widgets (`pawse/src/doc.md`), the icon files
(`ui_resources/src/doc.md`).

## Files

- `settings.rs` — `Settings`, `SettingPage`, `SettingGroup`, `SettingItem`, `SettingField`.
- `slider.rs` — `Slider` entity, `SliderEvent::Change`.
- `cover_placeholder.rs` — `cover_placeholder` (notes icon), `artist_placeholder` (person).
- `cover_thumb.rs` — `cover_thumb` (`Arc<Image>`, placeholder on decode failure), `cover_tile`
  and `artist_tile` (`Arc<RenderImage>`).
- `artist_avatar.rs` — up to three covers stacked for an artist row (oldest-first input,
  oldest on top).
- `tooltip.rs` — `NativeTooltip::native_tooltip` for `gpui_component::Button`.
- `scrollbar_metrics.rs` — `SCROLLBAR_GUTTER`.

## Images

- The placeholders draw `icons/placeholder-notes.svg` and `icons/s1-artists.svg` by asset
  path: the app must register `ui_resources::assets::Assets`, nothing in the crate says so.
- `cover_tile` / `artist_tile` show images the caller decoded and owns, and the caller frees
  their atlas tile (`pawse/src/doc.md`, cover cache). `cover_thumb` takes the small
  `Arc<Image>` thumbnails of list rows.

## Settings

- Layout only: reading and writing values lives in the `SettingField` closures the caller
  supplies, which run on every render. Deliberately minimal: no search, no sidebar.
- In a horizontal item the description spans the full row under label and field, so a wide
  field cannot squeeze it into a column. `SettingItem::unlabeled` is for a group whose title
  already names its only row.
- Active tab and one shared scroll handle live in keyed state under the `Settings` id, so
  each instance needs its own id (`pawse-settings`, `pawse-tools`).
- `initial_page` / `initial_anchor` are read when that state is created and again whenever
  `page_request` differs from the number it last saw; a re-render with the same number
  leaves the user's tab alone. To switch tabs while shown, the caller bumps `page_request`.
- A `SettingGroup::anchor` named by `initial_anchor` is scrolled to the top, by the group's
  index among the scroll container's direct children: every group must stay one child.

## Slider

- `live_update = true` emits `Change` on every position update (volume); `false` moves
  the thumb during a drag and emits once on release (seek). `set_value_silent` updates
  without an event; `is_interacting()` lets callers ignore their own updates mid-drag.
- gpui captures the mouse during a drag, so `on_mouse_up` never reaches the element.
  Release is caught by `on_drop` and by a capture-phase `window.on_mouse_event` registered
  every frame from a canvas overlay (as Zed's scrollbar does), deduplicated by the
  `interacting` flag. The same canvas records `track_bounds`.
- Mid-drag the drag preview is the topmost hit target, so `on_hover(false)` fires while the
  cursor is still over the slider. A release outside `track_bounds` then has no hover
  transition to clear `hover_value`, so both release paths pass the release position and
  `end_interaction` clears it when that is outside the track.
- The hover tooltip is sized in rem, so it follows the app's font scale.

## NativeTooltip

- Use `native_tooltip(text)` instead of `Button::tooltip` on a button that disappears on its
  own click (header Settings / Tools / Update, the Playlists AI button, a scrobble rule's
  delete) and on a button sharing a row with one, so a row's tooltips look alike.
- Why: `Button::tooltip` (gpui-component 0.7.1) is one overlay per window, hidden only by
  that button's `on_hover(false)` or mouse down. When the click removes the button, a late
  hover report from the old frame asks for the tooltip and nothing hides it. gpui's own
  tooltip lives in the element's state and goes with the element; on the button itself, so
  the pending click hides it.
- Differences: placed at the cursor, no slide animation. `Button` only: gpui's tooltip
  needs an element with an id.

## Scrollbar gutter

- `SCROLLBAR_GUTTER` mirrors the private `THUMB_ACTIVE_INSET * 2 + THUMB_ACTIVE_WIDTH` in
  `gpui-base`'s `scrollbar.rs`; re-check it when bumping `gpui-component`.
- Reserve it only where the thumb would land on something with an edge of its own (the tag
  dialog's inputs). Do not push the band into a parent's padding with a negative margin: the
  scrollbar vanishes.

## Decisions

Owner's calls; ask before changing.

- Scrollbar thumbs float over list rows, as in Zed; no gutter in lists. The alternative was
  tried and dropped.

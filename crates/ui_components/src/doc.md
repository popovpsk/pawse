# ui_components

Reusable GPUI widgets shared across the app. Free of app-level dependencies so `pawse`
can depend on it without cycles.

## Files

- `lib.rs` — module declarations.
- `settings.rs` — Settings widget: top tab bar over a scrollable content area.
- `slider.rs` — Horizontal slider with `live_update` and non-live modes, tooltip, thumb visibility.
- `artist_avatar.rs` — Stacked 3‑cover composite (up to 3 `Arc<Image>` layers, oldest on top).
- `cover_placeholder.rs` — `cover_placeholder(size, radius, bg, fg)` — inline SVG placeholder for missing album art
  (`icons/placeholder-notes.svg`); `artist_placeholder` — the same tile with the person from the Artists tab icon
  (`icons/s1-artists.svg`, drawn a bit larger: the figure is thinner than the notes) for a missing artist photo.
- `cover_thumb.rs` — `cover_thumb` (`Arc<Image>`, falls back to the placeholder on a decode failure), `cover_tile`
  (`Arc<RenderImage>` or the notes placeholder) and `artist_tile` (`Arc<RenderImage>` or the person placeholder).
- `tooltip.rs` — `NativeTooltip::native_tooltip(text)` on a gpui-component `Button`: puts gpui's own tooltip (with
  gpui-component's `Tooltip` view) on the button's element, in place of `Button::tooltip`. Use it for a button that
  **disappears on its own click** (the header's Settings / Tools / Update, the Playlists tab's AI button that opens
  Tools, a scrobble rule's delete — the last rule's goes with its row) and for a button sharing a row with one (the
  rule's edit), so a row's tooltips look alike. `Button::tooltip` in gpui-component 0.7.1 is a "managed" tooltip:
  one overlay per window (`gpui_base::TooltipOverlay`), shown 500 ms after the button's `on_hover(true)` and hidden
  only by that button's `on_hover(false)` or mouse down. Clicking while moving the mouse leaves it stuck: a move
  during the press makes gpui report the pressed button as not hovered, and a move right after the release, before
  the next frame, reaches the old frame's listeners, which report it hovered again and ask for the tooltip; the
  next frame no longer has the button, so nothing ever hides it (seen on the Settings gear: "Settings" stayed over
  the opened settings). gpui's own tooltip lives in the element's state and is painted only while the element is,
  so it goes away with it. It sits on the button itself, not on a wrapper, so the button's pending click hides it
  for the whole press. Unlike the managed one it comes back after a click when the mouse moves over the button
  again, is placed at the cursor rather than centred under the trigger, and has no slide animation — the look the
  header's tab buttons already had. Implemented for `Button` only: gpui's tooltip needs an element with an id.

---

## Settings

### Hierarchy

```
Settings              tab bar + scrollable content
  SettingPage         one tab
    SettingGroup      titled card of rows
      SettingItem     label + optional description + field
        SettingField  control closure (switch, dropdown, …)
```

### Responsibility split

A horizontal `SettingItem` puts the label and the field on one line and the
description under both, across the full width: a wide field (a button group)
would otherwise squeeze the description into a narrow column.
`SettingItem::unlabeled` is a vertical row without a label, for a group whose
title already names its only row (a folder or server list).

`SettingGroup::separated` draws a hairline (`theme.border`) between rows, and
between the group description and the first row, for a group of many tall rows
(the scrobble rules) where the default 16px gap alone does not tell where one
row ends. Off by default: most groups have two or three short rows.

Widget owns **only layout**. All behavior lives in `SettingField::render` closures
supplied by the caller (`pawse` → `settings_view.rs`).

### State

Active tab + `ScrollHandle` via `Window::use_keyed_state` keyed on `Settings` id.
One scroll handle shared across tabs; switching resets offset to top.

`initial_page` is only read when the state is created, i.e. when the widget
appears. To switch tabs while it is already shown (a link from one settings
tab to another), the caller bumps `page_request`: when the number differs from
the one the state last saw, the widget jumps to `initial_page` once. A plain
re-render with the same number leaves the user's own tab choice alone.
The same request can name a group: `SettingGroup::anchor` tags a group and
`Settings::initial_anchor` picks one on `initial_page`; the jump then scrolls
that group to the top (`ScrollHandle::scroll_to_top_of_item`, one-shot — gpui
clears it once the child's bounds exist) instead of resetting to the top. The
index is the group's position among the scroll container's direct children, so
every group must stay one child. The state starts with `request` one behind
the caller's, so the first render goes through the same path.

### Theming

`cx.theme()` properties only — no app‑specific colors.

---

## Slider

### Modes

| `live_update` | Behavior | Use |
|---|---|---|
| `true` | Emits `Change` on every position update | Volume |
| `false` | Moves visually during drag, emits `Change` only on release | Track seek |

### GPUI event flow

GPUI captures the mouse during drag, so `on_mouse_up` never fires on the element.
Release is covered by two handlers, deduplicated via `interacting` flag:

1. **`on_drop`** — drag released over the slider.
2. **`window.on_mouse_event` capture‑phase** — fires for any left mouse‑up anywhere
   (same technique Zed's scrollbar uses). Registered inside a canvas overlay that also
   captures `track_bounds` for position‑to‑value mapping.

### Thumb & disabled

- Thumb hidden when `!disabled && (hovered \|\| interacting)`.
- Disabled → `opacity(0.4)`, `on_mouse_down` returns immediately.

### Tooltip vs. drag-release-outside-bounds

Mid-drag, GPUI treats the drag preview as the topmost hit target, so `on_hover(false)`
fires on the slider even while the cursor sits over it; `hover_value` keeps getting
refreshed by `on_drag_move` regardless. If the release lands outside `track_bounds`, no
further hover transition occurs to clear `hover_value`, so both `end_interaction` call
sites (`on_drop`, the capture-phase `on_mouse_event`) pass the release position and
`end_interaction` clears `hover_value` itself when that position falls outside the track.

### Track progress integration

`crates/pawse/src/track_progress_slider.rs` wraps the slider with `live_update = false`.
Guards: skips engine `PositionChanged` events while `is_interacting()` or when the delta
is `≥ 0.5` s (stale events after seek would otherwise snap the slider back). Disabled
until `EngineEvent::Loaded`.

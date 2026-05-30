# ui_components

Reusable GPUI widgets shared across the app. Free of app-level dependencies so `pawse`
can depend on it without cycles.

## Files

- `lib.rs` — module declarations.
- `settings.rs` — Settings widget: top tab bar over a scrollable content area.
- `slider.rs` — Horizontal slider with `live_update` and non-live modes, tooltip, thumb visibility.
- `artist_avatar.rs` — Stacked 3‑cover composite (up to 3 `Arc<Image>` layers, oldest on top).
- `cover_placeholder.rs` — `cover_placeholder(size, radius, bg, fg)` — inline SVG placeholder for missing album art.
- `fade.rs` — `FadeEdge` + `fade_overlay(edge, color, size_px, offset_px)` — gradient overlay pinned to one edge of a `relative` container, hides scrolling content.

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

Widget owns **only layout**. All behavior lives in `SettingField::render` closures
supplied by the caller (`pawse` → `settings_view.rs`).

### State

Active tab + `ScrollHandle` via `Window::use_keyed_state` keyed on `Settings` id.
One scroll handle shared across tabs; switching resets offset to top.

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

### Track progress integration

`crates/pawse/src/track_progress_slider.rs` wraps the slider with `live_update = false`.
Guards: skips engine `PositionChanged` events while `is_interacting()` or when the delta
is `≥ 0.5` s (stale events after seek would otherwise snap the slider back). Disabled
until `EngineEvent::Loaded`.

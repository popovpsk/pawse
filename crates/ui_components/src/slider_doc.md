# Slider Component

Custom horizontal slider built on GPUI primitives (`on_mouse_down`, `on_drag`, `on_drag_move`, `on_drop`, `window.on_mouse_event`).

## Architecture

**File**: `crates/ui_components/src/slider.rs`

Two modes controlled by `live_update`:

| Mode | `live_update = true` | `live_update = false` |
|------|---------------------|----------------------|
| On mouse-down | Emit value immediately | Move visually only (no event) |
| On drag-move | Emit value on every move | Move visually only (no event) |
| On release | Just end interaction | Emit final value as single event |
| Use case | Volume slider | Track progress (seek on release) |

## GPUI Event Flow (Critical)

GPUI's drag system **captures the mouse** once `on_drag` activates. This has major implications:

### 1. `on_mouse_up` / `on_mouse_up_out` are unreliable after drag starts

When a drag gesture activates (even a tiny movement after mouse-down), GPUI stops delivering `on_mouse_up` to the element. The mouse-up event goes to the drop target instead. This means:

- Simple click → `on_mouse_up` fires (sometimes, if no drag detected)
- Drag → `on_mouse_up` does **NOT** fire

**Never rely solely on `on_mouse_up` to finalize a slider interaction.**

### 2. `on_drop` fires when drag ends (but position may be wrong)

`on_drop::<DragSlider>` fires reliably when a drag gesture ends, regardless of where the mouse is. However, it doesn't receive the mouse position — so we can't recalculate the value from coordinates. Instead, we trust the value already set during drag-move and just emit `Change(this.value)`.

### 3. Global `window.on_mouse_event` for bulletproof mouse-up

We register a **capture-phase** `MouseUpEvent` handler via `window.on_mouse_event()` inside the canvas paint callback. This is the technique Zed's scrollbar uses. It fires for ANY mouse-up in the window, even outside the slider element. This guarantees we always end the interaction.

The handler is re-registered every frame (GPUI clears per-frame listeners), so it's always active while the slider is rendered.

```rust
window.on_mouse_event({
    move |event: &MouseUpEvent, phase: DispatchPhase, _window, cx| {
        if phase != DispatchPhase::Capture { return; }
        if event.button != MouseButton::Left { return; }
        entity.update(cx, |this, cx| {
            if !this.interacting { return; }
            this.interacting = false;
            if !this.live_update {
                cx.emit(SliderEvent::Change(this.value));
            }
            cx.notify();
        });
    }
});
```

### 4. Deduplication via `interacting` flag

There are 3 handlers that can finalize an interaction:
1. `on_mouse_up` (click without drag)
2. `on_mouse_up_out` (rare, element lost focus)
3. `on_drop` (drag released over target)
4. `window.on_mouse_event` capture-phase (drag released anywhere)

All check `if !this.interacting { return; }` and set `this.interacting = false`. Whichever fires first wins; the others skip.

## Event Sequence for Common Interactions

### Simple click (no drag)

1. `on_mouse_down` → `interacting = true`, visual update
2. `on_mouse_up` → `interacting = false`, emit `Change`
3. `window.on_mouse_event` capture → `interacting` already false, skip

### Click-and-drag within slider

1. `on_mouse_down` → `interacting = true`, visual update
2. `on_drag` activates
3. `on_drag_move` (multiple) → visual updates
4. `on_drop` → `interacting = false`, emit `Change`
5. `window.on_mouse_event` capture → `interacting` already false, skip

### Click-and-drag, release outside slider

1. `on_mouse_down` → `interacting = true`, visual update
2. `on_drag` activates
3. `on_drag_move` (multiple) → visual updates
4. `window.on_mouse_event` capture → `interacting = false`, emit `Change`
5. `on_drop` → `interacting` already false, skip

## Thumb Visibility

Thumb (the draggable circle) is hidden when the mouse is not over the slider and the user is not dragging, matching Spotify's progress bar behavior:

```rust
let show_thumb = !self.disabled && (self.hovered || self.interacting);
```

`hovered` is maintained via `on_hover` on the track element.

## Disabled State

When `disabled = true`:
- Whole slider renders at `opacity(0.4)`
- `on_mouse_down` handler returns immediately
- `on_mouse_up` / `on_drop` handlers return immediately
- Thumb is hidden (included in `show_thumb` check)
- Thumb visibility, drag, and global mouse-up are all blocked

## Track Progress Slider Specifics

**File**: `crates/ui/src/track_progress_slider.rs`

Uses `live_update = false` mode with additional engine integration:

### Position update guard

```rust
EngineEvent::PositionChanged(position) => {
    if !this.has_track || this.slider.read(cx).is_interacting() { return; }
    let new_position = position.as_secs_f32();
    let slider_position = this.slider.read(cx).value() * this.duration_secs;
    if (new_position - slider_position).abs() >= 0.5 { return; }
    // ... update slider
}
```

The `≥ 0.5` guard is **essential**: after a seek, old `PositionChanged` events from before the seek may arrive asynchronously. Without this guard, the slider would snap back to the old position.

The `is_interacting()` guard prevents the engine from overwriting the user's drag position.

### Disabled state

Slider starts `disabled = true`. Becomes enabled on `EngineEvent::Loaded`, disabled on `EngineEvent::TrackEnded` or `EngineEvent::Error`.

### Why not `update_value_by_position` on release

In previous versions, `on_mouse_up` called `update_value_by_position` to recalculate the value from the mouse position. This was wrong because `update_value_by_position` has a `if new_value != self.value` guard — but the value was already set by `update_value_visual_by_position` during drag. The condition would fail and no `Change` event would be emitted, making seeks appear to not work on simple clicks.

The fix: on release, directly emit `Change(this.value)` without recalculating. The value is already correct from the visual update.
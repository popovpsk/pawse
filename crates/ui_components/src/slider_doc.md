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

### Key methods

- `set_value_from_position(position) -> bool` — converts mouse position to stepped value, updates internal state, returns whether value changed. Caller decides whether to emit `Change` based on `live_update`.
- `end_interaction(cx)` — sets `interacting = false`, emits `Change` if `!live_update`, notifies. Guarded by `interacting` flag, safe to call from multiple handlers.

## GPUI Event Flow (Critical)

GPUI's drag system **captures the mouse** once `on_drag` activates. This means `on_mouse_up` does NOT fire on the element after a drag starts — the event goes to the drop target instead. For this reason, we do NOT use `on_mouse_up` or `on_mouse_up_out`. Instead, two mechanisms cover all release scenarios:

### 1. `on_drop` — drag released over the element

`on_drop::<DragSlider>` fires when a drag gesture ends and the payload is dropped onto the element. It doesn't receive mouse position, so we trust the value already set during drag-move and emit `Change(this.value)` via `end_interaction`.

### 2. Global `window.on_mouse_event` capture-phase — bulletproof mouse-up

Registered inside the canvas paint callback (re-registered every frame, since GPUI clears per-frame listeners). Fires for ANY left mouse-up in the window, even outside the slider. This is the technique Zed's scrollbar uses.

```rust
window.on_mouse_event({
    move |event: &MouseUpEvent, phase: DispatchPhase, _window, cx| {
        if phase != DispatchPhase::Capture { return; }
        if event.button != MouseButton::Left { return; }
        entity.update(cx, |this, cx| this.end_interaction(cx));
    }
});
```

### Deduplication via `interacting` flag

There are 2 handlers that can finalize an interaction:
1. `on_drop` (drag released over target)
2. `window.on_mouse_event` capture-phase (any mouse-up anywhere)

Both call `end_interaction`, which checks `if !self.interacting { return; }` and sets it to false. Whichever fires first wins; the other skips.

## Event Sequence for Common Interactions

### Simple click (no drag)

1. `on_mouse_down` → `interacting = true`, value update, emit `Change`
2. `window.on_mouse_event` capture → `end_interaction`, `interacting` already false, skip

(With `live_update = false`, step 1 emits no event; the capture handler emits `Change` on release.)

### Click-and-drag within slider

1. `on_mouse_down` → `interacting = true`, visual update
2. `on_drag` activates
3. `on_drag_move` (multiple) → visual updates
4. `on_drop` → `end_interaction`, emit `Change`
5. `window.on_mouse_event` capture → `interacting` already false, skip

### Click-and-drag, release outside slider

1. `on_mouse_down` → `interacting = true`, visual update
2. `on_drag` activates
3. `on_drag_move` (multiple) → visual updates
4. `window.on_mouse_event` capture → `end_interaction`, emit `Change`
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
- Thumb is hidden (included in `show_thumb` check)

## Canvas Overlay

An invisible `canvas` element covers the entire slider. It serves two purposes:
1. Captures rendered bounds (`track_bounds`) for position-to-value mapping
2. Registers the global capture-phase `MouseUpEvent` handler during paint (see above)

## Track Progress Slider Specifics

**File**: `crates/pawse/src/track_progress_slider.rs`

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

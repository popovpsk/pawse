# macOS System Media Integration

This document describes how the player integrates with macOS system UI: the Now Playing widget (Control Center / Lock Screen), the status bar menu, and remote media-key commands. It focuses on **why** specific architectural decisions were made.

## Scope

- **Now Playing widget** — metadata, artwork, playback state, elapsed time
- **Status bar menu** — track info, play/pause/next/previous via `NSStatusItem`
- **Remote commands** — media keys, Touch Bar, AirPods double-tap, Control Center controls via `MPRemoteCommandCenter`

## Crate Architecture

The integration is split into two crates:

| Crate | Purpose |
|-------|---------|
| `media_integration` | Platform-agnostic trait (`SystemMediaIntegration`) and data types (`NowPlayingInfo`, `MediaCommand`, `MediaPlaybackState`) |
| `macos_integration` | macOS-specific implementation using `objc2` for AppKit / MediaPlayer framework interop |

### Why two crates?

1. **`#![forbid(unsafe_code)]` at the workspace level.** The `ui` crate and most others must remain free of unsafe code. Objective-C interop requires `unsafe` for `define_class!` and raw `msg_send!`. Isolating unsafe to `macos_integration` keeps the rest of the codebase clean and auditable.
2. **Future ports.** A Linux MPRIS or Windows SMTC implementation can live in `linux_integration` / `windows_integration` crates and implement the same `SystemMediaIntegration` trait without touching macOS code.
3. **Compile-time separation.** `macos_integration` is a `cfg(target_os = "macos")` dependency of `ui`. Non-macOS builds do not compile or link it.

## Data Flow

```
  Audio Engine          EngineEvent           MediaBridge           update_* calls         MacOsIntegration
       |                     |                     |                         |                       |
       | ------------------> |                     |                         |                       |
       |                     | ------------------> |                         |                       |
       |                     |                     | ------------------------->                       |
       |                     |                     |                         |                       |
       |                     |                     | <--- flume channel ---- | <--- MPRemoteCommandCenter
       |                     |                     |                         |     NSStatusBar
       |                     |                     |                         |                       |
       |                     |                     |                         |                       v
       |                     |                     |                         |                  macOS System UI
       |                     |                     |                         |                  (user presses keys
       |                     |                     |                         |                   or menu items)
```

## Design Decisions

### 1. `OnceLock<Sender<MediaCommand>>` for bridging Objective-C to Rust

**Context:** `NSStatusItem` menu items require a `target` object and a `selector` (`onPlay:`, `onPause:`, etc.). We define a new Objective-C class `MediaCommandProxy` via `objc2::define_class!`.

**Decision:** Instead of storing the `flume::Sender<MediaCommand>` inside the proxy object (which would require Objective-C associated-object gymnastics or an ivar), we use a global `static COMMAND_SENDER: OnceLock<Sender<MediaCommand>>`. The proxy methods read this global and send commands.

**Why:**
- `NSStatusBar` is a singleton; there is never more than one status item per app lifetime, so a global is semantically correct.
- `OnceLock::set` is called once during `MacOsIntegration::new()`. If initialization fails, the sender is never set; the proxy methods simply no-op.
- Associated objects (`objc_setAssociatedObject`) require additional unsafe code and runtime overhead for a problem that is inherently single-instance.

### 2. `flume` channel instead of direct calls from callbacks

**Context:** Objective-C callbacks (menu actions, `MPRemoteCommandCenter` blocks) run on the main thread but outside GPUI's control flow.

**Decision:** All OS callbacks send a `MediaCommand` into an unbounded `flume` channel. A GPUI-spawned async task (`run_command_loop`) receives commands and calls back into `App::update()` to drive `EngineManager` and `PlaybackQueue`.

**Why:**
- **Re-entrancy safety.** `MPRemoteCommandCenter` handlers can be invoked by the system at any time. If we called `engine_manager.pause()` directly inside an ObjC block, we could re-enter GPUI's runtime or deadlock on internal locks.
- **Consistent execution context.** All player state mutations happen inside `App::update(|cx| { ... })`, the same as user clicks in the GPUI view tree.
- **GPUI's `ForegroundExecutor` is `!Send` and single-threaded.** Using a channel means the ObjC callback only does a non-blocking `try_send`; the heavy lifting (file I/O for `set_track`, seeking) happens in the GPUI-controlled async loop.

### 3. `RcBlock` for `MPRemoteCommandCenter` handlers

**Context:** `MPRemoteCommandCenter` expects either target/selector pairs or block handlers. We register blocks for Play, Pause, Toggle, Next, Previous, and ChangePlaybackPosition.

**Decision:** Use `block2::RcBlock` closures that capture cloned `flume::Sender`s.

**Why:**
- Blocks are the modern API and are required for `changePlaybackPositionCommand`, which provides a typed `MPChangePlaybackPositionCommandEvent` (the event object has a `positionTime()` method). Target/selector would lose this typed argument.
- `RcBlock` is `Send` and can be moved into the block's heap allocation. The command center retains the block internally, so the captured sender stays alive as long as the command center needs it.
- `Retained<AnyObject>` targets are collected in `RegisteredCommands` to ensure Rust keeps them alive until `MacOsIntegration` is dropped.

### 4. `RefCell` artwork caching in `MacOsIntegration`

**Context:** `update_now_playing` is called whenever track metadata changes (on `EngineEvent::Loaded`). It may also be called if the user seeks (though position has a separate path). Artwork is loaded from a file path.

**Decision:** Cache artwork in three `RefCell` fields:
- `cached_artwork_path: RefCell<Option<PathBuf>>` — detects when the track changed
- `cached_artwork: RefCell<Option<Retained<MPMediaItemArtwork>>>` — reused for Now Playing widget
- `cached_status_icon: RefCell<Option<Retained<NSImage>>>` — reused for status bar button

**Why:**
- `MPMediaItemArtwork` is expensive to create (it wraps a block handler). Creating it on every position update would be wasteful.
- File I/O (`NSImage::initWithContentsOfFile`) is comparatively slow. We only re-read the image when `artwork_path` changes.
- `MacOsIntegration` implements `SystemMediaIntegration` with `&self` methods because it is stored as `Rc<dyn SystemMediaIntegration>`. `RefCell` provides interior mutability for the cache without requiring `&mut self`.

### 5. Separate `update_position` from `update_now_playing`

**Context:** The engine emits `PositionChanged` events frequently (driven by the audio callback timer). The Now Playing widget expects both metadata and position updates.

**Decision:** The `SystemMediaIntegration` trait has two methods:
- `update_now_playing(&self, info: NowPlayingInfo)` — called once per track load
- `update_position(&self, elapsed_secs: f64, state: MediaPlaybackState)` — called on every `PositionChanged`, `Playing`, and `Paused` event

**Why:**
- Rebuilding the full `NSMutableDictionary` with title, artist, album, duration, artwork on every position tick would be CPU and allocation heavy.
- macOS `MPNowPlayingInfoCenter` merges dictionaries. `update_position_info` copies the existing dictionary, updates only `MPNowPlayingInfoPropertyElapsedPlaybackTime` and `MPNowPlayingInfoPropertyPlaybackRate`, and writes it back. This is O(1) in practice.
- **Playback rate is critical.** The Now Playing scrubber on macOS only moves when `playbackRate > 0`. When paused, we must explicitly set rate to `0.0`, otherwise the UI continues showing progression even though audio is stopped. This is why `update_position` takes `MediaPlaybackState` rather than just elapsed seconds.

### 6. `MediaPlaybackState` in `update_position`

**Decision:** `update_position` receives both `elapsed_secs` and `state`, then computes `rate = if Playing { 1.0 } else { 0.0 }`.

**Why not just set rate globally?**
- `MPNowPlayingInfoCenter` maintains one dictionary. If we set rate to `1.0` in `update_now_playing_info` and never touch it again, pausing would leave the old rate in the dictionary. The Control Center scrubber would keep advancing visually even though audio is paused.
- macOS does not automatically link `setPlaybackState(Paused)` to freezing the scrubber; it reads the rate field in the info dictionary.

### 7. Why `define_class!` and not `extern_class!`

**Context:** We need an Objective-C object that responds to `onPlay:`, `onPause:`, `onNext:`, `onPrevious:`.

**Decision:** Use `objc2::define_class!` to create `MediaCommandProxy` as a subclass of `NSObject`.

**Why:**
- `extern_class!` is for wrapping *existing* Apple classes. `NSObject` has no media-control selectors.
- `define_class!` generates the Objective-C runtime metadata at compile time. The resulting class behaves like a native ObjC object: it can be used as `setTarget` for `NSMenuItem` without any bridging wrappers.
- The alternative (using a pure Rust closure as target) is not supported by AppKit; `setTarget` must be an `id` (object pointer).

### 8. Status bar icon fallback chain

**Context:** `NSStatusItem` needs an icon when no track is playing.

**Decision:** Try `NSTouchBarAudioOutputVolumeHighTemplate`, then fall back to `NSActionTemplate`.

**Why:**
- There is no dedicated "music player" or "speaker" image in AppKit's `NSImageName` constants that is recommended for status-bar use.
- `NSTouchBarAudioOutputVolumeHighTemplate` (string value `"NSTouchBarAudioOutputVolumeHighTemplate"`) renders a high-volume speaker icon. Despite the "TouchBar" naming, it works in status bars and is the closest semantic match.
- `NSActionTemplate` (string value `"NSActionTemplate"`) is a generic gear/action icon that is guaranteed to exist on all macOS versions. It ensures the status item never appears blank.
- We do **not** use SF Symbols (e.g. `speaker.wave.3`) because they require macOS 11+ and `imageWithSystemSymbolName:`. The project targets a broader macOS base.

### 9. Lifetime: app-lived on macOS, window-lived elsewhere

**Context:** The bridge subscribes to `EngineEventsBus` and runs the command loop, and both must stay alive as long as playback can happen.

**Decision (non-macOS):** `MediaBridge` is a struct holding a `Subscription`, instantiated as `Entity<MediaBridge>` inside `MainView` (`#[cfg(not(target_os = "macos"))]`).

**Decision (macOS):** there is **no** `MediaBridge` entity. `media_bridge::setup(cx)` runs once at startup from `main()` and registers the engine-event subscription via `App::subscribe(...).detach()`, with the command loop spawned `detach`ed. Both are app-lived.

**Why:**
- On macOS the last window can close while audio keeps playing (see "Window Lifecycle" in `project.md`). If the integration were owned by `MainView`, closing the window would drop the `Subscription`, drop the `MacOsIntegration` (and with it `RegisteredCommands` → the captured `flume::Sender`s → the command loop ends). The system Now-Playing panel would then have no live handlers: its buttons stop working and `last_state` / playback state stop tracking reality. Making it app-lived — like `run_engine_events_bus` and the `is_playing` mirror — keeps the panel functional with no window.
- Windows/Linux quit when the last window closes, so window-lived ownership is correct there (and Windows SMTC needs the window `hwnd`, which only `MainView::new` has).
- The per-event panel updates (`apply_engine_event`) and the startup seeding (`seed_from_services`) are shared by both paths.

### 10. Why the trait uses `&self` and `dyn`

**Decision:** `SystemMediaIntegration` methods take `&self`, and `MediaBridge` stores `Rc<dyn SystemMediaIntegration>`.

**Why:**
- The bridge is shared between the GPUI event subscription closure and the command-loop async task (though both run on the same main thread). `Rc` allows shared ownership without `Arc` overhead.
- `&mut self` would require `Rc<RefCell<dyn SystemMediaIntegration>>`, adding an extra layer of borrow checking for no benefit — all implementations use interior mutability (`RefCell`, ObjC runtime) internally.
- `dyn` instead of generics keeps `MediaBridge` simple: one field type regardless of platform.

### 11. Idempotent remote-command registration (window reopen)

**Context:** `register_remote_commands` should be safe to run more than once against the process-wide `MPRemoteCommandCenter`. (Since the macOS integration is now app-lived and created once at startup — see decision 9 — window reopen no longer rebuilds it, but the defensive clearing remains valuable, e.g. across future re-registration or a `MainView`-owned path on other ports.)

**Decision:** `register_remote_commands` first calls `removeTarget(None)` on every command (play, pause, toggle, next, previous, changePlaybackPosition) before adding its handlers.

**Why:**
- `MPRemoteCommandCenter` is a process-wide singleton that retains handlers internally. Dropping a `MacOsIntegration` releases our `RegisteredCommands` target references but does **not** unregister the handlers. Without clearing them, a second registration would stack handlers and fire each command twice (e.g. `Next` would skip two tracks).
- `removeTarget(None)` wipes all existing handlers for a command, making registration idempotent. It also drops the old captured `flume::Sender`s, so any previous `run_command_loop` ends cleanly once its receiver sees the channel close.

### 12. Seeding the panel on construction

**Context:** Because the integration can be (re)built mid-playback — and because no fresh `EngineEvent::Loaded` arrives for an already-playing track — the panel would otherwise start from `Stopped` with no track info.

**Decision:** `seed_from_services` runs right after the integration is created: if `Services::playback_queue` has a current track, it reads `Services::is_playing` / `current_position_ms` / `current_duration_ms` and publishes the Now-Playing info, playback state, position, and seeds `last_state` accordingly.

**Why:** mirrors the footer-seeding pattern (`PlayButton` / `NowPlaying` / `TrackProgressSlider` seed from live `Services`). Without it, `MediaCommand::TogglePlayPause` — which branches on `last_state` — would resolve wrong, and the OS panel would show stale info retained by its singleton.

## Threading Guarantees

All `macos_integration` code runs on the **main thread only**:

- `MacOsIntegration::new` requires `MainThreadMarker` from `objc2_foundation`.
- `NSStatusBar`, `NSMenu`, `MPNowPlayingInfoCenter`, and `MPRemoteCommandCenter` are all AppKit / MediaPlayer APIs with main-thread affinity.
- The `flume` sender is `Send`, but the receiver loop runs inside `App::spawn`, which uses GPUI's `ForegroundExecutor` (a single-threaded local executor). The future never crosses threads, so `Rc`, `RefCell`, and `Retained<NSObject>` are safe inside it.

## Command Routing

Commands from the OS flow as follows:

1. User presses media key or taps status-bar menu item.
2. ObjC callback (`MediaCommandProxy` method or `RcBlock`) sends `MediaCommand` into `flume` channel.
3. `run_command_loop` receives it in an async GPUI task.
4. `cx.update(|cx| { ... })` runs on the main thread:
   - `MediaCommand::Play` → `engine_manager.play()`
   - `MediaCommand::Pause` → `engine_manager.pause()`
   - `MediaCommand::TogglePlayPause` → checks `last_state`, toggles
   - `MediaCommand::Next` / `Previous` → advances `PlaybackQueue`, calls `set_track()` + `play()`
   - `MediaCommand::Seek(f64)` → converts seconds to fraction, calls `engine_manager.seek(fraction)`
5. Engine emits `EngineEvent::Loaded` / `Playing` / `PositionChanged`.
6. The engine-event subscription (app-lived on macOS, `MediaBridge` elsewhere) picks up the event via `apply_engine_event` and calls `integration.update_*()`.
7. macOS system UI updates accordingly.

## Known Limitations

- **Platform only.** `MacOsIntegration` is macOS-specific. There is no Linux or Windows equivalent yet.
- **Artwork format.** `load_artwork` uses `NSImage::initWithContentsOfFile`, which supports whatever macOS ImageIO supports (PNG, JPEG, BMP, TIFF, etc.). It does not validate image dimensions or aspect ratio before passing to `MPMediaItemArtwork`.
- **Rate simplification.** The implementation always reports `playbackRate` as `1.0` (playing) or `0.0` (paused/stopped). It does not support fast-forward or rewind rates.
- **No seek gesture support.** Only `changePlaybackPositionCommand` is registered. Skip-forward/backward commands (15-second jumps) are not implemented.

## File Reference

| File | Responsibility |
|------|----------------|
| `crates/media_integration/src/lib.rs` | Trait and data types |
| `crates/macos_integration/src/lib.rs` | `MacOsIntegration` struct, artwork cache, trait impl |
| `crates/macos_integration/src/now_playing.rs` | `MPNowPlayingInfoCenter` updates, artwork loading |
| `crates/macos_integration/src/remote_command.rs` | `MPRemoteCommandCenter` registration |
| `crates/macos_integration/src/status_bar.rs` | `NSStatusItem`, `NSMenu`, default icon |
| `crates/pawse/src/media_bridge.rs` | GPUI entity: event forwarding, command loop, OS command dispatch |

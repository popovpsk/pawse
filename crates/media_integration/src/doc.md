# media_integration

Bridges the player to the OS "now playing" surfaces: the macOS Control Center /
Lock Screen widget and media keys, the Windows System Media Transport Controls
(SMTC), and the Linux MPRIS interface. It is a thin adapter — it owns no playback
state and makes no policy decisions.

Two directions cross this boundary:

- **Outbound**: the app pushes `NowPlayingInfo`, `MediaPlaybackState`, and elapsed
  position to the OS.
- **Inbound**: the OS sends transport commands (play / pause / next / seek …) back as
  `MediaCommand` values down a `flume::Sender` the caller supplied. The receiving end
  lives in `pawse::media_bridge`, not here.

## Responsibilities

- Define the platform-agnostic surface: the `SystemMediaIntegration` trait, the
  `NowPlayingInfo` / `MediaPlaybackState` / `MediaCommand` data types, and the
  `create_integration(sender, hwnd)` factory.
- Provide a per-OS backend behind that trait, selected at compile time.
- Translate between the app's vocabulary and each OS framework, and degrade to a
  no-op when no integration exists or initialization fails.

## Files

- `lib.rs` — the trait, the data types, and `create_integration` (cfg-dispatched to
  the platform backend; returns `None` on unsupported platforms or init failure).
  Also re-forbids `unsafe` for non-macOS targets (see below).
- `souvlaki_backend.rs` — `SouvlakiIntegration`, shared by Windows (SMTC) and Linux
  (MPRIS, pure-Rust `zbus`). `map_event` maps an OS `MediaControlEvent` to a
  `MediaCommand`; `playback` maps state + elapsed into a `souvlaki::MediaPlayback`.
  Unit tests cover both.
- `macos/mod.rs` — `MacOsIntegration`: the artwork cache and the trait impl. Holds no
  `unsafe`; it delegates the FFI to the two modules below.
- `macos/now_playing.rs` — `MPNowPlayingInfoCenter` dictionary updates (full-metadata
  vs. position-only) and artwork loading.
- `macos/remote_command.rs` — `MPRemoteCommandCenter` handler registration.

## Non-obvious behavior

- **Main thread only.** AppKit / MediaPlayer singletons have main-thread affinity, and
  `souvlaki::MediaControls` is `!Send`/`!Sync`. `MacOsIntegration::new` requires a
  `MainThreadMarker`. The integration is handed out as `Rc<dyn SystemMediaIntegration>`
  and never crosses threads. Only the `flume::Sender` (which is `Send`) leaves the
  thread, captured by the OS callbacks.

- **Commands go through a channel, never a direct call.** OS callbacks (ObjC blocks,
  souvlaki's attach handler) can fire at any time. They only do a non-blocking
  `send(MediaCommand)`; the actual engine/queue mutation happens later in the caller's
  GPUI loop. This keeps OS callbacks off GPUI's runtime and its internal locks.

- **Two update paths, deliberately asymmetric.** `update_now_playing` rebuilds the full
  metadata dictionary once per track. `update_position` is the hot path (the engine
  emits position at ~5 Hz) and only patches elapsed time + playback rate onto the
  existing dictionary instead of rebuilding it.

- **Playback rate is what freezes the scrubber.** The macOS Control Center scrubber and
  MPRIS clients advance from the reported rate, not from the playback-state flag. So
  both `update_now_playing` and `update_position` derive the rate from
  `MediaPlaybackState` (`1.0` playing, `0.0` otherwise); a paused track must report rate
  `0.0` or the UI keeps visually scrubbing. This is why `update_now_playing` takes the
  state rather than assuming the track is playing.

- **Progress is sanitized before it reaches the OS.** `souvlaki_backend::playback` (and
  the macOS duration/elapsed writes) carry a value only when it `is_finite()` and
  non-negative — `Duration::from_secs_f64` panics on `NaN` / `inf` / overflow, and the
  `f64` fields of the public `NowPlayingInfo` are not otherwise validated.

- **Artwork is cached and folded into a single write.** `MPMediaItemArtwork` wraps a
  block and is backed by `NSImage` file I/O, so it is rebuilt only when the artwork
  path changes. The full-metadata write places the cached artwork into the same
  dictionary it builds, so the widget never inherits a previous track's cover.

- **Remote-command registration is idempotent.** `MPRemoteCommandCenter` is a
  process-wide singleton that retains handlers internally; dropping our targets does
  not unregister them. `register_remote_commands` therefore calls `removeTarget(None)`
  on every command before adding its own, so re-registration can't stack handlers and
  fire a command twice. `RegisteredCommands` keeps the returned target tokens alive for
  the integration's lifetime.

- **`unsafe` is localized to macOS.** The crate allows `unsafe` (its macOS backend needs
  `objc2`), but `lib.rs` carries
  `#![cfg_attr(not(target_os = "macos"), forbid(unsafe_code))]`, so the facade and the
  souvlaki backend stay provably unsafe-free; only `macos/` may use it.

- **Graceful degradation.** `create_integration` returns `None` when the platform has no
  backend, when souvlaki fails to create or attach its controls, or when called off the
  main thread on macOS. The app then runs without any system media surface.

## Lifetime

The integration must outlive playback. On macOS it is app-lived (audio keeps running
with no open window); on Windows / Linux it is owned by `MainView` and dies with the
window, which is correct because those platforms quit when the last window closes. This
wiring lives in `pawse::media_bridge`; see `.docs/project.md`.

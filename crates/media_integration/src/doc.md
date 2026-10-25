# media_integration

Thin adapter between the player and the OS "now playing" surfaces: macOS Control Center / Lock Screen and media keys (`MPNowPlayingInfoCenter`, `MPRemoteCommandCenter`), Windows System Media Transport Controls, Linux MPRIS (`souvlaki` over pure-Rust `zbus`, no libdbus). It owns no playback state and makes no policy decisions. Outbound, the app pushes `NowPlayingInfo`, `MediaPlaybackState` and elapsed time; inbound, the OS sends `MediaCommand`s down a `flume::Sender` the caller supplied.

Not here: mapping engine events to these calls, the receiving end of the command channel and the integration's owner (`crates/pawse/src/doc.md`: `media_bridge.rs`); writing the cover file the OS reads (`LibraryService::get_cover_art_path_for_media`, same doc).

## Files

- `lib.rs` — `SystemMediaIntegration`, the data types, `create_integration(sender, hwnd)` (cfg-dispatched; `None` where there is no backend or init fails). Re-forbids `unsafe` off macOS.
- `souvlaki_backend.rs` — `SouvlakiIntegration`, shared by Windows and Linux: `map_event` (OS event → `MediaCommand`; Stop, Raise, Quit, volume and relative seek are ignored) and `playback`; dropping it calls `detach`. Unit-tested.
- `macos/mod.rs` — `MacOsIntegration`: artwork cache and the trait impl; no `unsafe` of its own.
- `macos/now_playing.rs` — `MPNowPlayingInfoCenter` writes (full metadata vs. position only) and artwork loading.
- `macos/remote_command.rs` — `MPRemoteCommandCenter` handler registration.

## Rules

- Main thread only. AppKit / MediaPlayer singletons have main-thread affinity and `souvlaki::MediaControls` is `!Send`. `MacOsIntegration::new` needs a `MainThreadMarker`; the integration is an `Rc<dyn SystemMediaIntegration>` and never crosses threads. Only the `flume::Sender` leaves it, captured by the OS callbacks.
- Commands go through the channel, never a direct call. OS callbacks (ObjC blocks, souvlaki's `attach` handler) can fire at any time; they only do a non-blocking `send`, which keeps them off GPUI's runtime and its internal locks.
- Two update paths, deliberately asymmetric. `update_now_playing` rebuilds the full metadata once per track; `update_position` is the hot path (every engine position tick) and on macOS only patches elapsed time and rate onto the existing dictionary.
- Playback rate freezes the macOS scrubber. Control Center advances from the reported rate, not the playback-state flag, so `update_now_playing` and `update_position` derive the rate from `MediaPlaybackState` (1.0 playing, 0.0 otherwise); a paused track must report 0.0 or the widget keeps scrubbing. That is why `update_now_playing` takes the state. The souvlaki path has no rate; it passes the state with the progress.
- Progress is sanitized before it reaches the OS: durations and elapsed values go through only when finite and non-negative (duration also positive). `Duration::from_secs_f64` panics on `NaN` / `inf`, and `NowPlayingInfo`'s `f64` fields are not validated.
- macOS artwork is cached: `MPMediaItemArtwork` wraps a block over `NSImage` file I/O, so it is rebuilt only when the artwork path changes, and the full-metadata write puts the cached artwork into the dictionary it builds — the widget never inherits the previous track's cover.
- Remote-command registration is idempotent. `MPRemoteCommandCenter` is a process-wide singleton that keeps handlers even when our tokens are dropped, so `register_remote_commands` calls `removeTarget(None)` on each command before adding its own; re-registering cannot stack handlers and fire a command twice. `RegisteredCommands` keeps the tokens alive for the integration's life.
- `unsafe` stays on macOS. The crate allows it (`objc2`), but `lib.rs` has `#![cfg_attr(not(target_os = "macos"), forbid(unsafe_code))]`, so the facade and the souvlaki backend stay provably safe; only `macos/now_playing.rs` and `macos/remote_command.rs` use it.
- Degrade to nothing: `create_integration` returns `None` for an unsupported platform, a souvlaki create/attach failure, or a call off the main thread on macOS. Windows needs `hwnd` for SMTC; every other platform ignores it.
- The artwork file must live where the host can read it. `NowPlayingInfo::artwork_path` is a filesystem path; souvlaki hands it to MPRIS as `mpris:artUrl = file://<path>` (not percent-encoded), which the desktop shell resolves outside our process. So the app writes it under `dirs::cache_dir()/pawse/artwork`, never `std::env::temp_dir()`: inside a Flatpak `/tmp` is the sandbox's private tmpfs, a `file:///tmp/...` URL points at nothing on the host, and a host file with the same name would show someone else's cover. Windows SMTC and the macOS widget consume the same path.
- The integration must outlive playback, which goes on with no window (macOS, and the tray on Windows and Linux): on macOS and Linux it is app-lived; on Windows it is rebuilt for whichever window exists, the main one or the tray's hidden one, because SMTC belongs to an `hwnd`. `SouvlakiIntegration`'s `Drop` detaches: souvlaki's Windows controls have no `Drop`, so a dropped one would stay enabled with its button handler, and the next integration on the same window would add a second handler (one media key, two skips).

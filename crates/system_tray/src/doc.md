# system_tray

The notification-area icon for Windows and Linux: an app icon, a tooltip, a menu with
"Show" and "Quit", and a left click that means Show. No GPUI, no pawse dependencies; the
caller gets `TrayEvent`s through the `EventSink` it passes. On every other target the
crate is empty.

Not here: when the icon exists, what Show and Quit do, closing the window into the tray,
the labels and icon images (`crates/pawse/src/doc.md`, "Tray").

## Files

- `lib.rs` — `Tray`, `TrayConfig`, `TrayLabels`, `IconImage` (RGBA), `TrayEvent`,
  `EventSink`, `TrayError`; dispatch to the platform module.
- `linux.rs` — StatusNotifierItem over D-Bus (`ksni`, pure-Rust `zbus` on `async-io`).
- `windows.rs` — `Shell_NotifyIcon` through `tray-icon` and its `muda` menu.

## Rules

- Dropping a `Tray` removes the icon; quitting without dropping it leaves a ghost icon in
  the Windows notification area until the pointer passes over it.
- `is_shown` is whether a host shows the icon now. The app keeps running after the
  last window closes only while it is true, so a desktop without a tray never leaves an
  invisible process behind.
- `set_labels` changes the menu texts of the living icon: a new icon could land in another
  place in the tray, and on Linux it costs a fresh registration.
- No `unsafe`: both backends are safe APIs, and the crate keeps the workspace's `forbid`.

### Linux

- Spawned with `assume_sni_available(false)`: with no `org.kde.StatusNotifierWatcher`
  (GNOME without the AppIndicator extension, a sandbox without `--talk-name`), `new` fails
  instead of waiting for a watcher that may never come. Later the watcher going away or
  coming back flips `is_shown` (`watcher_offline` / `watcher_online`); the service keeps
  running. The flag starts `true` before `spawn`, so a `watcher_offline` racing the
  return of `spawn` is not overwritten.
- `new` blocks on D-Bus (connect, register with the watcher): call it off the UI thread.
  `ksni` runs the item on its own thread; zbus must stay on `async-io`, since its `tokio`
  feature would switch every zbus user in the build (gpui, accesskit) to tokio.
- `set_labels` waits for that thread to emit `ItemsPropertiesUpdated`, a signal with no
  reply, so unlike `new` it may run on the UI thread.
- Inside Flatpak (`/.flatpak-info`) the item owns no well-known name: the sandbox may not
  own `org.kde.StatusNotifierItem-*`.
- The icon is sent as pixmaps (ARGB32, network byte order), every size given, so it does
  not depend on an installed theme icon. `_` in labels is a mnemonic marker and is
  doubled.

### Windows

- `new` and drop on the UI thread: `tray-icon` creates a hidden window there and needs that
  thread's message loop (gpui's `GetMessageW` loop serves every window of the thread).
- The icon is resource 1 of the executable (the app icon `winresource` embeds), so Windows
  picks the size for the DPI; the first `IconImage` is the fallback.
- `tray-icon` and `muda` keep their event handlers in `OnceLock`s: they can be set once
  per process. `install_handlers` sets them once and they forward to `SINK`, which every
  `new` replaces. `&` in labels is a mnemonic marker and is doubled.
- `window_handle` is the hidden window: SMTC binds to it while the main window is closed.

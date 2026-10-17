# updater::install

Per-platform backends behind `download_and_stage`: download the asset (on macOS and Linux
also install it) and return a `Staged`, which `lib.rs` applies (`../doc.md`, "Behavior").

Not here: when to check, which asset, toasts (`../doc.md`); the feature-off stand-ins
(`../disabled/doc.md`).

## Files

- `mod.rs` — `Staged` (carries the installer path on Windows only), dispatch by OS,
  `finalize_on_quit` (runs the installer on Windows, no-op elsewhere), the shared blocking
  `download_file`.
- `macos.rs`, `windows.rs`, `linux.rs` — one backend each.

## Download

- `download_file` retries and resumes with `Range`; any reply but `206` restarts from zero.
  The partial file is removed only once every attempt is spent.
- The body is stream-hashed (SHA-256) and compared with the asset's `digest`. A mismatch
  re-downloads from scratch (also how a bad resume heals). Enforced only when the API gave a
  `sha256:` digest, otherwise it warns and proceeds. It covers bytes altered in transit or on
  the CDN; it is not a substitute for signing.
- ureq's timeouts are not independent: `RecvBody` is checked against `RecvResponse` too, so
  `recv_response` (15 min) caps the whole transfer and `recv_body` (60 s) is the stall
  detector, re-armed on every read. Do not shorten `recv_response` as if it were a headers
  timeout: it fails slow downloads of a healthy file.

## macOS

- The new bundle is rsynced from the mounted dmg over the running one (`app_path()`) during
  the download, so it is already new at `Ready` and the running process carries on. The bundle
  must be writable by the user.
- `relaunch` is a detached `bash` that waits for our pid to go, then for `lsappinfo find
  pid=…` to come back empty (LaunchServices has let go; capped at 5 s), then `open`s the
  bundle. `apply_and_restart` calls it, then `cx.quit()`.
- Not `cx.restart()`: gpui's script waits only for the pid. LaunchServices learns of the exit
  later, and an `open` in that gap resolves to the old, dead instance and launches nothing
  (`-600 procNotFound`); the gap grows under load. Waiting on the LaunchServices view is what
  Sparkle does.
- Not `open -n`: `crates/pawse/Info.plist` sets `LSMultipleInstancesProhibited` (macOS's
  single-instance guard; `single_instance.rs` is compiled out there), and with it
  LaunchServices ignores `-n` and picks the stale instance.

## Windows

- The `-setup.exe` goes to `cache_dir/pawse/updates` (a running `.exe` cannot be overwritten)
  and runs once, from the quit handler: detached and windowless, `/S`, plus a `start` of the
  exe after an explicit apply.
- Unverified: that `/S` is silent for the cargo-packager NSIS installer
  (`installer-mode = "currentUser"`), that it handles the running instance, and whether it
  relaunches itself (making the `start` shim redundant). Check against a generated installer
  before relying on any of it.

## Linux

- The AppImage is downloaded as `<name>.new` beside `$APPIMAGE`, set to 0755 and renamed
  over it. A permission error becomes "update Pawse the same way it was installed".

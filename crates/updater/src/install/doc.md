# updater::install

Platform install backends. `download_and_stage` returns a `Staged` handle; the caller
(in `lib.rs`) applies it — `relaunch` + `cx.quit()` on macOS (the bundle was already
rsynced at download time), or `finalize_on_quit` (run the installer) on Windows when
the app is quitting with a pending, approved update.

- `mod.rs` — `Staged` + dispatch + the shared blocking `download_file` (`ureq`),
  which stream-hashes the body (SHA-256) and verifies it against the asset `digest`
  when one is supplied. It makes up to `DOWNLOAD_ATTEMPTS` tries, resuming an
  interrupted transfer with `Range: bytes=<received>-`; anything but a `206` reply
  restarts from zero, and so does a digest mismatch (which is how a bad resume heals
  itself). The partial file is removed only once every attempt is spent.

  The ureq timeouts are not independent: `RecvBody` is checked against `RecvResponse`
  too (`Timeout::preceeding`), so `recv_response` caps the **whole** transfer rather
  than just the headers, while `recv_body` is re-armed on every read. Hence
  `recv_response` is the per-attempt ceiling (15 min) and `recv_body` the stall
  detector (60 s). The previous 60 s `recv_response` capped a 16 MB dmg at ~270 KB/s
  and failed with `timeout: receive response`.
- `macos.rs` — download the dmg to a temp dir, `hdiutil attach -mountrandom`, parse
  the mount point from stdout, `rsync -a --delete` the new bundle over the running
  one (`app_path()`), and `hdiutil detach -force` via a `Drop` guard. Apply =
  `relaunch(app_path())` then `cx.quit()`: a detached `bash` (own process group) waits
  until our pid is gone, then until LaunchServices no longer lists it
  (`lsappinfo find pid=…` empty, capped at 5 s), then runs a plain `open` on the
  bundle, now updated. Needs the bundle to be user-writable (e.g. `~/Applications`);
  `rsync`/`hdiutil`/`lsappinfo` are preinstalled.

  **Why not `cx.restart()`.** gpui's restart script waits only for the pid and then
  runs `open`. LaunchServices learns about the exit later, through runningboard, and an
  `open` that lands in that gap resolves to the old, dead instance: CoreServicesUIAgent
  logs "dev.pawse.app is already running", tries to activate it, fails with `-600
  procNotFound` and launches nothing. The 0.8.3–0.8.6 updates on macOS 26.6 all ended
  that way. The gap is load-dependent: ~30–60 ms on an idle system, 100+ ms when other
  launches hit CoreServicesUIAgent at the same moment (Orca's computer-use helper starts
  three copies of itself whenever an app quits). End-to-end test on the real
  `/Applications/Pawse.app` (0.8.5-stamped build updating to the 0.8.6 release, extra
  launches at exit to load LaunchServices): gpui's script failed 9/10, this one
  relaunched 8/8, each time after LaunchServices' `QUITTING` for the old pid.

  **Why not `open -n`.** `crates/pawse/Info.plist` sets `LSMultipleInstancesProhibited`
  (that key is macOS's single-instance guard; `single_instance` covers only
  Windows/Linux), and with it LaunchServices ignores `-n` and still picks the running —
  here stale — instance (checked on a toy app: with the key, `open -n` next to a live
  instance starts nothing). Measured: `-n` failed 8/8 in the same test. Chromium's
  relauncher uses `create_new_instance`, which only helps apps without the key; waiting
  on the LaunchServices view is what Sparkle does (`NSRunningApplication.isTerminated`).
- `windows.rs` — download the NSIS `-setup.exe` to `cache_dir/pawse/updates` (a
  running `.exe` can't be overwritten). The install runs **once**, from the app's
  `on_app_quit` handler (`finalize_on_quit` → `launch_installer`): a detached
  `cmd /C "<setup>" /S & start "" "<exe>"` that installs silently after the app has
  exited, then relaunches it.

## Verify before trusting Windows

The NSIS flags are best-effort: silent `/S`, and a `cmd /C "<setup>" /S & start ""
"<exe>"` relaunch shim. Confirm against the cargo-packager-generated installer that
`/S` is silent, that it handles/closes the running instance, and whether it
relaunches on its own (in which case the shim is redundant).

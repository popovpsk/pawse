# updater::install

Platform install backends. `download_and_stage` returns a `Staged` handle; the caller
(in `lib.rs`) applies it — `cx.restart()` on macOS (the bundle was already rsynced at
download time), or `finalize_on_quit` (run the installer) on Windows when the app is
quitting with a pending, approved update.

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
  `cx.restart()` (gpui re-`open`s the same bundle, now updated). Needs the bundle to
  be user-writable (e.g. `~/Applications`); `rsync`/`hdiutil` are preinstalled.
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

# updater::install

Platform install backends. `download_and_stage` returns a `Staged` handle; the caller
(in `lib.rs`) applies it — `cx.restart()` on macOS (the bundle was already rsynced at
download time), or `finalize_on_quit` (run the installer) on Windows when the app is
quitting with a pending, approved update.

- `mod.rs` — `Staged` + dispatch + the shared blocking `download_file` (`ureq`).
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

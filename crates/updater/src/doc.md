# updater

Auto-update from GitHub Releases, modelled on Zed's `auto_update`, for macOS (dmg), Windows
(NSIS installer) and Linux (AppImage): polls the latest release, downloads the asset for this
OS and arch, stages it, applies it on the user's click or, on Windows, at quit. The
`AutoUpdater` entity, polling, toasts and install live here; `pawse` only calls in.

Not here: the Settings switch, the app-menu item and the header update button (`pawse`:
`settings_view.rs`, `app_menu.rs`, `main_view/header.rs`, each gated on `is_supported()`);
toast strings (`ui_resources/src/doc.md`, read at toast time — this crate's only use of it).

## Files

- `lib.rs` — public API (`init`, `check_now`, `set_enabled`, `apply_and_restart`, `handle`,
  `is_supported`, the `CheckForUpdates` action, `ToastWindow`), `AutoUpdater` with its
  `Status` and poll loop (`POLL_INTERVAL`, 6 h), the toasts.
- `github.rs` — `fetch_latest`: `releases/latest`, per-OS asset choice, the asset's `digest`.
- `version.rs` — tag parsing (leading `v` stripped), `is_newer` (strictly greater).
- `install/` — per-platform download and apply (`install/doc.md`).
- `disabled/` — stand-ins when `self-update` is off (`disabled/doc.md`).

## Behavior

- `check_now` works with auto-update off and, unlike automatic polls, toasts "up to date" and
  errors. Once `Ready`, polls do nothing; a manual check re-shows the toast.
- Toasts go to the window the app's `ToastWindow` global picks (`pawse`: the active main
  window, never the mini player), or to the active window when it sets none.
- The "ready" toast is persistent with a no-op `on_click` so a click dismisses it: the player
  often sits in the background and a timed toast would be missed. It only notifies; applying
  is the header button, shown by `has_staged_update`.
- Apply: macOS replaced the bundle during the download, so apply is `install::relaunch` then
  `cx.quit()`. Linux likewise replaced the AppImage: apply sets the restart path to
  `$APPIMAGE` and calls `cx.restart()`. Windows: `apply_and_restart` only sets `apply_on_quit`
  and quits; the installer runs once, in the quit handler (`Staged::finalize_on_quit`), never
  anywhere else, so it cannot launch twice. That handler runs it only if the user applied or
  auto-update is on, so a manual check with auto-update off never installs silently, and it
  relaunches the app only after an explicit apply.
- `is_supported`: macOS and Windows installer builds always, the portable build never
  (compile-time). Linux only when `$APPIMAGE` is set and no `AM-updater` file sits next to it
  (`managed_by_am`), so `.deb`, pacman/AUR and Flatpak copies show no update UI and are left
  to what installed them. That falls out of the `$APPIMAGE` check and needs no per-package
  code; keep it so.

## Release contract

What the code and `release.yml` both rely on.

- The current version is what `pawse` passes to `init` (`CARGO_PKG_VERSION`); the workflow
  stamps the crate version to the tag (`scripts/stamp_version.py`), otherwise every check
  sees a newer build. `/releases/latest` ignores drafts and prereleases.
- The asset rules are per-OS functions taking the arch, compiled under `cfg(any(<os>, test))`
  so `cargo test` covers every platform's rule on any host. macOS matches the `.dmg` suffix
  alone; Windows `_x64-setup.exe` (x86_64) or `_arm64-installer.exe` (aarch64); Linux
  `.AppImage` containing the arch.
- Exactly one asset may end with `-setup.exe`, the x64 installer, and at most one `.dmg`.
  Clients up to 0.5.8 match Windows by a bare `ends_with("-setup.exe")` and take the first
  hit of GitHub's asset list, which is sorted by name; `arm64` sorts before `x64`, so a second
  `-setup.exe` would make x64 users silently run the arm64 installer. The digest cannot catch
  it: the wrong asset is genuine. Hence the arm64 installer is renamed `_arm64-installer.exe`
  in the workflow's collect step, and the release job fails on a second `-setup.exe` or
  `.dmg`. Installed copies cannot be fixed, so the naming is the guarantee: do not tidy it.
- Trust is our release over HTTPS plus a SHA-256 check against the asset's `digest`
  (`install/doc.md`); no code-signature verification.

## Build variants

- The Windows portable zip is built with `--no-default-features` in its own `portable` job.
  `pawse`'s `self-update` feature forwards to `updater/self-update`; off, `disabled/` replaces
  `github.rs` and `install/`, `is_supported()` is a compile-time `false`, and the optional
  network, hashing and file dependencies are not linked. `crates/pawse` has no `#[cfg]` for it.
- In this crate the feature appears in three places: the `mod github` / `mod install`
  declarations in `lib.rs`, the outer split in `is_supported()`, the gate on `managed_by_am`.
  Check all three when changing one.
- `release.yml` scans every built binary for the `pawse-updater` user agent: required in
  installer builds, forbidden in the portable one. Keep that string in the crate. The scan is
  `LC_ALL=C tr -c '[:print:]' '\n' | grep -c`, not `findstr` (exits 2 on the long lines of a
  binary, which a found-or-not guard reads as a pass) and not `grep -a` (once missed a string
  the build contained). `ci.yml` also compiles the portable configuration (`disabled/doc.md`).

## Decisions

Owner's calls; ask before changing.

- Portable is a separate compilation, not a runtime flag or marker file. A `portable.txt`
  next to the exe was dropped as fail-open: deleting it, or copying just `pawse.exe` out of
  the zip, re-enabled updating, and running the NSIS installer from an unpacked folder installs
  a second copy into `%LOCALAPPDATA%` while the portable one keeps finding the same update.
  Code that is not compiled in cannot be switched on from outside.
- Windows has no file-based opt-out: an installed copy always reports supported and the
  Settings switch is the only lever. A marker a user can create is one a user can delete.
- The digest check is fail-open (a missing or non-`sha256:` digest warns and proceeds): it is
  defense-in-depth over HTTPS, and an unrecognized future format must not brick the channel.

# updater

Self-contained auto-update for macOS (dmg) and Windows (nsis), modeled on Zed's
`auto_update`. Checks GitHub Releases, downloads the right asset, stages an install,
and applies it on the user's go-ahead. Pawse only calls `init` + wires the
`CheckForUpdates` action; everything else (entity, polling, toast, install) lives here.

## Files

- `lib.rs` — public API (`init`, `check_now`, `set_enabled`, `apply_and_restart`,
  `handle` + `AutoUpdater::has_staged_update` for the header button, the
  `CheckForUpdates` action) and the `AutoUpdater` GPUI entity. Holds a `Status`
  state machine (`Idle/Checking/Downloading/Ready`), a poll loop (`POLL_INTERVAL`,
  6h), and pushes a persistent "ready to install" toast via `gpui_component`
  (`autohide(false)`; clicking it only dismisses — the no-op `on_click` exists so a
  body click closes it, since the player often sits in the background and a timed
  toast would be missed). The toast only notifies — applying is triggered separately
  by pawse's header update button (`has_staged_update` gates it; click calls
  `apply_and_restart`). State changes call `cx.notify()` so observers (the header)
  re-render. Blocking
  network/disk work is offloaded to `cx.background_executor()`; `app_path()` is read
  on the main thread and passed into the installer.
- `version.rs` — semver parse (strips a leading `v`) + `is_newer`. Unit-tested.
- `github.rs` — `GET /repos/popovpsk/pawse/releases/latest` (blocking `ureq`,
  rustls). Parses `tag_name` + picks the per-OS asset (`*.dmg` / `*-setup.exe`).
- `install/` — platform install backends (see `install/doc.md`).

## Non-obvious behavior / contract

- **No signing.** Trust = our GitHub release over HTTPS. There is no signature
  verification; if that ever changes, add it in `install::download_file`.
- **Apply contract.** macOS rsyncs the new bundle during download, so the bundle on
  disk is already updated and apply is just `cx.restart()`. Windows downloads the
  installer and runs it **once, in the `on_app_quit` handler** — never from
  `apply_and_restart` directly (which only sets `apply_on_quit` then quits), so there
  is no double-launch. The quit handler runs the installer only when the user
  explicitly applied (clicked the header update button) **or** auto-update is enabled;
  a manual check with auto-update off therefore never silently installs on a normal
  quit.
- **Current version must equal the release tag.** The version compared is the one
  pawse passes to `init` (`env!("CARGO_PKG_VERSION")`). The release workflow must
  stamp the crate version to match the tag, or every check sees a newer build.
- **Only published releases are seen.** GitHub's `/releases/latest` ignores drafts
  and prereleases, so a drafted release is invisible until published.
- **Toasts are localized.** Runtime update notices (`up_to_date`, `update_ready_t`,
  `update_check_failed_t`) are read from `ui_resources::i18n::strings()` at toast
  time, so they follow the active language (including a live language switch). This
  is the one place the crate depends on `ui_resources`.
- **Linux is out of scope.** The crate compiles on Linux (pawse builds there for
  `.deb`/AppImage), but pawse `#[cfg]`-gates the `init` call, the menu item and the
  setting off Linux, so nothing polls and the "Check for Updates" action isn't shown.
  `select_asset` also matches nothing there as a backstop.

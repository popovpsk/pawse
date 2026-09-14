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
  rustls). Parses `tag_name` + picks the per-OS asset, carrying the asset's `digest`
  (`sha256:…`) through to the downloader. The per-OS rules are split into
  `macos_asset` / `windows_asset` / `linux_asset`, which take the arch as an argument
  and are compiled under `cfg(any(<os>, test))` so every platform's rule is exercised
  by `cargo test` on any host — the Windows rule in particular must not go untested,
  see the asset naming contract below.
- `install/` — platform install backends (see `install/doc.md`).

## Non-obvious behavior / contract

- **No signing, but digest-checked.** Trust = our GitHub release over HTTPS. There
  is no code-signature verification, but `install::download_file` stream-hashes the
  download (SHA-256) and compares it against the release asset's `digest` from the
  GitHub API: a mismatch is a hard failure and the partial file is removed. The check
  is enforced only when the API reports a `sha256:` digest (older assets predate the
  field); a missing or non-`sha256:` digest logs a warning and proceeds (fail-open by
  design — the digest is defense-in-depth over HTTPS, so an unrecognized future format
  must not brick the update channel, only fall back to the HTTPS-only posture). This is
  not a substitute for signing; it only closes "bytes altered in transit / on the CDN".
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
- **Release asset naming contract — exactly one asset may end with `-setup.exe`, and
  it must be the x64 installer.** GitHub returns release assets sorted by name (not by
  upload order — verified on v0.5.6/0.5.7/0.5.8, where the ids are unordered but the
  names are alphabetical), and `select_asset` takes the *first* match. Versions
  shipped up to 0.5.8 match the Windows asset with a bare
  `name.ends_with("-setup.exe")`. Adding a second `-setup.exe` therefore makes every
  copy already in the field pick whichever sorts first: `arm64` sorts before `x64`, so
  x64 users would silently download the arm64 installer, run it unattended from the
  quit handler (`/S`), and end up with a binary their CPU cannot execute. The digest
  check does not help — the wrong asset is a genuine asset. That is why the arm64
  installer is renamed to `_arm64-installer.exe` in the release workflow's collect
  step, and why `release.yml` fails the publish if a second `-setup.exe` (or a second
  `.dmg`, which `macos_asset` matches by suffix alone) ever appears. Fixing the
  matcher here only protects future versions; the copies already installed cannot be
  fixed, so the naming is the real guarantee and must not be "tidied up" later.
- **Windows portable builds opt out.** The portable zip ships a `portable.txt` next to
  `pawse.exe`; `is_portable()` looks for it and makes `is_supported()` false, which
  removes the menu item, the settings toggle and the poll loop. Running the NSIS
  installer from an unpacked portable folder would install a second, separate copy and
  leave the portable one untouched. Same shape as `managed_by_am` on Linux: a marker
  file means somebody else owns updating this copy.
- **Toasts are localized.** Runtime update notices (`up_to_date`, `update_ready_t`,
  `update_check_failed_t`) are read from `ui_resources::i18n::strings()` at toast
  time, so they follow the active language (including a live language switch). This
  is the one place the crate depends on `ui_resources`.
- **Linux updates only the AppImage.** `is_supported()` is true there only when
  `$APPIMAGE` is set *and* no `AM-updater` sits next to it, so a copy installed from
  `.deb`, from the pacman/AUR package, or running inside Flatpak reports unsupported
  and the whole update UI disappears — those are updated by whatever installed them.
  This falls out of the `$APPIMAGE` check and needs no per-packaging-format code;
  anything that changes the Linux branch must keep that property.

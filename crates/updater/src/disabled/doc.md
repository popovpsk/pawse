# updater::disabled

Stand-ins for `github.rs` and `install/`, compiled in their place (`#[path]` in `lib.rs`)
when `self-update` is off (the Windows portable build), so that neither the crate nor
`crates/pawse` needs a `#[cfg]` for a binary without an updater. Why the portable build is a
separate compilation: `../doc.md`, "Decisions".

## Files

- `github.rs` — `Found`, `fetch_latest()` returning an error. Never reached: `is_supported()`
  is a compile-time `false` here and `init()` returns first.
- `install.rs` — `Staged`, `download_and_stage()` (error), `finalize_on_quit()` (no-op),
  `appimage_path()` on Linux only, `relaunch()` on macOS only.

## Rules

- A hand-maintained mirror: every item keeps the name, signature and `cfg` of its counterpart
  in `../github.rs` and `../install/mod.rs`, including `appimage_path()` Linux-only and
  `relaunch()` macOS-only, because `apply_and_restart()` calls them from per-OS blocks that
  are not feature-gated. Nothing type-checks the two trees against each other.
- Drift shows only when the portable configuration is compiled. CI does that on every push to
  `main` and pull request: clippy on Ubuntu, `cargo check` on both Windows runners (the
  Windows leg matters because the real `Staged` has its `installer` field only there). No CI
  job builds it on macOS: after touching `relaunch()` run
  `cargo check -p updater --no-default-features` on a Mac.
- No network or filesystem dependencies here: `ureq`, `sha2`, `serde`, `serde_json`,
  `tempfile` and `dirs` are optional and enabled only by `self-update`; using one would put
  it back into the portable binary.

## Decisions

Owner's calls; ask before changing.

- A parallel tree, not `#[cfg(not(feature = "self-update"))]` inside `install/mod.rs` — feature
  gates would interleave with that file's per-OS gates and spread the switch over four files
  instead of one directory. The cost is the duplicated signatures above.

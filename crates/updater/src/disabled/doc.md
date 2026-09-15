# disabled

Stand-ins for `github.rs` and `install/` that are compiled in place of them when the
`self-update` feature is off. They exist so that nothing else in the crate — and
nothing at all in `crates/pawse` — needs a `#[cfg]` to build a binary with no updater
in it. See the parent `../doc.md` for why the portable build is a separate
compilation rather than a runtime flag.

## Files

- `github.rs` — `Found` + `fetch_latest()`. Returns an error; never called, because
  `is_supported()` is a compile-time `false` in this configuration and `init()` bails
  on it before any entity exists.
- `install.rs` — `Staged` + `download_and_stage()` + `finalize_on_quit()`, and
  `appimage_path()` under `cfg(target_os = "linux")`.

## Non-obvious behavior / contract

- **These files are a hand-maintained mirror.** Every item here must keep the exact
  name, signature and `cfg` of its counterpart in `../github.rs` / `../install/mod.rs`
  — including `appimage_path()` being Linux-only, since `apply_and_restart()` calls it
  from a `cfg(target_os = "linux")` block that is *not* feature-gated. Nothing
  type-checks the two trees against each other; drift shows up only when the portable
  configuration is compiled, which `ci.yml` does on ubuntu (clippy) and on both
  Windows runners (`cargo check`) for every push to `main` and every pull request.
- **Why a parallel tree instead of `#[cfg]` on the real bodies.** Putting
  `#[cfg(not(feature = "self-update"))]` inside `install/mod.rs` would interleave
  feature gates with the per-OS gates that file is already built out of, and would
  spread the switch across four files instead of one directory. The cost is the
  duplicated signatures documented above; the benefit is that the whole difference
  between the two builds is readable in one place.
- **They must carry no network or filesystem dependency.** The point of the feature is
  that `ureq`, `sha2`, `serde`, `serde_json`, `tempfile` and `dirs` are not linked at
  all — they are optional deps enabled only by `self-update`. Reaching for any of them
  here would silently put them back into the portable binary.

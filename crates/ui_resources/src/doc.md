# ui_resources

The UI **data layer**: resources the UI consumes but that are not UI logic.
Extracted from the `pawse` crate so that binary stays focused on views and app
state.

## Files

- `lib.rs` — re-exports the three modules.
- `assets.rs` — `Assets` (`rust_embed`), the `gpui::AssetSource` that embeds the
  icon SVGs from `../assets/icons/**`. Passed to `Application::new().with_assets(...)`
  in `pawse::main`. The `#[folder = "assets"]` path is relative to this crate's
  `CARGO_MANIFEST_DIR`, i.e. `crates/ui_resources/assets/`.
- `themes.rs` — bundles the theme JSON under `../themes/` via `include_str!`,
  stages them to `dirs::data_dir()/pawse/themes/`, and registers them with
  `gpui_component::ThemeRegistry`. `register_bundled_themes` takes a generic
  `on_loaded` closure, so it does not depend on `pawse`'s `SettingsStore`.
- `i18n/` — hardcoded localization tables (see below).

## Data folders

- `assets/` — `icons/` (embedded), `app-icon/` and `pawse.svg` (used by
  `cargo-packager` / `winresource`; their paths are referenced from
  `crates/pawse/Cargo.toml` and `crates/pawse/build.rs` as
  `../ui_resources/assets/...`).
- `themes/` — 21 bundled theme JSON files.

## i18n module

Compile-time-checked translation tables for 20 languages, no runtime resource
files.

- `i18n/mod.rs` — the `Strings` schema (one `SharedString` field per UI label),
  the `lang!` and `languages!` macros, the interpolation accessor methods on
  `impl Strings`, and the `Lang` enum (`code` / `display_name` / `all` /
  `strings` / `from_code` / `from_locale`).
- `i18n/<code>.rs` — one `pub static <CODE>: Strings = lang!{ ... };` per
  language (`en`, `es`, `zh`, `pt`, `ru`, `ja`, `de`, `fr`, `ko`, `it`, `tr`,
  `pl`, `nl`, `uk`, `vi`, `id`, `th`, `cs`, `sv`, `hi`).

### Contract / non-obvious behavior

- **Static vs templated.** Plain labels are read as `tr(cx).key.clone()` — the
  field is `SharedString::new_static`, so it is the `Borrowed(&'static str)`
  variant and the clone is allocation-free (safe on the render hot path). Strings
  that interpolate runtime values are stored as `*_t` fields holding `{}`
  placeholders and are read through the methods on `impl Strings`
  (`disc`, `audio_spec`, `bitrate`, `n_tracks`, `bp_*`, `failed_*`); those
  allocate a `String`, so call them off the hot path / cache the result.
- **Completeness is enforced by the compiler.** `lang!` expands to a full
  `Strings { .. }` struct literal, so a language missing or misspelling a key
  fails to compile. `Lang::strings` is an exhaustive `match`, so adding a `Lang`
  variant without a table also fails to compile. Adding a new string = add a
  field here + a line in every `<code>.rs` (compiler lists what's missing).
- **No plural rules.** `n_tracks` is two-form (n == 1 vs. otherwise). Languages
  with richer plural categories use the general form.
- **`display_name` is an endonym** shown in the language picker; it is *not*
  itself localized.
- `tr` / "which language is active" lives in `pawse` (`localization.rs` +
  `SettingsStore::language`), not here, to keep the dependency one-way
  (`pawse → ui_resources`).

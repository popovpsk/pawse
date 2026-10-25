# ui_resources

The UI data layer: embedded icon SVGs, bundled theme JSON and the hardcoded translation
tables, kept out of `pawse` so that crate holds only views and app state. It holds the
active-language cache (`i18n::set_active` / `active` / `strings`) but not the choice of
language.

Not here: resolving the saved language and announcing a change (`pawse::localization`,
`pawse/src/doc.md`), applying a theme to the window (`settings_store.rs`), drawing the icons
(`ui_components/src/doc.md`).

## Files

- `assets.rs` — `Assets`, the gpui `AssetSource` over `assets/icons/**` (rust-embed); a
  missing path is an error, not `Ok(None)`. Passed to `Application::with_assets`.
- `themes.rs` — `register_bundled_themes(cx, on_loaded)`: writes the bundled theme JSON to
  `data_dir()/pawse/themes/` and registers that folder with `ThemeRegistry`.
- `build.rs` — `rerun-if-changed` for everything under `assets/`.
- `assets/` — only `icons/` is embedded. `app-icon/` and `pawse.svg` (the master it is
  generated from, `scripts/gen-icons.sh`) are read by path from `crates/pawse/Cargo.toml`
  (cargo-packager), `crates/pawse/build.rs` (winresource) and the Flatpak files.
- `themes/` — one JSON per theme family.
- `i18n/mod.rs` — `Strings` (the schema), `lang!`, `languages!`, `Lang`, the accessor methods,
  the active-language atomic. `i18n/<code>.rs` — one `pub static <CODE>: Strings` each.
- `i18n/{cast,playlist_import,bit_perfect,view_menu,similar,artist_card,tools,spectrum,equalizer,tray}.rs` — feature
  tables: a struct, a `static` per language (all 20 in one file) and a `*_strings()` picker.

## Assets and themes

- `build.rs` is load-bearing: rust-embed does not watch the folder, so without it a new icon
  compiled into nothing and rendered blank. Check an embed with
  `grep -a "icons/<name>.svg" target/<profile>/pawse`.
- A new theme needs its JSON in `themes/` and a row in `BUNDLED_THEMES`.
- The bundled files are rewritten on every launch; edits to them in the data dir do not
  survive. `ThemeRegistry::watch_dir` loads the folder in a spawned task, then calls
  `on_loaded` once. `pawse` re-applies the saved theme there, because the registry is empty
  until that task has run.

## Translations

- `tr().key.clone()` is allocation-free (`SharedString::new_static`) and fine in `render`.
  Text with runtime values is a `*_t` field with positional `{}`, read through a method
  (`disc`, `n_tracks`, `size`, `failed_*`, the feature tables' own); those allocate, so cache
  them off the render path. `fill` pairs arguments with `{}` in order and silently drops any
  mismatch: a translation cannot reorder arguments and must keep the English count of `{}`.
  Only `cast`, `playlist_import`, `bit_perfect` and `equalizer` test this.
- The compiler enforces completeness: `lang!` is a full struct literal, `Lang::strings` and
  each `*_strings()` are exhaustive matches. `lang!` takes `plural:` and `reveal_folder:`
  first, then keys in any order.
- Adding a string: a field in `Strings` and a line in every `<code>.rs`. Adding a language:
  `<code>.rs`, a `languages!` row, a static and `for_lang` arm in each of the nine feature
  tables, and the `twenty_languages` test.
- `plural:` is a CLDR family per language (`OneOther`, `EastSlavic`, `Polish`, `Czech`) used
  only by `n_tracks` (`n_tracks_one/few/other`); no other count is pluralized.
- Per-language non-text fields: `plural`, `decimal_separator` (only `size`) and
  `reveal_folder` as `(mac, win, other)`, picked by `cfg` in `lang!`. The other per-OS text,
  `CastStrings::unreached_hint`, is picked by `cfg!` in its method.
- The persisted form of a language is `code()`; `index()` is only the atomic's encoding and
  follows the `languages!` order. `display_name` is an endonym, not localized.
  `from_locale` reads only the language prefix and falls back to English, so `zh` is the
  Simplified table and `pt` is Brazilian Portuguese.
- `view_menu.rs`' `a_to_z` / `z_to_a` follow the language's own alphabet where it has one
  (`А–Я`, `A–Ö`). `artist_card_strings_for(lang)` takes the language explicitly; the other
  pickers read `active()`.

## Decisions

Owner's calls; ask before changing.

- Hardcoded compile-time tables, no runtime resource files — the compiler checks
  completeness for every language.
- Long, feature-specific texts get their own table in their own file, not new fields in
  `Strings` — keeps the per-language files small.

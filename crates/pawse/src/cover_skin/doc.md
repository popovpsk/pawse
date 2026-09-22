# cover_skin

Generates the running theme from the current album cover: the light/dark mode and the
whole lightness scale come from the cover, the accent hue comes from its dominant colour,
and the structure — how far every token sits from the background — comes from the theme
the user picked. Off by default, switched on by `dynamic_theme` in settings.

## Files

- `mod.rs` — the `CoverSkin` entity: subscribes to engine + settings, extracts the palette
  on the background executor, writes `Theme::colors`/`tokens`/`mode`.
- `color.rs` — OKLCh ↔ `Hsla` conversion and gamut mapping.
- `palette.rs` — `CoverPalette`: median lightness, accent hue, confidence.
- `scheme.rs` — turns a base `ThemeColor` plus a `Tint` into the generated one.

## Accent selection

The cover is rasterised to 64×64. Pixels below `MIN_CHROMA` or outside the lightness band
are dropped, the rest land in 360 one-degree hue slots. Every hue is then scored over a
**sliding ±15° window** (an idea taken from Material Color Utilities' `Score`): a hue split
across a bin boundary would otherwise lose to a smaller but unsplit one — that is what made
warm covers come out blue. Windows covering less than 1% of the image are skipped; among the
rest the heaviest by Σ chroma² wins, so a small vivid area beats a large washed-out one.

Material's own scoring is proportion-dominated (`proportion·70 + (chroma−48)·0.1|0.3`), which
suits large low-chroma surfaces but picks muddy colours for an accent, so only the windowing
and the 1% cutoff were taken from it.

`confidence` is the RMS chroma of the winning window over the whole image, normalised by
`FULL_TINT_CHROMA`. Below `MIN_TINT_STRENGTH` nothing is tinted — a black-and-white cover
keeps the user's colours.

## Generation

`anchors` picks target background/foreground lightness from the cover's median lightness:
dark covers land in 0.12–0.27, light ones in 0.93–0.99 (measured from the 36 bundled themes).
The contrast span is the base theme's own, floored at `MIN_SPAN`.

`relit` maps every one of the 138 tokens through `t = (L − L_bg) / (L_fg − L_bg)` onto the new
pair. There is no separate "invert" branch: when the target pair is decreasing, the whole scale
turns over by itself, and each token keeps its relative place. Tokens are walked generically
through serde (`Hsla` serialises as `#rrggbbaa`), so nothing is missed — `every_token_survives_the_serde_walk`
guards that.

`tinted` then applies the hue. Accents are anchored to an absolute tone (0.78/0.13 dark,
0.52/0.15 light) offset by each token's distance from the theme's own `primary`, so a pale theme
still yields a punchy accent while hover/active relationships survive. What sits on the accent is
regenerated for contrast. Surfaces get the hue at a chroma scaled by strength. Body text, muted
text, danger/success/warning/info and the chart colours are never touched.

## Non-obvious behaviour

- `Theme` carries `tokens` next to `colors`, and gpui-component reads those in ~160 places, so
  both are rewritten together — otherwise checkboxes and group boxes stay in the old scheme.
- A third copy lives in the `gpui_base` theme global (scrollbars, resize handles, TextView
  defaults). `Theme::global_mut` does not touch it, so every write ends with `Theme::sync_base`;
  without it the tinted `scrollbar*` tokens never reach the painter and a flipped scheme leaves
  dark scrollbars on a light window.
- The work is keyed on `cover_art_id`, not `album_id`: tracks with no album tag all carry
  `album_id: None`, so keying on the album would freeze the skin across every untagged track,
  and per-track covers inside one album would never update.
- Surfaces are blended toward the cover in OKLab a/b, not snapped: themes whose background
  already carries more chroma than the surface target (Adventure Time, Catppuccin, Solarized
  Dark) would otherwise swing to a fully saturated cover hue on a minimum-strength tint.
- `Theme::change` is never called: it reloads the config and would wipe the generated colours.
  `theme.mode` is assigned directly so widgets branching on it (ghost button hover) stay correct.
- Every apply starts from `rebase`, which re-applies the user's theme, so tints never compound.
- Hue is never interpolated between the theme's colour and the cover's: a partial rotation lands
  on a hue present in neither (a blue theme 22% of the way to gold reads as magenta).
- `TrackEnded`/`Stopped` only restore when the queue has no current track; they fire mid-queue
  on every track change.
- The theme picker's live preview and its Escape-revert write `Theme::colors` straight through
  `apply_theme`, with no `SettingsStore` change to observe, so both close paths in
  `settings_view` call `cover_skin::reapply`. The preview itself is deliberately left raw: a
  rebase re-applies the *saved* theme, which would undo the preview being looked at.

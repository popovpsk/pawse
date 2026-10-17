# cover_skin

Generates the running theme from the current album cover: light or dark and the whole
lightness scale come from the cover's median lightness, the accent hue from its dominant
colour, and the structure — how far every token sits from the background — from the theme
the user picked. Off by default (`dynamic_theme`). One `CoverSkin` entity, held by
`MainView`; `cover_skin::reapply` is the only call from outside.

Not here: `ThemeChoice`, `apply_theme` and the setting (`settings_store.rs`); the theme
picker (`pickers.rs`); the small-cover cache (`cover_art_cache.rs`); the blurred backdrop
(`cover_backdrop.rs`, `pawse/src/doc.md`).

## Files

- `mod.rs` — `CoverSkin`: follows `PlaybackStatus` (a `track_changed` status refreshes, or
  restores when there is no track), `CatalogChanged` (re-evaluates even for the same cover)
  and settings; extracts the palette on the background executor, writes `Theme`, runs the
  crossfade.
- `color.rs` — OKLCh ↔ `Hsla`, gamut mapping.
- `palette.rs` — `CoverPalette`: median lightness, accent hue, confidence.
- `scheme.rs` — `anchors`, `relit`, `tinted`, `mix`, `matches`.

## Generation

- Keyed on `cover_art_id`, not `album_id`: untagged tracks all have no album, so an album key
  would freeze the skin across them, and per-track covers inside one album would never update.
- No thumbnail or no track restores the user's scale and fades the tint out. A cover with no
  usable accent (confidence below `MIN_TINT_STRENGTH`) gets no tint, but its lightness still
  re-lights the scale.
- Accent: each hue is scored over a sliding window, so a hue split across a bin boundary does
  not lose to a smaller unsplit one (that made warm covers come out blue). Windows under 1% of
  the image are skipped; the heaviest by Σ chroma² wins. `confidence` is the tint strength.
- `relit` maps every token's lightness linearly from the theme's background–foreground pair
  onto the pair `anchors` derives from the cover. There is no invert branch: a decreasing
  target pair turns the scale over by itself.
- Tokens are walked through serde, so none is missed. `every_token_survives_the_serde_walk`
  pins the token count; when it changes after a gpui-component bump, give the new token a role
  in `tinted`.
- `tinted`: accents sit at an absolute tone offset by each token's distance from the theme's
  own `primary`, so a pale theme still gets a punchy accent while hover / active
  relationships survive. Surfaces and text share `blend`, which scales chroma by the token's
  chroma relative to `background` or `foreground`; that keeps the theme's internal structure.
  Danger / success / warning / info and the chart series are not tinted.
- A partial rotation lands on a hue present in neither theme nor cover, so `blend` pays in
  chroma (`CHROMA_DIP`). Without it opposite-hue pairs cancel to grey. A colour with no chroma
  has no hue to rotate and takes the cover's; that ramps in over `FLAT_CHROMA` rather than
  switching, or two nearly identical greys either side of the threshold would land on hues far
  apart.

## Transition

- Track changes and stop / restore crossfade. The launch pass, the setting toggle, a theme
  change and `reapply` are instant; the launch pass would otherwise open on the plain theme
  and fade into the cover's.
- `apply` snapshots what is on screen, rebases, builds the target, re-commits the snapshot so
  the rebase never reaches a frame, then walks precomputed frames. They are built off the UI
  thread (`mix` costs two serde walks each); a tick only assigns.
- `mix` interpolates in OKLab a/b, not OKLCh: a polar lerp between two whole themes would
  sweep the accent through hues belonging to neither end.
- Every tick checks that the theme still `matches` what it last committed and gives up
  otherwise. The theme picker's hover preview writes `Theme::colors` through `apply_theme`
  with no `SettingsStore` change to observe, and a fade in flight would paint over it.

## Writing the theme

- `Theme` carries `tokens` next to `colors` (read in ~160 places); both are rewritten.
- A third copy lives in the `gpui_base` global (scrollbars, resize handles, TextView
  defaults), which `Theme::global_mut` does not touch: every write ends with
  `Theme::sync_base`.
- `Theme::change` is never called: it reloads the config and would wipe the generated colours.
  `theme.mode` is assigned directly from the background lightness.
- Every apply starts from `rebase`, which re-applies the saved theme, so tints never compound.
- The picker's hover preview and the revert on close write `Theme::colors` through
  `apply_theme`; the choice and the close in `pickers.rs` call `reapply`. The preview itself
  stays raw: a rebase would re-apply the saved theme and undo the preview being looked at.

## Decisions

Owner's calls; ask before changing.

- No `App::reduce_motion` guard on the crossfade: it is a colour crossfade, not motion.
  Switching `dynamic_theme` off removes every animation this module produces.
- The accent is the heaviest window by Σ chroma², not Material Color Utilities' proportion
  dominated score, which picks muddy accents; only its windowing and the 1% cutoff are used.

# spectrum_view

The spectrum in Cover view: one `SpectrumView` entity, held by `CoverModeView`, that reads the
active target's `SignalTap`, runs `spectrum::Analyzer` once per frame and paints one of four
styles into whatever box the cover view gives it. Also its group of settings.

Not here: the analysis itself (`spectrum/src/doc.md`), the tap and its latency
(`audio_output/src/doc.md`, "Signal tap"), where the box sits in the cover stack and how
big it is (`pawse/src/doc.md`, "Cover mode and backdrop"), the toggle under Full
screen (`main_view.rs`, `pawse/src/doc.md`, "Main view").

## Files

- `mod.rs` — `SpectrumView`: frame loop, tap hand-over, latency refresh, settings, hover;
  `block` (gap and height range per style) and the layout constants the cover view uses.
- `paint.rs` — the four styles drawn with paths, quads and shaped lines; label formatting.
- `settings.rs` — `settings_group`: Style, Tilt, Smoothing, Frame rate; Appearance shows
  it right under Cover view.

## Frame loop

- `set_wanted(true)` starts a task that calls `tick` and sleeps what it returns: to the
  next `frame_grid` tick while anything moves (`FAST` or `SLOW` by the Frame rate setting,
  `SLOWEST` with reduce motion), 250 ms when the tap has nothing new and the analyzer has
  settled. Karaoke wakes on the same `SLOW` ticks, so at 30 fps their frames coincide
  instead of adding up, and at 60 every karaoke frame is a spectrum frame; position ticks
  and the lyrics autoscroll animation still draw frames of their own. The ballistics run
  on the measured `dt`, so the curve moves at the same speed at either rate.
- The settled poll does not notify, so a paused track costs no frames; "settled" ignores
  peaks and zone levels for the styles that do not draw them. `set_wanted(false)` drops
  the task, deactivates the tap and resets the curve.
- Every notify redraws the window from the root down to this view: the main view, the
  cover view and the spectrum render each frame, while the cached header, footer, lyrics
  and queue panels are replayed (`pawse/src/doc.md`, "Cached blocks"). gpui has no visibility query, so the prepaint stamps `painted_at`: when
  no frame was painted for a second (minimized, covered, screen locked) the loop skips the
  analysis and only notifies every 250 ms, which draws as soon as the window is visible
  again. Leaving the settled poll restarts the stamp, so a resumed track is not taken for a
  hidden window.
- The cover view calls `set_wanted` from its render with "cover view active, spectrum on,
  a track, a tap" (a no-op unless the value changes), and with `false` from
  `set_active(false)`: an inactive cover view is not rendered, so its render could never
  stop the loop. Dropping the view deactivates the tap too.
- Every tick asks `Player::signal_tap` again: casting to AirPlay swaps to that output's tap,
  a renderer has none. The old tap is deactivated (its writer stops copying) before the new
  one is activated.
- While audio flows through the local `Output`'s tap, `Output::refresh_tap_latency` runs
  every 3 s on the background executor (CoreAudio property reads), so a device switch is
  picked up; a paused or settled spectrum asks nothing.
- The point count follows the plot width measured in the last prepaint (one point per 3 px
  for the minimal styles, 2 px for zones and analyzer).
- Zone readouts are re-formatted at most every 125 ms, and only for the analyzer style.

## Painting

- The canvas paints inside `entity.update`, so it reads the analyzer in place and paints
  text with the same `cx`; nothing is cloned per frame except the shaped labels and paths.
- Curves do not go through `PathBuilder`: lyon's fill and stroke tessellation was a third
  of the main thread's work per frame. `trace` flattens the curve itself (quadratics
  through the point midpoints, `FLATNESS`), the fill is a strip of trapezoids from the curve
  down to `Plot::floor` (the axis is monotonic, so they never overlap) and the stroke is a
  mitered ribbon (`MITER_MIN` limits the miter at sharp peaks), both pushed to `gpui::Path`
  as solid triangles.
- Accent is the theme's primary (the cover skin's accent when the dynamic theme is on);
  grid and labels use foreground and muted foreground. On the cover style the cover view
  measures the mean luma of the same bottom `COVER_FRACTION` of the thumbnail the curve
  lies on (`bottom_edge_luma`), once per thumbnail; above `COVER_LIGHT_LUMA` the ink is
  dark, else light. The curve is the accent itself; only when it would not read on that
  side is its OKLCh lightness moved, to at most `COVER_INK_DARK_MAX_L` or at least
  `COVER_INK_LIGHT_MIN_L`, and always `COVER_INK_GAP` away from the bottom's own
  lightness, so a mid-tone cover near the split still gets a near-white or near-black
  curve. Hue is kept, chroma as far as sRGB allows (`cover_ink`, `cover_skin::color`). The
  hover dividers and zone names stay black or white.
- Cover style spans the whole cover width. Near the bottom corners every point is held
  above the corner arc (`Plot::floor`, `COVER_RADIUS` less half the stroke) and the fill's
  bottom edge follows the same arcs: gpui clips to rectangles only, and the curve stays
  inside the rounded image because every point it bends through is inside it.
- Silhouette draws its hover labels in the 16 px under its plot, which the block height
  includes.
- The box height is the cover view's choice (`Block::fit`), so plot heights are what is
  left after each style's fixed label rows (`ZONES_PLOT`, `ANALYZER_PLOT`,
  `SILHOUETTE_LABELS`); the dB range always spans the plot.
- Labels never overlap on a narrow plot: a zone name or level is drawn only when it fits
  its zone with `LABEL_PAD` on each side ("LOW MID" spans one octave), a frequency tick
  only clear of the end labels, and the analyzer's legend only clear of the hover
  readout.
- Hover shows zone names and dividers (cover, silhouette) or the readout — frequency, note,
  level, zone (zones, analyzer). The analyzer's readout takes the right end of the legend row.
  The readout text is formatted on a new point or every 125 ms, like the zone levels, and
  only the hairline and dot follow the live curve: digits changing at frame rate cannot be
  read, and paint allocates no strings.
- A readout level below the bottom of the style's dB scale shows "—": the curve is pinned to
  the floor there, and leakage numbers like −170 dB only look like signal.
- dB values use the theme's mono font; zone names, Hz, dB and the legend are not translated.

## Decisions

Owner's calls; ask before changing.

- All four styles stay: On the cover, Silhouette (default), Zones, Analyzer.
- The toggle lives in Cover view under Full screen (stacked, not side by side: beside it,
  it landed on the cover); every other setting is a group in Appearance, right under
  Cover view, and its description does not say where the toggle is.
- The toggle icon is static: animating it would redraw the whole window.
- Cover style curve is the accent as is, relit only when unreadable (2026-10-18, chosen on
  real covers over black/white, an accent-tinted near-white and the cover's own hue).

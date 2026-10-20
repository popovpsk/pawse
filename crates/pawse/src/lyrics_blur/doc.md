# lyrics_blur

Blurred lyrics rows: rows away from the active one blur with their distance from it. One
`LyricsBlur` entity, held by `LyricsView`, owns every row's blur state, bakes rows on the
background executor and hands the view a `RowBlur` per row to draw. The pyramid itself is
built by the `text_blur` crate.

Not here: the pyramid, its geometry and the crossfade (`text_blur/src/doc.md`), the lyrics
panel, its rows, autoscroll and the hover flag (`pawse/src/doc.md`, "Lyrics"), the setting
(`settings_store.rs`, `settings_view.rs`).

## Files

- `mod.rs` — `LyricsBlur`: per-row sigma easing, bake scheduling, release; `Frame` (what
  the view passes each render), `RowBlur` (text alpha, backing alpha, the level layers),
  `measure` (the per-row canvas); the behaviour constants.
- `raster.rs` — the gpui side of a bake: `GlyphRaster` (the platform text system),
  `line_glyphs` (shaping), `GlyphCache`, `bake` (glyphs to `text_blur` and back to
  `RenderImage`s), `BlurredLine::paint`.

## Behaviour

- Off by default (`lyrics_blur`), and only for synced lyrics. While the view follows the
  song (`Frame::following`: autoscroll on and the pointer off the list) a row's target is
  its distance from the active row times `STEP` below it and `PAST_STEP` above it (sung
  rows recede faster than the ones ahead), up to `MAX_SIGMA`; before the first row goes
  active the distance counts from a row above the first, so a short intro (too short for
  the "♪ ♪ ♪" intro row) does not flash the whole text sharp. Lit rows (active, lingering)
  and untimed rows stay sharp, and every row does while the view does not follow.
- Each row's sigma eases to its target every render (`TAU`, `dt` capped at `DT_MAX`, snaps
  within `EPS`); animation frames are requested only while some row still moves, so a
  settled panel asks for no frames.
- A row with no bake for the current style is text faded by its sigma (`PENDING_FADE`).
  Backing lines are not baked: they fade out as their row blurs (`BACKING_FADE`).

## Baking

- gpui has no blur. A row's glyphs come from gpui's own `PlatformTextSystem`, taken from
  the platform before `Application::with_platform` in `main.rs` (the `GlyphRaster`
  global), placed the way `paint_line` places them (left aligned, `pixel_snap`ped line
  height, the same subpixel variants), so level 0 matches the text (off by at most a
  device pixel while a scroll moves rows by fractions of one). Painting stretches a level
  over its bounds; the atlas's bilinear sampling finishes the blur on the GPU.
- A bake is keyed by text, measured box, font, colour and scale factor. The colour is the
  inactive one and lives in the image (sprites are not tinted). Shaping needs the window,
  so it runs in `render`; each round takes at most `BATCH` stale rows, nearest the active
  row first, and the next round starts when it lands. One round is scheduled at a time:
  the rounds share the glyph cache, and on Linux the platform rasterizer takes a lock
  that gpui's own text drawing also takes. A release drops the round's task; a bake
  already running finishes in the background and is thrown away.
- `GlyphCache` keeps every glyph bitmap the rows of a song asked for, keyed by the full
  `RenderGlyphParams`; a round takes it and gives it back. It is dropped on release and
  on a style change.
- A row keeps its previous bake on screen while a resize rebakes it. A shaped height that
  disagrees with the measured box (`FIT`) gives no bake at all and is not retried until
  the box changes. Only one-byte-per-pixel glyph bitmaps are baked, emoji are left out,
  and a row with no glyph drawn gets no bake (it stays faded text).

## Drawing

- `measure` sits in every built timed row while the blur is on (the lyrics list builds
  only rows near its visible area, with a margin of at least `NEAR`, `pawse/src/doc.md`,
  "Lyrics"; a row never built is measured and baked once it comes near). In prepaint it
  records the row's box and whether the row is near the list's visible area (the content
  mask grown by `NEAR` of its height). A far row draws no layers, only faded text, so a
  long song does not lay out layers nobody sees; a row that comes near asks for the next
  frame, and a scroll faster than the margin in one frame shows faded text for that frame.
  Resizes and reveals in one frame share a single redraw request.
- Each level layer is a div carrying the opacity around a canvas: canvas does not apply
  opacity and gpui's opacity scope is crate-private.
- Atlas tiles have no padding: each level carries a transparent 1-texel ring outside the
  bounds it is drawn into, and only the bilinear filter at the edge reads it, so sampling
  never reaches a neighbour tile. A level outside the content mask is not painted, so it
  never reaches the atlas.
- Images go through `drop_atlas_tile` when the rows, the setting or a bake change, and
  once the panel has closed: `MainView` calls `LyricsView::release_blur` when the
  slide-out ends. While the panel slides out `LyricsView` stops syncing, so the rows keep
  their blur frozen instead of snapping sharp and nothing new is baked.

## Decisions

Owner's calls; ask before changing.

- The blur is baked on the CPU and stretched by gpui; no gpui fork with a blur shader and
  no wgpu render read back into gpui.
- No thread pool and no hand-written SIMD for the bake: it is already small, and a pool
  would only shorten the time to the last row.

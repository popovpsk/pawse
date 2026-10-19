# text_blur

The CPU half of blurred text: glyph coverage bitmaps in, a pyramid of four blurred BGRA
levels out, plus the crossfade that turns two neighbouring levels into any blur in between.
Pure Rust, no GPUI, no dependencies: the caller rasterizes glyphs, places them in device
pixels and paints the levels stretched over the text box.

Not here: shaping, glyph rasterization and its cache, the atlas, painting and when a row
blurs (`pawse/src/lyrics_blur/doc.md`).

## Files

- `lib.rs` — `LEVEL_SIGMAS`, `LEVELS`, `MAX_SIGMA`, re-exports.
- `mask.rs` — `Coverage` (one glyph bitmap and its offset from the pen), `Stamp` (a coverage
  at a pen position), `Mask::compose`.
- `pyramid.rs` — `bake`, `Pyramid`, `Level`, `level_factor`.
- `plane.rs` — `Plane` (one f32 channel): `halved`, `padded`, `gaussian`, `to_bgra`;
  `overlap`. Public so the bench can time each step.
- `mix.rs` — `mix`: which levels to draw at a sigma and how opaque.
- `tests.rs` — the gaussian against a plain convolution, ink and position kept on every
  level from 0.75x to 2x, tails inside the bounds, the mask grid, the crossfade.
- `benches/bake.rs` — `cargo bench -p text_blur`: a synthetic 49-line song at two font
  sizes and two scales, the whole bake and each step.

## Spec

- Coordinates are device pixels relative to the line origin. `Mask::compose` covers the
  union of the drawn coverages (a coverage that is empty or whose length is not
  width × height is skipped), rounded out to multiples of `align`, and composites them
  "over".
- Level `k` has sigma `LEVEL_SIGMAS[k] × scale` device pixels and is stored at
  `level_factor` device pixels per texel (the largest power of two not above that sigma,
  at least 1; below 1x two levels can share a factor). The mask is aligned to the last
  level's factor so every halving is exact; the gaussian at each level adds only the
  sigma that the halvings and the GPU's bilinear stretch leave out (`residual`).
- Each level has its own transparent margin: the gaussian's radius, rounded up to the next
  level's factor and never smaller than the previous level's margin. Rounding keeps a
  sharper level, halved, on the next level's texel grid; the next level's ratio is
  measured there. One margin sized for the last level would leave level 0 mostly empty.
- `Pyramid::ratios[k]` is `Σ(a·b)/Σa` of the sharper image against level `k`: level
  `k − 1` halved to level `k`'s factor, or for `k = 0` the unblurred mask at level 0's
  factor. `mix` uses it: the blurrier level at `t`, the sharper at
  `(1 − t) / (1 − t·r)`, which keeps the total ink of a linear crossfade under "over"
  blending.
- `mix`: sigma 0 is the text alone; under `LEVEL_SIGMAS[0]` level 0 fades in under the
  text; between two levels `t` is log2 of the sigma ratio; from `MAX_SIGMA` up the last
  level alone.
- `to_bgra` writes the colour into every texel and the coverage into alpha, with a
  transparent 1-texel ring around the level (`Level::width`/`height` exclude it). The
  caller draws only the inner area; the ring is there for the bilinear filter at the
  edge.
- `gaussian` is separable and zero padded. It computes the same sums in the same order as
  the plain per-pixel loop, so the output is bit-identical; it is written as one slice
  operation per kernel tap, which the compiler vectorizes, and it skips rows that are
  all zero. `overlap` sums in eight lanes for the same reason.

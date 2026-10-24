# equalizer

The parametric equalizer: bands (`Band`, `Kind`), their RBJ biquad filters, the response a
UI draws, the automatic preamp, the Equalizer APO / AutoEq text format, and the
`Processor` the engine runs on every decoded batch. Pure Rust, no I/O, no GPUI.

Not here: where in playback it runs and how the output learns about it
(`audio_engine/src/doc.md`, `audio_output/src/doc.md`), the AutoEq download
(`autoeq/src/doc.md`), the editor and its settings (`pawse/src/tools/equalizer/doc.md`).

## Files

- `band.rs` — `Band`, `Kind`, the limits (`MIN_FREQ` … `MAX_GAIN`, `MAX_BANDS`), `clamped`,
  `is_flat`.
- `filter.rs` — `Coefficients` (RBJ Audio EQ Cookbook) and their magnitude, `Response`,
  `preamp_db`.
- `processor.rs` — `Control` (the shared band list) and `Processor` (per engine).
- `apo.rs` — `parse` / `write` for `ParametricEQ.txt` and Equalizer APO filter lines.
- `tests.rs` — response, preamp, processing, crossfade, parsing.

## Spec

- Filters are biquads in transposed direct form II with f64 coefficients and state; f32
  loses low shelves at high sample rates. Shelves take a Q like Equalizer APO's `LSC` /
  `HSC`, so AutoEq profiles sound as they do there. Pass filters ignore gain.
- `is_flat` (disabled, or a gain filter under 0.005 dB) bands are skipped everywhere. No
  non-flat band means no `Chain`: `prepare` returns false and the batch is never touched
  or converted, so an enabled but flat equalizer keeps playback bit-perfect.
- Values are clamped at design time (`clamped`; NaN takes a default), and the frequency
  to 0.49 × the sample rate: settings and imported files are user input.
- The preamp is not a setting: `preamp_db` = minus the response maximum, sampled every
  1/48 octave from `MIN_FREQ` to 0.49 × the sample rate plus each band's centre, so the
  curve never boosts — also above 20 kHz, where hi-res files carry ultrasonic noise and a
  high shelf keeps rising. AutoEq's own `Preamp:` line is ignored on import (the same
  number) and written on export.
- `Control` is one per app; every engine holds a `Processor` on it. `set` bumps a
  generation unless the bands are equal to the current ones; `prepare` compares it once
  per batch (one atomic load) and takes the lock only on a change.
- A change crossfades over one batch from the old chain (or the dry signal) to the new
  one: switching coefficients under a running filter clicks. The new chain's sections take
  the state of the old section built from the same band (same index and kind), so an
  untouched narrow bass band keeps ringing instead of restarting from zero, which would
  dip its frequency for longer than a batch. A sample rate or channel change rebuilds
  without a fade (the output rebuilds its stream there anyway). Filter state otherwise
  runs on across batches and tracks, so gapless transitions stay continuous.
- After each batch, state under 1e-30 or not finite is zeroed: decaying state would turn
  denormal in long silence (slow on x86), and a NaN would never leave the filter.
- `parse` reads `Filter…:` lines only (a leading BOM is dropped); `ON`/`OFF`, `PK`/`PEQ`,
  `LSC`/`LS`, `HSC`/`HS`, `LP`/`LPQ`, `HP`/`HPQ`, `Fc`, `Gain`, `Q` or `BW Oct`; `None` (an
  empty slot, as REW writes them) is skipped. A peak needs gain and Q, shelves and passes
  default to Q 0.707. Any other filter type, and a `Channel:` other than `all`, is an
  error rather than a silent skip or merge: half of a correction, or the left channel's on
  both, is worse than none. So is a file with more than `MAX_BANDS` filters (`None`
  slots do not count): it is refused, not cut. `write` uses `LPQ`/`HPQ`, the forms that carry a Q.

## Decisions

Owner's calls; ask before changing.

- At most 32 bands (`MAX_BANDS`). Not for the DSP, which would take far more: each band
  is a row of three inputs and a point on the curve, and a file with thousands of
  filter lines would build thousands of inputs. AutoEq uses 10, REW exports about 20.
- IIR biquads, not FFT convolution: zero latency, no pre-ringing, and AutoEq's
  parametric profiles are exactly this.
- One band list for everything: a loaded AutoEq profile becomes ordinary editable bands,
  not a separate correction layer.

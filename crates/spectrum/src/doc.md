# spectrum

The analysis half of the spectrum display: a window of mono samples in, smoothed and
tilted levels at log-spaced points between 20 Hz and 20 kHz out, plus a held peak per point
and a level per frequency zone. Pure Rust (`realfft`), no GPUI, no audio I/O: the caller
fills `Analyzer::signal_mut` and calls `update` once per frame.

Not here: where the samples come from (`audio_output::SignalTap`, `audio_output/src/doc.md`),
drawing, styles and the frame loop (`pawse/src/spectrum_view/doc.md`).

## Files

- `lib.rs` — `Analyzer`, `Settings`, `ZONES`, `window_len`, the axis (`position`,
  `frequency`, `zone_of`).
- `tests.rs` — calibration, tilt, release and peak hold, Nyquist, resampling on a new point
  count.

## Spec

- The window is about 170 ms at any rate (`window_len`: next power of two, 4096–32768), so
  bass resolution does not drop on hi-res files. `signal_mut` is always `MAX_WINDOW` long;
  only its last `window_len(rate)` samples are analysed.
- Blackman window, FFT normalised by 1/N. Point levels are per-bin power averaged over
  `smoothing_octaves` around the point (a band narrower than 1.5 bins interpolates instead),
  calibrated so a full-scale sine at a bin centre reads 0 dB, then
  `+ tilt × log2(f / 1 kHz)`. A wider band reads a sine lower; that is the averaging, not a
  bug.
- Zone levels are the sum of power in the zone as dBFS RMS against a full-scale sine
  (`4·ΣP / mean(w²)`), with no tilt. They are a different quantity from the point levels;
  the analyzer style labels them separately.
- Ballistics are in dB: attack 12 ms, release `release_seconds`; peaks hold 1.4 s, then
  fall 14 dB/s. `update(None, dt)` is silence: everything releases towards `SILENCE_DB`.
- Points above Nyquist read `SILENCE_DB`; the axis stays 20 Hz–20 kHz at every rate.
- `set_points` does nothing at the same count and otherwise resamples levels, peaks and
  holds onto the new points (linear in axis position), so the caller can call it every
  frame with the plot width and a width animation never drops the curve to silence.
- `is_settled(full)` is what lets the caller stop drawing after a pause: the curve under
  −120 dB, and with `full` (a style that draws peaks and zone levels) also the peaks under
  −120 dB and every zone under −90 dB, where the readout shows "—".
- `signal_tail_mut(rate)` is the part `update` reads at that rate; filling only it saves the
  copy of the unused front.
- Mono analysis of (L+R)/2 (the tap mixes it down); no per-channel or mid/side view.
- Zones: Sub < 60 < Bass < 250 < Low mid < 500 < Mid < 2k < High mid < 6k < Air. Their names
  are English technical labels in every language, like Hz and dB.

## Decisions

Owner's calls; ask before changing.

- Defaults: tilt 4.5 dB/oct, smoothing 1/3 octave for the minimal styles and 1/6 for zones
  and analyzer; tilt and smoothing are otherwise the user's setting.

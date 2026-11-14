# loudness

How loud a track is and how much to turn it up or down: an EBU R128 / BS.1770 meter, the
quick (sampled) and full measurement of a track over any `AudioSource`, the gain rule with
its clipping ceiling, and the per-engine `GainStage` the engine runs on decoded batches.
Pure Rust, no I/O beyond the source it is handed, no GPUI.

Not here: reading ReplayGain / R128 tags (`audio_decoder/src/doc.md`), where the stage runs
and how the output learns about it (`audio_engine/src/doc.md`), what gets measured when and
where the numbers are kept (`pawse/src/normalization/doc.md`, `music_library/src/doc.md`).

## Files

- `meter.rs` — `Meter` (K-weighting, 400 ms blocks every 100 ms, gating, sample peak),
  `Measurement`.
- `measure.rs` — `Span`, `Estimate`, `full`, `quick`, the window constants.
- `gain.rs` — `REFERENCE_LUFS`, `HEADROOM_DB`, `Measurement::replay_gain`, `applied_db`,
  `factor`.
- `stage.rs` — `Switch` (the one on/off every engine reads), `GainStage`.
- `tests.rs` — meter levels and gating, the gain rule, the stage, quick vs full on a
  synthetic source and on a decoded fixture.

## Spec

- The meter is our own small BS.1770-4 implementation as libebur128 builds it (no crate to
  vendor for Flatpak): the K-weighting shelf and high-pass are
  designed per sample rate from the analog prototypes, f64 state, absolute gate −70 LUFS,
  relative gate −10 LU. Mono counts twice (dual mono): playback puts it on both speakers,
  so it sounds as loud as the same signal in stereo. 5.0, 5.1 and 7.1 use the surround
  weights (LFE 0, surrounds 1.41); other layouts weigh every channel 1. Peak is the sample
  peak, not true peak: four-times oversampling would cost more than the decode.
- `start_window` resets the filters and skips the first 100 ms before counting: after a
  seek the filters would ring on a jump in the signal. Blocks never span two windows.
- `full` decodes the span sequentially without the warm-up skip, so it agrees with ffmpeg's
  `ebur128` and rsgain to a tenth of a LU. `stop` is polled per batch. Seeks are coarse: audio
  before the asked position that a seek lands on is skipped (in `quick` windows too), so a
  cue track never meters the end of the previous one. A stream that changes
  sample rate or channels midway stops there and comes back with `full: false`, so a later
  full measurement can still replace it.
- `quick` reads `WINDOWS` windows of `WINDOW` spread evenly over the span, visited in
  bit-reversed order so any prefix is still spread across the whole track: a deadline that
  cuts it short still leaves an even sample. Fewer than `MIN_WINDOWS` measured is `None`.
  A span no longer than all windows together is measured whole and comes back `full`. The
  cost does not grow with the track's length, which is what makes an hour-long cue track
  affordable. Sample peak from windows can miss the real one.
- A span is a cue track inside its image: `start` plus `length`. Seeks are fractions of the
  source's duration, so `quick` and an offset `full` need a known duration.
- Gain is `REFERENCE_LUFS` (−18, ReplayGain 2) minus the loudness, the same scale as RG 2
  tags, R128 tags shifted by 5 dB, and Jellyfin's `NormalizationGain`. `applied_db` clamps
  to −30…+20 dB (tags are user input) and caps a boost so the peak stays `HEADROOM_DB` (3)
  under full scale; an unknown peak allows no boost at all. Cuts are never capped. Under
  0.005 dB is exactly unity.
- `GainStage` holds the track's factor and the one it last applied. A batch it has nothing
  to do for (both at unity) is left as decoded, so normalization off, or a track at 0 dB,
  stays bit-perfect. A change ramps linearly over one batch; `set_track(.., false)` jumps
  straight to the new factor (a fresh start after the output was cleared), `true` ramps
  from the previous track's factor (gapless). The switch is read once per batch.

## Decisions

Owner's calls; ask before changing.

- Track gain only, never album gain: the queue changes live (edits, the similar-tracks
  continuation), and an album's own gain needs every track of it measured.
- The same 3 dB boost ceiling for every source, full measurements and tags included, so a
  track does not change level when its quick estimate is replaced by a full one.

# exclusive/macos

Bit-perfect output on macOS: hog mode on the device, our own IOProc, and the
device's stream format switched to the track's sample rate.

## Files

- `mod.rs` — `MacosBackend`: init (hog → format → IOProc → listeners), pause /
  resume (`AudioDeviceStart`/`Stop` on our IOProc), tear-down that restores the
  original rate and releases hog unless `suppress_cleanup` is set.
- `format.rs` — reading and applying the stream format, rate changes.
- `sample_rate.rs` — the device's available rates and picking the closest one.
- `hog.rs`, `ioproc.rs`, `listeners.rs`, `sleep.rs`, `cf.rs` — hog mode, the
  IOProc, property listeners, the sleep assertion, CoreFoundation helpers.

## A rate change is asynchronous

Setting `kAudioDevicePropertyStreamFormat` to a new sample rate returns at once,
but the device switches later: on the built-in headphone output the stream
format and the nominal rate both still read the old rate right after the call
and show the new one about 105–115 ms later (measured 44.1 ↔ 96 kHz). A track
change in exclusive mode (`Output::recreate_exclusive`) drops the old backend
and builds a new one, which starts its IOProc right away. Without waiting, the
first ~110 ms of the new track played at the old rate: slowed down going
44.1 → 96, sped up going 96 → 44.1, a different sound in each direction on every
switch.

`apply_format` therefore waits after a successful set until the stream format
reports the requested rate (`wait_for_rate`, polling every 2 ms, up to 1 s; a
timeout only logs a warning), before the IOProc is created. The wait happens on
the engine thread inside `Output::write`, while the new ring buffer is still
empty, so nothing is heard during it. `set_and_wait_sample_rate`, used to
restore the original rate on tear-down, shares the same wait. mpv's exclusive
CoreAudio output waits for the same reason (`ca_change_physical_format_sync`).

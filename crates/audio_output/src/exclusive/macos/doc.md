# exclusive/macos

Bit-perfect output on macOS: hog mode on the device, our own IOProc, and the device's
stream format switched to the track's sample rate.

Not here: the facade, `render::fill`, swapping and fallback to shared
(`audio_output/src/doc.md`).

## Files

- `mod.rs` — `MacosBackend`: init, pause / resume (`AudioDeviceStart`/`Stop` on our
  IOProc), tear-down, `restore_device_state`.
- `format.rs` — reading and applying the stream format, rate changes and the wait for them.
- `sample_rate.rs` — the device's available rates and `get_best_samplerate`.
- `hog.rs` — device lookup by UID, hog mode acquire / release / owner pid.
- `ioproc.rs` — IOProc registration and the callback that calls `render::fill`.
- `listeners.rs` — property listeners: stream format, is-alive, hardware volume, mute.
- `sleep.rs` — the sleep assertion while playing. `cf.rs` — minimal CoreFoundation.
- `latency.rs` — `output_latency`: the device's output latency for the signal tap, read
  for shared and exclusive alike (`audio_output/src/doc.md`, "Signal tap").

## Init and tear-down

- Order: hog → format → IOProc → listeners. A failed step releases hog if this call
  acquired it. The IOProc is created only after the rate has settled.
- `get_device_id_by_uid` tries `TranslateUIDToDevice`, then scans all devices by UID (the
  fast path returns 0 for some hot-plugged DACs). An empty UID is the default device.
- Tear-down stops the IOProc, removes the listeners, destroys the IOProc, then — unless
  `suppress_cleanup` — restores the original rate (only if it differs) and releases hog.
  Hog is released when the *current hogging pid is ours*, not by the `hog_acquired` flag: a
  recreated backend finds our pid already holding hog and records `false`.
- `Output::recreate_exclusive` sets `suppress_cleanup` on the old backend and passes
  `original_rate` to the new one: no hog release (another app could grab the device
  mid-swap) and no rate restore in between, and the final restore returns the pre-exclusive
  rate. If the new backend fails, `restore_device_state` releases hog and restores the rate
  (without waiting) before `Output` opens a shared stream.
- Pause stops the IOProc and releases the sleep assertion; resume starts the IOProc and
  takes the assertion. `write` returns 0 unless the IOProc runs. No idle-release path — hog
  mode has no other clients.

## Format

- The IOProc assumes one interleaved packed native f32 buffer. `apply_format` reads the
  format back after setting it and returns `UnsupportedFormat` if it is not packed f32 or
  the channel count differs — the IOProc would otherwise feed f32 bytes into a buffer the
  device reads differently (noise). No mono → stereo promotion: the ring buffer is already
  sized for the caller's channels, and `Output` falls back to shared.
- Rate: `get_best_samplerate` (ported from DeaDBeeF) scores `distance × 2 + modulo` and
  multiplies the score by 100 for rates below the source: an exact match wins, else the
  nearest rate above with integer multiples favoured; downscaling is a last resort. The
  target is capped at 192 kHz; with no rate list it is used as is.
- **A rate change is asynchronous.** Setting the stream format returns at once, but the
  format and nominal rate keep reading the old rate until the device actually switches.
  `Output::recreate_exclusive` builds a new backend on a track change and it starts its
  IOProc immediately, so without waiting the first part of the new track plays at the old
  rate (slow or fast, depending on direction). `apply_format` therefore polls the stream
  format (`wait_for_rate`, every 2 ms, up to 1 s; a timeout only logs) before the IOProc
  exists. This runs on the engine thread inside `Output::write` while the new ring buffer
  is empty, so nothing is heard. `set_and_wait_sample_rate`, which restores the rate on
  tear-down, shares the wait. mpv's exclusive output waits for the same reason.

## Listeners

- Each registration leaks one `Arc` (the shared state, or a per-element volume / mute
  entry) into CoreAudio and recovers it only when unregistered. `tear_down_inner`
  unregisters before anything is freed; `AudioObjectRemovePropertyListener` blocks until an
  in-flight callback returns, so freeing right after is safe.
- Callbacks only write atomics or push an event: a format change by another process updates
  `device_sample_rate` (shown as a sample-rate mismatch, no event), is-alive going false
  sets `alive` and queues `DeviceDisconnected`, volume and mute are read from the main
  element, else per channel (lowest volume, any channel muted). We only observe hardware
  volume and mute, never set them.
- The event queue holds 32; the oldest is dropped.

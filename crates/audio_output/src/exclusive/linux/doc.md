# exclusive/linux — native sample rate through PipeWire

The Linux arm of `exclusive::Backend`. It is not an exclusive grab of the hardware (no raw
`hw:` access, no ALSA mixer): it plays through PipeWire and makes PipeWire run the graph —
and so the device — at the source rate, so nothing along the chain resamples. The
user-facing name is "Native sample rate".

Not here: the facade, `render::fill`, swapping and fallback to shared, the shared path's
PipeWire pin and the missing-plugin check (`audio_output/src/doc.md`).

## Files

- `mod.rs` — `PipewireBackend`: the ring-buffer writer thread (`render_loop`), the
  `Backend` impl, spawn and join of the threads.
- `pcm.rs` — opening the PCM: its name, the `PIPEWIRE_PROPS` guard that forces the rate,
  hw-params negotiation.
- `status.rs` — the status thread: what the device is actually doing
  (`/proc/asound/.../hw_params`) plus sink volume and mute from `pactl`, stored in atomics
  that `device_snapshot` reads lock-free.

## How the rate is forced

`pipewire-alsa` sets `node.rate = 1/<PCM rate>` on the stream it creates every time the PCM
is prepared. PipeWire treats `node.force-rate = 0` as "force whatever is in `node.rate`", so
one constant in `pcm.rs` serves every rate — nothing is recomputed per track. A forced rate
bypasses `clock.allowed-rates` and reconfigures the driver even while it runs, which is why
switching tracks reopens the DAC at the new rate.

`node.force-rate` reaches the stream only through `PIPEWIRE_PROPS`, which
`pw_stream_connect` reads from the environment. `ForceRate` sets it and restores the
previous value when `open` returns.

- The window is as small as it can be, but `set_var` still races any concurrent `getenv` in
  the process. It covers exactly one `snd_pcm_open` + `prepare`, and the writer thread is
  the only writer.
- The plugin connects its stream during `prepare`, so `configure` must run inside that
  window. Later `prepare` calls from `render_loop` (after a pause) do not reconnect, so
  they do not need the variable.
- The PCM name carries the target sink (`pipewire:NODE=<node>`), so this path does not
  depend on `PIPEWIRE_NODE`, unlike the shared cpal path. An empty or non-`pw:` UID opens
  the plain `pipewire` PCM — the default sink.

## Format

f32 only: the PipeWire graph is f32 and so is our pipeline, so `Format::FloatLE` means zero
conversions on either side. If the plugin refuses it, or negotiates a rate other than the
source's, `open` returns `UnsupportedFormat` and `Output` falls back to shared rather than
quietly converting.

## Reported device rate

- A sink backed by an ALSA card is measured from `/proc/asound/.../hw_params` only. Pause
  calls `pcm.drop()`, the substream closes and the file reads `closed` — that means
  unknown, so the status thread stores 0 and no rate is claimed. Falling back to the sink's
  `pactl` spec there would report the rate the idle PipeWire node fell back to, not what the
  DAC does while playing (a phantom mismatch on every pause).
- The `pactl` rate is used only for sinks with no ALSA card (Bluetooth and the like), where
  it is the only reading.
- `Output::bit_perfect_status` also ignores the rate while playback is stopped: an idle
  non-card sink keeps reporting its idle rate, and a device that is not running cannot be
  resampling anything.

## Threads

- Writer (`pw-exclusive`): blocking `writei` of one period at a time. Never spawns
  processes and never reads files. Pause drops the PCM; resume prepares it again. A write
  error that `try_recover` cannot fix marks the backend dead and queues `DeviceDisconnected`.
- Status (`pw-status`): 1 s tick, `pactl` every other tick — split out so process spawns
  cannot stall playback. It logs one warning (with `pw-metadata -n settings` attached) when
  the device rate stops matching the source.

Each has its own `running` flag (`LinuxShared` for the writer, `StatusShared` for the
status thread); `Drop` clears both and joins both.

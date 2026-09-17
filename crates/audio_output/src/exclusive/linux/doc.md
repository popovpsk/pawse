# exclusive/linux — native sample rate through PipeWire

The Linux arm of `exclusive::Backend`. It is *not* an exclusive grab of the
hardware (there is no raw `hw:` access here, and no ALSA mixer): it plays through
PipeWire and makes PipeWire run the graph — and therefore the device — at the
source rate, so nothing along the chain resamples. The user-facing name is
"Native sample rate".

## Files

- `mod.rs` — `PipewireBackend`: ring-buffer writer thread (`render_loop`), the
  `Backend` impl, and lifecycle (spawn/join of the writer and status threads).
- `pcm.rs` — everything about opening the PCM: the device name, the
  `PIPEWIRE_PROPS` guard that forces the rate, and hw-params negotiation.
- `status.rs` — the status thread: what the device is *actually* doing
  (`/proc/asound/.../hw_params`) plus sink volume/mute from `pactl`, stored in
  atomics that `device_snapshot` reads lock-free.

## How the rate is forced

`pipewire-alsa` sets `node.rate = 1/<PCM rate>` on the stream it creates for us
every time the PCM is prepared. PipeWire treats `node.force-rate = 0` as "force
whatever is in `node.rate`", so the single constant in `pcm.rs` is enough for
every rate — nothing has to be recomputed per track. A forced rate bypasses
`clock.allowed-rates` entirely and reconfigures the driver even while it is
running, which is why switching tracks reopens the DAC at the new rate.

`node.force-rate` can only be handed to the stream through `PIPEWIRE_PROPS`,
which `pw_stream_connect` reads from the environment. `ForceRate` sets it, and
restores the previous value when `open` returns. Two consequences:

- The window is as small as it can be, but `set_var` still races any concurrent
  `getenv` in the process. The writer thread is the only writer, and the window
  covers exactly one `snd_pcm_open` + `prepare`.
- The plugin connects its stream during `prepare`, so `configure` must run inside
  that window. Later `prepare` calls from `render_loop` (after a pause) do not
  reconnect the stream, so they do not need the variable.

The PCM name carries the target sink (`pipewire:NODE=<node>`), so this path does
not depend on `PIPEWIRE_NODE` at all — unlike the shared cpal path, which still
does. An empty or non-`pw:` UID falls back to the plain `pipewire` PCM, i.e. the
default sink.

## Reported device rate

A sink backed by an ALSA card is measured from `/proc/asound/.../hw_params` only.
Pause calls `pcm.drop()`, the substream closes, and the file then reads `closed` —
that means *unknown*, so the status thread stores `0` and no rate is claimed.
Falling back to the sink's `pactl` sample spec there would report the rate the
idle PipeWire node fell back to, which is not what the DAC does while playing and
showed up as a phantom sample-rate mismatch on every pause. The `pactl` rate is
still used for sinks with no ALSA card (Bluetooth and other non-card sinks), where
it is the only reading available.

`Output::bit_perfect_status` additionally ignores the rate while playback is
stopped: an idle non-card sink keeps reporting its idle rate, and a device that
is not running cannot be resampling anything.

## Why f32 only

The graph is f32 and our pipeline is f32, so `Format::FloatLE` means zero
conversions on either side. If the plugin ever refuses it we return
`UnsupportedFormat` and `Output` falls back to shared mode rather than quietly
converting.

## Threads

- writer: blocking `writei` of one period at a time. Never spawns processes and
  never reads files.
- status: 1 s tick, `pactl` every other tick. Split out precisely so process
  spawns can't stall playback. It logs one warning (with `pw-metadata -n settings`
  attached) when the device rate stops matching the source.

Both are joined in `Drop`; each has its own `running` flag (`LinuxShared` for the
writer, `StatusShared` for the status thread) and both are cleared there.

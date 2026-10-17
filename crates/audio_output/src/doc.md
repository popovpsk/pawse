# audio_output

The sound-card side of playback. `Output` is the facade the engine writes to: it owns one
active stream — shared (cpal) or exclusive (a platform backend in `exclusive/`) — and
swaps it when the format, device or mode changes, reporting trouble as `OutputEvent`s. The
lib root is `output.rs`. `write` converts the engine's batches to f32 before the ring buffer.

Not here: what drives the writes, fades and the idle-release deadline
(`audio_engine/src/doc.md`), the AirPlay `EngineOutput` (`cast/src/doc.md`), the device
picker and settings (`pawse/src/doc.md`), the per-platform exclusive paths
(`exclusive/macos/doc.md`, `exclusive/windows/doc.md`, `exclusive/linux/doc.md`).

## Files

- `output.rs` — `Output`, `EngineOutput`, `FadeEvent`, `OutputEvent`,
  `native_mode_available`, `pipewire_alsa_missing`.
- `cpal_stream.rs` — `AudioOutput` trait, `CpalOutputStream` (shared mode), `FadeState` /
  `apply_fade_gain`, the volume curve.
- `device.rs` — `DeviceManager`: selection by UID, enumeration, the Linux PipeWire sink list
  (`pactl`) and the `PIPEWIRE_NODE` pin.
- `default_watch.rs` — flag set when the system default output changes.
- `ring_buffer.rs` — `AudioRingBuffer`, the SPSC f32 buffer between engine and callback.
- `bit_perfect.rs` — `BitPerfectStatus` / `BitPerfectIssue`, `UNITY_VOLUME_TOLERANCE`.
- `thread_priority.rs` — `RenderThreadPriority`, `boost_decoder_thread`.
- `exclusive/` — `ExclusiveOutput` over a platform `Backend`; `render.rs` is the one render
  function all three backends share; `unsupported.rs` is the stub for other targets.

## Shared stream (`cpal_stream.rs`)

- The stream opens in the device's own default sample format. F32 is the direct path;
  I32/I16/U16/U8 are converted in the callback (S/PDIF and HDMI reject F32); any other
  format is a build error and `Output` installs a fallback. Linux asks for a ~20 ms period
  and retries with the device default if refused.
- `write` returns 0 unless the stream plays and blocks up to 10 ms on a full buffer; the
  engine retries the rest.
- `pause` does not stop a playing stream: it sets `silenced`, the callback emits zeros
  without draining the buffer, and `resume` clears it and calls `Stream::play`. `release`
  (`Output::release_paused`, called by the engine's deadline) calls `Stream::pause` on a
  stream that is not playing. There is no own "running" flag: cpal's play/pause are
  idempotent on CoreAudio, WASAPI and ALSA, and cpal may start a stream by itself at build
  (CoreAudio does; ALSA at its start threshold), so a flag would be wrong for a fresh
  stream. The callback reads `silenced` with `Acquire`, so after seeing `false` it also
  sees the fade `handle_play` set up before `resume`.
- `pause` on a stream that never played (`Idle`) calls `Stream::pause` for the same reason.
  `Output::new` pauses the startup stream; a swap while paused ends in `StreamSwap` →
  `sync_play_state` → `pause`; a swap while playing calls `resume` on the new stream, so a
  format change mid-playback does not stop and restart it.
- Known gaps: macOS starts the unit at build and `pause` stops it at once, a brief attach
  and detach — launching or switching device while paused with another app playing may
  still click (avoiding it would mean no stream until the first play). Linux: an ALSA PCM
  is PREPARED right after build, where `snd_pcm_pause` fails (cpal ignores the error); the
  engine's startup deadline releases it later, and a swap while paused is not covered.
- `FadeState` is the lock-free envelope (gain, step, target) shared with the exclusive
  backends and the cast `Render`. The callback ramps it per frame and posts a `FadeEvent`
  at the target. A finished fade-out (gain 0, no ramp) is `frozen`: silence without
  draining the buffer, so a later fade-in replays the same samples. `reset` pins unity.
- In steady state `apply_fade_gain` skips the multiply when volume × gain is within
  `UNITY_VOLUME_TOLERANCE` of 1, so a near-unity signal stays untouched; the bit-perfect
  indicator uses the same tolerance so the two cannot disagree.
- App volume goes through `calculate_volume_scaled`. `Output` keeps it (`app_volume`)
  across an exclusive round trip; exclusive ignores it (unity, slider locked).

## Swapping streams (`Output`)

- `current` is `None` only during a swap or when no device could be opened; every accessor
  handles it. A swap drops the old stream first — hog mode, an IOProc or a cpal client on
  the same device must be gone before the new one opens.
- Every deliberate swap (format change, device switch, exclusive toggle, following the
  default, disconnect recovery, `shutdown`) holds `transition` for its whole duration and a
  `StreamSwap` guard whose `Drop` re-applies the play intent to the stream it installed.
  Swaps do not nest, so the lock is taken only at the entry points.
- `is_playing` is the engine's intent (last `resume` / `pause`), not the stream's state. A
  `resume` while no stream exists is thus not lost, and the engine's fade-driven pause never
  waits for a `FadedOut` an idle stream cannot send.
- The stream is rebuilt, in the same mode, when the batch's rate, channels or bit depth
  differ from the current config. An exclusive rebuild marks the old backend
  `suppress_cleanup` (no hog release or rate restore in between) and passes the
  pre-exclusive `original_rate` on; if the new one fails, `restore_device_state`, then
  shared fallback and a `Recovered` event.
- Fallbacks only go down: selected device → system default → `DEFAULT_CONFIG` on the
  default. `set_exclusive(false)` is infallible; `set_exclusive(true)` pins the resolved UID
  first, so leaving exclusive lands on the same device (macOS may move the default while
  hog is held).
- No stream: `write` tries to open one at the batch's format at most every 2 s and sleeps
  20 ms otherwise, so the engine loop does not spin; success emits `Recovered`. `resume`
  opens nothing — a guessed format would mean a second open once the real one arrives. The
  reopen only `try_lock`s `transition`, so a swap's `None` window is never taken for "no
  device" (which would open a second stream on the same device — EBUSY on some cards — and
  show a false "Audio output opened").
- A pinned device that exists but fails to open (busy, still initializing) keeps the pin
  and waits for the next retry. One whose device is gone is cleared by
  `DeviceManager::resolve_device` and falls to the default.
- A lost device is reported by the backend (`ExclusiveEvent::DeviceDisconnected`) or by a
  cpal stream error other than an underrun. `Output::write` handles it outside the
  `current` lock (`handle_device_disconnect` takes the stream out): clear the selection,
  shared on the default. It is the one recovery path — the UI must not also call
  `set_exclusive(false)`.
- `shutdown` is not a destructor: it drops the track and any exclusive hold but keeps a
  minimal shared stream on the default device, so later calls find a valid output.

## Devices and the system default

- A device is identified by a UID string (CoreAudio UID, cpal's id elsewhere, `pw:<node>`
  for a PipeWire sink), never a cached cpal handle — those go stale on unplug. `None` means
  "follow the system default". Enumeration re-queries every time; on Linux that spawns
  `pactl`, so callers use `selected_device_uid` instead of enumerating twice.
- With nothing pinned the shared stream follows the default. macOS: nothing to do — cpal
  opens a `DefaultOutput` unit and CoreAudio moves it. Windows: an `IMMNotificationClient`
  sets a flag on `OnDefaultDeviceChanged(eRender, eConsole)`; MMDevice calls it on its own
  thread, so the COM wrapper is `Send + Sync`, and COM is STA on the thread that builds
  `Output`, as cpal does. Linux: a `pactl subscribe` child plus the `audio-default-watch`
  thread compare the default sink on each server event; the stream is pinned with
  `PIPEWIRE_NODE`, so PipeWire would not move it on its own.
- `Output::write` takes the flag once per batch (an atomic swap) and, if the output is
  shared with nothing pinned, rebuilds the stream on the new default. Writes happen only
  while playing, so a change during a pause is picked up after resume. Exclusive never
  follows: hog mode holds the device it grabbed.
- `set_follow_default(true)` — the app sets it when the header device picker is hidden —
  also makes leaving exclusive drop the pin `set_exclusive(true)` placed.
- Linux pins a sink by setting `PIPEWIRE_NODE` before building the stream. `set_var` races
  any concurrent `getenv` in the process, so this is best-effort; the realtime callback
  never touches the environment. Without `pactl`, enumeration falls back to ALSA cards
  grouped by card.

## Linux and the PipeWire ALSA plugin

- The shared path reaches PipeWire only through `pipewire-alsa`: ALSA's `default` becomes a
  PipeWire stream, `PIPEWIRE_NODE` picks the sink, and the native-rate path opens the
  `pipewire:NODE=` PCM. A running server does not mean the plugin is there — Raspberry Pi
  OS (Trixie), bare Arch, NixOS without `services.pipewire.alsa.enable` ship without it,
  Void ships it unlinked from `/etc/alsa/conf.d`. Without it ALSA's `default` is just
  card 0 (often an HDMI port with nothing attached) and device selection is silently
  ignored.
- `pipewire_alsa_missing()` = PipeWire is running (`pipewire-0` in `XDG_RUNTIME_DIR`) and
  ALSA's hints have neither a `pipewire` nor a `pulse` PCM. The socket alone proves nothing:
  on PulseAudio systems (Ubuntu 22.04 and older, Debian 11) PipeWire runs only for screen
  sharing and ALSA's `default` goes through `pulse` and plays fine. Cached per process;
  alsa-lib reads its config once anyway. `native_mode_available()` on Linux is PipeWire
  running and the `pipewire` PCM present (a `pulse`-only system does not qualify); cached
  too, since the UI asks on every render tick.

## Exclusive backends (`exclusive/`)

macOS (hog mode), Windows (WASAPI exclusive) and Linux (PipeWire at the source rate, shown
to users as "Native sample rate") implement `Backend`; `Output` treats them alike.

- `render::fill` is the one render function: silence unless playing or frozen, fade at unity
  gain, no app volume (`ExclusiveOutput::set_volume` is a no-op). It runs in the device
  callback or render thread, so everything it touches is lock-free.
- `Backend::write` returns 0 unless the backend is in `STATE_PLAYING`; `pause` and `resume`
  flip that state.
- A backend that cannot carry the source's rate and channels fails in `new`
  (`UnsupportedFormat` or a device error) and `Output` falls back to shared. Sample format:
  macOS and Linux need f32 end to end and refuse otherwise; Windows converts to the integer
  format the DAC accepts.
- The backend's own state is published as `DeviceSnapshot` atomics (hardware volume, mute,
  device rate) and a bounded event queue (32, oldest dropped).
- `suppress_cleanup`, `allow_cleanup` and `original_rate` matter on macOS only; the others
  change no persistent device state.

## Bit-perfect status

`Output::bit_perfect_status` reads atomics only, no syscalls. `write` records the source
rate and bit depth; the exclusive backend's `DeviceSnapshot` gives hardware volume, mute
and device rate. Issues: not exclusive; system volume below unity; muted; sample-rate
mismatch (only while playing and only when the device rate is known); source deeper than 24
bits (f32 keeps 24 mantissa bits); no source.

## Thread priority

`RenderThreadPriority::acquire` promotes to real time through MMCSS on Windows (the
exclusive render thread) and does nothing elsewhere; cpal's `audio_thread_priority`
feature covers the Windows shared callback. `boost_decoder_thread` (the engine thread): QoS
user-initiated on macOS, nice -5 on Linux (needs `RLIMIT_NICE`), above normal on Windows.
The rule itself is in `CLAUDE.md` (Conventions).

## Tests

`Output` and `CpalOutputStream` tests open the real default output device and fail on a
headless machine by design. The pactl JSON parser, card grouping, rate scoring and
`hw_params` parsing are device-free.

## Decisions

Owner's calls; ask before changing.

- No direct ALSA on Linux — all audio goes through PipeWire. Playing straight to ALSA cards
  was tried early and failed (crackling, dropouts, EBUSY). A missing plugin is the app's
  startup gate (`pawse/src/doc.md`), not a fallback.
- The shared path leaves resampling and DSP to the OS mixer; only exclusive paths touch the
  raw signal.
- No real-time promotion on Linux (it would need rtkit / libdbus); the decode thread is
  never real-time.

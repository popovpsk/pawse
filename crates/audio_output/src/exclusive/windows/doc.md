# exclusive/windows

Bit-perfect output on Windows: WASAPI in exclusive, event-driven mode, one
`wasapi-exclusive` render thread per stream.

Not here: the facade, `render::fill`, swapping and fallback to shared
(`audio_output/src/doc.md`).

## Files

- `mod.rs` — `WasapiBackend` and the render loop: primes one buffer, starts the client,
  then fills a buffer on every device event.
- `format.rs` — picks the first format the device accepts and initializes the client.
- `device.rs` — device lookup by endpoint id. `volume.rs` — endpoint volume and mute.
  `sleep.rs` — the sleep assertion (`SetThreadExecutionState`) while started.

## Rules

- The render thread creates and owns every COM object (MTA); `new` waits for its setup
  result, so a failure is synchronous and `Output` can fall back to shared. Drop stops the
  loop, wakes the thread and joins it.
- Format: source rate and channels, tried in order f32, s32, s24 left-justified in 32 bits,
  s16 — most DACs advertise only integer formats in exclusive mode, so float-only
  negotiation fails on capable hardware. None accepted → `UnsupportedFormat`, and `Output`
  falls back to shared (cpal resampling). The render thread produces f32 and converts on
  the way out.
- Period: the device minimum for both buffer duration and periodicity. On
  `AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED` the period is recomputed from the aligned buffer size
  and `Initialize` runs again on a fresh client — a client that failed `Initialize` cannot
  be reused.
- Pause stops the client and then resets it: queued frames are flushed so resume re-primes
  fresh data and a `clear()` during the pause is not undone by stale audio replaying.
- `AUDCLNT_E_DEVICE_INVALIDATED` while rendering marks the backend dead and queues
  `DeviceDisconnected`; any other `GetBuffer` error is transient.
- The thread takes real-time priority through `RenderThreadPriority` (MMCSS). Endpoint
  volume and mute are polled every few dozen periods into the snapshot atomics; they are
  observed only. No persistent device state changes, so `original_rate` is 0 and
  `suppress_cleanup` does nothing.
- Device lookup: `GetDevice` by the endpoint id (cpal's device id), else a scan of active
  render endpoints; an empty id is the default endpoint.

## The wake-up event is the device's event

`WasapiShared::event` is the handle given to `IAudioClient::SetEventHandle`, so the render
loop cannot tell a manual `SetEvent` from "the device has played the buffer". A manual wake
makes it call `GetBuffer` and fill a whole period early. With a short period that replaced
audio that had not been played yet, and every gapless transition clicked: the engine sends
`Play` at the seam even though the stream is already playing.

So `resume` wakes the thread only when `want_play` really changes. `pause` still wakes it,
which is harmless: the loop takes the stop branch and does not fill. Anything new that
signals this event must follow the same rule.

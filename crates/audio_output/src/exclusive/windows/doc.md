# exclusive/windows

Bit-perfect output on Windows: WASAPI in exclusive, event-driven mode, one
`wasapi-exclusive` render thread per stream.

## Files

- `mod.rs` — `WasapiBackend` and the render loop: primes one buffer, starts the
  client, then fills a buffer on every device event.
- `format.rs` — picks the first format the device accepts (f32, s32, s24 in a
  32-bit container, s16) and initializes the client.
- `device.rs`, `volume.rs`, `sleep.rs` — device lookup, endpoint volume, the
  sleep assertion.

## The wake-up event is the device's event

`WasapiShared::event` is the handle given to `IAudioClient::SetEventHandle`, so
the render loop cannot tell a manual `SetEvent` from "the device has played the
buffer". A manual wake makes it call `GetBuffer` and fill a whole period early.
With a 3 ms period that replaced audio that had not been played yet, and every
gapless transition clicked: the engine sends `Play` at the seam even though the
stream is already playing, and `resume` used to wake the thread every time.

`resume` now wakes the thread only when `want_play` really changes. `pause`
still wakes it, which is harmless: the loop then takes the stop branch and does
not fill. Anything new that signals this event must go through the same rule.

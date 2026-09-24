# audio_engine

The playback engine: one `audio-engine` thread that owns the decoder and feeds
`audio_output`, driven by `Command`s and reporting `EngineEvent`s.

## Files

- `engine.rs` — the run loop, commands, fades, seeking.
- `stream.rs` — `StreamingSource` (a decoder on its own thread for network
  sources) and the `Source` enum the loop plays from.
- `engine_manager.rs` — the GPUI-side handle.
- `types.rs` — `Command`, `EngineEvent`, `StreamTrack`.

## Streaming

A network read can block for seconds. The engine thread must never block on it,
or pause, seek and skip stop responding. So a stream is decoded on a
`stream-decoder` thread that runs up to 96 batches ahead into a bounded channel,
and the loop only polls that channel:

- `Poll::Pending` means "no audio yet". The loop then waits for commands in
  10 ms slices instead of spinning. After 250 ms without audio it emits
  `Buffering(true)`; the next batch emits `Buffering(false)`.
- A fade only advances while the output plays samples, so a pause or seek fade
  started just before a stall would never finish. After 400 ms of starvation the
  pending pause or seek is applied directly; a pause or seek requested while
  already buffering skips the fade.
- A seek is a message with an epoch. Batches decoded before it carry the old
  epoch and are dropped, so the engine never plays stale audio after a seek.
- Dropping the `StreamingSource` calls its interrupt, which wakes a reader
  blocked on the network so the decoder thread can exit.

Opening a stream (reading the headers) also happens off the engine thread, in
`pawse::services::start_track`. It first sends `Command::Prepare { play }`,
which stops the old track and shows buffering; `SetStreamTrack` arrives when the
decoder is ready. Play and pause that arrive in between are remembered
(`pending_play`) instead of being lost, since there is no track to act on yet.

APE and DSD decoders are tied to `std::fs::File`, so those formats are never
streamed: `pawse` downloads them whole first (`audio_decoder::can_stream`).

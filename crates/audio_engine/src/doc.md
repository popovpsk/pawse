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
- A read blocked on bytes that are not downloaded yet gives up as soon as a
  newer seek is queued (`MediaStream::give_up_waiting_when`, fed with "the
  control channel is not empty"). Without it, seeking into a missing part and
  back to a downloaded one would wait for the missing part first. The error of
  the abandoned read carries the old epoch and is dropped like its batches.
- Dropping the `StreamingSource` calls its interrupt, which wakes a reader
  blocked on the network so the decoder thread can exit.

Opening a stream (reading the headers) also happens off the engine thread, in
`pawse::playback_opener::PlaybackOpener::start`. It first sends `Command::Prepare { play,
track_duration }`, which stops the old track and emits
`EngineEvent::Preparing { duration }` so the UI switches to the new track (title,
cover, a disabled slider at 0 with the catalog duration) before any audio; `SetStreamTrack` arrives when the
decoder is ready. Play and pause that arrive in between are remembered
(`pending_play`) instead of being lost, since there is no track to act on yet.

`Buffering(true)` is not sent on `Prepare` itself. The moment playback was requested is kept
in `prepare_started`. While the track is still being prepared, the loop waits for commands with
`recv_timeout` up to `BUFFERING_AFTER` (250 ms) past that moment and raises buffering only on the
timeout. After a stream is installed, the first starvation continues the count from
`prepare_started` instead of restarting it. So the indicator appears 250 ms after the request no
matter which phase is slow, and an open that finishes sooner (a partly cached file, a fast LAN
server) never flashes it. This is the same rule as the stall indicator during playback.

APE and DSD decoders are tied to `std::fs::File`, so those formats are never
streamed: `pawse` downloads them whole first (`audio_decoder::can_stream`).

## Following the system default device

This lives in `audio_output` (`default_watch.rs` + `Output::write`), but the
engine thread drives it. When no device is pinned (`selected_uid == None`), the
shared stream has to follow the system default output:

- macOS: nothing to do. When the chosen device is the system default, cpal opens
  it as a `DefaultOutput` audio unit, and CoreAudio moves that unit to the new
  default by itself.
- Windows: an `IMMNotificationClient` registered on an `IMMDeviceEnumerator`
  sets a flag on `OnDefaultDeviceChanged(eRender, eConsole)`. The flag is only
  touched by that callback and by `write`, and MMDevice calls the callback on
  its own thread, so the COM wrapper is marked `Send + Sync`. COM is initialized
  STA on the thread that builds `Output`, the same model cpal uses.
- Linux: a long-lived `pactl subscribe` child and an `audio-default-watch`
  thread that reads its output. On a server event the thread compares the
  current default sink with the last one and sets the flag if it changed. The
  child is killed when `Output` is dropped, and the thread then ends on EOF. We
  need this because `resolve_device` pins the stream to the default sink with
  `PIPEWIRE_NODE`, so PipeWire does not move it on its own.

`Output::write` takes the flag once per batch, an atomic swap with no syscalls.
If it is set, the output is shared and nothing is pinned, the shared stream is
rebuilt on the new default. Writes only happen while playing, so a change made
during a pause is picked up by the first batch after resume. Exclusive mode
never follows the default, because hog mode holds the device it grabbed.

When the header device picker is hidden (`show_device_picker = false`, the
default), `Output::set_follow_default(true)` also makes leaving exclusive mode
drop the pin that `set_exclusive(true)` placed. The output then goes back to the
system default instead of staying on the previously exclusive device.

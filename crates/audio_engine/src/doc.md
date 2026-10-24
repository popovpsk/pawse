# audio_engine

The playback engine: one `audio-engine` thread owns the decoder, feeds an `EngineOutput`,
is driven by `Command`s and reports `EngineEvent`s. It does fades, seeking, cue-track
ranges, gapless hand-over, streamed sources, the equalizer pass and the idle release of
the output.

Not here: decoding (`audio_decoder/src/doc.md`), devices, modes and the ring buffer
(`audio_output/src/doc.md`), the filters (`equalizer/src/doc.md`), deciding what to open
and when — `PlaybackOpener`, `Player`, the AirPlay and cast targets (`pawse/src/doc.md`,
`pawse/src/cast/doc.md`).

## Files

- `engine.rs` — `AudioEngine` (the handle) and `AudioEngineLoop` (the thread): commands,
  fades, seeking, release deadline.
- `stream.rs` — `StreamingSource` (a decoder on its own thread for network sources) and the
  `Source` enum the loop polls (local `Decoder` or stream).
- `engine_manager.rs` — `EngineManager`, the GPUI-side handle: pumps engine events into a
  channel the UI reads and mirrors the control calls. The app reaches it through
  `pawse::cast::Player`.
- `types.rs` — `Command`, `EngineEvent`, `StreamTrack`.
- `lib.rs` — re-exports `MediaStream`, `Superseded` and `can_stream` from `audio_decoder`.
- `test/main.rs` — manual smoke binary.

The engine writes to an `Arc<dyn audio_output::EngineOutput>`: `Output` (sound cards) or
`cast::AirPlayOutput` (`cast/src/doc.md`), where a second engine runs next to the local
one. `EngineOutput` is `AudioOutput` plus the fade and `release_paused` calls the loop makes.

## The loop

- Wait policy: playing → `try_recv` (pacing comes from `write` blocking on a full buffer);
  playing but starved → `recv_timeout` in 10 ms slices; preparing with a play requested →
  `recv_timeout` up to the buffering deadline; parked with a release deadline →
  `recv_deadline`; otherwise a plain `recv`, so an idle engine never wakes up.
- A batch `write` only partly accepted is kept (`current_audio_batch`, its written front
  discarded in place) and written first.
  It survives a pause: resume continues from the exact same sample.
- `PositionChanged` goes out about every 200 ms, relative to the cue `track_start`. It is
  silent while a seek fade runs (the UI is already at the target) and after the track end.
- `SetLocalTrack` paths go through the `TrackResolver` before `Decoder::open` (the app maps
  cached server tracks to files and rejects ones not downloaded). Failures become
  `EngineEvent::Error`.
- `Seek` takes a fraction of the effective range — the cue track, or the whole file.

## Equalizer

- Every engine gets the app's one `equalizer::Control` (`with_resolver`; `new` makes an
  empty one) and runs its own `Processor` on each freshly decoded batch, after
  `trim_to_track_end` and before the first `write`. A batch is processed once: the part a
  partial write left over (`current_audio_batch`) is already processed.
- A batch the processor has no work for is left as decoded; one it filters is converted to
  F32 in place first (`AudioSamples::f32_mut`).
- On the output side only `EngineOutput::set_equalized` changes, called when
  `Processor::is_active` flips; `Output` turns it into a bit-perfect issue, the AirPlay
  output ignores it. The Tools page sets the same flag on a change, so it is right while
  paused too.
- A change reaches the ear after what the ring buffer holds (128 ms) plus the device
  latency (seconds on AirPlay); the loop never flushes for it.

## Fades

- Pause, resume and seek ramp the gain in the output callback (`begin_fade`,
  `take_fade_event`); the loop keeps feeding the buffer meanwhile. `FadeIntent` says what
  to do when the ramp lands: `PauseOut` → pause, `SeekOut` → seek, then fade back in.
- A faded-out pause does not clear the output: the unplayed samples stay so the next
  fade-in resumes from the same spot. Play during `PauseOut` reverses into a fade-in.
- Pause during `SeekOut` applies the seek first, so it pauses where the slider already is.
  A seek while playing pins the UI to the target at once; the real seek follows the ramp.
- `Play { fade_in: false }` is the gapless start (see below).

## Cue ranges

- `install` takes `start_offset` / `track_duration`: it seeks the file to `track_start`
  and sets `track_end` when that is before the file's end. The seek is coarse (up to a FLAC
  frame early), so the cut and the next start can be off by tens of ms.
- `trim_to_track_end` cuts each freshly decoded batch at `track_end`. The buffer is kept
  across tracks (gapless), so without the cut the start of the next track would play twice.
  No whole frame left before `track_end` (float position rounding can leave a fraction)
  ends the track. A batch that is empty on its own does not.

## Streaming

A network read can block for seconds; the engine thread must never block on it, or pause,
seek and skip stop responding. A stream is decoded on a `stream-decoder` thread into a
bounded channel, and the loop only polls it.

- `Poll::Pending` means "no audio yet". After 250 ms without audio the loop emits
  `Buffering(true)`; the next batch emits `Buffering(false)`.
- A fade advances only while the output plays samples, so one started just before a stall
  would never finish. After 400 ms of starvation the pending pause or seek is applied
  directly; a pause or seek requested while already buffering skips the fade.
- A seek is a message with an epoch. Batches and errors decoded before it carry the old
  epoch and are dropped, so stale audio is never played after a seek.
- The decoder thread never blocks on a full channel while a seek is queued (`deliver`
  drops the item and returns to the control channel), or a seek would hang behind it.
- A read blocked on bytes that are not downloaded yet gives up as soon as a newer seek is
  queued (`MediaStream::give_up_waiting_when`, fed with "the control channel is not
  empty"); otherwise seeking into a missing part and back would wait for the missing part
  first. The abandoned read's error carries the old epoch and is dropped like its batches.
- `StreamingSource::open` blocks until the headers are read — never call it on the engine
  thread. Dropping the source calls its interrupt, which wakes a reader blocked on the
  network so the decoder thread can exit.
- APE and DSD are never streamed (`audio_decoder::can_stream`); the app downloads them
  whole first.

## Preparing a track that is not ready

`PlaybackOpener` sends `Command::Prepare { play, track_duration }` first. It clears the
output, pauses, stops the old track and emits `EngineEvent::Preparing { duration }`, so the
UI switches to the new track before any audio. Then comes `SetStreamTrack`,
`SetLocalTrack { prepared: true }` (downloaded or cached meanwhile) or `Fail`.

- `play` is remembered as `pending_play`; Play and Pause arriving in between update it
  instead of being lost, since there is no track to act on yet. `SetLocalTrack` acts on it
  only with `prepared: true`.
- `Buffering(true)` is not sent on `Prepare`. The moment playback was requested is kept in
  `prepare_started`, and the loop raises buffering only 250 ms (`BUFFERING_AFTER`) after
  it; the first starvation after install continues that count. So the indicator appears
  250 ms after the request whichever phase is slow, and a quick open (partly cached file,
  fast LAN server) never flashes it.

## Pause and the idle release

Pause does not stop the shared stream at once. `cpal_stream` mutes it and the engine
releases it later — `RELEASE_AFTER_PAUSE`, 10 s on macOS, 1 s elsewhere. Why: stopping a
CoreAudio unit while another app plays the same device clicks or bursts (it reproduces with
bare cpal, and zeros rendered before the stop do not help); players that avoid it (mpv,
Cog, Chromium) keep the unit running with silence and stop it after an idle timeout.
Elsewhere the artifact was never heard; the delay only covers quick pause/resume and frees
the device sooner for exclusive apps and PipeWire's suspend-on-idle. The stream side of
this is in `audio_output/src/doc.md`.

- The deadline is the engine's state, not the stream's. `pause_output` (every pause on a
  user or track action) sets `release_at`; `handle_play`, the only way into `Playing`,
  clears it. The engine starts with it set, so a startup stream that never played is freed
  too. On the deadline it calls `Output::pause` and `release_paused` on whatever stream is
  current — a swap during the pause is re-paused by `StreamSwap`, and the engine does not
  care which stream it is.
- The end of a track (`TrackEnded`) and a failed stream set `release_at` without pausing:
  the ring buffer still holds the tail and the app may send the next track into the same
  running stream. At the end of the queue the deadline pauses and stops the output once the
  tail has played. This covers exclusive outputs too (`Output::pause` stops them).
- `handle_play` starts the fade-in before `resume`, so a pause without a fade (gain at
  unity) cannot leak one full-gain callback before the ramp.
- Exclusive outputs stop at once on pause: hog mode has no other clients.

## Gapless: keeping the tail

The engine counts a track as ended when its last sample is written into the ring buffer,
not when it is heard: the buffer still holds the tail. The app answers `TrackEnded` with
`SetLocalTrack` + `Play { fade_in: false }` at once, and the tail must not be thrown away.

- `end_track` sets `ended_naturally`, only if no fade was running (a pause fade left
  running into the next track would end frozen at zero). `handle_command` takes the flag
  for every command, so only a `SetLocalTrack` straight after the end sees it. That one
  resets the track state but leaves the ring buffer, the fade and `needs_flush` alone: the
  next track is written right behind the tail into the same running stream.
- Any other command in between consumes the flag, and the next `SetLocalTrack` clears as
  usual (`Stop` and `Prepare` clear themselves). The release deadline drops it too: by then
  the tail has played.
- The engine cannot tell the app's gapless `SetLocalTrack` from a user's click landing in
  the few milliseconds between `TrackEnded` and it; that click hears the tail, faded by its
  `Play { fade_in: true }`, before the chosen track.
- Not gapless: a track in another format (`Output::write` rebuilds the stream with a new
  buffer and the old tail goes with it) and a remote track that is not cached (`Prepare`
  clears the output and pauses while it downloads).

## Decisions

Owner's calls; ask before changing.

- No gapless across a format change — different albums are not expected to flow into each
  other.

# cast (app side)

Where playback goes: this computer, an AirPlay speaker, or a Chromecast/DLNA renderer.
This module makes a receiver look like the engine to the rest of the app: `Player` is the
single handle for play, pause, seek, stop and starting a track, and its merged event stream
(`Player::events()`, read by `run_engine_events_bus`) feeds now playing, the queue,
scrobbling, Discord, media keys and the web remote the same way on every target.

Not here: protocols, the media server, discovery threads and ports
(`crates/cast/src/doc.md`); the output picker popup (`audio_settings.rs`,
`pawse/src/doc.md`); the local engine (`audio_engine/src/doc.md`); `PlaybackOpener`
(`pawse/src/doc.md`).

## Files

- `mod.rs` — `Player`, `CastState`, `connect` / `disconnect`, `start_discovery` /
  `set_enabled`, event forwarding, the hand-over (`switch`), the volume modes.
- `media.rs` — a library `Track` → `cast::Media` for renderers: which file or server
  stream, the cue offset and length, title / artist / album, the large cover. A server
  track is a `cast::Source::Stream` opener (`stream_source`) that holds the first stream
  for the `Source`'s life, so the shared download is not cancelled between the probe and
  the device's first request; later calls open more readers on the same download.

Tests cover `Progress::follow` and the `media.rs` helpers only.

## Targets

There is always exactly one: `Local`, `AirPlay` or `Renderer`. Each has an id; the active id
is an atomic, and a target's events are forwarded only while it is the active one, so a
target being torn down (the local engine's `Stopped`, a closing session) never reaches the UI.

- **Local** — the app's engine and its `PlaybackOpener` (`Player::local`, also used for the
  launch-time restore).
- **AirPlay** — a second `AudioEngine` on `cast::AirPlayOutput` with its own
  `PlaybackOpener`, on the same equalizer `Control` as the local one (renderers get the
  original file, so no equalizer there). Positions are shifted back by what the speaker has not played yet
  (`AirPlayOutput::pending`; negative progress while the previous track's tail still plays).
  Now playing goes to the device on every `Loaded` on the engine event bus, looked up on a
  background task; each lookup carries a generation and only the newest is handed over, so a
  slow lookup of a skipped track cannot win. Progress (`Progress`) goes out on `Loaded`, play,
  pause, stop and on a position jump (a seek). The app's own title switches on `Loaded`,
  about 2 s before the speaker plays the track; the speaker is told only once it is heard
  (`airplay/src/doc.md`, "The next track is named when it is heard"). The device's remote
  buttons (`AirPlayOutput::commands`) run the same `services::` functions as the media
  keys, only while that target is active.
- **Renderer** — a `cast::Session`. Starting a track emits `Preparing` at once and resolves
  the media on a `cast-open` thread (`media::resolve`: the opener's locator order and
  fallbacks — local file, cached copy, server stream, or a whole download for formats that
  cannot stream); a newer start supersedes it. `Ended` becomes `TrackEnded`, so the queue
  advances exactly as with the engine; `Loaded` carries the source's sample rate and bit
  depth for the now-playing line.

## Switching (`switch`)

- The current track, position and play state are taken, the new target becomes active, the
  old one is released, and the track starts on the new target at the same position, playing
  only if it was playing. Engines start it paused and seek through `resume_at` /
  `resume_playing` once `Loaded` arrives (the launch-restore path), so nothing from the start
  of the track is heard; renderers get the position in the load. Both are reset at the start
  of every `switch`, so a resume left over from a quick earlier switch never lands on the
  next target.
- An AirPlay stream is closed on an `airplay-close` thread: the RTSP `TEARDOWN` can take
  seconds when the speaker is gone and `switch` runs on the UI thread. `Player::shutdown`
  closes it synchronously and waits up to 2 s for a renderer.
- Leaving this computer fades out instead of cutting (`fade_out_local`): a track still being
  opened is cancelled (`PlaybackOpener::cancel`, so it can never start locally), a playing
  engine is paused (which fades it) and stopped `LOCAL_FADE_OUT` later — unless this computer
  is the target again or a newer fade-out started (`local_fades`); a paused engine is stopped
  at once. AirPlay fades in like the local engine; a renderer starts the way the device does,
  since a volume ramp over SOAP would be audible steps.
- `connect` opens the receiver on `net_runtime`'s blocking pool; a second click on another receiver
  wins (`CastState::connecting`). The media server starts only for renderers (AirPlay needs the
  DACP server, which discovery starts, and two UDP ports per stream, fixed: see "Ports" in
  `crates/cast/src/doc.md`). A failed connect is a notification and changes nothing;
  `disconnect` also drops a pending connect.
- A lost receiver moves playback back to this computer, paused at the last position, with a
  notification. A receiver that answers but never connected back takes the same way back
  (`unreached` → `back_to_this_computer`) with a message box (`explain_unreached`) instead:
  the cause is on this side, usually the firewall, and a toast would read as the device
  dropping. That is `SessionEvent::NeverFetched` (a renderer that never came for the track)
  and `AirPlayOutput::unreached` (an AirPlay 2 speaker that never asked for our clock). An
  AirPlay 2 handshake that already fails that way (`cast::ConnectError::Unreached`: the Hisense TV does
  not finish the `SETUP` without clock answers) gets the same box instead of the "could not
  connect" toast, and nothing was switched. One text serves all of them, so it says only that
  the device answers but never connected back. The box gives the fix for the OS, on Linux as
  copyable `ufw` / `firewalld` commands for `cast::PORTS`. The texts are
  `CastStrings::unreached*` in every language, and the Linux sentence names the port range, so
  if `cast::PORTS` changes the strings change with it.

## Spectrum tap

`Player::signal_tap` is the tap of whatever plays: the local `Output`'s (handed to
`Player::new`), the AirPlay output's own, or `None` for a renderer, which plays the file
itself so no samples pass through the app.

## Volume

"Change the device's volume" (`cast_device_volume`, on by default) is kept in a `Cell` on the
player, so `Services::volume_locked` needs no settings lookup: exclusive mode when not casting,
`leaves_volume_to_device` when casting.

- **On.** The slider (and the web remote's) is the device's volume. Renderers report it on
  connect and on change (`CastState::volume`); AirPlay cannot be asked, so it starts at the app
  volume capped at 50 %. Nothing is saved; leaving the device brings back the app volume.
- **Off.** The device keeps its own volume. AirPlay is connected without a volume and the
  slider stays the app volume, applied to the samples before they are sent
  (`AirPlayOutput::set_gain` via `Player::set_app_volume`) and saved as usual. A renderer
  fetches the file itself, so there is nothing to scale: the slider is locked and parked at
  full, as in exclusive mode.
- Switching the mode while casting to AirPlay applies at once (off: gain = app volume, the
  speaker stays where it was and its volume is forgotten so a reconnect sends none; on: gain
  back to full, app volume capped at 50 % sent). `activate` applies the mode and volume once
  more to a speaker that just connected, so a change made during the handshake is not lost.
- Renderers report their volume in both modes, so `CastState::volume` is current when the mode
  comes back on; `effective_volume` shows it only while the mode is on.

## Discovery

- `cast_enabled` ("Stream to network devices", on by default) off: no Streaming section, and
  `start_discovery` / `connect` do nothing. Turning it off while casting runs `disconnect`,
  `drop_server` (the next renderer starts a new media server) and drops the `Discovery` from
  `CastState`, which stops its threads; the task copying its receivers into `CastState` holds
  it weakly, so that drop is the last. The DACP server stays until quit (the `cast` crate
  starts it once per run).
- `start_discovery` runs when the output picker opens, not at launch; later openings refresh
  at most every 20 s. The active receiver and the one being connected to are passed to the
  discovery (`mark_in_use` → `Discovery::set_in_use`, on every switch and connect), so they
  are never checked nor dropped meanwhile.
- `CastState::searching` (`mark_searching`, newest round wins) drives "Looking for devices…"
  for a fixed time after each search; then "No devices found" if the list is still empty.

## Decisions

Owner's calls; ask before changing.

- Renderers get the original file, never a volume-scaled stream: scaling would mean
  converting every track to PCM, and a renderer reads far ahead of what it plays (often the
  whole file), so a change would come much later still. Hence the locked slider with the
  device volume mode off.
- Discovery starts when the picker opens, not at launch: no multicast until the user looks
  for a device.
- An unreached receiver (`NeverFetched`, AirPlay's `unreached`) is a message box with the
  firewall fix, not a toast.

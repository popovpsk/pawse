# cast (app side)

Where playback goes: this computer, an AirPlay speaker, or a Chromecast/DLNA
renderer. The protocols and the media server are in the `cast` crate; this
module makes a receiver look like the engine to the rest of the app.

## Files

- `mod.rs` — `Player`, `CastState`, `connect`/`disconnect`,
  `start_discovery`, the event forwarding and the hand-over.
- `media.rs` — a library `Track` → `cast::Media` for renderers: which file or
  server stream, the cue offset and length, title/artist/album, the large
  cover.

## Player

`Services::player` replaced the `EngineManager` handle (the local engine's
`EngineManager` lives inside the player, `Player::local`, for the launch-time
restore). Everything that used to call the engine (play, pause, seek, stop,
starting a track — `Services::start_track` calls `Player::start`) calls the
player,
and `run_engine_events_bus` reads `Player::events()`, so now playing, the
queue, scrobbling, Discord, media keys and the web remote work the same on
every target.

There is always one target:

- **Local** — the app's engine and its `PlaybackOpener`, as before.
- **AirPlay** — a second `AudioEngine` running on `cast::AirPlayOutput`, with
  its own `PlaybackOpener`. Its positions are shifted back by what the speaker
  has not played yet (`AirPlayOutput::pending`).
- **Renderer** — a `cast::Session`. Starting a track emits `Preparing` right
  away and resolves the media on a `cast-open` thread (the same locator order
  and fallbacks as the opener: local file, cached copy, server stream, or a
  whole download for formats that cannot stream); a newer start supersedes it.
  Session events are mapped to `EngineEvent`s; `Ended` becomes `TrackEnded`,
  so the queue advances exactly as with the engine. `Loaded` carries the
  source's sample rate and bit depth for the now-playing line.

Every target has an id; the active id is an atomic. Each target's events are
forwarded only while it is the active one, so a target being torn down (the
local engine's `Stopped`, a closing session) never reaches the UI.

## Switching (`switch`)

The current track, position and play state are taken, the new target becomes
active, the old one is stopped and released (the local engine stops, an AirPlay
engine and its stream are shut down, a session tells the device to stop), and
the track is started on the new target at the same position, playing only if
it was playing. Engines start it paused and seek through `resume_at` /
`resume_playing` once `Loaded` arrives (the restore-at-launch path), so nothing
from the start of the track is heard; renderers get the position in the load.
`resume_at` / `resume_playing` are reset at the start of every switch, so a
resume left over from a quick earlier switch never seeks or starts playback on
the next target.

Releasing an AirPlay target closes its stream on an `airplay-close` thread: the
RTSP `TEARDOWN` can take seconds when the speaker is gone, and the switch runs
on the UI thread. On quit (`Player::shutdown`) it is closed synchronously.

`connect` opens the receiver on the background executor (a Chromecast app
launch or an AirPlay RTSP handshake takes a moment). The media server is
started only for renderers, so AirPlay alone never opens a listening socket
(nor a firewall prompt); the picker shows
"Connecting…" meanwhile, and a second click on another receiver wins. A failed
connect is a notification and nothing changes. Choosing a local device in the
picker disconnects first. On quit `Player::shutdown` stops a renderer and waits
up to 2 s for it.

When a receiver is lost (see the `cast` crate), playback moves back to this
computer, paused at the last position, with a notification.

## Volume

While casting, the volume slider (and the web remote's) is the device's volume:
renderers report it on connect and when it changes on the device
(`CastState::volume`); AirPlay cannot be asked, so it starts at the app volume
capped at 50 %. None of it is saved; leaving the device brings back the app
volume. `Services::volume_locked` is exclusive mode only when not casting.
The sleep timer's fade only touches the local output.

## Discovery

`start_discovery` runs when the output picker opens (`on_open_change`), not at
launch: no multicast until the user looks for a device. Later openings ask for
a fresh SSDP search and mDNS browse at most every 20 s (after a sleep the list
may have been emptied). "Looking for devices…" shows for the first 6 s and for
4 s after each refresh (`mark_searching`, the newest round wins), then "No
devices found" if the list is still empty.

## Picker

Below the local devices, a "Streaming" section lists receivers with an icon
per kind (`cast.svg`, `airplay.svg`, `network-speaker.svg`). The active one has
the check mark (local devices lose theirs), and the picker button shows the
receiver's icon with "Playing on …" (`AudioSettings::casting_to`, rebuilt only
when `CastState` changes, not in `render`). The exclusive-mode button and the
bit-perfect indicator are hidden while casting: they describe the local
output.

## Testing

Nothing here has GUI-less tests beyond `media.rs`; the crate's tests cover the
protocols. The hand-over, picker and volume were checked by running the app in
a scratch `HOME` with the web remote on and a temporary hook (since removed)
that connected receivers by name, against real DLNA and AirPlay receivers.
The picker itself has not been clicked through by an automated run.

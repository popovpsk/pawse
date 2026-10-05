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
  has not played yet (`AirPlayOutput::pending`). The device gets now playing
  too: on every `Loaded` on the engine event bus (by then the queue's current
  track is the loaded one, as for `media_bridge`) the title, artists, album
  and large cover are looked up on a background task and handed to
  `set_now_playing` (`publish_now_playing`; the subscription lives in the
  target). Each lookup carries a generation and only the newest one is
  handed over, so after a quick skip a slower lookup of the previous track
  cannot win. Progress is sent from the engine's own positions in
  `forward_airplay` (`Progress`): on `Loaded`, play, pause and stop, and when
  a position is more than 1.5 s off where the last one should have moved
  (a seek), as the engine position minus `pending()`, which is negative while
  the previous track's tail still plays. Remote buttons from the device
  (`AirPlayOutput::commands`) run `services::play`, `pause`,
  `toggle_play_pause`, `play_next` and `play_previous`, like the media keys,
  while the target is active.
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

Leaving this computer fades out instead of cutting (`fade_out_local`): a track
still being opened is cancelled (`PlaybackOpener::cancel`, so it can never
start on the local engine), a playing engine is paused, which fades it out
over 300 ms, and it is stopped 400 ms later unless this computer is the target
again by then or a newer fade-out has started (`local_fades`), so a quick
back-and-forth never stops the engine in the middle of the next fade. A paused engine is stopped at once. On the receiver side,
AirPlay fades in like the local engine (it is the same engine); a renderer
starts the way the device does, since a volume ramp over SOAP would be audible
steps.

Releasing an AirPlay target closes its stream on an `airplay-close` thread: the
RTSP `TEARDOWN` can take seconds when the speaker is gone, and the switch runs
on the UI thread. On quit (`Player::shutdown`) it is closed synchronously.

`connect` opens the receiver on the background executor (a Chromecast app
launch or an AirPlay RTSP handshake takes a moment). The media server is
started only for renderers (AirPlay needs only the DACP server, which
discovery starts, and two UDP ports per stream; the ports are fixed, see the
`cast` crate's doc, Ports); the picker shows
"Connecting…" meanwhile, and a second click on another receiver wins. A failed
connect is a notification and nothing changes. Choosing a local device in the
picker disconnects first. On quit `Player::shutdown` stops a renderer and waits
up to 2 s for it.

When a receiver is lost (see the `cast` crate), playback moves back to this
computer, paused at the last position, with a notification.

A renderer that answers every poll but never came for the track
(`SessionEvent::NeverFetched`) takes the same way back (`back_to_this_computer`,
shared with `lose`), so the user only has to go one way once the cause is
fixed, but is told with a message box, not a toast: a toast would read as the
device dropping, and the cause is on this side, the firewall most of the
time. The box (`explain_unreached`, an `AlertDialog`) says what happened and
gives the fix for the OS it runs on: Windows Security's "allow an app", the
macOS Firewall options, or on Linux the `ufw` / `firewalld` commands for
`cast::PORTS`, as copyable rows (`pipewire_alsa_gate::command_row`). The
texts are `CastStrings::unreached*` in all 20 languages, with the Linux
sentence and the commands naming the port range; if `cast::PORTS` changes,
the strings and the README's Firewall section change with it.

## Volume

Settings → General → Streaming → "Change the device's volume"
(`cast_device_volume`, on by default). The player keeps it in a `Cell`
(`Player::device_volume`, set at launch and by `volume_mode_changed`), so
`Services::volume_locked` needs no settings lookup.

- **On.** While casting, the volume slider (and the web remote's) is the
  device's volume: renderers report it on connect and when it changes on the
  device (`CastState::volume`); AirPlay cannot be asked, so it starts at the
  app volume capped at 50 %. None of it is saved; leaving the device brings
  back the app volume.
- **Off.** The device keeps its own volume. AirPlay is connected without a
  volume (no `SET_PARAMETER volume` at all) and the slider stays the app
  volume: it is applied to the samples before they are sent
  (`AirPlayOutput::set_gain`, through `Player::set_app_volume`), set on the
  local output too and saved as usual. A change is heard after what the
  speaker already holds (about 2.5 s: the queue and the AirPlay latency), as
  a step. A renderer fetches the file itself, so there is nothing to scale:
  the slider is locked and parked at full, as in exclusive mode
  (`Player::leaves_volume_to_device`). Scaling would mean converting every
  track to PCM (no original file any more), and a renderer reads far ahead of
  what it plays, often the whole file, so a change could come much later
  still.
- Switching it while casting to AirPlay takes effect at once: off sets the
  gain to the app volume and leaves the speaker where it was (and forgets its
  volume, so a reconnect sends none); on sets the gain back to full and sends
  the app volume capped at 50 %. `activate` applies the current mode and app
  volume once more to a speaker that just connected, so a mode or slider
  change made during the handshake is not lost. Renderers report
  their volume in both modes (`CastState::volume` follows it either way), so
  it is current when the mode comes back on; `effective_volume` shows it only
  while the mode is on.

`Services::volume_locked` is exclusive mode when not casting and
`leaves_volume_to_device` when casting. The sleep timer's fade only touches
the local output.

## Discovery

Casting can be turned off (Settings → General → Streaming → "Stream to network
devices", `cast_enabled`, on by default): the picker then has no Streaming
section, and `start_discovery` and `connect` do nothing. Turning it off while
casting moves playback back to this computer (`disconnect`), drops the media
server (`drop_server`; the next renderer starts a new one) and drops the
`Discovery` from `CastState`, which stops its threads (`set_enabled`); the task
that copies its receivers into `CastState` holds it weakly, so that drop is
the last one. The DACP server and its announcement stay until quit: the
`cast` crate starts them once per run.

`start_discovery` runs when the output picker opens (`on_open_change`), not at
launch: no multicast until the user looks for a device. Later openings ask for
a fresh search (SSDP, mDNS, unicast queries and a check of every receiver not
heard from within 6 s) at most every 20 s. Discovery is told which receivers
are in use, the active one and the one being connected to
(`CastState::mark_in_use` → `Discovery::set_in_use`, on every switch and
connect), so they are never checked nor dropped meanwhile. "Looking for devices…" shows for the first 6 s and for
4 s after each refresh (`mark_searching`, the newest round wins), then "No
devices found" if the list is still empty.

## Picker

Below the local devices, a "Streaming" section lists receivers with an icon
per kind (`cast.svg`, `airplay.svg`, `dlna.svg`, the DLNA mark drawn in a circle). The active one has
the check mark (local devices lose theirs), and the picker button shows the
receiver's icon with "Playing on …" (`AudioSettings::casting_to`, rebuilt only
when `CastState` changes, not in `render`). The exclusive-mode button and the
bit-perfect indicator are hidden while casting: they describe the local
output. Like the add-to-playlist popup, the picker's background is
`cover_backdrop::popover_bg`, which follows the interface opacity setting over
the blurred cover but never goes below 90 % (`POPOVER_MIN_OPACITY`): a list of
devices or playlists has to stay readable.

## Testing

Nothing here has GUI-less tests beyond `media.rs`; the crate's tests cover the
protocols. The hand-over, picker and volume were checked by running the app in
a scratch `HOME` with the web remote on and a temporary hook (since removed)
that connected receivers by name, against real DLNA and AirPlay receivers.
The picker itself has not been clicked through by an automated run.

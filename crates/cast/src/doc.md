# cast

Playing the library on network receivers: Chromecast, DLNA/UPnP renderers and AirPlay
speakers. GPUI-free. The protocol clients are `chromecast`, `airplay` and the renderer half
of `dlna`; this crate is what they share: discovery, the media server, deciding what bytes a
device gets, and the session that drives a renderer. Two ways a receiver gets audio:

- **Renderers pull** (Chromecast, DLNA): the device gets a URL on our HTTP server and fetches
  the track itself; we send play/pause/seek and poll. `Session` is that remote player.
- **AirPlay is pushed**: the sender streams PCM in real time, so a speaker is an engine
  output. `AirPlayOutput` implements `audio_output::EngineOutput` and the app runs a second
  `AudioEngine` on it; gapless, cue tracks, DSD and fades work unchanged.

Not here: `Player`, `CastState`, target switching, volume modes, user-facing messages
(`crates/pawse/src/cast/doc.md`); the wire protocols (`crates/chromecast/src/doc.md`,
`crates/airplay/src/doc.md`, `crates/dlna/src/doc.md`); the engine
(`crates/audio_engine/src/doc.md`).

## Files

- `lib.rs` — `connect` (`Receiver` → `Session`; refuses AirPlay), re-exports.
- `discovery.rs` — `Discovery`, `Receiver`; `unicast_mdns.rs` — legacy unicast mDNS queries.
- `server.rs` — `MediaServer`. `media.rs` — `Media`, `probe`, `plan`, format tables.
  `pcm.rs` — `PcmReader` (WAV or raw L16 with byte ranges). `flac.rs` — a seek table for a
  FLAC that has none.
- `session.rs` — `Session`, its worker, the `Driver` trait; `dlna_driver.rs`,
  `chromecast_driver.rs` — the two drivers.
- `airplay_output.rs` — `AirPlayOutput`; `dacp.rs` — the DACP server for AirPlay remote
  buttons.
- `net.rs` — `PORTS`, `listen`, `local_ip_for` (the interface address that routes to a
  device; goes into media URLs), interface helpers.
- `tests.rs`, `fake_dlna.rs` — see Testing.

## Ports

Devices connect back to us: renderers fetch the track (TCP), AirPlay speakers send to our
control and timing ports (UDP), remote buttons arrive at DACP (TCP). A firewall that drops
incoming connections leaves discovery working, so the device is listed and never plays.

- Everything a device connects to takes the first free port of `PORTS` (39831–39840): TCP for
  the media server and DACP (`net::listen`), UDP for AirPlay control and timing (two per
  stream; a reconnect or a switch between speakers can hold two streams for a moment). A
  taken range falls back to a random port with a warning. Same range on every platform: the
  macOS and Windows firewalls allow an app, not a port.
- The user learns the range from the app's message box on an unreached device
  (`SessionEvent::NeverFetched`, `AirPlayOutput::unreached`; `crates/pawse/src/cast/doc.md`,
  "Switching"), whose Linux text names `PORTS`; change both together.
- Discovery replies are not covered (they come back to random ports); behind such a firewall
  discovery lives on multicast mDNS, SSDP NOTIFY and unicast queries to known hosts, whose
  answers connection tracking lets in.

## Media server

- The app runs one at a time, on IPv6 and IPv4 at once; media URLs are IPv4, so a device
  reachable only over IPv6 is an error. Dropping the last `Arc` stops it.
- A path is `/<random token>/<n>.<ext>` and only what a session published (its track and
  cover) is served. A session removes only its own entries, so one being torn down never
  takes files from the next. The LAN is otherwise trusted, like the web remote.
- DLNA headers are always sent (`transferMode.dlna.org`, `contentFeatures.dlna.org` with
  `DLNA.ORG_OP=01`, `CI=1` for converted media); some TVs refuse media without them. The
  write timeout is 30 minutes since a paused device stops reading.
- `was_requested(path)`: set by any `GET`/`HEAD` that finds a published path (track or
  cover), cleared on unpublish. It proves the device can reach us (`NeverFetched`).
- A server track is a `Source::Stream` opener: each request opens its own reader on the same
  download.

## What a device gets (`plan`)

`probe` opens a decoder (also over the network). The codec comes from the decoder, not the
extension (an `.m4a` is AAC or ALAC). The driver's `Accepts` says what the device takes.

- A whole file in a format the device takes is sent as it is (`Delivery::Original`).
  Chromecast takes MP3, AAC, FLAC up to 24 bit, Opus in WebM and WAV, up to 96 kHz and two
  channels. A DLNA renderer takes what its `GetProtocolInfo` sink list names (`dlna_mimes`);
  with no sink list it is assumed to take MP3, FLAC and WAV.
- Everything else is PCM (`Delivery::Pcm`): ALAC, APE, WavPack, DSD, formats over the limits
  and **every cue track**, since a device can only play a whole file (the first track of an
  image too). WAV, or `audio/L16` for a renderer that lists L16 but no WAV. The length is
  exact up front (cue or decoder); an unknown length cannot be converted and `plan` fails.
- PCM stays within `pcm_limits` (Chromecast 96 kHz / 2 channels, DLNA 192 kHz / 8): the rate
  is halved until it fits (`rubato`), extra channels are downmixed, otherwise samples pass
  through. This is a stream to a device, so there is no OS resampler to defer to.
- `PcmReader::open(spec, offset)` seeks the decoder coarsely to 50 ms before the offset's
  frame (the seek takes an `f32` ratio, which on an hour-long cue image can land a few frames
  late) and skips to the exact one. Lossless sources give the same bytes as a straight decode;
  a converted stream restarts its resampler per range (continuous, not bit-identical). A short
  decoder is padded with silence and a long one cut, so the body matches the header's length.
- **A Chromecast gets Ogg (Opus, Vorbis) as PCM**: Ogg has no index and the receiver seeks in
  it badly. Not a WebM remux: PCM needs no muxer and works for server streams.
- **A Chromecast gets a local FLAC without a SEEKTABLE with a new head** (`Body::Prefixed`:
  `fLaC`, STREAMINFO, a built table, then the original frames byte for byte; other metadata
  is left out, the cover goes in the LOAD). Without a table the receiver reads the end of the
  file on a seek and may stay there. Not WAV (1.5–2 times the bytes); DLNA renderers get the
  file as it is.
  - Points every ~10 s: a byte estimate, then the next frame header that passes the CRC-8
    and agrees with STREAMINFO. A file that has a real table, is not FLAC or yields no points,
    and any indexing failure, is served as it is.
  - Server streams not cached yet are sent as they are: `media_stream` fetches 4 MB at every
    jump, so building the table would download the whole file first. Such a FLAC seeks badly
    on a Chromecast.

## Sessions

The worker thread owns the driver; the handle sends commands and reads `SessionEvent`s.

- A burst of commands is coalesced: everything before the newest `Load` is dropped except
  volume changes and `Close`; adjacent seeks and adjacent volume changes collapse into the
  last (a dragged slider must not queue a SOAP call per pixel).
- A load probes, plans, publishes track and cover, builds the URL for the interface that
  reaches the device and hands it over. A DLNA `SetAVTransportURI` that fails is retried once
  after a `Stop`; a Chromecast driver ignores media statuses whose content id is not the
  loaded URL.
- `Buffering(true)` is always followed by `Buffering(false)`, also when the track is replaced,
  stopped or fails. `close(wait)` stops what is ours on the device and closes the driver;
  dropping the session does the same without waiting.
- Volume is the device's own; its value at connect and later changes made on the device
  arrive as `Volume` (Chromecast pushes, DLNA is asked every fifth poll).

### Starting mid-track

Chromecast takes `currentTime` in LOAD. DLNA has no start position (a Seek before Play can be
accepted and ignored) and some renderers start playing the moment they get a URI. So `send`
silences the device (`Driver::silence_start`), sets the URI, plays, waits for PLAYING, seeks,
waits for the position and restores the sound: the first moments of the track are never heard.

- Silencing is `SetMute` when `GetMute` answers and the device is not muted already, otherwise
  volume to 0 and back; a fault on `GetMute` (not a network error) is remembered. A renderer
  without RenderingControl starts audibly.
- Anything failing after the device got the URI stops it before the sound comes back, so it
  cannot play from 0:00. Restoring is tried three times; failing that, the next volume change
  also unmutes. If the app dies in between, the device stays silent until its volume or mute
  is touched.
- The wait for PLAYING (`START_WAIT`, 15 s, at most the unreached wait below) takes PLAYING or
  PAUSED only from a device that has asked for the track. If it runs out and the device never
  asked, the seek is skipped: a device stuck loading refuses it with a UPnP fault, which would
  turn `NeverFetched` into `Failed`.

### Loading paused

A DLNA renderer gets nothing until play (`Current::on_device`), since some start as soon as
they get the URI: the track is probed and published, `Loaded` and `Paused` are sent, seeks only
move the position, and play sends it as above. Until then polls ignore the device; one that
still holds our previous track is stopped first. Stop and close touch the device only if it
holds something of ours (`ours_on_device`). Chromecast loads paused with `autoplay: false`.

### Position and state from polls

Renderers report whole seconds and report the old state for a moment after a command, so a
report close to the estimate is agreement, a small step back is never shown, positions far
behind the target are ignored for 4 s after a seek or mid-track load, and our own state
changes win over the device's for 1.5 s.

- `reset_at_end`: a renderer can answer PLAYING 0:00 once before STOPPED; for 10 s after the
  estimate came within 5 s of the end a far-behind position is ignored, so that STOPPED stays
  an `Ended` and the queue advances.
- `restart_unconfirmed`: a device seeking on its own passes through 0:00; a position under 1 s
  far behind the estimate counts only once the next report has moved on.
- `device_state`: some renderers keep saying PLAYING when paused with their own button and only
  the position stops. A position repeating over three consecutive PLAYING polls and 2.5 s is a
  pause, unless the session started, loaded or seeked in the last 4 s or the track is near its
  end. Other states and failed polls reset the count; while the session shows a pause, PLAYING
  counts only once the position moves.

### End of track and unreached devices

- The device is believed only after it was seen PLAYING for this load (DLNA reports STOPPED
  while loading). Then Chromecast IDLE/FINISHED, or a DLNA STOPPED within 5 s of the end (or
  without a duration), is `Ended`; an earlier STOPPED means someone stopped it on the device:
  shown as paused, and play reloads at that position.
- Whether the device ever asked the media server for the track decides how a start that does
  not happen ends. The waits count from the moment the device was told (`loaded_at`, stamped
  in `send` and again by play for a track loaded paused) and are `Worker` fields (`patience`,
  and `unreached_after`, at most the patience; `Session::start_with`) so tests do not wait.
  - It did, and still never played within 30 s (`NEVER_STARTED`):
    `Failed("the device did not start playing")`; long buffering stays buffering.
  - It never did within 20 s (`UNREACHED_AFTER`) although it answers every poll:
    `NeverFetched`, the firewall case (a VPN or a guest network isolating clients does the
    same). Idle, stopped, buffering and loading all count: a device that cannot connect back
    does not necessarily sit idle (some report TRANSITIONING forever). Until it has asked,
    its PLAYING and PAUSED count as `Buffering` too (`seen_playing` is set only by a PLAYING
    after a request): some renderers answer PLAYING at once without the track, and believing
    it would make the session read the frozen position as a pause and stop there.
  - A failed load without a request, reported by the device or a load command that fails or
    times out on our side (`refused`): within the first 3 s (`REFUSED_AT_ONCE`, long enough
    for the first poll after the 1.5 s hold) an ordinary `Failed` with "(it never asked for
    the track)", since the device may dislike the format (a firewall that rejects instead of
    dropping looks the same); later `NeverFetched`, the device having given up on its own
    connection to us. Either way `NeverFetched` needs a device that still answers: `refused`
    polls once more, and one that does not gets the plain `Failed`, then `Lost`.
  - A Chromecast may answer `LOAD` only once the media is loaded or has failed, so with the
    media server out of reach the session thread would sit in `LOAD` for 30 s and end in a
    plain `Failed`. `Loading::unreached` lets the driver stop waiting: `Client::load` gives up
    once the 20 s have passed with nothing requested, and `refused` turns that into
    `NeverFetched`.
  - Another Chromecast answers `LOAD` at once with IDLE and then leaves every media status
    request unanswered while its player tries to connect, so polls fail instead of reporting
    a state. A failed poll is `NeverFetched` too (`unreached_in_silence`) once the same 20 s
    have passed with nothing requested while we show playing, and only if
    `Driver::still_answers` (Chromecast: the receiver status still lists our app; DLNA: never).
    Otherwise failed polls count toward `Lost`: a device that fetched the track, or that
    stops answering altogether, never gets the firewall message.
  - Not `Lost`: a device that is off stops answering polls and ends in `Lost` first.
  - AirPlay is pushed, so nothing is fetched there; see AirPlay output for its check.
- **Lost**: four failed polls in a row over at least 4 s, a closed Chromecast connection, or
  its receiver app closed or taken by another sender. The session does not talk to the device
  first. For DLNA only `GetTransportInfo` counts (some renderers fault on `GetPositionInfo`
  when nothing is loaded).

## AirPlay output

- The sender takes 44.1 kHz 16-bit stereo only. `write` maps channels, resamples with
  `rubato` when the rate differs and queues up to 0.5 s; a full queue makes it wait up to
  200 ms and report nothing written *before* touching the resampler, so a retry never doubles
  audio. No OS resampler on this path.
- The sender thread pulls packets through `Render` like a sound card pulls a callback: fades
  are applied there with `audio_output::FadeState`, and a finished fade-out renders silence
  without draining the queue, as `cpal_stream` does.
- `Render` also feeds its own `SignalTap` (`signal_tap()`), before gain, at 44.1 kHz stereo.
  Its latency is the stream's `latency_frames` (`LATENCY_FRAMES` plus the receiver's
  device latency, set at each stream start): what is rendered is heard that much later, so
  the app's spectrum follows the speaker. `clear` and a flushing pause `flush` the tap:
  the receiver drops what it had buffered, so those frames are never heard.
- **Two volumes.** `set_device_volume` is the speaker's own; `connect` takes an `Option`, and
  `None` starts without one so the speaker keeps its volume (also on a reconnect until one is
  set). `set_gain` (also `AudioOutput::set_volume`) scales samples in `Render` with the local
  output's curve. The app uses one or the other (`crates/pawse/src/cast/doc.md`, "Volume").
- **Pause is immediate.** A pause after the engine's fade-out flushes the speaker's buffer;
  frames sent but not heard are put back in front of the queue (`Render::rewind`, from a
  history), so resume continues where the listener stopped. Silence rendered after the
  fade-out or on an underrun is counted and not rewound, so heard audio never comes back. A
  pause without a finished fade is not flushed. `clear` flushes too.
- `pending()` is the queue plus `Stream::unheard_frames` (about 2 s latency plus the
  receiver's `Audio-Latency`, never more than was sent since the last start or flush); the app
  subtracts it from the engine's positions. The engine moves on to the next track ~2 s before
  it is heard; the device is told only once it is heard (`crates/airplay/src/doc.md`).
- The last `set_now_playing` and `set_progress` are sent again to a reconnected stream: the
  engine reports `Playing` before it resumes the output, so progress for a resume that
  reconnects arrives while there is no stream. Remote buttons arrive on `commands()` from the
  stream's events and from DACP.
- A stream that died while paused is reconnected on resume; only a failure there, or losing
  the speaker while playing, goes to `lost()`. The handshake runs without the stream lock and a
  closed output never reconnects.
- **An AirPlay 2 speaker that cannot reach us** (`crates/airplay/src/doc.md`, "An AirPlay 2
  speaker that cannot reach us"; AirPlay 1 speakers are never judged):
  `airplay::StreamEvent::Unreached` comes out as `unreached()`, and a handshake failing with
  `airplay::Error::Unreached` as
  `ConnectError::Unreached` from `connect` or as `unreached()` from a reconnect on resume,
  apart from `lost()`, so the app can explain the firewall instead of reporting a lost
  speaker.

## DACP

The AirPlay remote buttons, set up as OwnTone and Music Assistant do it.

- One server per app run, started with discovery (so the announcement is out before a speaker
  is picked), on IPv6 and IPv4 at once: a TV that resolves the announced host connects over
  IPv6 link-local when it has one, and an IPv4-only server got its buttons only now and then.
- Announced by its own `mdns-sd` daemon as `iTunes_Ctrl_<airplay::dacp_id()>._dacp._tcp` with
  OwnTone's TXT on host `pawse-<id>.local.`, all interface addresses.
- Each stream registers a route under its `Active-Remote`. Every request is answered `204`
  except `getproperty`, which gets `400`: shairport-sync shows its remote as available only on
  `200` or `400` and counts `204` as a failure.

## Discovery

`Discovery::start` starts the threads (and DACP); the app does it when the output picker
opens, so a user who never casts sends no multicast and never sees macOS's local network
prompt.

- Multicast is not trusted to say whether a device is there: some routers drop mDNS between
  hosts for hours while SSDP mostly gets through and unicast always does. Receivers are found
  and kept by unicast when multicast is gone. Not by rejoining the groups from our side, which
  changes nothing there.
- **Four threads feed one list**, keyed `chromecast:<id>`, `airplay:<mac>`, `dlna:<udn>`; a
  receiver that comes back at another address is updated in place: `cast-mdns` (`mdns-sd`
  browsing the three service types), `cast-ssdp` (search every 60 s and on `refresh`, to the
  multicast group and to every known host), `cast-notify` (SSDP announcements: `alive` from a
  known renderer counts as hearing from it, from an unknown renderer it is described and
  added; `byebye` only puts the receiver up for a check; the socket is reopened on `refresh`),
  `cast-check` (legacy unicast mDNS every 60 s and on `refresh`, and the checks below).
- **Known hosts** (IPv4 of listed receivers, of renderers that announced themselves, of mDNS
  answers; never our own, loopback or link-local; at most 64; forgotten after 30 minutes) get
  the unicast M-SEARCH and unicast mDNS queries. Not every NOTIFY sender: on a large network a
  query per minute each would look like a scan. Unicast mDNS queries leave from an ephemeral
  port so answers come back by unicast (RFC 6762 §6.7); a missing SRV/TXT/A is asked of the
  responder, and a service still without an A record gets the responder's address. TTLs are
  ignored: a receiver lives by the checks, not by the records.
- **Checks instead of expiry.** A receiver is checked directly when nothing was heard for
  150 s, when it said goodbye (`mdns-sd` removal or `ssdp:byebye`), or when it has not been
  heard 6 s after a `refresh`. DLNA: the description must come back with the same UDN.
  Chromecast/AirPlay: a unicast mDNS answer for the record it is listed with, still the same
  receiver (id, still supported); only a host that answers no mDNS gets a TCP connect. A
  receiver is looked up again right before its check and removed only if nothing was heard of
  it since the check began. **Receivers in use are never checked or removed** (`set_in_use`:
  the active one and the one being connected to): a stray connection to a speaker that is
  playing is not worth the risk.
- **`refresh` restarts mDNS**, not only the searches: `mdns-sd` backs its browse queries off up
  to an hour and a speaker does not announce unless asked; after a sleep a second `browse` on
  the same daemon was not always enough. So each refresh starts a new daemon (`Browsing`).
- **IPv4 only.** A record with only an AAAA address is not listed; its host name is resolved
  with the system resolver on a `cast-lookup` thread (not `mdns-sd`'s `resolve_hostname`: it
  blocks its daemon thread when the listener's channel is full). Services announced from one
  of our own addresses are skipped (a Mac with AirPlay Receiver on would offer itself). Which
  records qualify is decided by `Device::from_service` / `from_airplay_service` in
  `chromecast` and `airplay` (their docs).
- **One AirPlay device, two records.** Both `_raop._tcp` and `_airplay._tcp` give the id
  `airplay:<mac>`. A supported RAOP record wins: an AirPlay 2 record for the same id and
  address only counts as hearing from it, and a RAOP record replaces an AirPlay 2 one. An
  AirPlay 2 record at another address, or while the RAOP entry is up for a check, replaces the
  entry, so a stale RAOP address is not kept alive by the other record. RAOP is what was
  tested on most speakers, so a speaker that played before keeps playing the same way; AirPlay
  2 is for devices without a usable RAOP record (TVs announce only `_airplay._tcp`).

## Testing

The fake Chromecast fetches the media URL it is given, so a test sees the exact bytes a device
gets. `FakeRenderer` (`tests.rs`) is a scripted `Driver` with whole-second positions for the
renderer quirks above; it asks for the track with a `HEAD` whenever it starts playing, as a
real one would, or it would count as unreached. `fake_dlna.rs` is a DLNA renderer on loopback
with real SOAP in five moods (plays, stuck loading, stopped, claims PLAYING without fetching,
fetches then stays loading); the unreached-device cases run on it and on
`FakeChromecast::cannot_reach_media` / `reply_to_load` / `stop_answering` with a short
patience, since a firewall drop is indistinguishable from a device that never connects. A
Chromecast whose requests go unanswered is tested with `ChromecastDriver::with_request_timeout`
(test-only), or each failed poll would take 10 s.

## Known limits

- A device that closes its player at the end of a track without reporting a stop leaves the
  session on the last second, as playing; the queue does not move on. A rule taking a frozen
  position near the end for the end was tried and dropped.
- Track changes on renderers are not gapless (each track is a new load after the end); DLNA
  `SetNextAVTransportURI` and Chromecast queues are not used. A renderer's own next/previous
  buttons do nothing: DLNA cannot report them.
- On AirPlay, a next track that is a server track not cached yet makes the engine's `Prepare`
  clear the output, and that flush cuts the last ~2.5 s the speaker still holds; a cached one
  keeps the tail.

## Decisions

Owner's calls; ask before changing.

- The unreached checks err on the side of silence: a missed firewall message is acceptable, a
  working device interrupted by one is not. `NeverFetched` needs nothing requested for 20 s
  and a device that still answers; do not loosen either without asking.
- A device that can take the original file gets the original file; PCM only where it cannot
  (codec, limits, cue segments). Not the rest of the track as PCM cut from the position to
  avoid the audible 200–300 ms head of a DLNA mid-track start: tried, then reverted — raw PCM
  over the network is a regression for every FLAC stream. Any change to what format or bytes a
  device gets goes to the owner as a question first.

# cast

Playing the library on network receivers: Chromecast, DLNA/UPnP renderers and
AirPlay speakers (AirPlay 1 and AirPlay 2). GPUI-free; `pawse::cast` wires it into the player.
The protocol clients live in their own crates (`chromecast`, `airplay`, and the
renderer half of `dlna`); this crate is what is common to them.

There are two ways a receiver gets audio, and the crate has one of each:

- **Renderers pull** (Chromecast, DLNA). The device is handed a URL on our own
  HTTP server and fetches the track itself; we only send play/pause/seek and
  poll its state. `Session` is that remote player.
- **AirPlay is pushed.** The sender streams PCM in real time, so an AirPlay
  speaker is an engine output: `AirPlayOutput` implements
  `audio_output::EngineOutput`, and the app runs a second `AudioEngine` on it.
  Everything the engine does (gapless, cue tracks, streaming sources, DSD,
  fades) works unchanged.

## Files

- `lib.rs` — `connect` (a `Receiver` → `Session`), re-exports.
- `discovery.rs` — `Discovery`, `Receiver`, `ReceiverKind`: the list of
  receivers and the four threads that keep it (see Discovery below).
- `unicast_mdns.rs` — legacy unicast mDNS queries: the query, a DNS reply
  parser (PTR, SRV, TXT, A, compressed names) and follow-ups for whatever a
  reply left out.
- `server.rs` — `MediaServer`: the HTTP/1.1 server devices fetch from.
- `media.rs` — `Media` (what to play), `probe`, `plan` (original bytes or PCM),
  the per-device format tables.
- `pcm.rs` — `PcmSpec`/`PcmReader`: decoding to WAV or raw L16 with byte ranges.
- `flac.rs` — a seek table for a FLAC file that has none (`indexed`): the
  metadata blocks, frame headers, the new head.
- `session.rs` — `Session` and its worker thread: load, commands, polling,
  position, end-of-track detection. `Driver` is what a protocol implements.
- `dlna_driver.rs`, `chromecast_driver.rs` — the two `Driver`s.
- `airplay_output.rs` — `AirPlayOutput`.
- `dacp.rs` — the DACP server for AirPlay remote buttons and its mDNS
  announcement.
- `net.rs` — `local_ip_for`: the address of the interface that routes to a
  device, which is the host put into media URLs; the IPv4 interfaces and a
  socket that sends multicast out of one of them.
- `tests.rs` — the HTTP server, PCM ranges against a straight decode, and whole
  Chromecast sessions against `chromecast::testing::FakeChromecast`.

## Media server

- One server per app run, bound to `0.0.0.0` on an ephemeral port, started the
  first time a renderer is connected. A thread per connection (at most 32),
  HTTP/1.1 keep-alive, `GET` and `HEAD`, single byte ranges (`bytes=a-b`,
  `a-`, `-n`), `416` past the end.
- Nothing is browsable: a path is `/<128-bit random token>/<n>.<ext>` and only
  what a session published (its current track and cover) is served. A session
  removes only its own entries (on a new load, stop, close, failure or loss),
  so a session that is being torn down never takes files from the next one.
  The LAN is otherwise trusted, like the web remote.
- Request heads are capped at 16 KiB; anything but `GET`/`HEAD` gets `405`
  and the connection is closed. Media URLs are IPv4: the server listens on
  `0.0.0.0` only, and a device reachable only over IPv6 is an error.
- DLNA headers are always sent: `transferMode.dlna.org: Streaming` and
  `contentFeatures.dlna.org` with `DLNA.ORG_OP=01` (byte seek) and `CI=1` for
  converted media. Some TVs refuse media without them.
- A paused device stops reading; the write timeout is 30 minutes, so the
  connection outlives a long pause. A device that reconnects asks for a range.

## What a device gets (`plan`)

- A whole file in a format the device takes is sent as it is
  (`Delivery::Original`), with its real length. Chromecast takes MP3, AAC
  (`.aac`, `.m4a`/`.mp4`), FLAC, Opus in WebM and WAV, up
  to 96 kHz and two channels. A DLNA renderer takes what its
  `GetProtocolInfo` sink list names (`dlna_mimes` maps codec + extension to
  the MIME types renderers use); a renderer without a ConnectionManager, or
  whose `GetProtocolInfo` fails, is assumed to take MP3, FLAC and WAV.
- Everything else becomes PCM (`Delivery::Pcm`): ALAC, APE, WavPack, DSD,
  formats over the limits, and **every cue track**, since a device can only
  play a whole file. The container is WAV, or `audio/L16` (big-endian, no
  header) for a renderer that lists L16 but no WAV. 16-bit when the source is
  16-bit or lossy (and always for L16), 24-bit otherwise. The frame count comes from the cue
  length or the decoder's duration, so the length is exact and known up front.
- **A Chromecast gets Ogg (Opus, Vorbis) as PCM.** Ogg has no index, and the
  receiver cannot seek in it. Measured on the Xiaomi TV Stick (2026-10-04)
  with an Ogg Opus track: the first SEEK while playing buffered for over
  4 s, the third ended the media session (`IDLE` with `ERROR`) half a second
  later, so every later command got `INVALID_MEDIA_SESSION_ID`; seeking by
  a new LOAD at the position failed the same way about half the time while
  playing. Loads at a position while paused were fine. The same packets
  remuxed into WebM with Cues seeked cleanly, but PCM needs no muxer, works
  for server streams too, and is about 1.5 Mbit/s for 48 kHz 16-bit stereo,
  less than a 24/96 FLAC. The sound is the same: the receiver would decode
  the same Opus.
- A cue track is always cut, also the first one of an image, which starts at
  0:00 (`plan` takes a set length as a segment too); sent whole, the device
  would play on into the next tracks.
- PCM is kept within what the device plays (`Accepts::pcm_limits`):
  Chromecast 96 kHz and two channels, DLNA 192 kHz and eight. Over the limit
  the rate is halved until it fits (192 → 96, 352.8 → 88.2 kHz for DSD), with
  `rubato`, and more channels are downmixed to stereo; within the limits the
  samples are passed through untouched. Like AirPlay, this is a stream to a
  device, so there is no OS resampler to defer to.
- The codec comes from the decoder (`audio_decoder::Decoder::codec`), not the
  extension: an `.m4a` is AAC or ALAC.
- **A Chromecast gets FLAC with a seek table.** A local FLAC file without a
  SEEKTABLE block (363 of 1609 in the user's library) is served with a new
  head: `fLaC`, the original STREAMINFO and a SEEKTABLE, followed by the
  file's frames byte for byte (`Body::Prefixed`; the other metadata, covers
  included, is left out: the cover goes in the LOAD). Points are about 10 s
  apart (at most 8192, wider apart beyond that): from a byte estimate (the
  position's share of the audio bytes) the next frame header is looked for
  within the maximum frame size plus 32 bytes (16 KB to 1 MB; 64 KB when
  STREAMINFO does not say). A sync code counts only if the header's CRC-8
  matches, its reserved values are not used, its rate, bit depth and
  channels agree with STREAMINFO, its blocking strategy is the first
  frame's, a fixed-size frame has the stream's block size (or ends the
  stream) and its sample number is inside the stream; each point is that
  frame's sample number and offset. All 363 such files of the user's
  library got valid points (9626, each checked against the frame at its
  offset), the slowest in 30 ms (13 minutes of 24/96).
  Measured on the Xiaomi TV Stick (2026-10-04): without a table the receiver
  first reads the end of the file on a seek (to bisect the stream) and then
  sometimes stays there: it buffers for seconds or jumps to the end and
  reports the track finished. Loaded paused in the middle of the track (a
  switch to the Chromecast during a pause), it reported the track finished
  3 s later, or its app closed itself 12 s later. With a table, the same
  files (Tarantula 24/44.1, Pneuma 24/96) seek at once and stay paused.
  WAV would avoid it too but is 1.5–2 times the bytes; DLNA renderers get
  the file as it is (no trouble seen there). Server streams that are not
  cached yet are sent as they are: `media_stream` fetches 4 MB at every
  jump, so building the table would download the whole file before the
  first note.
- Server tracks are served from `RemoteMedia` streams (`Source::Stream`); each
  request opens its own reader on the same download, which doubles as the
  cache. The app keeps the first stream open for the track's lifetime so the
  download is not cancelled between the probe and the device's request.

## PCM ranges

`PcmReader::open(spec, offset)` maps the byte offset to a frame, asks the
decoder for a coarse seek to 50 ms before that time (the seek takes an `f32`
ratio, which on an hour-long cue image can land a few frames late) and skips
from where it landed to the exact frame (and to the byte within a frame for an unaligned offset). Lossless
sources give the same bytes as a straight decode
(`pcm_ranges_match_a_straight_decode`). A converted stream starts a fresh
resampler at every range and drops its delay, so it is continuous but not
bit-identical across ranges. A decoder that ends early is padded
with silence and a longer one is cut, so the body always matches the length
in the header. A source whose length is unknown cannot be converted (the WAV
header needs it); `plan` fails with a message instead.

## Sessions

- The worker owns the driver. Commands queue on a channel; a burst is
  coalesced: only the newest `Load` survives (with volume changes from before
  it), adjacent seeks and adjacent volume changes collapse into the last, so
  dragging the slider does not queue a SOAP call per pixel.
- A load probes the media (opening a decoder, also over the network), plans
  the delivery, publishes it, builds the URL for the interface that reaches
  the device and hands it over. `Loaded` carries the duration and the source's
  sample rate and bit depth.
- **Starting mid-track.** Chromecast takes `currentTime` in LOAD. DLNA has no
  start position, a Seek sent before Play is accepted and ignored (gmrender
  plays from 0:00), and some renderers start playing the moment they get a
  URI (the HiBy R1 does, without a Play). So `send` silences the device
  (`Driver::silence_start`) before `SetAVTransportURI`, plays, waits until it
  reports PLAYING (up to 15 s: the R1 can sit in TRANSITIONING for 8),
  seeks, waits until the reported position is within 1 s of the target (at
  most 3 s, then one more seek and wait) and 300 ms more (a device may report the new
  position before its output follows), and restores the sound
  (`restore_sound`), so the first moments of the track are never heard. Silencing is `SetMute` when
  `GetMute` answers and the device is not muted already; otherwise the volume
  is set to 0 and back (the R1 faults on `GetMute`; the driver remembers a
  fault, not a network error, and does not ask again). A renderer without
  RenderingControl starts audibly. If anything fails after the device got the
  URI, it is stopped before the sound comes back, so it does not play the
  track from 0:00. Restoring is tried three times; if that fails, the next
  volume change from the app also unmutes. If the app dies in between, the
  device stays silent until its volume (or mute) is touched.
- **Loading paused.** A DLNA renderer gets nothing until play
  (`Current::on_device` is false): a renderer like the R1 would start playing
  as soon as it got the URI. The track is probed and published, `Loaded` and
  `Paused` are sent, seeks only move the position, and play sends it the
  usual way, from wherever the position is by then. Until then polls ignore
  the device, which may still hold our previous track or play something of
  its own. A device that still holds our previous track is stopped first, so
  it cannot play that one (from its own buttons) while the app shows another.
  Stop and close stop the device only if it holds something of ours
  (`ours_on_device`). Chromecast loads paused with `autoplay: false`.
- **Polling.** Every second the driver reports state, position and duration.
  Between polls the position is extrapolated every 200 ms. Renderers report
  whole seconds (gmrender), so a reported position from 1.3 s behind to 0.3 s
  ahead of the estimate is taken as agreement and does not move it
  (`agrees_with`); a step back smaller than 1.5 s is never shown (`JITTER`). After a seek or a mid-track load,
  positions far behind the target are ignored for 4 s while the device catches
  up. Our own state changes are trusted over the device's for 1.5 s, since
  devices report the old state for a moment.
- **Paused on the device.** The HiBy R1 keeps answering PLAYING when it is
  paused with its own button, and sends no event; only the position stops.
  So a position that has not changed for 2.5 s and three polls in a row of
  PLAYING answers while the session plays is a pause (`device_state`), unless
  the session itself started, loaded or seeked in the last 4 s (the device may
  still be buffering) or the track is within 5 s of its end, by the device's
  position or within 10 s of the estimate getting there. Time spent in
  other states does not count: the R1 sits in TRANSITIONING at 0:00 for up to
  8 s before a track starts, and its first PLAYING 0:00 after that is not a
  pause. A failed poll starts the count over, so a network hiccup (the device
  playing on from its buffer, or catching up afterwards) is not taken for a
  pause. A device that really stalls while still answering PLAYING shows as
  paused until its position moves again. The device's frozen position is
  shown. While the session
  shows a pause, a PLAYING report counts only once the position moves, so the
  same device resuming on its button is noticed within a poll and our own
  pause is not undone by a stale PLAYING. Renderers report whole seconds, so a
  playing device always moves within 2.5 s. Play from the app while the device
  still says PLAYING sends Pause first: a device that believes it is playing
  may ignore a Play (not verified on the R1).
- **End of track.** The device is believed only after it has been seen
  PLAYING for this load (DLNA renderers report STOPPED while loading). Then
  Chromecast's IDLE/FINISHED, or a DLNA STOPPED within 5 s of the end (or
  without a known duration), is `Ended`. A STOPPED earlier means someone
  stopped it on the device: the session shows it as paused, and play reloads
  the track at that position. A track that does not start within 30 s of a
  load (or of play, for a track loaded paused) fails.
- Some renderers reset the position just before they stop: the R1 answers
  PLAYING 0:00 for one poll, then STOPPED. Believing that 0:00 would make the
  STOPPED look like a stop in the middle of the track, so the queue would not
  advance. For 10 s after the estimate came within 5 s of the end
  (`end_reached`), a reported position far behind the estimate is ignored
  (`reset_at_end`).
- A `Buffering(true)` is always followed by `Buffering(false)`, also when the
  track is replaced, stopped or fails.
- **Near the end** of a track (the last 3 s by the estimate) the session polls
  every 250 ms instead of every second, so the next track is loaded sooner
  after the device stops.
- **Losing the device.** Four failed polls in a row over at least 4 s (the
  polls are faster near the end), a closed Chromecast
  connection, the receiver app being closed or replaced by another app all end
  the session with `Lost`; the session does not try to talk to the device
  first. For DLNA only `GetTransportInfo` counts: some renderers fault on
  `GetPositionInfo` when nothing is loaded.
- Chromecast: statuses that arrive before our receiver app is launched are
  dropped when the driver is built. After that, a `RECEIVER_STATUS` without
  our app session means another sender took the device (`Lost`).
- `close` stops playback on the device and waits up to the given time; the
  app uses it on quit so a TV does not keep playing a stream that is about to
  disappear. Dropping the session sends the same stop without waiting.
- Volume is the device's own (Chromecast receiver volume, DLNA
  RenderingControl 0–100); its value at connect and later changes made on the
  device arrive as `Volume` (Chromecast pushes them, DLNA is asked every
  fifth poll).

## AirPlay output

- The `airplay` sender takes 44.1 kHz 16-bit stereo only (both protocols). `write` maps channels (mono is
  doubled, 5.1 is downmixed), resamples with `rubato` (`FftFixedIn`, 1024-frame
  chunks) when the source rate differs, and queues up to 0.5 s; a full queue
  makes `write` wait up to 200 ms and then report nothing written, before
  touching the resampler, so a retry never doubles audio. There is no OS
  resampler on this path — the stream goes to a device, not a sound card —
  so this is the one place the app resamples.
- The `airplay` sender thread pulls 352-frame packets through `Render`, the
  way a sound card pulls a callback: fades are applied there with the shared
  `audio_output::FadeState`, and a finished fade-out renders silence without
  draining the queue, exactly like `cpal_stream`.
- **Pause is immediate.** A pause after the engine's fade-out flushes the
  speaker's buffer. Frames that were sent but not heard yet are put back in
  front of the queue (`Render::rewind`, from a history of rendered frames),
  so resume continues where the listener stopped hearing, not 2 s later.
  Silence rendered after the fade-out finished (or on an underrun) is counted
  and not rewound, so no audio that was already heard comes back. A pause
  without a finished fade (the end of the queue, a stall) is not flushed: the
  speaker plays out what it has.
- Positions: the engine counts frames it wrote, which play 2 s and more later.
  `pending()` is the queue plus what the speaker holds (the 2 s latency plus
  the receiver's `Audio-Latency`, 0.25 s for shairport-sync, but never more
  than was sent since the last start or flush, so a seek shows its target
  instead of jumping back); the app subtracts it from the engine's
  positions. In the app the title of the next track still switches when the
  engine moves on, about 2 s before it is heard; the device is told only
  once the track is heard (`airplay`'s doc, "the next track is named when
  it is heard").
- **Now playing and the remote.** The last `set_now_playing` (title,
  artist, album, cover) and `set_progress` are kept and sent again to a
  reconnected stream, after it is installed: the engine reports `Playing`
  before it resumes the output, so the progress for a resume that reconnects
  arrives while there is no stream yet. Remote buttons from the device arrive
  on `commands()`, beside `lost()`, from two places: the stream's own events
  (the AirPlay 2 event connection; the `airplay-forward` thread sorts them
  from `Lost`) and DACP.
- **DACP** (`dacp.rs`), set up as OwnTone and Music Assistant do it: one
  HTTP server for the app run, started with discovery (when the output
  picker first opens, so the announcement is out before a speaker is
  picked, and nothing is announced for a user who never casts), on the
  first free port from 39831 (Music Assistant's range), listening on IPv6
  and IPv4 at once (one socket with `IPV6_V6ONLY` off and, off Windows,
  `SO_REUSEADDR`, so a restart inside the old connections' TIME_WAIT still
  gets it; only when no port takes that does it fall back to IPv4 alone,
  with a warning; port 0 when the whole range is taken), announced by its
  own `mdns-sd` daemon as `iTunes_Ctrl_<airplay::dacp_id()>._dacp._tcp` with
  OwnTone's TXT (`txtvers=1`, `Ver=131077`, `DbId=1`, `OSsi=0x2012E`) on the
  host `pawse-<id>.local.` with all interface addresses, IPv6 included. The
  Hisense TV resolves that host and connects over IPv6 link-local when it
  has one: with a service the Mac's own mDNS responder announced it pressed
  every button through to us at once, while our IPv4-only server and
  announcement got its buttons only now and then (2026-10-04). Each stream
  registers a route under its `Active-Remote`; a known command goes to the
  route it names. Every request is answered `204` except `getproperty`,
  which gets `400`:
  shairport-sync polls `getproperty?properties=dmcp.volume` every second and
  shows its remote as available only on `200` (a server with properties,
  as OwnTone's) or `400` (one without), counting `204` as a failure
  (`dacp.c`); its commands reached us either way (tested on the Pi). One
  connection at a time, its head read within 2 s in all. Commands are logged
  (info); an ignored request (a volume report, an unknown path or remote) is
  logged at info the first time its path (up to `=`) is seen and at debug
  after that.
- A stream that died while paused (a speaker may drop an idle session) is
  reconnected on resume; only a failure there, or losing the speaker while
  playing, is reported on `lost()`. The handshake runs without holding the
  stream lock, so the UI thread (positions, volume) never waits on it, and a
  closed output never reconnects.

## Discovery

- Started by the app only when the output picker opens, so a user who never
  casts never sends multicast (and never sees macOS's local network prompt).
  It keeps running afterwards, until the app quits.
- Multicast is not trusted to say whether a device is there. On the test
  Wi-Fi the router drops 224.0.0.251 (mDNS) between hosts in both
  directions for hours, while 239.255.255.250 (SSDP) mostly gets through and
  unicast always does (see Testing). Everything below is built so a receiver
  is found and kept by unicast when multicast is gone, the way the LMS bridges
  (ping before removing), pychromecast (polling known hosts) and Home
  Assistant (`poll_availability`) ended up doing it.
- **Four threads feed one list** (`Inner`), keyed `chromecast:<id>`,
  `airplay:<mac>`, `dlna:<udn>`; a receiver that comes back at another address
  is updated in place:
  - `cast-mdns`: `mdns-sd` browsing `_googlecast._tcp`, `_raop._tcp` and
    `_airplay._tcp`.
  - `cast-ssdp`: an SSDP search for renderers every 60 s and on `refresh`,
    to the multicast group and to every known host (`dlna::discover_renderers`
    with hosts).
  - `cast-notify`: `dlna::NotifyListener`, SSDP announcements on port 1900.
    An `ssdp:alive` from a known renderer counts as hearing from it (a new
    `LOCATION` is described again); one from an unknown UDN whose NT is a
    MediaRenderer or AVTransport is described and added, at most once per
    30 s per UDN. The socket is reopened on every `refresh`, since the
    interfaces may have changed during a sleep.
  - `cast-check`: legacy unicast mDNS queries every 60 s and on `refresh`,
    and the checks below.
- **Known hosts** are the IPv4 addresses of the listed receivers, of
  renderers that announced themselves (an SSDP NOTIFY whose NT is a renderer,
  or from a listed one) and of mDNS answers; never the computer's own
  addresses, at most 64 (the oldest goes first), each forgotten after 30
  minutes of silence. Not every NOTIFY sender: on a large network the
  unicast queries every minute would look like a scan.
  They get the unicast M-SEARCH and the unicast mDNS queries: the Pi's
  AirPlay speaker is found through the NOTIFY of the renderer on the same
  Pi when no mDNS multicast gets through.
- **Legacy unicast mDNS** (`unicast_mdns`): PTR queries for the three service types
  are sent from an ephemeral port, to 224.0.0.251 out of every IPv4
  interface and to `<host>:5353` of every known host. A query from a port
  other than 5353 must be answered by unicast to that port (RFC 6762 §6.7),
  so the answers do not depend on multicast getting back to us. When an
  answer lacks the SRV, TXT or A record, the responder is asked for it; a
  service that still has no A record gets the responder's address. The TTLs
  of these answers (at most 10 s for legacy queries) are ignored: a receiver
  lives by the checks, not by the records (the LMS bridges went back to
  plain multicast after taking those TTLs for the device's lifetime).
- **Checks instead of expiry.** Each receiver remembers when it was last
  heard of (an `mdns-sd` resolve, a unicast mDNS answer, an SSDP reply or
  NOTIFY, a passed check). A receiver is checked directly when:
  - nothing was heard for 150 s;
  - it said goodbye: `mdns-sd`'s `ServiceRemoved` (a goodbye or an expired
    record) or an `ssdp:byebye`, since some stacks send byebye when they
    restart;
  - 6 s after a `refresh` it has not been heard from again, so a device that
    was switched off leaves the list soon after the picker is opened (about
    10 s, measured with gmrender stopped: it sends no byebye).

  A DLNA renderer must hand out its description with the same UDN. A
  Chromecast or AirPlay speaker is asked by unicast mDNS (1 s) and must
  answer with a service that is still the same receiver (same id, still
  supported: a speaker that started asking for a password goes, and so does
  another device that got its address); only a host that does not answer
  mDNS at all is checked with a TCP connection (2 s). Each receiver is
  looked up again right before its check, so one heard of (or moved) while
  an earlier check ran is not checked at its old address, and a receiver
  that fails is removed only if nothing was heard of it since its check
  began. **The receivers in use are never checked nor removed**
  (`Discovery::set_in_use`: the active one and the one being connected to):
  their session talks to them anyway, and a stray connection to a speaker
  that is playing is not worth the risk.
- **`refresh` restarts mDNS**, not only the searches. `mdns-sd` repeats a
  browse query at 1, 2, 4 … s up to an hour, and a speaker does not announce
  itself unless asked. After the Mac sleeps, the network interface comes
  back (often with a new DHCP address) and `mdns-sd` drops every service it
  had on it, or lets their 120 s records expire; a second `browse` on the
  same daemon was not always enough, so every refresh starts a new daemon
  (`Browsing`): a fresh cache, queries from 1 s and no known answers that
  would keep a responder quiet.
- **IPv4 only.** The media server is IPv4, and an IPv6 link-local address
  without a scope cannot be connected to. Some devices answer a browse with
  only their AAAA record (the Xiaomi TV Stick announced only `fe80::…`, and
  `mdns-sd` does not ask for the A record once it has an address), so a
  record without IPv4 is not listed; its host name is looked up with the
  system resolver on a `cast-lookup` thread (macOS, Windows and Linux with
  nss-mdns resolve `.local` names) and the receiver is listed with that
  address. `mdns-sd`'s own `resolve_hostname` is not used: it blocks its
  daemon thread when the listener's channel is full. The unicast queries
  ask the device for its A record themselves.
- Rejoining the multicast groups from our side was tried and does nothing
  on the test Wi-Fi: with the Pi sending to 224.0.0.251, 239.255.255.250 and
  a fresh 239.77.0.1, the Mac got none of the first and all of the others,
  before and after a leave and join of each group and after joining another
  one. On macOS mDNSResponder holds 224.0.0.251 anyway, so a leave and join
  of ours sends no IGMP report.
- Chromecasts without the audio-out capability bit are skipped. A RAOP
  record that needs encryption, a password, a codec other than ALAC or a
  format other than 44.1/16/2 is skipped, and so is an AirPlay 2 record that
  needs a code shown on the device, a password or a home membership (see
  `airplay`).
- **One AirPlay device, two records.** Many speakers announce both
  `_raop._tcp` and `_airplay._tcp`; both give the id `airplay:<mac>` (the RAOP
  instance prefix, the AirPlay 2 `deviceid`). A supported RAOP record wins:
  an AirPlay 2 record for an id listed over RAOP at the same address only
  counts as hearing from it, and a RAOP record replaces an AirPlay 2 one. An
  AirPlay 2 record at another address, or while the RAOP entry is up for a
  check (its record said goodbye), replaces the entry: a stale RAOP address
  is not kept alive by the other record, and RAOP takes the entry back when
  it is heard again. RAOP is what was tested on
  most speakers, so a speaker that played before keeps playing the same way;
  AirPlay 2 is for devices without a usable RAOP record (TVs from Hisense,
  LG, Samsung, Sony and Roku announce only `_airplay._tcp`). A receiver is
  checked by the record it is listed with.
- Services announced from one of the computer's own addresses are skipped
  (in `mdns-sd` answers, unicast ones and host names resolved to IPv4): a Mac
  with AirPlay Receiver on would otherwise offer itself.

## Testing

- Automated: `tests.rs` here (HTTP server, PCM ranges and conversion, whole
  Chromecast sessions against `chromecast::testing::FakeChromecast`) and the
  unit tests next to each module. The fake fetches the media URL it is given,
  so a test sees the exact bytes a device would get. `FakeRenderer` in
  `tests.rs` is a scripted `Driver` with whole-second positions for the
  renderer quirks: the silent mid-track start, a pause that still says
  PLAYING, a long buffering start, and the 0:00 before STOPPED.
- On the test Wi-Fi, multicast stops reaching hosts from time to time (the
  router sends no IGMP queries). Measured on 2026-10-04 between the Mac and
  the Pi: 224.0.0.251 dropped both ways, 239.255.255.250 and a fresh
  239.77.0.1 delivered both ways, unicast always delivered. Then `mdns-sd`
  gets no answers at all, in the app, in tests and in a bare Python socket
  alike, while a legacy unicast query to the device's address and an
  M-SEARCH to `<device>:1900` are answered. macOS's own `dns-sd` is not a
  fair comparison: its first query asks for unicast answers (QU).
- Discovery was checked live against the Pi (2026-10-04): both receivers
  found and kept across a refresh; gmrender stopped and a refresh made gone
  in 11.6 s; started again and back in 0.3 s (its NOTIFY on start).
- The Pi on Wi-Fi drops out of multicast from time to time, for every
  client: its AirPlay (mDNS) and DLNA renderer (SSDP) vanish from macOS's own
  `dns-sd` too, while unicast works (`dig @<pi> -p 5353 _raop._tcp.local PTR`
  answers). Seen both ways: the Pi receiving no multicast at all, and the Pi
  answering queries whose answers never reached the Mac. The router sends no
  IGMP queries. Restarting avahi on the Pi helped once, not the next time;
  Wi-Fi power saving is off there
  (`/etc/NetworkManager/conf.d/wifi-powersave-off.conf`). A receiver missing
  from the picker on this rig is a network problem first.
- Manual, on real receivers: DLNA against gmrender-resurrect 0.3 (GStreamer)
  and the HiBy R1 ("HiBy MediaRender", a gmrender derivative), and AirPlay
  against shairport-sync 4.3 (classic build), whose `-o stdout` output was
  recorded and checked for frequency and discontinuities. Chromecast: the
  sessions against the fake, and the Xiaomi TV Stick 4K (Android TV) through
  a temporary test that loaded a track paused or playing, paused it and
  seeked eight times while printing every receiver and media status, with
  the device volume at 0 and restored after.

## Known limits

- Track changes on renderers are not gapless: each track is a new load after
  the device reports the end. On the R1 the gap is about a second: the end is
  noticed within ~0.3 s, `SetAVTransportURI` takes ~0.3 s there, and the
  device buffers before it plays.
  DLNA `SetNextAVTransportURI` and Chromecast queues are not used yet. The R1
  accepts `SetNextAVTransportURI` but never fetches the next track and still
  stops at the end, so it would gain nothing there.
- A renderer's own next/previous buttons do nothing: DLNA has no way for a
  renderer to tell the controller about them, and the R1's buttons change
  nothing that can be polled.
- On AirPlay, when the next track is a server track that is not in the cache
  yet when the current one ends (the prefetch starts 60 s before the end),
  the engine's `Prepare` clears the output, and that flush cuts the last
  ~2.5 s the speaker still holds. A cached next track keeps the tail (the
  engine's natural-end path), as locally.
- AirPlay 2 works with transient pairing and NTP timing only: devices that
  pair with a code shown on them (Apple TVs set that way), passwords, PTP-only
  receivers (shairport-sync's AirPlay 2 build) and buffered audio are not
  supported (see `airplay`).
- A server FLAC without a seek table that is not cached yet seeks badly on a
  Chromecast (see "What a device gets").
- The sleep timer's volume fade does not reach a cast device; the timer still
  pauses on time.

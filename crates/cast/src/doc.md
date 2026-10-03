# cast

Playing the library on network receivers: Chromecast, DLNA/UPnP renderers and
AirPlay 1 (RAOP) speakers. GPUI-free; `pawse::cast` wires it into the player.
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
- `discovery.rs` — `Discovery`: mDNS browsing for `_googlecast._tcp` and
  `_raop._tcp` (one `mdns-sd` daemon, one thread selecting over both browses
  and a refresh channel) and an SSDP search for renderers every 60 s or on
  `refresh`. `Receiver`, `ReceiverKind`.
- `server.rs` — `MediaServer`: the HTTP/1.1 server devices fetch from.
- `media.rs` — `Media` (what to play), `probe`, `plan` (original bytes or PCM),
  the per-device format tables.
- `pcm.rs` — `PcmSpec`/`PcmReader`: decoding to WAV or raw L16 with byte ranges.
- `session.rs` — `Session` and its worker thread: load, commands, polling,
  position, end-of-track detection. `Driver` is what a protocol implements.
- `dlna_driver.rs`, `chromecast_driver.rs` — the two `Driver`s.
- `airplay_output.rs` — `AirPlayOutput`.
- `net.rs` — `local_ip_for`: the address of the interface that routes to a
  device, which is the host put into media URLs.
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
  (`.aac`, `.m4a`/`.mp4`), FLAC, Vorbis/Opus in Ogg, Opus in WebM and WAV, up
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
- PCM is kept within what the device plays (`Accepts::pcm_limits`):
  Chromecast 96 kHz and two channels, DLNA 192 kHz and eight. Over the limit
  the rate is halved until it fits (192 → 96, 352.8 → 88.2 kHz for DSD), with
  `rubato`, and more channels are downmixed to stereo; within the limits the
  samples are passed through untouched. Like AirPlay, this is a stream to a
  device, so there is no OS resampler to defer to.
- The codec comes from the decoder (`audio_decoder::Decoder::codec`), not the
  extension: an `.m4a` is AAC or ALAC.
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
  start position: with autoplay the session plays, waits until the renderer
  reports PLAYING and seeks; without autoplay the seek is kept until the user
  presses play, so nothing is heard before it.
- **Polling.** Every second the driver reports state, position and duration.
  Between polls the position is extrapolated every 200 ms. Renderers report
  whole seconds (gmrender), so a reported position from 1.3 s behind to 0.3 s
  ahead of the estimate is taken as agreement and does not move it
  (`agrees_with`); a step back smaller than 1.5 s is never shown (`JITTER`). After a seek or a mid-track load,
  positions far behind the target are ignored for 4 s while the device catches
  up. Our own state changes are trusted over the device's for 1.5 s, since
  devices report the old state for a moment.
- **End of track.** The device is believed only after it has been seen
  PLAYING for this load (DLNA renderers report STOPPED while loading). Then
  Chromecast's IDLE/FINISHED, or a DLNA STOPPED within 5 s of the end (or
  without a known duration), is `Ended`. A STOPPED earlier means someone
  stopped it on the device: the session shows it as paused, and play reloads
  the track at that position. A track that does not start within 30 s of a
  load (or of play, for a track loaded paused) fails.
- A `Buffering(true)` is always followed by `Buffering(false)`, also when the
  track is replaced, stopped or fails.
- **Losing the device.** Four failed polls in a row, a closed Chromecast
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

- RAOP takes 44.1 kHz 16-bit stereo only. `write` maps channels (mono is
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
  positions. The title of the next track still
  switches when the engine moves on, about 2 s before it is heard.
- A stream that died while paused (a speaker may drop an idle session) is
  reconnected on resume; only a failure there, or losing the speaker while
  playing, is reported on `lost()`. The handshake runs without holding the
  stream lock, so the UI thread (positions, volume) never waits on it, and a
  closed output never reconnects.

## Discovery

- Started by the app only when the output picker opens, so a user who never
  casts never sends multicast (and never sees macOS's local network prompt).
  It keeps running afterwards; mDNS adds and removes services as they come
  and go, renderers not seen by SSDP for 200 s are dropped.
- **`refresh` browses mDNS again**, not only SSDP. `mdns-sd` repeats a browse
  query at 1, 2, 4 … s up to an hour, and a speaker does not announce itself
  unless asked. After the Mac sleeps, the network interface comes back (often
  with a new DHCP address) and `mdns-sd` drops every service it had on it, or
  lets their 120 s records expire; with the query interval already at tens of
  minutes the list stayed empty until the app was restarted. Calling `browse`
  again replaces the listener and starts the query schedule from 1 s (the old
  schedule stops because its listener is gone), so a live speaker is back
  within a second of the picker opening.
- Chromecasts without the audio-out capability bit are skipped. AirPlay
  speakers that need encryption, a password, a codec other than ALAC or a
  format other than 44.1/16/2 are skipped (see `airplay`).
- Receivers are keyed `chromecast:<id>`, `airplay:<mac>`, `dlna:<udn>`.

## Testing

- Automated: `tests.rs` here (HTTP server, PCM ranges and conversion, whole
  Chromecast sessions against `chromecast::testing::FakeChromecast`) and the
  unit tests next to each module. The fake fetches the media URL it is given,
  so a test sees the exact bytes a device would get.
- Manual, on real receivers: DLNA against gmrender-resurrect 0.3 (GStreamer)
  and AirPlay against shairport-sync 4.3 (classic build), whose `-o stdout`
  output was recorded and checked for frequency and discontinuities.
  Chromecast has only been run against the fake so far.

## Known limits

- Track changes on renderers are not gapless: each track is a new load.
  DLNA `SetNextAVTransportURI` and Chromecast queues are not used yet.
- On AirPlay, when the next track is a server track that is not in the cache
  yet when the current one ends (the prefetch starts 60 s before the end),
  the engine's `Prepare` clears the output, and that flush cuts the last
  ~2.5 s the speaker still holds. A cached next track keeps the tail (the
  engine's natural-end path), as locally.
- AirPlay 2 only devices (pairing, encryption) are not supported.
- The sleep timer's volume fade does not reach a cast device; the timer still
  pauses on time.

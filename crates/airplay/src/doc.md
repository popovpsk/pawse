# airplay

An AirPlay sender, a network sound card. `Stream::start` connects to one speaker over
AirPlay 1 (RAOP) or AirPlay 2 (transient pairing, encrypted RTSP, realtime stream, NTP
timing), pulls 44.1 kHz 16-bit stereo from a `Render` callback in 352-frame packets and
sends it as uncompressed ALAC. It knows nothing about tracks, the engine or discovery.

Not here: `AirPlayOutput` (the engine side), discovery, the DACP server that receives
remote buttons, the UDP port range (`crates/cast/src/doc.md`); choosing between local,
AirPlay and renderers (`crates/pawse/src/cast/doc.md`).

Not supported: devices that pair with a code shown on them (HomeKit pair-setup and
pair-verify with stored keys), passwords, buffered audio (type 103), PTP timing, groups,
now playing on Apple TVs (they draw it only from MediaRemote: `POST /command` with
protobufs after pair-verify).

## Files

- `lib.rs` — `Device` (from `_raop._tcp` / `_airplay._tcp` records), `Protocol`, `Shows`,
  `Error`, `volume_db`, `dacp_id`.
- `stream.rs` — `Stream`: sender, retransmit and event threads; `Timing` (playhead,
  `unheard_frames`).
- `handshake.rs` — `Link`, the session set up for either protocol; `TimingServer` (the NTP
  responder); sender id, User-Agent, UDP port binding.
- `pairing.rs`, `srp.rs` (SRP-6a client, 3072-bit group, SHA-512; a test-only server),
  `tlv.rs` (TLV8, 255-byte fragments) — transient pair-setup.
- `secure.rs` — HKDF-SHA512, `FrameCipher` (control and event frames), `AudioCipher`;
  ChaCha20-Poly1305.
- `rtsp.rs` — minimal RTSP client, plain or encrypted: one request at a time, bodies up to
  64 KiB, 5 s connect and I/O timeouts; answers may say `RTSP/1.0` or `HTTP/1.1`.
- `events.rs` — `EventChannel`, the AirPlay 2 event connection (the device's requests to
  us), decrypted and answered.
- `metadata.rs` — `NowPlaying`, `Cover`, `RemoteCommand`: DMAP list, progress line, a remote
  command from an event or a DACP path.
- `rtp.rs` — audio, sync, timing and retransmit packets, NTP time. `alac.rs` — ALAC in the
  uncompressed ("escape") form.
- `fake.rs` — test-only AirPlay 2 receiver on loopback.

## Threads and errors

- Per stream: `airplay-sender` (owns the RTSP connection), `airplay-control` (retransmit
  requests), `airplay-timing` (NTP responder), and on AirPlay 2 `airplay-events`. Timing and
  retransmits have a thread each so a reply is never delayed behind the other socket.
- A failed request while playing ends the stream with `StreamEvent::Lost`. While not
  playing the stream just ends (`is_alive` turns false) and the owner reconnects on play.
  Optional requests (metadata, progress, keep-alive) end it only on an `Io` error; any
  other status is logged.
- Dropping a `Stream` sends `TEARDOWN` and joins every thread: it can block for the RTSP
  timeout, so drop it off the UI thread.
- Volume, title and progress are coalesced: only the newest value is sent once the previous
  request is done.

## RAOP session

`ANNOUNCE` carries an SDP for AppleLossless (`fmtp` 352 frames, 16 bit, 2 channels, 44100);
`RECORD` the first sequence number and RTP time; `SET_PARAMETER volume` follows if a volume
was given. The `SETUP` answer's `server_port` is the audio port and `control_port` the
speaker's control port (audio port + 1 when absent); its timing port is not used. An
`OPTIONS` every 15 s, playing or not, keeps the connection: Apple TV 4 and HomePod drop RAOP
sessions that stay quiet on RTSP while playing (per OwnTone).

## AirPlay 2 session

Order and values follow Music Assistant's `cliairplay` (`ap2_client.c`) and OwnTone, which
play on third-party receivers; deviating breaks real devices.

1. `GET /info` in the clear.
2. **Transient pair-setup** (`POST /pair-setup`, `X-Apple-HKP: 4`, TLV8, M1–M4): SRP-6a with
   user `Pair-Setup` and the fixed code `3939`; M1 flags `0x10` mean transient. The device's
   proof must match, so a device that ran SRP with another secret fails here; a 470 means it
   wants a code shown on it. `k` and `u` hash values padded to 384 bytes; A, B, x, M1 and
   the key `K = SHA512(S)` are unpadded.
3. From here every request and answer is encrypted: frames of at most 1024 plaintext bytes,
   `[2-byte little-endian length][ciphertext][16-byte tag]`, the length is the AAD, the
   nonce is 4 zero bytes and a per-direction 64-bit little-endian frame counter. Keys:
   HKDF-SHA512 over `K`, salt `Control-Salt`, info `Control-Write-Encryption-Key` (ours) and
   `Control-Read-Encryption-Key` (the device's).
4. Session `SETUP` (binary plist): `deviceID` (the 8-byte sender id, colon form),
   `sessionUUID`, `timingProtocol` `NTP`, `timingPort`. The answer's `eventPort` is
   connected to (failure is only logged) and read as in "The remote".
5. `RECORD` without headers, before the stream `SETUP` (Samsung receivers play silence
   otherwise, per Music Assistant).
6. Stream `SETUP`: one realtime stream (`type` 96, ALAC `ct` 2, 44.1/16/2, `spf` 352) with
   `shk` = the first 32 bytes of `K`, `latencyMin` 11025, `latencyMax` 88200 and
   `streamConnectionID` = the session id = the RTP SSRC. The answer's `dataPort` and
   `controlPort` are read by name.
7. `SET_PARAMETER volume`, last (Sonos ignores it earlier, per OwnTone).

- `POST /feedback` every 2 s, playing or not, keeps the session. From the session `SETUP` to
  the volume, requests wait up to 10 s.
- User-Agent: AirPlay 2 sends `AirPlay/670.6.2` like Music Assistant (HomePods on OS 27
  check it). RAOP sends `AirPlay/999.0.0` to Apple speakers (`am` with `AudioAccessory` or
  `AppleTV`) and `iTunes/7.6.2 (Windows; N;)` to the rest, as libraop does.
- The sender id (`DACP-ID`, `deviceID`) is random but fixed for the app run, and its first
  hex digit is never 0: shairport-sync strips leading zeros from the `iTunes_Ctrl_` name
  before comparing it with the `DACP-ID` header.
- **The timing responder runs before the SETUPs**, for RAOP too. The Hisense TV sends NTP
  requests to our `timingPort` right after `RECORD` and does not answer the stream `SETUP`
  until they are answered.
- Audio packet: the RTP header, then the ALAC frame encrypted with `shk`, the 16-byte tag
  and 8 nonce bytes. The nonce is 4 zero bytes, the sequence number little-endian, then
  zeros; its last 8 bytes travel at the end of the packet; AAD is header bytes 4–11. The
  nonce repeats when the sequence number wraps (every ~8.7 minutes): that is what Music
  Assistant and OwnTone send and receivers expect. Retransmits resend the stored encrypted
  packet.

## Streaming and timing

- `FLUSH` (pause with flush, seek) carries the next sequence number and RTP time and clears
  the retransmit history; `pause(false)` sends nothing and the speaker plays out what it
  has. A pause flush hands the frames sent but not heard back with `Render::rewind`; a seek
  flush does not.
- The sender keeps a clock, the frame that should be audible now (the playhead). Packets go
  out while their first frame is no further than `LATENCY_FRAMES` (2 s) ahead of it; a
  start or flush re-anchors the clock so the first packet plays 2 s later. More than a
  second behind, it re-anchors instead of bursting.
- A sync packet (control port) goes out with the first packet and then every second. The
  first after a start or flush has the extension bit, the first audio packet the marker
  bit. AirPlay 2 with NTP timing uses the same packet.
- The control and timing sockets are the ones the speaker sends to: they take the first
  free ports of the range given to `Stream::start` (`0..=0` in tests means any) and fall
  back to a random port with a warning. The audio socket only sends and stays random.
- Retransmit requests are answered from the last 1024 packets. RAOP: inside the control
  port's `0xd6` reply. AirPlay 2: the stored packet is sent again to the audio port, as
  OwnTone does, because an encrypted packet in the `0xd6` reply is 1452 bytes and Apple's
  receiver SDK reads the control port into a 1444-byte `RTCPPacket`, cutting off the nonce.
  A packet no longer kept gets Apple's 8-byte "futile" reply (`80 d6 00 01 <seq> 00 00`) on
  AirPlay 2 so the receiver stops asking; RAOP gets none.
- `unheard_frames` is what was sent but not played yet plus the latency the speaker adds:
  `Audio-Latency` from the `RECORD` answer (capped at 2 s) and, on AirPlay 2, the
  `latencyMin` we ask for, which Apple's receiver SDK adds to every packet's play time. It
  is never more than was sent since the last start or flush, so after a seek "engine
  position minus unheard" holds at the target instead of jumping back 2 s. With no clock
  (paused after a flush) nothing is unheard.

## Now playing and progress

- `set_now_playing` sends what Music Assistant and OwnTone send to non-Apple-TV receivers:
  title, artist and album as DMAP (`application/x-dmap-tagged`), then the cover with its own
  type (`image/jpeg` / `image/png`; `image/none` with no body for a track without one, which
  Apple's receiver SDK takes as "no artwork", so the previous cover does not stay). Both
  carry `RTP-Info: rtptime=` of the frame heard when they go, so a receiver that applies
  metadata at that frame applies it at once.
- `Device.shows` gates each part. RAOP: the kinds `md` lists (`0` text, `1` artwork, `2`
  progress; none without `md`, as OwnTone, libraop and pyatv do). AirPlay 2: feature bits
  17, 15 and 16 (text, artwork, progress).
- `set_progress(heard_ms, duration_ms)` sends `progress: start/current/end` in RTP time.
  `heard_ms` is the track position the listener hears now, as the owner computes it (engine
  position minus `unheard_frames` and its own queue; negative while the previous track's
  tail still plays). The heard frame is the next frame to send minus `unheard_frames`;
  `start` is that minus `heard_ms`; `current` is the heard frame but never before `start`.
  The last `start`/`end` are kept until a flush; new metadata sends
  them again with a fresh `current`. Duration 0 sends no line, but the start still counts
  for the hold below.
- The owner calls `set_progress` again after anything that moves the mapping: a new track,
  a seek, a pause or resume (a flush hands frames back and they go out again with new
  timestamps).
- **The next track is named when it is heard.** The owner's engine moves to the next track
  as soon as the previous one is decoded, about 2 s before its end is heard (that lead keeps
  gapless playback seamless), and sends the new progress then (its `start` still ahead) and
  the new title right after. A progress whose `start` is still ahead is held, and so is a
  title that comes while it is held (the newest one); both go out once that frame is heard,
  playing or paused without a flush, so the device switches title and bar with the sound. A
  flush drops the wait but keeps the title, which then goes with the next progress. A title
  that comes before its progress goes at once; `pawse` sends the progress first.

## The remote

Two ways in; the receiver chooses.

- **DACP** (both protocols): every request carries `DACP-ID` (`dacp_id()`) and
  `Active-Remote` (the session id, `Stream::active_remote`). A receiver looks up
  `iTunes_Ctrl_<DACP-ID>._dacp._tcp` and sends `GET /ctrl-int/1/<command>` with that
  `Active-Remote`. `dacp_command` maps `play`, `pause` / `discrete-pause` / `stop`,
  `playpause`, `nextitem`, `previtem`; the server is in `cast`.
- **The event connection** (AirPlay 2): requests encrypted with keys of their own (HKDF over
  `K`, salt `Events-Salt`; the device encrypts with `Events-Write-Encryption-Key`, we with
  `Events-Read-Encryption-Key`, the reverse of the control channel, as in OwnTone's
  `pair_ap`). A binary plist `{type: sendMediaRemoteCommand, value: play | paus | plps |
  nitm | pitm | stop}` becomes `StreamEvent::Remote`. Every request gets a `200` (with its
  version, `Audio-Latency: 0`, its `Server` and `CSeq`, as Music Assistant answers);
  anything else, and the device closing the connection, is logged at info so a device's own
  form shows up.
- An Apple-SDK receiver uses the event connection when the session `SETUP`'s `sourceVersion`
  is at least the SDK's MediaRemote-commands minimum, DACP otherwise. We send none, so such
  devices use DACP.

## Devices

- **RAOP** (`Device::from_service`): instance names are `<MAC>@<name>`. Offered only when
  the device takes this stream: no encryption (`et` lists 0), ALAC (`cn` lists 1), UDP,
  44100/16/2, no password; missing keys count as yes.
- **AirPlay 2** (`Device::from_airplay_service`): the id is `deviceid` without colons, the
  same id as the device's RAOP record. Offered when `features` (`0xLOW,0xHIGH`) has bit 9
  (audio), 38 or 48 (AirPlay 2) and 46 or 48 (HomeKit / CoreUtils pairing), and nothing
  asks for more than transient pairing: no PIN flag (0x8 in `flags`/`sf`), no password
  (0x80, `pw`), `acl` 0, `act` not 2. Flag 0x200, which Music Assistant and pyatv read as
  "pairing required", does not exclude a device: the Hisense TV sets it and takes
  transient pairing.
- Volume is AirPlay's −30…0 dB scale, linear in dB; 0 is −144 (mute). A stream started
  without a volume (`Stream::start(.., None, ..)`) sends no `SET_PARAMETER volume` in either
  handshake, so the device keeps its own volume; a later `set_volume` is sent as usual (the
  app's volume modes: `crates/pawse/src/cast/doc.md`).

## Testing

- `stream.rs` runs whole AirPlay 2 sessions against `fake::FakeReceiver`. Like the Hisense
  TV, the fake answers the stream `SETUP` only after our timing port answered a clock
  request.
- `srp.rs` and `secure.rs` check against vectors made with other implementations
  (`srptools` as pyatv uses it, Python `cryptography`).
- RAOP has no fake; check it against a shairport-sync AirPlay 1 build. shairport-sync's
  AirPlay 2 build cannot be the AirPlay 2 test receiver: it handles only PTP sessions.

## Decisions

Owner's calls; ask before changing.

- The progress bar on the Hisense TV stays as it is: it keeps it for the first track of a
  session only and loses it after a seek or a track change. Every form tried (OwnTone's
  bundle, iTunes' order, `mper`/`astm`/`caps`, a progress re-sent after a seek) and Music
  Assistant's `cliairplay` behave the same on it, as do other macOS AirPlay senders.

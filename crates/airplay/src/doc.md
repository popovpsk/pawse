# airplay

An AirPlay sender: a network sound card. It takes 44.1 kHz 16-bit stereo from
a `Render` callback and streams it to one speaker, over AirPlay 1 (RAOP) or
AirPlay 2 (transient pairing, encrypted control channel, realtime stream with
NTP timing). `cast::AirPlayOutput` puts it behind the audio engine. Not
supported: devices that pair with a code shown on them (HomeKit pair-setup and
pair-verify with stored keys), passwords, buffered audio (type 103), PTP
timing, groups.

## Files

- `lib.rs` — `Device` (from an mDNS `_raop._tcp` or `_airplay._tcp` record)
  and `Protocol`, constants, `volume_db`, `random`.
- `stream.rs` — `Stream`: the sender thread (packets, sync, flush, keep-alive,
  metadata), the retransmit thread and the event thread.
- `handshake.rs` — `Link`: the session set up for either protocol (RTSP for
  RAOP; `/info`, pairing, two SETUPs with binary plists for AirPlay 2), and
  `TimingServer`, the NTP responder.
- `pairing.rs` — transient pair-setup (M1–M4) and the session keys.
- `srp.rs` — the SRP-6a client (3072-bit group, SHA-512); a test-only server.
- `secure.rs` — HKDF-SHA512 key derivation, the encrypted control frames
  (`FrameCipher`) and encrypted audio packets (`AudioCipher`), all
  ChaCha20-Poly1305.
- `tlv.rs` — TLV8 (pairing bodies), with 255-byte fragments.
- `metadata.rs` — `NowPlaying`, `Cover`, `RemoteCommand`: the DMAP item list,
  the progress line, reading a remote command from an event or a DACP path.
- `events.rs` — `EventChannel`: the AirPlay 2 event connection (the device's
  requests to us), decrypted and answered.
- `rtsp.rs` — a minimal RTSP client (one request at a time, bodies up to
  64 KiB, 5 s connect and I/O timeouts), plain or encrypted; answers may say
  `RTSP/1.0` or `HTTP/1.1`.
- `rtp.rs` — audio headers, sync, timing and retransmit packets, NTP time.
- `alac.rs` — ALAC frames in the uncompressed ("escape") form.
- `fake.rs` — a test-only AirPlay 2 receiver on loopback (pairing with the
  code it is given, encrypted RTSP, SETUP, decrypting the audio).

## RAOP session

`OPTIONS *`, `ANNOUNCE` with an SDP for AppleLossless (`fmtp` 352 frames,
16 bit, 2 channels, 44100), `SETUP` with our control and timing ports (the
answer names the server's audio, control and timing ports), `RECORD` with the
first sequence number and RTP time, `SET_PARAMETER volume`. An `OPTIONS`
every 15 s, playing or not, keeps the connection (libraop sends one every
25 s in both states; OwnTone notes that Apple TV 4 and HomePod drop RAOP
sessions that stay quiet on RTSP while playing).

## AirPlay 2 session

Order and values follow Music Assistant's `cliairplay` (`ap2_client.c`) and
OwnTone, which play on third-party AirPlay 2 receivers:

1. `GET /info` in the clear.
2. **Transient pair-setup**: `POST /pair-setup` with `X-Apple-HKP: 4` and TLV8
   bodies. M1 `{Method 0, State 1, Flags 0x10}` → M2 `{State 2, Salt,
   PublicKey}`; SRP-6a with user `Pair-Setup` and the fixed code `3939` →
   M3 `{State 3, PublicKey A, Proof M1}` → M4 `{State 4, Proof}`. The device's
   proof must match, so a device that ran SRP with another secret fails
   here. SRP conventions: `k` and `u` hash the values padded to 384 bytes,
   everything else (A, B, x, M1, the key `K = SHA512(S)`) is unpadded. The
   test vector comes from `srptools` as pyatv uses it.
3. From here every request and answer is encrypted: frames of at most 1024
   plaintext bytes, `[2-byte little-endian length][ciphertext][16-byte tag]`,
   the length is the AAD, the nonce is 4 zero bytes and a per-direction
   64-bit little-endian frame counter. Keys: HKDF-SHA512 over `K`, salt
   `Control-Salt`, info `Control-Write-Encryption-Key` (ours) and
   `Control-Read-Encryption-Key` (the device's).
4. Session `SETUP` (binary plist): `deviceID` (our 8-byte sender id, colon
   form), `sessionUUID`, `timingProtocol` `NTP`, `timingPort`. The answer
   names an `eventPort`; we connect to it (no connection is only logged) and
   read the device's requests there (see "Now playing and the remote").
5. `RECORD` without headers, before the stream SETUP (Samsung receivers play
   silence otherwise, per Music Assistant).
6. Stream `SETUP`: one stream, `type` 96 (realtime), `ct` 2 (ALAC),
   `audioFormat` 0x40000 (44.1/16/2), `spf` 352, `sr`, `shk` (the audio key,
   the first 32 bytes of `K`), our `dataPort` and `controlPort`,
   `latencyMin` 11025, `latencyMax` 88200, `streamConnectionID` (the session
   id, also the RTP SSRC). The answer's `dataPort` and `controlPort` are read
   by name.
7. `SET_PARAMETER volume`, last (Sonos ignores it earlier, per OwnTone).

`POST /feedback` every 2 s, playing or not, keeps the session; no answer (or
a broken connection) ends the stream like any failed request, an answer that
is not 2xx is only logged. The SETUPs wait up to 10 s.
User-Agent is `AirPlay/670.6.2` like Music Assistant's (HomePods on OS 27
check it). RAOP sends `AirPlay/999.0.0` to Apple speakers (`am` with
`AudioAccessory` or `AppleTV`) and `iTunes/7.6.2 (Windows; N;)` to the rest,
as libraop does (OwnTone sends `AirPlay/999.0.0` to all). The sender id
(DACP-ID, `deviceID`) is random but kept for the whole app run, and its first
hex digit is never 0: shairport-sync strips leading zeros from the
`iTunes_Ctrl_` name before comparing it with the `DACP-ID` header
(`mdns_avahi.c`), so such an id would never be matched.

**The timing responder runs before the SETUPs.** The Hisense TV sends three
NTP requests to our `timingPort` right after RECORD and does not answer the
stream SETUP until they are answered (measured 2026-10-04: with the responder
started after the handshake the SETUP timed out; started with the socket, it
was answered within 35 ms). `TimingServer` starts when its socket is bound,
for RAOP too.

Audio packets: the RTP header as for RAOP, then the ALAC frame encrypted with
`shk`, the 16-byte tag and 8 nonce bytes. The nonce is 4 zero bytes, the
sequence number little-endian, then zeros; its last 8 bytes travel at the end
of the packet. AAD is header bytes 4–11 (timestamp and SSRC). The nonce
repeats when the sequence number wraps (every ~8.7 minutes); this is what
Music Assistant and OwnTone send and what receivers expect, and it only
weakens the secrecy of audio that is played aloud anyway. Retransmits resend
the stored encrypted packet.

Not yet seen on the TV: `FLUSH` (pause), a reconnect, `SET_PARAMETER volume`
(its `volumeControlType` is 2, so AirPlay volume may move the TV's own).

## Now playing and the remote

- `Stream::set_now_playing` sends what Music Assistant and OwnTone send to
  receivers that are not Apple TVs: `SET_PARAMETER` with
  `application/x-dmap-tagged` (an `mlit` with `mikd` 2, `minm` title, `asar`
  artist, `asal` album), then the cover with its own type (`image/jpeg` or
  `image/png`, or `image/none` with no body for a track without one, which
  Apple's receiver SDK takes as "no artwork", so the previous cover does
  not stay), both with `RTP-Info: rtptime=` the frame heard when they go
  (a receiver that applies metadata at that frame applies it at once).
  Apple TVs draw now playing only from MediaRemote (`POST /command` with
  protobufs, after pair-verify), which is not done.
- What a device shows gates each part (`Device::shows`): a RAOP speaker gets
  the kinds its `md` lists (`0` text, `1` artwork, `2` progress; none without
  `md`, as OwnTone, libraop and pyatv do), an AirPlay 2 one those of its
  feature bits 17, 15 and 16 (`kAirPlayFeature_AudioMetaData*` in Apple's
  SDK).
- `Stream::set_progress(heard_ms, duration_ms)` sends `progress:
  start/current/end` in RTP time. `heard_ms` is the track position the
  listener hears now, as the owner computes it (engine position minus
  `unheard_frames` and its own queue; negative while the previous track's
  tail still plays). The sender maps it on its own timeline: the frame heard
  now is the next frame to send minus `unheard_frames`, `start` is that
  minus `heard_ms`, and `current` is the heard frame but never before
  `start` (OwnTone sends `max(position, start)` too). The last `start`/`end`
  are kept until a flush; new metadata sends them again with a fresh
  `current`. Paused (no clock) nothing is unheard, so the next frame to send
  is the one heard at the position: a progress sent while paused holds after
  the resume. The owner sends it again after anything that moves the
  mapping: a new track, a seek, a pause or resume (a flush hands frames back
  and they go out again with new timestamps). Without a duration (0) no line
  is sent (it would end before the position), but the start still counts
  for the hold below.
- **The next track is named when it is heard.** The owner's engine moves on
  to the next track as soon as the previous one is decoded, about 2.25 s
  before its end is heard (that lead is what keeps gapless playback seamless),
  and sends the new progress then (its `start` still ahead) and the new title
  right after. A progress whose `start` is still ahead is held, and so is a
  title that comes while it is held (the newest one); both go out once that
  frame is heard, playing or paused without a flush, so the device switches
  title and bar with the sound instead of 2 s early. A flush (a pause or a
  seek inside those 2 s) drops the wait but keeps the title, which then goes
  with the next progress. A title that comes before its progress goes at
  once; `pawse` sends the progress first.
- The Hisense TV keeps the progress bar for the first track of a session
  only and loses it after a seek or a track change. Every form tried (OwnTone's
  bundle, iTunes' order with the title's `rtptime` as the middle number,
  `mper`/`astm`/`caps`, a progress re-sent after a seek) and Music
  Assistant's cliairplay behaved the same on it (2026-10-04); three other
  macOS AirPlay senders show the same, so it is left as it is.
- Both are coalesced like the volume (only the newest is sent) and go over
  both protocols. A device that answers them with an error status is only
  logged; a broken connection ends the stream as usual.
- **The remote, two ways.**
  - **DACP** (both protocols): every request carries `DACP-ID` (our sender
    id, `dacp_id()`, 16 hex digits, fixed for the app run) and
    `Active-Remote` (the session id, `Stream::active_remote`). A receiver
    looks up `iTunes_Ctrl_<DACP-ID>._dacp._tcp` and sends
    `GET /ctrl-int/1/<command>` with that `Active-Remote`. Music Assistant
    takes its remote buttons this way from AirPlay 2 receivers too.
    `dacp_command` maps `play`, `pause` / `discrete-pause` / `stop`,
    `playpause`, `nextitem`, `previtem`; the server itself is in `cast`.
  - **The event connection** (AirPlay 2): requests encrypted with keys of
    their own (HKDF over `K`, salt `Events-Salt`; the device encrypts with
    `Events-Write-Encryption-Key`, we with `Events-Read-Encryption-Key`, the
    reverse of the control channel, as in OwnTone's `pair_ap`). A binary
    plist `{type: sendMediaRemoteCommand, value: play | paus | plps | nitm |
    pitm | stop}` becomes `StreamEvent::Remote(Play | Pause | PlayPause |
    Next | Previous | Pause)`; every request gets a `200` (with the request's
    version, `Audio-Latency: 0`, its `Server` and `CSeq`, as Music Assistant
    answers), anything else is logged (info) so a device's own form shows
    up, and so is the device closing the connection. The `airplay-events`
    thread reads it until the stream closes or dies.
  - Which of the two a receiver built on Apple's SDK uses is its choice
    (`AirPlayReceiverServerSendMediaCommandWithOptions` in the 366 sources):
    the event connection when the session `SETUP`'s `sourceVersion` is at
    least `kAirPlaySourceVersion_MediaRemoteCommads_Min` (its value is not
    in the sources), DACP otherwise. We send no `sourceVersion`, so DACP;
    Apple Music (`AirPlay/960.13.1`) gets the event connection, which is why
    it saw no DACP request from the TV.
  - The Hisense TV sends its buttons over DACP (`playpause`, `nextitem`,
    `previtem`, and `setproperty?dmcp.device-prevent-playback=`), measured
    2026-10-04. It resolves the service's host and connects over IPv6
    link-local whenever the host has an IPv6 address, so the server and its
    announcement in `cast` take IPv6 (see `cast`'s doc). Apple Music on the
    Mac talks to it over IPv6 link-local too and streams buffered audio over
    TCP port 6000.

## Both protocols

Pausing and seeking send `FLUSH` with the next sequence number and RTP time;
closing sends `TEARDOWN`. A failed request while playing ends the stream
with `StreamEvent::Lost`; while paused the stream just ends (`is_alive` turns
false) and the owner reconnects on play. Volume changes are coalesced: only
the newest value is sent once the previous `SET_PARAMETER` is done.

## Timing

- The sender keeps a clock: the frame that should be audible now (the
  playhead). Packets are sent while their first frame is no further than
  `LATENCY_FRAMES` (2 s) ahead of the playhead, so the speaker's buffer holds
  2 s; a start or a flush re-anchors the clock so the first packet plays 2 s
  later. If the thread falls more than a second behind it re-anchors instead
  of bursting.
- A sync packet (control port) goes out with the first packet and then every
  second: "this RTP time is playing at this NTP time, this is the next RTP
  time". The first one after a start or flush has the extension bit, the first
  audio packet the marker bit. AirPlay 2 with NTP timing uses the same packet.
- The speaker asks for our clock on the timing port; the answer carries its
  send time back plus our receive and send times. Timing and retransmits have
  a thread each, so a reply is never delayed behind the other socket.
- The control and timing sockets are the ones the speaker sends to, so they
  take the first free ports of the range given to `Stream::start` (the `cast`
  crate's fixed range, which a firewall can allow; `0..=0` in the tests means
  any port) and fall back to a random port with a warning when the range is
  taken. The audio socket only sends and stays on a random port.
- Retransmit requests are answered from the last 1024 packets: over RAOP
  inside the control port's `0xd6` reply, over AirPlay 2 by sending the
  stored packet again to the audio port, as OwnTone does. An encrypted
  packet in the `0xd6` reply is 1452 bytes, and Apple's receiver SDK reads
  the control port into a 1444-byte `RTCPPacket`, so the packet's nonce
  would be cut off. A packet no longer kept gets Apple's 8-byte "futile"
  reply (`80 d6 00 01 <seq> 00 00`, as iOS sends it) on AirPlay 2, so the
  receiver stops asking.
- `unheard_frames` is how much was sent but not played yet, plus the latency
  the speaker adds itself: `Audio-Latency` from the RECORD answer (11025
  frames on shairport-sync, none from the Hisense TV, whose `/info` lists
  zero latencies), and over AirPlay 2 also the `latencyMin` we ask for in
  the stream `SETUP` (11025), which Apple's receiver SDK adds to every
  packet's play time (`audioLatencyOffset`); never more than was sent since
  the last start or
  flush: right after a seek nothing old is waiting, so a position shown as
  "engine position minus unheard" holds at the seek target instead of
  jumping back 2 s.
- On a flush the frames that were sent but not heard are handed back with
  `Render::rewind`, so the source can play them again after a pause.

## ALAC

A frame is a channel-pair element: tag 1 (3 bits), instance 0 (4), 12 unused
bits, no explicit size (the 352 frames from `fmtp`), no shift, the escape bit,
then each sample as 16 big-endian bits, left then right, then the END tag.
Nothing is compressed; at 44.1 kHz that is about 1.4 Mbit/s.

## Devices

- **RAOP** (`Device::from_service`): instance names are `<MAC>@<name>`. A
  device is offered only when it can take this stream: `et` lists 0 (no
  encryption), `cn` lists 1 (ALAC), `tp` has UDP, `sr`/`ss`/`ch` are
  44100/16/2 and `pw` is not true; missing keys are taken as yes.
- **AirPlay 2** (`Device::from_airplay_service`): the id is `deviceid`
  without colons, so it is the same id as the device's RAOP record. Offered
  when `features` (`0xLOW,0xHIGH`) has bit 9 (audio), bit 38 or 48 (AirPlay
  2) and bit 46 or 48 (HomeKit / CoreUtils pairing), and nothing asks for
  more than transient pairing: no PIN flag (0x8 in `flags`/`sf`), no
  password (0x80, `pw`), `acl` 0 and `act` not 2 (a home's members or the
  current user only). Flag 0x200, which Music Assistant and pyatv read as
  "pairing required", does not exclude a device: the Hisense TV sets it
  (`flags=0x244`) and takes transient pairing.

Volume is AirPlay's −30…0 dB scale, linear in dB; 0 is −144 (mute). A
stream started without a volume (`Stream::start(.., None, ..)`) sends no
`SET_PARAMETER volume` in either handshake, so the device keeps its own
volume; a later `set_volume` is sent as usual. The app does that when the
volume slider is not to touch the device (`cast`'s doc).

## Testing

- `stream.rs` runs whole AirPlay 2 sessions against `fake::FakeReceiver`:
  the request order, the event connection, decrypted audio that matches what
  was rendered, `/feedback`, volume, TEARDOWN, a refused pairing when the
  code differs, metadata and progress (a cover the fake refuses ends
  nothing; the next track's progress and title wait until it is heard),
  retransmits (again on the audio port, "futile" for a forgotten packet)
  and remote buttons sent over the encrypted event connection. Like the TV, the fake answers the stream SETUP only after our
  timing port answered a clock request.
- `srp.rs` and `secure.rs` check against vectors made with other
  implementations (`srptools`, Python `cryptography`).
- RAOP was tested against shairport-sync 4.3 (classic build) on the Pi: its
  log shows the session, the 2.25 s lead and the flushes, and its `stdout`
  output is the expected sine without discontinuities.
- AirPlay 2 was tested against the Hisense 55U7SE (AirPlay SDK 3.6.0.126,
  `AirTunes/377.40.00`) on 2026-10-04: pairing in 128 ms, the session up in
  240 ms, a 440 Hz tone heard clean, `/feedback` answered every 2 s, no
  retransmit requests. shairport-sync's AirPlay 2 build cannot be the test
  receiver for this: it handles only PTP sessions ("ntp stream handling is
  not implemented").

# airplay

An AirPlay 1 (RAOP) sender: a network sound card. It takes 44.1 kHz 16-bit
stereo from a `Render` callback and streams it to one speaker. No AirPlay 2:
no pairing, no encryption, no buffered audio. `cast::AirPlayOutput` puts it
behind the audio engine.

## Files

- `lib.rs` — `Device` (from an mDNS `_raop._tcp` record), constants,
  `volume_db`.
- `stream.rs` — `Stream`: the RTSP session, the sender thread, the timing and
  control threads.
- `rtsp.rs` — a minimal RTSP client (one request at a time, `Content-Length`
  bodies up to 64 KiB, 5 s connect and I/O timeouts).
- `rtp.rs` — audio, sync, timing and retransmit packets, NTP time.
- `alac.rs` — ALAC frames in the uncompressed ("escape") form.

## The session

`OPTIONS *`, `ANNOUNCE` with an SDP for AppleLossless (`fmtp` 352 frames,
16 bit, 2 channels, 44100), `SETUP` with our control and timing ports (the
answer names the server's audio, control and timing ports), `RECORD` with the
first sequence number and RTP time, `SET_PARAMETER volume`. Pausing and
seeking send `FLUSH` with the next sequence number and RTP time; closing sends
`TEARDOWN`. While paused an `OPTIONS` every 15 s keeps the connection. A
failed request while playing ends the stream with `StreamEvent::Lost`; while
paused the stream just ends (`is_alive` turns false) and the owner reconnects
on play. Volume changes are coalesced: only the newest value is sent once the
previous `SET_PARAMETER` is done.

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
  audio packet the marker bit.
- The speaker asks for our clock on the timing port; the answer carries its
  send time back plus our receive and send times. Timing and retransmits have
  a thread each, so a reply is never delayed behind the other socket.
- Retransmit requests are answered from the last 1024 packets.
- `unheard_frames` is how much was sent but not played yet, plus the latency
  the speaker adds itself (`Audio-Latency` from the RECORD answer, 11025
  frames on shairport-sync), but never more than was sent since the last
  start or flush: right after a seek nothing old is waiting, so a position
  shown as "engine position minus unheard" holds at the seek target instead
  of jumping back 2 s.
- On a flush the frames that were sent but not heard are handed back with
  `Render::rewind`, so the source can play them again after a pause.

## ALAC

A frame is a channel-pair element: tag 1 (3 bits), instance 0 (4), 12 unused
bits, no explicit size (the 352 frames from `fmtp`), no shift, the escape bit,
then each sample as 16 big-endian bits, left then right, then the END tag.
Nothing is compressed; at 44.1 kHz that is about 1.4 Mbit/s.

## Devices

Instance names are `<MAC>@<name>`. A device is offered only when it can take
this stream: `et` lists 0 (no encryption), `cn` lists 1 (ALAC), `tp` has UDP,
`sr`/`ss`/`ch` are 44100/16/2 and `pw` is not true; missing keys are taken as
yes. Apple TVs and HomePods usually need pairing and are left out.

Volume is AirPlay's −30…0 dB scale, linear in dB; 0 is −144 (mute).

Tested against shairport-sync 4.3 (classic build) on the Pi: its log shows the
session, the 2.25 s lead and the flushes, and its `stdout` output is the
expected sine without discontinuities.

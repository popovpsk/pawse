# audio_decoder

Turns a file or a network stream into `AudioBatch`es for the engine. `Decoder` picks a
backend by extension: Monkey's Audio (`ape-decoder`), DSF/DFF (the `dsd` crate, decimated
to PCM) or Symphonia for everything else. It also gives `audio_embedding` frame-exact and
byte-placed access into FLAC, WAV and ALAC files.

Not here: the `AudioSource` trait and batch types (`audio_common`), DSD parsing and
decimation (`dsd`, no doc), what the engine does with batches (`audio_engine/src/doc.md`),
the network readers behind `MediaStream` (`media_stream/src/doc.md`).

## Files

- `decoder.rs` — lib root: `Decoder`, `Codec`, `MediaStream`, `can_stream`, the three
  backends, planar → interleaved mapping, all tests.
- `splice.rs` — byte-level FLAC access behind `seek_near`: `Splicer`, the shared-stream
  `Handle`, the spliced virtual stream.

## Rules

- `open_stream` decodes from a `MediaStream` (`Read + Seek`) and refuses what `can_stream`
  rejects (ape, dsf, dff): those backends read a `File`, so the app downloads them whole.
- `Decoder::codec` names the codec (APE and DSD by backend). `cast` uses it to decide what a
  receiver can play as is: an `.m4a` may be AAC or ALAC.
- ALAC's bit depth is read from its magic cookie (`alac_bit_depth`): Symphonia's MP4 reader
  leaves `bits_per_sample` empty and its CAF reader puts the container's 0 there. The 16-bit
  fallback would label 24-bit files as 16 and make `cast` send them as 16-bit PCM.
- A packet that fails to decode is skipped. A batch may be empty (Vorbis yields an empty
  first packet): end of stream is `Ok(None)` only.
- The codec registry is our own (`CODECS`), not `symphonia::default::get_codecs()`: Symphonia
  has no Opus decoder, so `symphonia-adapter-libopus` is registered next to the defaults. Do
  not "simplify" back — Opus would stop playing and no other codec's test would notice. The
  adapter builds libopus from source through `cmake` (`opusic-sys`, statically linked), so
  every platform that compiles the app needs `cmake` on `PATH`.
- `seek` takes a fraction of a known duration. It is `SeekMode::Coarse` (FLAC lands up to a
  frame early), clamps the target to the track's `start_ts` (a file whose timeline starts
  late rejects a seek to 0 otherwise), resets the decoder (not for Opus) and returns where
  it landed.

## Frame-exact access

`exact_frames` / `seek_frame` (`AudioSource` defaults: none) are for `audio_embedding`, which
decodes only some windows. Offered only for FLAC, PCM (WAV/AIFF) and ALAC whose time base is
`1/rate`, whose track starts at 0 and whose **first packet** has timestamp 0 — that packet is
read on open and kept in `pending`, so playback still decodes it first. The last check
matters: Symphonia reports `start_ts` 0 for every FLAC, but a FLAC cut with
`ffmpeg -ss … -c copy` keeps its frame numbers and the STREAMINFO length of the whole file.

- `seek_frame` is an accurate seek to a timestamp and returns the frame it landed on; the
  caller skips the rest and checks it, since FLAC's search can overshoot.
- `exact_frames` is the container's frame count (STREAMINFO, WAV data size, MP4 sample
  table).
- Symphonia's MP4 reader cannot seek after reading to the end of the file ("no atom pending
  read"); use a fresh `Decoder`.

## `seek_near` (FLAC by bytes)

`seek_near(fraction, span)` (default `None`) lands on the first FLAC frame at or after
`fraction` of the audio bytes and returns that frame's number. `audio_embedding` places a
whole FLAC's windows this way: Symphonia's FLAC seek is a binary search over bytes — hundreds
of requests a track over the network; this is one.

- Only when `exact_frames` is set, the codec is FLAC and STREAMINFO is in `extra_data`
  (`Splicer::new`); everything else answers `None` and keeps `seek_frame`.
- Splicing: a fresh Symphonia reader is opened on a virtual stream — `fLaC`, the STREAMINFO
  block flagged last, then the file's bytes from the chosen offset. The reader skips the
  partial frame there (its CRC fails); the first packet it returns is the next real frame,
  numbered from the stream start, so its `pts` is the exact frame. It is kept in `pending`;
  what follows is bit-identical to an exact seek to that frame.
- `splice::audio_start` walks the metadata block headers (seeking over bodies, so a picture
  is not read), after an ID3v2 tag if one precedes `fLaC`. The offset stays at least `slack`
  before the end so a whole frame follows it: twice STREAMINFO's maximum frame size, 64 KB
  when unknown.
- `MediaStream::will_need(start, len)` (default no-op) tells a network stream what is about
  to be read — `span` frames at the average bytes per frame, ×1.25 for louder stretches,
  plus `slack` — so a ranged reader fetches it in one request.
- A file is reopened per splice (`Origin::File`). A stream given to `open_stream` is shared
  behind a mutex (`Origin::Shared`); Symphonia and every splice read through their own
  `Handle` with its own position.
- After a splice the reader sees only the bytes after the offset, so `seek` and `seek_frame`
  first reopen at the audio start (`unsplice`); the windows path's fallback seek to frame 0
  must see the whole file.

## Opus

- Always 48 kHz F32, mono or stereo only: the adapter rejects 5.1/7.1 at decoder creation
  ("unsupported number of channels").
- The adapter trims the encoder pre-skip once, on the first packet it decodes, and `reset`
  does not re-arm it — so a seek builds a fresh decoder instead of resetting.
- Opus needs history: a seek lands `OPUS_PREROLL_MS` before the target, decodes and drops
  packets up to it (`discard_before`) and keeps the first packet to play in `pending`. 320
  ms, not the 80 ms spec minimum — a dense high-bitrate file was still off at 80.
- The OpusHead output gain (Q7.8 dB) is mandatory for players and libopus does not apply it,
  so `opus_output_gain` scales the decoded samples. ReplayGain / R128 tags are not applied.
- The position `seek` returns is the first packet that will be played, in the stream's own
  timeline (container `start_ts` and pre-skip included), clamped at 0.

## Tests

Fixtures come from `make generate`; the Opus ones need an ffmpeg with `libopus`, and
`late_start_opus.opus` has a late timeline and a sweep plus noise so a cold decoder state is
audible. Every test that decodes Opus is `#[cfg_attr(miri, ignore)]`: libopus is C and miri
aborts the whole binary on it.

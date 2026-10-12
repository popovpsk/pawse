# audio_embedding

Turns a track's audio into a vector that describes how it **sounds** (not its tags):
Discogs-EffNet "multi" (MTG / Essentia), 1280 numbers per track. Tracks whose vectors
point the same way after centering sound alike; `pawse::similar_tracks` stores the
vectors and builds "radio from a track" out of them. Every analysis also gives the
scores of MTG's Discogs-EffNet style classifier on the same patches — 400 Discogs
styles, `styles.rs` — which the app turns into genres by sound. GPUI-free, knows nothing about
`music_library`: the input is an `audio_common::AudioSource` (the caller opens
`audio_decoder::Decoder`). This crate is the CPU half — decoding, the CUE range, which
samples the patches need, the model file, the similarity maths and
`EMBEDDING_VERSION`; resampling, the mel spectrogram and the model run on the graphics
card in `audio_embedding_gpu`. The crate owns no threads and does not know wgpu.

## Files

- `lib.rs` — the public API: `Job` (a source plus an optional CUE `TrackRange`),
  `prepare`, `Prepared` and `PatchInput` (the PCM windows), `Embedding`, `EmbedError`,
  `EMBEDDING_VERSION`, `DIM`, and the front-end constants and tables the GPU crate
  builds from (`hann_window`, `mel_filters`, `frame_count`, `PATCH_SAMPLES` …).
- `decode.rs` — `read_mono`: pulls batches from the source, mixes them down to mono
  f32 and applies the CUE range.
- `frontend.rs` — the reference front end: `Resampler` (to 16 kHz; also the runtime
  resampler of the full pass), the Hann window and mel filters, and, behind
  `test-support`, `MelFrontend` (the mel spectrogram on the CPU, realfft);
  `patch_starts` and `spread` (the 32 patches the model sees).
- `windows.rs` — `layout` (which samples each chosen patch needs, merged into
  ranges), `plan` (when the windows path applies) and decoding only those windows
  from a source that can seek to an exact frame.
- `test_support.rs` (feature `test-support`, used by `audio_embedding_gpu`'s tests and
  converter) — `signal`, the synthetic test signal, and `FakeSource`, an in-memory
  `AudioSource`; `Prepared::reference_mel` (the CPU mel of every patch) is behind the
  same feature.
- `similarity.rs` — `MeanAccumulator`, `TopN`, `centered_cosine`, `SCAN_CHUNK`: the
  maths of a nearest-neighbour query over vectors streamed in chunks.
- `styles.rs` — `STYLES` (400) and `LABELS`, the classifier's classes in output order
  (`"Parent---Style"`, copied from MTG's `discogs-effnet-bsdynamic-1.json`), and
  `genres`: the style scores of a track → its genre rows (see "Genres by sound").
- `model_file.rs` — `MODEL` (url, file name, size, SHA-256), `ensure(dir, cancelled)`
  and `discard_if_corrupt(dir)`.
- `oracle_tests.rs` — the reference checks against Essentia (see Tests).

## Two steps, split at I/O

- `prepare(job)` does everything that may wait: reading and seeking the source,
  decoding, choosing the patches and cutting the samples each of them needs. It is a
  free function and runs on the caller's thread, one track per call, so a slow source
  (network, torrent) only delays its own track.
- `audio_embedding_gpu::Device::submit(&[Prepared])` runs the rest — resampling, mel,
  the model, the mean — on the GPU and never touches I/O. Callers batch whatever is
  ready, so one slow track does not hold the others.
- `Prepared` is the PCM the patches need: `rate()` (the source rate on the windows
  path, 16 000 on the full pass), `resampled_len()` (the length of the whole analysed
  range at 16 kHz, which says where the zeros start), and per patch a `PatchInput`:
  its first mel frame, how many of its 128 frames exist (fewer only for a track
  shorter than one patch), and the samples with the index of the first one. Patches
  share the merged windows they were decoded into. Size: ~12 MB for 32 patches at
  44.1 kHz, ~4 MB at 16 kHz, ~50 MB at 192 kHz.
- `reference_mel` turns a `Prepared` into exactly the mel patches the old CPU path
  computed (same functions, same global indices): the tests and the converter's probe
  use it, the app never does.

## Decoding only the windows

The 32 patches depend only on the exact number of samples in the analysed range:
`⌊len·16000/rate⌋` resampled samples → mel frames → candidate starts → `spread`.
And each patch is local: mel frame `k` reads resampled samples `[256k−256, 256k+256)`,
resampled sample `i` reads source samples `⌊i·rate/16000 − 1⌋` and the next one. So
when the length is known before decoding, `windows::plan` lists the source ranges
the patches need (merged where they overlap) and `prepare` decodes only them: ~27%
of a four-minute track, ~2.06 s of audio (33024 resampled samples) in each of 32
places.

- It applies when the source reports `AudioSource::exact_frames` and can
  `seek_frame` (audio_decoder: FLAC, WAV/AIFF, ALAC — see its doc), the range has at
  least a second, every patch is whole, and the windows cover at most 60%
  (`MAX_COVERAGE`) of the range — under ~110 s the full pass is as cheap and simpler.
  Lossy codecs need decoder state from before a seek (bit reservoir, overlap,
  priming) and MP3 seeks only coarsely, so they stay on the full pass, which is
  also the fallback for anything unexpected below.
- **The result is bit for bit the full pass's**: the windows hold the source
  samples, the full pass the 16 kHz ones, and `reference_mel` of both is identical
  (`windows_give_the_same_patches_as_a_full_decode`, real files too). That holds by
  construction: the resampler and the mel spectrogram use the same
  `frontend::resampled_at` / `MelFrontend::frame` with global sample indices, zero
  outside the signal exactly where the streaming path has zeros; and a seek lands on a
  frame boundary at or before the window (`seek_frame` returns where), the samples up
  to the window are skipped. Measured on 40 FLAC/ALAC/WAV files of `music-test`
  (3.2 h) when the mel was still on the CPU: 40 of 40 vectors identical, `prepare`
  3.7× faster (0.25 → 0.07 s a track). On the GPU the windows are resampled there
  (`audio_embedding_gpu`), with exact integer positions.
- A CUE range starts where the full pass starts: the same `decode::seek_start`
  (seek by fraction, then skip), its landing turned back into a frame. The length is
  the full pass's too: the range or the 30-minute cap (`analysed_limit`), or to the
  end of the file.
- Before decoding, `ends_at` checks that the file really has `exact_frames`: it seeks
  to the last frame and decodes up to the claimed end, not past it. Symphonia's MP4
  reader cannot seek after it reached the end of the file (`isomp4: no atom pending
  read`), so reading to EOF there would break every window after it.
- Any surprise — the tail does not match, a seek lands after its target, a window
  ends early, a seek fails — falls back to the full pass. If the windows path moved
  the source, it is first put back at frame 0 (a range seeks by itself); if even
  that fails, the track fails. A track the plan rejects (short, or no exact length)
  is not touched at all.
- Two cases still fail where the old full pass worked: an MP4 that decodes fewer
  frames than its sample table says (a damaged last packet) makes `ends_at` read to
  the end, after which symphonia's reader cannot read again; and a source that cannot
  return to frame 0. Both need a damaged file.
- Bit for bit holds for undamaged files. A packet that fails to decode is skipped by
  the decoder: the full pass then shifts everything after it, the windows only inside
  one window — both vectors are valid, they just differ.
- CAF (ALAC/PCM) and Ogg FLAC pass the decoder's checks too but have no fixture; a
  CAF whose frame count leaves out the remainder frames is caught by `ends_at`.
- Memory: the merged windows, ~27% of a four-minute track at its source rate.

### A whole FLAC: windows at byte positions

For a FLAC without a CUE range the source can `seek_near` (audio_decoder), and
`windows::near` places the 32 windows by bytes instead of by time: window `k` aims at
`(k + 0.5) / 32` of the audio bytes, lands on the next FLAC frame, takes the first
patch (mel frame) whose source range starts at or after that frame, and decodes it
from there. The same rule runs for a local file and for a server stream, so a
recording gets the same vector wherever it is read from.

- **Why.** Exact times need Symphonia's binary search over bytes for every window;
  over the network that was ~290 requests a track and Navidrome spent ~1 s of CPU on
  them (`pawse/src/similar_tracks/doc.md`). By bytes it is one request a window
  (`seek_near` with a `span` hint). The positions only have to be spread, not exact:
  on `music-test` (2184 tracks) the quality metric (same artist from other albums
  in the top 10, centred) was 0.479 with exact times, 0.479 with every window moved
  by up to ±3 s and 0.478 with this placement; vectors moved by a median centred
  cosine of 0.993 (owner's choice: 32 windows by bytes, over 16 or 10 windows, which
  cost 0.007–0.017 of the metric). Locally it is also faster: `prepare` of the 1640
  FLACs of `music-test` on 7 threads took 18.0 s against 23.8 s.
- **Fit.** A window that would start past the analysed part (the last ones, where
  bytes and time drift apart) is aimed again, earlier by the ratio between where it
  landed and the latest start that fits, up to `NEAR_ATTEMPTS` times. For a track
  past the 30-minute cap (an unsplit cue image served as one file) the byte share of
  the analysed part is calibrated first: `seek_near` at the time share, corrected by
  where it landed, until within 1% (`CALIBRATED`). A window never starts before the
  previous one ends its first frame (`first` strictly grows). A `seek_near` that
  fails (no frame after the offset — a large tag after the audio, say) is aimed 2% of
  the share earlier (`REAIM`) instead of sending the track to the full pass, which
  over the network would read the whole file; a network failure fails every later
  read at once (`RangeReader` keeps it), so those attempts cost nothing.
- **Everything else is unchanged:** the `plan` (coverage rule, 32 patches, the 16 kHz
  grid), WAV/ALAC and CUE ranges (time positions through `seek_frame`), lossy and
  short tracks (full pass). A source without `seek_near` keeps time positions — that
  includes `audio_embedding_gpu`'s probe input, so the probe rows in the weights file
  still match. A window that does not fit or cannot be read falls back to the full
  pass, as below; on `music-test` none does (1518 FLACs windowed, 122 short ones full,
  0 fallbacks).
- No `ends_at` check here: each window is read from its own landing, so a file whose
  STREAMINFO overstates its length fails a window and takes the full pass. One that
  understates it is analysed over the length it claims (the full pass, which
  `ends_at` used to send it to, would decode past it); the vector is still that
  file's sound.

## Decoding and the CUE range

- Channels are averaged; S16 / 32768, S24 / 8388608, S32 / 2147483648, F32 as is,
  except NaN / ±Inf samples, which become 0 (one bad sample must not poison the
  vector). A vector that still comes out non-finite (absurdly large float samples
  overflow the power spectrum) is `EmbedError::NonFinite`: it would turn the library
  mean, and with it every query, into NaN.
- `AudioSource::seek` takes a **fraction of the duration**, not seconds. A range seeks
  to `start / duration`, then skips `(start − landed) × rate` samples, because a seek
  lands on a packet boundary. A source without a duration decodes from the start and
  skips. Reading stops at `length`.
- On the full pass (lossy codecs, anything without an exact length, tracks under
  ~110 s — "variant B" in the GPU plan) the whole range is decoded (what the PoC
  measured), **up to 30 minutes** (`MAX_ANALYSED`), and resampled to 16 kHz on the
  CPU while decoding (the streaming `Resampler`). The 16 kHz signal must stay until
  the end because the patch choice depends on the final length: 64 KB per second
  (~15 MB for 4 minutes, at most ~115 MB at the cap), then only the patches' windows
  are kept (`layout`; the whole signal when they cover it). Without the cap a 10-hour
  audiobook would take 2.3 GB; with it a mix or a long recording is described by its
  first half hour. The windows path keeps the same cap so its numbers stay the full
  pass's; lifting it moves the vectors of long recordings, a version change. A later
  "variant A" (two passes, source-rate windows for every format) is not done.
- Under one second of audio is `EmbedError::TooShort`.

## Matching the training data

The model learned on audio prepared by Essentia (`MonoLoader(sampleRate=16000,
resampleQuality=4)` + `TensorflowInputMusiCNN`). Every step below copies that,
including its quirks; "correct" DSP would drift from what the weights expect.

- **Resampling** is libsamplerate's *linear* converter, which lags exactly one input
  sample: `out[i] = lerp(x[⌊p⌋], x[⌊p⌋+1], frac p)`, `p = i·from/16000 − 1`, zeros
  outside the signal, `⌊len·16000/from⌋` outputs. Without the −1 the output differs
  from Essentia by ~15–23% of the amplitude; with it the vectors agree to cos
  ≥ 0.99998. Do not replace it with a sinc resampler (rubato etc.). The rule that
  playback leaves resampling to the OS does not apply: this is offline analysis.
  16 kHz input passes through untouched, as Essentia does.
- **Mel**: frames of 512, hop 256, zero-centred (`FrameCutter(startFromZero=false)`:
  frame k covers samples `[256k − 256, 256k + 256)`, zeros outside), so a signal of
  `len` samples has `1 + ⌈(len − 256)/256⌉` frames (1 when `len ≤ 256`). Essentia emits
  one frame more; the tests compare the shared length. Symmetric Hann
  `0.5 − 0.5·cos(2πi/511)`, unnormalised (Essentia's zero-phase shift does not change
  the magnitude). Power spectrum `|X|²` (MelBands `type="power"`). 96 Slaney-mel
  bands from 0 to 8 kHz, triangles linear in Hz, `unit_tri` normalisation (divided by
  half their width). `log10(1 + 10000·e)`.
- **Patches**: 128 frames (~2.05 s) × 96 bands, starting every 62 frames; a trailing
  partial patch is dropped; a track shorter than one patch gets one zero-padded patch.
- **`spread32`**: of `total` patches take all when `total ≤ 32`, otherwise
  `(i·total + total/2) / 32` for `i = 0..32`. Measured in the PoC on 2184 real tracks:
  same-artist-in-top-10 is 48.0% against 48.2% for every patch, the median cosine to
  the whole-track vector 0.997 (worst 0.957). Contiguous blocks were worse — 64 from
  the middle had worst tracks at 0.35, 4×16 at 0.66 — so never take one block.
- **Batch** is fixed at 64 (`bs64` model): a run is filled with the patches of as
  many items as fit, the rest zero-padded, and only the real rows are averaged per
  item (in f64). One track alone wastes half a run, so callers should pass several.
  The model's output for a row depends neither on the padding nor on the other rows
  (no batch statistics); `audio_embedding_gpu`'s
  `a_track_embeds_the_same_alone_and_in_a_batch` guards that.
- The **stored vector is the plain mean**, neither normalised nor centred. Centering
  depends on the library (its mean), so it happens at query time.

## Genres by sound

`Embedding::styles` holds the mean over the track's patches of the classifier's sigmoid
outputs, one score in 0..1 per Discogs style. Scores are low by nature (400 classes,
several plausible at once): on 196 tracks of `music-test` the best style of a track
scored 0.1–0.7, typically 0.2–0.4. `genres(scores)` turns them into what the app stores
as genre rows:

- the style name without its parent (`Rock---Grunge` → `Grunge`); a name under two
  parents (16 of them: `Disco`, `Dub`, `Industrial`, `Hardcore`, …) is one genre with
  the higher score;
- scores under `GENRE_FLOOR` (0.05) are left out — the app's display thresholds (0.12
  for an album's mean, 0.2 for a track on a genre page) are above it, so they can be
  tuned without a re-analysis;
- the 13 `Non-Music` labels (`Spoken Word`, `Audiobook`, `Field Recording`,
  `Interview`, …) are not genres: skits and intros score on them;
- best first, ties by name.

The full 400 scores are stored too (`music_library`'s `styles` column), so another
reading of them later needs no GPU. Any change to `genres` changes what is stored and so
the `genres` part of `EMBEDDING_VERSION`.

## `EMBEDDING_VERSION`

`effnet-multi-styles-gpu-1:5cc5a7ec:gpu1:spread32:flacbytes:genres1` = weights release
+ first 8 hex of its SHA-256 + front-end revision + patch selection (`flacbytes`: a
whole FLAC's windows sit at byte positions, below) + the genre rows' revision
(`genres1`, above). Any change that moves the numbers (model, resampler, mel, patch
choice, averaging) or the genre rows must change the string; the app then drops the
old rows and re-analyses everything. Refactors that keep the oracle tests bit for bit
do not. `effnet-multi-gpu-1:7de20391:gpu1:spread32:flacbytes` was the multi model alone
(no style scores; the vectors are the same, the file and its SHA are not).
`effnet-multi-gpu-1:7de20391:gpu1:spread32` was the same with time positions for
every format; FLAC vectors moved by a centred cosine of ~0.993 (median), the quality
metric did not (0.479 both). The string before it, `effnet-multi-1:65cfde30:fe1:spread32`,
was the tract path over the ONNX file; the GPU vectors agree with it to cosine 1.000000000 on
52 `music-test` tracks of every format, so the change only follows the new file (the
plan's choice) — the numbers did not move.

## Similarity

Vectors are never held in memory as a whole and never quantised (owner's decision:
full f32). A query streams them in `SCAN_CHUNK` (1024) rows — a ~5 MB buffer for any
library size:

- `MeanAccumulator` — the library mean as a streamed sum (f64).
- `Query` — what to look for: any vector (a track's, or the mean of a queue), `n`,
  `Direction::Nearest` or `Farthest`, and a set of ids to skip (`excluding`). It knows
  only ids and vectors; who to skip and why (the seed, an artist's tracks) is the
  caller's business.
- `TopN::new(query, mean)` / `feed(ids, vectors)` / `finish()` — cosine of
  `x − mean` and `query − mean`, row norms on the fly, the best `n` in a min-heap
  (for `Farthest` the cosine is negated inside the heap and restored in the result),
  excluded ids skipped, ties broken by the lower id. The result, best first, is
  exactly what sorting every cosine would give. A query vector that is empty or not
  as long as the mean finds nothing, rather than reading the rows at a wrong stride.
  Several `TopN`s can be fed from the same chunks, so one pass answers several
  queries.
- Centering is mandatory: raw vectors put almost every pair at 0.7–0.99; centred
  medians in the PoC were 0.58 same album, 0.50 same artist, −0.05 unrelated.
- A vector equal to the mean scores 0, not NaN.
- Cost on 10k tracks (55 MB database, warm file cache, M3 Max): ~20 ms per full pass,
  ~14 ms of it reading. A reader/compute pipeline would save at most ~5 ms; keeping
  the matrix in memory is an option only for features that query hundreds of times
  in a row.

## The model file

- Weights are **not** in the binary: they are CC BY-NC-SA 4.0 (Music Technology
  Group, Universitat Pompeu Fabra; Alonso-Jiménez, Serra, Bogdanov, "Music
  Representation Learning Based on Editorial Metadata from Discogs", ISMIR 2022) and
  pawse is MIT. They live in the owner's repository `popovpsk/pawse-models`: release
  `effnet-multi-styles-gpu-1` (immutable, 34 MB) holds `effnet-multi-styles-gpu-1.bin`,
  MTG's `discogs_multi_embeddings-effnet-bs64-1.onnx` (SHA `65cfde30…`) and
  `discogs-effnet-bsdynamic-1.onnx` (the style classifier, SHA `a280825b…`), both from
  essentia.upf.edu, converted into one file for the GPU shaders by
  `audio_embedding_gpu`'s converter — a derivative under the same licence and
  attribution. `MODEL` pins the URL, size and SHA. The earlier release
  `effnet-multi-gpu-1` (the multi model alone, 16 MB) stays published but unused; the
  ONNX release `effnet-multi-1` is deleted (no app release ever used it). Old files in
  the models folder are left alone.
- `ensure(dir, cancelled)`: a file of the right size is trusted without hashing (no SHA
  on every start). Otherwise a blocking `ureq` GET (GitHub redirects to its object
  store; ureq follows) streams into `<name>.partial` while hashing; a matching SHA is
  renamed into place atomically, a mismatch deletes the partial and returns
  `Checksum`. `cancelled` is polled between reads (64 KB): when it says yes the partial
  is deleted and the result is `EmbedError::Cancelled` — the app's switch can be turned
  off mid-download. One download at a time per process (a static mutex): a second
  caller waits, then finds the finished file, so two threads never write the same
  `.partial`. The body
  limit is `size + 1`: ureq's limit fails a read after exactly `limit` bytes, so
  `size` itself would reject the genuine file.
- Network errors are `EmbedError::Network`; like every network client here it blocks
  the caller's thread.
- `discard_if_corrupt(dir)` is for a file of the right size that does not load: it
  hashes the file and deletes it only on a SHA mismatch, so `ensure` downloads it
  again next time. A genuine file is kept even if it fails to load, or the app would
  download 34 MB on every attempt.

## Inference

On the GPU, in `audio_embedding_gpu` (see its doc). The tract CPU and Metal backends
that ran the ONNX graph here are gone: no CPU inference (owner's decision), and tract
was ~20 MB of the release binary. tract stays a dev-dependency of the GPU crate, as the
reference its tests and the weights converter run.

## Tests

- `windows.rs` tests: the windows give exactly the full pass's patches on a fake
  source (16 / 22.05 / 44.1 / 48 / 96 kHz, mono and stereo, seeks that land up to
  4096 frames early, CUE ranges with and without a length), a header that claims
  more frames than the file has falls back, and on real files through
  `audio_decoder` (`fixtures/long_44_stereo.flac`, `.m4a` (ALAC),
  `long_22_mono.wav`, and a CUE range of the FLAC — `make generate` builds them).

- Oracles against Essentia. `scripts/gen_embedding_golden.py` (developer-only, never in
  CI or at runtime) writes `tests/data/`:
  - `mel_16k.f32` — Essentia mel of a 3 s, 16 kHz signal. Ours must agree to 1e-4
    (measured 4.3e-6).
  - `resample_44k.f32` — `es.Resample(44100 → 16000, quality=4)` of a 1 s signal. The
    mean absolute difference must stay under 1% of the mean amplitude (measured
    0.015%; dropping the −1 lag gives 14.6%). Not bit-exact: libsamplerate
    accumulates its phase differently.
  - `embedding.f32` — Essentia mel of a 75 s signal (enough for `spread32` to pick),
    `spread32`, ONNX Runtime, mean. Ours runs the whole path from samples and must
    reach cos ≥ 0.9999 (measured 0.99999999999998).
  - `styles.f32` — the same patches through the style model (`activations`, ONNX
    Runtime), mean over the patches. Ours must reach cos ≥ 0.9999 and a max difference
    of 1e-4 (measured 1.000000000000 and 6e-7).
- Both sides synthesise the **same signal** (`signal` in `oracle_tests.rs` and in the
  script: chords of sines, an envelope, LCG noise on wrapping `u32`, f64 maths with
  libm `sin`, rounded to f32), so no audio file is committed. Keep the two in step,
  operation order included.
- To rebuild the goldens: `python3 -m venv venv && venv/bin/pip install essentia numpy
  onnxruntime` (installs on macOS arm64 with Python 3.9; `TensorflowInputMusiCNN`
  needs no TensorFlow), then `venv/bin/python scripts/gen_embedding_golden.py <multi>
  <styles>` (the two ONNX files). Regenerating gave the same bytes for the three older
  files.
- The model tests (the `embedding.f32` and `styles.f32` oracles, alone vs batched) live in
  `audio_embedding_gpu` now, with the GPU path; `mel_16k.f32` is checked there against
  the GPU mel too.
- `a_full_pass_keeps_the_mel_of_the_whole_signal`: the 16 kHz windows of the full pass
  give exactly the mel the old streaming path computed from the whole signal (short,
  one-patch, CUE and long cases).
- Everything else is plain unit tests: streaming resampler and mel equal the whole
  signal for any chunking, frame counts, patch starts and the 0/1/32/33-patch
  selections, the CUE range and the 30-minute cap on a fake source, non-finite
  samples, similarity against a full sort, `model_file` against a loopback HTTP
  server (no network): success, a wrong SHA, a truncated file, cancellation, an
  unreachable server; and `discard_if_corrupt`.
- The crate is in neither `SAN_CRATES` nor `MIRI_CRATES` (ureq's TLS has FFI).

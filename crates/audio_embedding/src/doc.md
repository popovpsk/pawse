# audio_embedding

Turns a track's audio into a vector that describes how it **sounds** (not its tags):
Discogs-EffNet "multi" (MTG / Essentia), 1280 numbers per track. Tracks whose vectors
point the same way after centering sound alike; `pawse::similar_tracks` stores the
vectors and builds "radio from a track" out of them. GPUI-free, knows nothing about
`music_library`: the input is an `audio_common::AudioSource` (the caller opens
`audio_decoder::Decoder`), the output a plain `Box<[f32]>`. The crate owns no threads.

## Files

- `lib.rs` — the public API: `Job` (a source plus an optional CUE `TrackRange`),
  `prepare`, `Prepared`, `Embedding`, `EmbedError`, `EMBEDDING_VERSION`, `DIM`.
- `decode.rs` — `read_mono`: pulls batches from the source, mixes them down to mono
  f32 and applies the CUE range. The fake `AudioSource` the tests use lives here.
- `frontend.rs` — everything between mono samples and the model input: `Resampler`
  (to 16 kHz), `MelFrontend` / `MelStream` (the mel spectrogram), `patch_starts`,
  `spread` and `select_patches` (the 32 patches the model sees).
- `windows.rs` — the fast path of `prepare`: `plan` (which source samples the 32
  patches need) and decoding only those windows from a source that can seek to an
  exact frame.
- `analyzer.rs` — `Analyzer`: loading (backend choice and the agreement check),
  `embed`, the CPU fallback.
- `backend.rs` — `Backend`: which devices the model can run on, in order of
  preference per platform, and how each builds its tract plan.
- `similarity.rs` — `MeanAccumulator`, `TopN`, `centered_cosine`, `SCAN_CHUNK`: the
  maths of a nearest-neighbour query over vectors streamed in chunks.
- `model_file.rs` — `MODEL` (url, file name, size, SHA-256), `ensure(dir, cancelled)`
  and `discard_if_corrupt(dir)`.
- `oracle_tests.rs` — the reference checks against Essentia (see Tests).

## Two steps, split at I/O

- `prepare(job)` does everything that may wait: reading and seeking the source,
  decoding, resampling, the mel spectrogram, choosing patches. It is a free function
  and runs on the caller's thread, one track per call, so a slow source (network,
  torrent) only delays its own track.
- `Analyzer::embed(&[Prepared])` only runs the model over finished `Prepared`s and
  never touches I/O. Callers batch whatever is ready, so one slow track does not hold
  the others. `embed` packs the patches of all the items, in order, into runs of
  `patches_per_run()` (64) rows — two full tracks per run, a track may continue in the
  next run — and gives every output row back to its item (`pack`, unit-tested). GPU
  inference lives inside `embed` (see Backends); a GPU front end (resampling and mel
  on the device, `Prepared` holding PCM windows) belongs there later, without
  changing callers.
- `Prepared` for 32 patches is ~1.5 MB. `Analyzer` is loaded once (~0.1 s on the CPU,
  ~1 s with a GPU and its check, see Backends) and shared
  through an `Arc`; tract's `run` builds its own state per call, so one plan serves
  several threads.

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
- **The result is bit for bit the full pass's**, so this changed no
  `EMBEDDING_VERSION`. That holds by construction: the resampler and the mel
  spectrogram call the same `frontend::resampled_at` / `MelFrontend::frame` with
  global sample indices, zero outside the signal exactly where the streaming path
  has zeros; and a seek lands on a frame boundary at or before the window
  (`seek_frame` returns where), the samples up to the window are skipped. Measured
  on 40 FLAC/ALAC/WAV files of `music-test` (3.2 h): 40 of 40 vectors identical,
  `prepare` 3.7× faster (0.25 → 0.07 s a track).
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
- Memory: one merged window's mono samples at a time plus the patches, instead of
  the mel frames of the whole range.

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
- On the full pass the whole range is decoded (what the PoC measured), **up to 30 minutes**
  (`MAX_ANALYSED`). Nothing keeps the PCM: the resampler and the mel spectrogram are
  streaming, so a track costs its mel frames only, 24 KB per second of audio (~6 MB
  for 4 minutes). The mel frames must stay until the end because the patch choice
  depends on the final frame count, so without the cap a 10-hour audiobook would
  take ~860 MB; with it a track never takes more than ~43 MB, and a mix or a long
  recording is described by its first half hour. The windows path keeps the same
  cap so its numbers stay the full pass's; lifting it (it costs the windows path
  nothing) moves the vectors of long recordings, a version change.
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
  The 64 is baked into the ONNX graph — declaring a smaller input (32 or 8) fails at
  load — so a run costs the full batch: ~0.55 s on one M3 Max core and ~550 MB of
  peak memory (tract's intermediate tensors) per run in flight. The model's output for a row depends neither on the
  padding nor on the other rows; `a_track_embeds_the_same_alone_and_in_a_batch` guards
  that with five tracks whose third one crosses into a second run.
- The **stored vector is the plain mean**, neither normalised nor centred. Centering
  depends on the library (its mean), so it happens at query time.

## `EMBEDDING_VERSION`

`effnet-multi-1:65cfde30:fe1:spread32` = model release + first 8 hex of its SHA-256 +
front-end revision + patch selection. Any change that moves the numbers (model,
resampler, mel, patch choice, averaging) must change the string; the app then drops
the old rows and re-analyses everything. Refactors that keep the oracle tests bit
for bit do not.

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
  pawse is MIT. They live in the owner's repository `popovpsk/pawse-models`, release
  `effnet-multi-1` (immutable), as an unmodified copy of Essentia's
  `discogs_multi_embeddings-effnet-bs64-1.onnx`. `MODEL` pins the URL, size and SHA.
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
  download 16 MB on every attempt.

## Inference

### Backends

The model runs on the first backend in this order that builds and agrees with the
CPU: **Metal** (macOS) → CPU. More GPUs are meant to slot into `Backend` the same
way — a CUDA backend from tract where it supports one, wgpu on Linux — each a variant
with its own `plan` and a place in `Backend::gpus()` for its platforms; the CPU is
always last and always built.

- `Analyzer::load` builds the CPU plan first (it is also the fallback), then tries
  each GPU in `gpus()`: build its plan (`catch_unwind`: `tract-metal` panics when
  there is no Metal device), run one fixed pseudo-random batch (`probe`) on it and on
  the CPU, and accept it only if every row's cosine to the CPU's is at least
  `AGREEMENT` (0.9999; a NaN in any row rejects, `worst_cosine`); otherwise it logs
  why and tries the next one.
  The check costs one CPU run (~0.55 s, ~550 MB transient) per load, which is once
  per session; loading takes ~0.8–1.8 s instead of ~0.1 s.
- If a GPU run fails later — an error, a panic, a non-finite number in its output, or
  a spot check that disagrees — the analyzer logs it once, marks itself degraded and
  runs that batch and every later one on the CPU plan; `backend()` then says `Cpu`.
  The spot check reruns every `SPOT_CHECK_EVERY`-th (64) GPU batch on the CPU (~1–2%
  more work, its CPU result is the one returned): `tract-metal` commits a command
  buffer and waits without looking at its status, so a GPU fault or timeout can come
  back as `Ok` with stale but finite bytes, and those vectors would be stored for
  good. The check bounds that to the batches since the last one; the rest of a silent
  fault cannot be seen.
- **Metal** is `tract-metal` (a macOS-only dependency, `cfg(target_os = "macos")`):
  the typed, decluttered model goes through `MetalTransform`, is optimised and planned
  with `skip_order_opt_ram`, as tract's own Metal runtime does. Its kernels are
  compiled at run time from sources inside the crate (plus one prebuilt macOS
  metallib), so building needs no Metal toolchain. It takes
  `Device::system_default()`: on an Intel Mac with two GPUs that is the discrete one,
  held for the session (GPUI prefers the low-power one); releases are arm64-only. It pulls `tract-transformers` back in on
  macOS. One plan serves every thread; each thread gets its own Metal stream
  (thread-local in `tract-metal`). Weights are uploaded once.
- tract keeps one process-wide GPU device context (`tract-gpu`), and `tract-metal`
  sets it on first use: only one tract-based GPU backend can initialise per process.
  A later CUDA backend from tract would have to be tried instead of Metal, not after
  it; a wgpu backend outside tract is not affected.
- Numbers: on the 48 tracks checked against their stored CPU vectors the cosine was
  1.000000000 (to 9 places) but not bit for bit — a GPU sums in another order — so
  this kept `EMBEDDING_VERSION`: CPU and Metal vectors mix in one library without a
  difference a similarity could see. Measured on an M3 Max (batch 64, two tracks a
  run): 1 thread 19 tracks/s, 2 → 31, 3 → 38, 4 → 43, 6 → 37; the CPU did ~9 with
  three threads. The process's peak footprint stayed ~800 MB whatever the thread
  count (most of it the load-time CPU probe).

### tract

`tract-onnx` 0.23, pure Rust. Not `ort`: it downloads C++ binaries at
build time (breaks the offline Flatpak), 2.0 is still an RC, and it needs x86-64-v3.
`tract-onnx` is built without its default `transformers` feature (an EffNet needs
no transformer ops; it saves ~1 MB of binary and one crate). The rest of tract
still adds ~23 MB to the release binary. `tract-linalg` features stay default:
`multithread-mm` would only add threads, and
the app parallelises by track if ever (8 threads inside one run gave +15% in the
PoC). `tract-linalg` builds assembly kernels through `cc` (MASM on Windows x64,
clang on Windows ARM64); only CI proves those targets.

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
- Both sides synthesise the **same signal** (`signal` in `oracle_tests.rs` and in the
  script: chords of sines, an envelope, LCG noise on wrapping `u32`, f64 maths with
  libm `sin`, rounded to f32), so no audio file is committed. Keep the two in step,
  operation order included.
- To rebuild the goldens: `python3 -m venv venv && venv/bin/pip install essentia numpy
  onnxruntime` (installs on macOS arm64 with Python 3.9; `TensorflowInputMusiCNN`
  needs no TensorFlow), then `venv/bin/python scripts/gen_embedding_golden.py <model>`.
- Tests that run the model read its path from `PAWSE_EFFNET_MODEL` and are
  `#[ignore = "needs PAWSE_EFFNET_MODEL"]`; run them explicitly, in `--release`
  (tract in debug is very slow):
  `PAWSE_EFFNET_MODEL=… cargo test --release -p audio_embedding -- --ignored`.
- Everything else is plain unit tests: streaming resampler and mel equal the whole
  signal for any chunking, frame counts, patch starts and the 0/1/32/33-patch
  selections, the CUE range and the 30-minute cap on a fake source, non-finite
  samples, similarity against a full sort, `model_file` against a loopback HTTP
  server (no network): success, a wrong SHA, a truncated file, cancellation, an
  unreachable server; and `discard_if_corrupt`.
- The crate is in neither `SAN_CRATES` nor `MIRI_CRATES` (tract is heavy; ureq's TLS
  has FFI).

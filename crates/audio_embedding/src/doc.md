# audio_embedding

The CPU half of the sound analysis. It turns a track (an `audio_common::AudioSource`, optionally a
CUE `TrackRange`) into `Prepared`, the PCM windows of the 32 patches the model looks at, and holds
what surrounds the model and needs no GPU: genre rows from style scores, the similarity maths, the
model-file download and `EMBEDDING_VERSION`. GPUI-free, no threads, no wgpu, knows nothing about
`music_library`.

Not here: resampling, mel and the networks (`audio_embedding_gpu/src/doc.md`; there is no CPU
inference), format-level seeking (`audio_decoder/src/doc.md`), range reads over the network
(`media_stream/src/doc.md`), when tracks are analysed and how vectors are stored
(`pawse/src/similar_tracks/doc.md`, `music_library/src/doc.md`, "Sound vectors").

## Files

- `lib.rs` — the API: `Job`, `TrackRange`, `prepare`, `Prepared` / `PatchInput`, `Embedding`
  (vector, 400 style scores, version), `EmbedError`, `EMBEDDING_VERSION`, `DIM`, and the front-end
  constants and tables the GPU crate builds from (`hann_window`, `mel_filters`, `PATCH_SAMPLES` …).
- `decode.rs` — mono mix-down, CUE range seek, the 30-minute cap.
- `frontend.rs` — `Resampler` (the full pass's), Hann window, mel filters, `patch_starts`,
  `spread`; under `test-support` the CPU mel (`MelFrontend`, realfft).
- `windows.rs` — `layout` / `plan` (which source samples the patches need, and whether decoding
  only those pays), the time-positioned reader, `near` (by bytes, FLAC).
- `similarity.rs`, `styles.rs` (`LABELS` = MTG's `discogs-effnet-bsdynamic-1.json` classes in
  output order, `genres`), `model_file.rs`.
- `test_support.rs` (`test-support`, used by the GPU crate's tests and converter) — `signal`,
  `FakeSource`; `Prepared::reference_mel` is behind the same feature.
- `oracle_tests.rs`, `tests/data/*.f32` — Essentia goldens.

## Two steps, split at I/O

- `prepare(job)` does everything that may wait (reading, seeking, decoding, choosing patches,
  cutting samples) on the caller's thread, one track per call, so a slow source delays only its own
  track. `audio_embedding_gpu::Device::submit(&[Prepared])` does the rest and never touches I/O.
- `Prepared` carries each patch's samples in the source rate on the windows path and at 16 kHz on
  the full pass (`rate()`), `resampled_len()` (the analysed range at 16 kHz, which says where the
  zeros start) and per patch its first mel frame and how many of its 128 frames exist (fewer only
  for a track shorter than one patch). `reference_mel` is the CPU reference of what the GPU
  computes from it; tests and the converter's probe rows use it, the app never does.

## Matching the training data

The model learned on audio prepared by Essentia (`MonoLoader(sampleRate=16000, resampleQuality=4)`
+ `TensorflowInputMusiCNN`). Copy it, quirks included; "correct" DSP drifts from what the weights
expect.

- **Resampling** is libsamplerate's linear converter, which lags one input sample:
  `out[i] = lerp(x[⌊p⌋], x[⌊p⌋+1], frac p)`, `p = i·from/16000 − 1`, zeros outside the signal,
  `⌊len·16000/from⌋` outputs (without the −1 it is 15–23% of the amplitude off). Not a sinc
  resampler; the playback rule that resampling is left to the OS does not apply to offline
  analysis. 16 kHz input passes through.
- **Mel**: frames of 512, hop 256, zero-centred (frame k covers `[256k−256, 256k+256)`), so `len`
  samples give `1 + ⌈(len−256)/256⌉` frames (1 when `len ≤ 256`; Essentia emits one more, tests
  compare the shared length). Symmetric Hann `0.5 − 0.5·cos(2πi/511)`, power spectrum, 96 Slaney
  bands 0–8 kHz, triangles linear in Hz normalised by half their width, `log10(1 + 10000·e)`.
- **Patches**: 128 frames × 96 bands every 62 frames; a trailing partial patch is dropped; a track
  shorter than one patch gets one zero-padded patch. **`spread`**: of `total` patches take all when
  `total ≤ 32`, else `(i·total + total/2) / 32` for `i = 0..32`. Never one contiguous block: it gave
  far worse vectors for tracks whose middle differs from the rest.
- The **stored vector is the plain mean** over the patches, neither normalised nor centred;
  centering depends on the library's mean and happens at query time.

## Decoding, the CUE range, the full pass

- Channels are averaged; NaN / ±Inf samples become 0 (one bad sample must not poison the vector).
  A vector that still comes out non-finite is `EmbedError::NonFinite`: it would turn the library
  mean, and every query, into NaN. Under one second is `TooShort`.
- `AudioSource::seek` takes a fraction of the duration, not seconds. A range seeks to
  `start / duration`, then skips `(start − landed) × rate` samples (a seek lands on a packet
  boundary); a source without a duration decodes from the start and skips.
- The **full pass** (lossy codecs, anything without an exact length, tracks under ~110 s, and the
  fallback for everything below) decodes the whole range and resamples on the CPU while decoding.
  The 16 kHz signal is kept to the end because the patch choice depends on the final length
  (64 KB per second), then only the patches' windows are kept.
- **At most 30 minutes** are analysed (`MAX_ANALYSED`): a mix or long recording is described by its
  first half hour (a 10-hour audiobook would take 2.3 GB). The windows path keeps the same cap;
  lifting it moves the vectors of long recordings, a version change.

## Decoding only the windows

The patches depend only on the exact number of samples in the range, and each is local: mel frame
`k` reads resampled samples `[256k−256, 256k+256)`, resampled sample `i` reads source samples
`⌊i·rate/16000 − 1⌋` and the next. So when the length is known before decoding, `windows::plan`
lists the source ranges the patches need (merged where they overlap) and `prepare` decodes only
them: ~27% of a four-minute track.

- Applies when the source reports `exact_frames` and can `seek_frame` (`audio_decoder`: FLAC,
  WAV/AIFF, ALAC), the range has at least a second, every patch is whole, and the windows cover at
  most 60% of the range (`MAX_COVERAGE`). Lossy codecs need decoder state from before a seek and
  MP3 seeks coarsely: full pass.
- **The result is bit for bit the full pass's** (`windows_give_the_same_patches_as_a_full_decode`).
  By construction: the same `frontend::resampled_at` / `MelFrontend::frame` with global sample
  indices, zero outside the signal exactly where the streaming path has zeros, and a seek lands at
  or before the window (`seek_frame` returns where) with the rest skipped. Holds for undamaged
  files; a packet the decoder skips shifts everything after it on the full pass but only one window
  here.
- A CUE range starts where the full pass starts (`decode::seek_start`) and has its length: the
  range, the 30-minute cap, or the end of the file.
- `ends_at` first checks the file really has `exact_frames`: it seeks to the last frame and decodes
  to the claimed end, not past it (Symphonia's MP4 reader cannot seek after reading to the end:
  `isomp4: no atom pending read`).
- Any surprise (tail mismatch, a seek landing after its target, a window ending early, a failed
  seek) falls back to the full pass; if the windows path moved the source it is put back at frame
  0 (a range seeks by itself), and if that fails the track fails.

### A whole FLAC: windows at byte positions

For a FLAC without a CUE range the source can `seek_near` (`audio_decoder/src/doc.md`) and
`windows::near` places the windows by bytes: window `k` aims at `(k + 0.5) / 32` of the audio
bytes, lands on the next FLAC frame, takes the first patch whose source range starts at or after
it, and decodes from there. The same rule runs for a local file and a server stream, so a recording
gets the same vector wherever it is read from.

- **Why.** Symphonia's time seek is a binary search over bytes, hundreds of requests a track over
  the network, and servers pay per request, not per byte; by bytes it is one request a window.
  The positions only have to be spread, not exact: moving every window by seconds left the quality
  metric where it was.
- **Fit.** Bytes and time drift apart toward the end, so a window that would start past the
  analysed part is re-aimed earlier (`NEAR_ATTEMPTS`); past the 30-minute cap (an unsplit cue image)
  the byte share of the analysed part is calibrated first. A window never starts at or before the
  previous one's first frame. A failed `seek_near` is re-aimed `REAIM` earlier rather than sent to
  the full pass, which over the network would read the whole file; a network failure fails every
  later read at once (`RangeReader`), so those attempts cost nothing. A window that still does not
  fit or cannot be read: full pass.
- **Unchanged:** `plan`, WAV/ALAC and CUE ranges (time positions), lossy and short tracks. A source
  without `seek_near` keeps time positions, including the GPU crate's probe input, so the probe rows
  in the weights file still match.
- No `ends_at` here: a STREAMINFO that overstates the length fails a window and takes the full
  pass; one that understates it is analysed over the length it claims.

## Genres by sound

`Embedding::styles` is the mean over the patches of the classifier's sigmoid outputs, one score in
0..1 per Discogs style (low by nature: 400 classes, several plausible at once). `genres(scores)`
makes the `(genre id, score)` rows the app stores:

- A genre is the style name without its parent (`Rock---Grunge` → `Grunge`); a name under two
  parents (16 of them: `Disco`, `Dub`, `Industrial`, `Hardcore` …) is one genre with the higher
  score. Its id is `genre_id(label)`: the index in `LABELS` of the first label with that name; the
  database holds only ids. `LABELS` come with the weights, so ids change only with the model.
- Scores under `GENRE_FLOOR` (0.05) are left out. It sits below the app's display thresholds
  (`similar_tracks/genres.rs`), which can therefore be tuned without a re-analysis.
- The 13 `Non-Music` labels (`Spoken Word`, `Audiobook`, `Field Recording` …) are not genres:
  skits and intros score on them. Best first, ties by name.
- The full 400 scores are stored too (`music_library`'s `styles` column), so another reading of
  them needs no GPU. Any change to `genres` or `LABELS` changes the `genres` part of the version.

## `EMBEDDING_VERSION`

`effnet-multi-styles-gpu-1:5cc5a7ec:gpu1:spread32:flacbytes:genres1` = weights release + first 8 hex
of its SHA-256 (a test requires `MODEL`'s prefix in it) + front-end revision + patch selection
(`flacbytes`: a whole FLAC's windows at byte positions) + genre rows' revision.

- Anything that moves the numbers must change it: model, resampler, mel, patch choice, the
  30-minute cap, averaging, genre rows. The app then drops other versions' rows and re-analyses.
- Refactors that keep the oracle tests bit for bit do not. The windows path does not either: it
  equals the full pass.
- The owner's check for such a change: `pawse/src/similar_tracks/doc.md`, "Checking by hand".

## Similarity

Vectors are never held in memory as a whole and never quantised; a query streams them in
`SCAN_CHUNK` (1024) rows.

- `MeanAccumulator` (f64 sum) gives the library mean; `Query` is a vector, `n`, `Nearest` /
  `Farthest` and ids to skip. It knows only ids and vectors; who to skip and why is the caller's
  business.
- `TopN` scores the cosine of `x − mean` against `query − mean`, ties by the lower id; the result,
  best first, equals sorting every cosine. Several `TopN`s can be fed from the same chunks. A query
  vector that is empty or not as long as the mean finds nothing, rather than reading rows at a
  wrong stride; a vector equal to the mean scores 0, not NaN.
- Centering is mandatory: raw vectors put almost every pair at 0.7–0.99.
- A full pass is cheap (tens of ms for 10k tracks), so no in-memory matrix or reader/compute
  pipeline.

## The model file

- Weights are not in the binary: they are CC BY-NC-SA 4.0 (Music Technology Group, Universitat
  Pompeu Fabra; Alonso-Jiménez, Serra, Bogdanov, "Music Representation Learning Based on Editorial
  Metadata from Discogs", ISMIR 2022) and pawse is MIT. This attribution is written only here. The
  file is the converter's output from MTG's two ONNX models, a derivative under the same licence
  (`audio_embedding_gpu/src/doc.md`, "The weights file").
- Published in the owner's repository `popovpsk/pawse-models` as the immutable release
  `effnet-multi-styles-gpu-1`, which holds only that `.bin`; earlier releases stay published.
  `MODEL` pins url, size and SHA-256.
- `ensure(dir, cancelled)`: a file of the right size is trusted without hashing. Otherwise a
  blocking `ureq` GET streams into `<name>.partial` while hashing; a matching SHA is renamed into
  place atomically, a mismatch deletes the partial (`Checksum`). Old files in the folder are left
  alone. `cancelled` is polled between reads and deletes the partial (`Cancelled`), so the app's
  switch can be turned off mid-download.
- One download at a time per process (a static mutex): a second caller waits, then finds the
  finished file. The body limit is `size + 1`: ureq fails a read after exactly `limit` bytes, so
  `size` would reject the genuine file.
- `discard_if_corrupt(dir)` is for a file of the right size that does not load: it deletes it only
  on a SHA mismatch. A genuine file is kept even if it fails to load, or the app would download
  it on every attempt.

## Tests

- `scripts/gen_embedding_golden.py` (developer-only, never in CI or at runtime) writes
  `tests/data/` from Essentia: `mel_16k.f32` (ours within 1e-4), `resample_44k.f32` (mean absolute
  difference under 1% of the mean amplitude; not bit-exact, libsamplerate accumulates its phase
  differently), `embedding.f32` and `styles.f32` (a 75 s signal, enough for `spread` to pick;
  checked in `audio_embedding_gpu`).
- Both sides synthesise the same signal (`signal` in `test_support.rs` and in the script: chords of
  sines, LCG noise on wrapping `u32`, f64 maths with libm `sin`, rounded to f32), so no audio is
  committed. Keep the two in step, operation order included. Rebuild: `python3 -m venv venv &&
  venv/bin/pip install essentia numpy onnxruntime`, then `venv/bin/python
  scripts/gen_embedding_golden.py <multi.onnx> <styles.onnx>`.
- `windows.rs` compares the windows with the full pass on a fake source (rates 16–96 kHz, mono and
  stereo, seeks landing early, CUE ranges) and on real files through `audio_decoder`
  (`fixtures/long_44_stereo.flac` / `.m4a`, `long_22_mono.wav`; `long_44_cut.flac` must take the
  full pass; `make generate` builds them).
- The crate is in neither `SAN_CRATES` nor `MIRI_CRATES` (ureq's TLS has FFI).

## Decisions

Owner's calls; ask before changing.

- A whole FLAC gets 32 windows by bytes, not 16 or 10 (those cost quality).
- Vectors are never quantised: full f32.
- No CPU inference.

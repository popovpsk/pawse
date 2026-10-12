# audio_embedding_gpu

The sound analysis on the graphics card: resampling to 16 kHz, the mel spectrogram and
Discogs-EffNet run as wgpu compute shaders, on every platform. The CPU only decodes
and cuts PCM windows (`audio_embedding::prepare`). There is no CPU inference: without
a hardware adapter the feature is unavailable (owner's decision). GPUI-free; knows
nothing about decoding, files or the library: the input is `audio_embedding::Prepared`
(PCM windows plus their positions), the output `audio_embedding::Embedding`. The only
file it reads is its weights file. The crate owns no threads; the caller's thread
submits and waits.

## Files

- `lib.rs` — the API: `open(weights)` → `Availability` (`Ready(Device)`, `NoGpu`,
  `Failed`), `adapter_name()` (the cheap check, no weights), `Device` (`submit`,
  `embed`, `name`, `agreement`, `patches_per_run`), `Submitted::wait`, the per-track
  mean, the periodic check and giving up.
- `gpu.rs` — instance, adapter order (`order`, `candidates`), device creation with the
  error and device-lost handlers.
- `arch.rs` — the network, hardcoded: `BLOCKS` (kernel, stride, channels, squeeze),
  plane sizes, and `tensors()` — every tensor of the weights file in file order.
- `weights.rs` — the weights file: `encode` (used by the converter), `Weights::parse`,
  the byte ranges the shaders bind.
- `input.rs` — packing tracks into runs (`pack`), the per-patch resampling parameters
  (`patch_info`, `ratio`, `supported`).
- `kernels.rs` — shader modules, bind group layouts, pipelines (with override
  constants).
- `engine.rs` — buffers, the fixed list of dispatches for a run (`Step`s), upload,
  submit, readback; `Tap`s let tests read any buffer after any step.
- `probe.rs` — the probe input and the agreement maths.
- `shaders/*.wgsl` — `patch.wgsl` (the per-patch struct, prepended to the others that
  need it), `resample`, `gemm`, `mel`, `stem`, `depthwise`, `excite`, `pool`. The
  front-end sizes (`PATCH_SAMPLES`, `PATCH_FRAMES`, `HOP`) and the depthwise tile come
  from the Rust constants (`audio_embedding`, `DEPTHWISE_TILE`), prepended by
  `Kernels::new`, so the two sides cannot disagree.
- `onnx.rs` — test-only (also `#[path]`-included by `examples/convert.rs`): the conv
  tensors of the ONNX graph and reference runs on tract, with any block's output.
- `tests.rs` — the GPU tests.

## Adapters

- `wgpu` is the version GPUI pulls on Linux (29.x), with `metal`, `dx12` and `vulkan`
  (no GL), so Linux builds one copy. On macOS only this crate uses wgpu.
- `Backends::PRIMARY`. Adapters of `DeviceType::Cpu` (WARP, llvmpipe, lavapipe) are
  never used. The rest are tried in order: discrete, integrated, virtual, other; within
  a kind Metal, DX12, Vulkan. `open` takes the first one whose kernels build (inside a
  validation error scope) and whose probe agrees (below); the others are logged. None
  → `Failed` with every reason; no hardware adapter at all → `NoGpu`. A weights file
  that does not parse is `Err` (the caller treats it like a corrupt download).
- `Limits::downlevel_defaults()` except `max_storage_buffers_per_shader_stage: 8`:
  only compute is used, and the downlevel set asks for nothing a compute device lacks
  (4 colour attachments, 2048-pixel textures, 16 KiB uniform bindings — the parameter
  slots are 256 bytes — and 16352 bytes of workgroup memory; the largest kernel,
  depthwise, uses 15824). `Limits::default()` asked for 8 colour attachments, and a
  Raspberry Pi 4's V3D 4.2 (4) refused the whole device. Storage bindings keep the
  default 128 MiB: the PCM buffer is capped exactly there (`PCM_CAP`), the largest
  other buffer is ~75 MB.
- `on_uncaptured_error` and the device-lost callback record a fault; every later
  submit or wait returns it (and never panics — wgpu's default handler would).
- DX12 uses FXC (wgpu's default; no `dxcompiler.dll` to ship). Windows and Linux are
  proven only by CI and the owner's machines; the probe catches a backend that builds
  but computes wrong.

## The weights file

- `examples/convert.rs` writes it from MTG's
  `discogs_multi_embeddings-effnet-bs64-1.onnx` (SHA-256 `65cfde30…`, from
  <https://essentia.upf.edu/models/feature-extractors/discogs-effnet/>; `pawse-models`
  no longer hosts it):
  `cargo run --release -p audio_embedding_gpu --example convert -- <onnx> <out>`.
  It reads the raw ONNX initializers (tract's protobuf), not a tract-optimised graph,
  so the numbers are the original ones. The file is CC BY-NC-SA 4.0 like the model
  (see `audio_embedding/src/doc.md`, "The model file").
- Layout: `PWSEFNET`, format `u32`, tensor count `u32`, then per tensor `(offset u64,
  floats u64)`; every tensor starts at a multiple of 256 bytes (the storage-binding
  offset alignment), so the whole file is uploaded as one buffer and each layer binds
  its range. `parse` checks magic, format, count, every length against `arch::tensors`,
  alignment and bounds.
- Order: the 81 convolutions in graph order (`Conv__237` … `Conv__317`), weight then
  bias, then the probe rows. BatchNorm is already folded into the ONNX weights, and the
  separate `Pad` nodes are symmetric (1 or 2, from the graph's `pad_const` tensors),
  so padding is `kernel / 2` in the shaders.
- Arrangement (done by `encode`): every 1×1 convolution (expand, squeeze, excite,
  project, head) is stored input-major `[in][out]`, the layout the GEMM tile and the SE
  loops read coalesced; depthwise `[C][k][k]` and the stem `[32][9]` as in ONNX.
- The DFT and mel tables are not in the file: `Engine::new` builds them from
  `audio_embedding::hann_window` / `mel_filters`, the CPU reference's own functions.
- Any change to the arrangement, the probe or `prepare` (which builds the probe input)
  needs a new file — and with it a new release in `pawse-models`, `MODEL` and
  `EMBEDDING_VERSION`.

## A run

The batch is fixed at 64 patches (the model's `bs64`, owner's decision); a partial run
computes zeros for the rest. Activations are laid out `[C][B][H][W]`, so every 1×1
convolution is one GEMM over all 64 images.

1. **Upload.** `pack` fills runs with up to 64 patches and `PCM_CAP` (32 M) samples,
   in order; a track may continue in the next run. Per patch the CPU writes a 12-word
   `PatchInfo`; the windows go through `write_buffer_with` straight into staging
   memory. The PCM buffer grows (×1.5) as needed, never past `PCM_CAP`.
2. **Resample** (`resample.wgsl`), 33024 samples per patch (128 frames). The reference
   is `audio_embedding`'s `resampled_at`: `p = i·from/16000 − 1`, lerp, zeros outside.
   WGSL has no f64 and `i·step` in f32 is useless at `i` ~ 28.8 M, so positions are
   exact integers: `from/16000 = P/Q` reduced, the CPU gives per patch `base` and `r0`
   from `y0·P = A·Q + R` (i128), the shader adds `j·P` split as `(j/Q)·P` and
   `(j%Q)·P` so nothing overflows `u32` for any rate `supported` accepts (every real
   rate up to 768 kHz; a prime rate above ~268 kHz is refused per track). A zero
   fraction takes the left sample exactly (16 kHz, integer ratios).
3. **DFT** as a GEMM: `[514 × 512] · frames`, rows `w[k]·cos` / `w[k]·sin` built in f64.
   The "frames" load mode reads frame `n`, sample `k` straight from the resampled
   patches (`y[n/128][256·(n%128) + k]`), 16 contiguous samples per load.
4. **Mel** (`mel.wgsl`): `re² + im²`, the sparse triangles, `log10(1 + 10000·e)` as
   `log2 · log10(2)`. Rows past `PatchInput::frames` (a track shorter than one patch)
   are zero, as in the CPU reference.
5. **Network**: stem (3×3/2, one thread computes all 32 channels of a pixel); per
   block expand GEMM + swish → depthwise + swish, which also writes the per-plane mean
   for SE → excite (`squeeze` with swish, `excite` with sigmoid, both from
   shared memory) → project GEMM that multiplies its input by the gate on load, adds
   the bias and, where the block has one, the residual; head GEMM + swish → mean over
   4×3 → 64 × 1280 rows read back.
6. **Mean** per track on the CPU in f64 (as before); a non-finite vector is
   `NonFinite` for that track.

GEMM (`gemm.wgsl`): 64×64 tiles, 16-deep k-steps, 4×4 outputs per thread in `vec4`
registers. Override constants select the load mode (plain, gated, frames) and the
epilogue (bias, swish, residual). Sums are two-level — 16 products per k-step, then
the k-steps — which halved the mel error against the CPU. Per-thread arrays with
dynamic indices spilled to memory on Metal (the first version was 4× slower), so
there are none in hot loops. Depthwise loads its input planes zero-padded into shared
memory (≤ 3300 floats) with the taps, so the inner loop has no bounds checks;
`depthwise_lanes` picks threads per plane so each thread computes ~4 outputs and the
planes still fit.

## Buffers and runs in flight

One set of activation buffers (`P`/`Q` block in/out, `E` expanded, `D` depthwise; the
frontend borrows `D` for the resampled patches, `E` for the spectrum, `Q` for the mel)
serves every run: wgpu orders submissions on the queue (WebGPU semantics), so a run
waits for the previous one on the GPU, while the CPU prepares and encodes the next.
Only the readback buffer is per run. ~154 MB of buffers (`E` ~75 MB is block 2's
expansion alone), plus the weights (16 MB) and the PCM buffer (25 MB, growing for
hi-res) — ~220 MB of GPU memory while a `Device` lives; the app opens one per pass
and drops it at the end (`pawse::similar_tracks`).

## Checks

- At `open`: `probe_input()` — two synthetic tracks through `audio_embedding::prepare`
  (130 s at 44.1 kHz on the windows path, so the GPU resamples; 80 s at 16 kHz on the
  full pass), 64 patches, one run. The `Device` keeps them (~15 MB) for the later
  checks, so a check does not allocate ~30 MB of signal again. The signal uses only integer LCG and f32
  arithmetic, so it is the same on every platform. Its rows are compared with the
  file's probe rows (CPU mel + tract on the CPU, written by the converter): every row's
  cosine at least `AGREEMENT` (0.999), a NaN fails. The threshold is the owner's choice,
  set loose on purpose: different GPUs round differently (FMA contraction, `exp`/`log`
  precision, denormals), M3 Max measures 1 − 1e-9, and a card that misses the threshold
  loses the feature, so it is meant to catch garbage, not rounding.
- Every `CHECK_EVERY` (64) runs the probe runs again; a failure, a GPU fault, a failed
  wait or a non-finite track whose re-check fails makes the device give up: logged
  once, every later submit is `EmbedError::Model`, which stops the app's pass, and so
  is every result a `wait` returns from then on — runs already in flight when the
  check failed included. Vectors saved since the previous check cannot be told apart;
  they stay (owner's decision for now). The app retries with a new `Device` (see
  `pawse/src/similar_tracks/doc.md`).
- A non-finite track alone (absurd float samples overflow the power spectrum) is the
  track's problem when the re-check passes: `NonFinite`, the pass goes on.

## Numbers (M3 Max, Metal)

- Resampling against the f64 formula: ≤ 1e-6 at 8 / 16 / 22.05 / 44.1 / 48 / 96 / 192
  kHz and 44101 Hz, windows at the start, middle and end of a 30-minute track.
- Mel against the CPU reference (realfft): ≤ 2e-5 (measured 1.3e-5). Both against an
  f64 DFT: CPU max 7.6e-6 (mean 1.8e-7), GPU max 1.1e-5 (mean 2.7e-7) — the gap is
  the two errors added, in the lowest bands (narrow filters on the strongest bins).
  Against Essentia: < 1e-4.
- Every block against tract on the same mel: cosine 1.000000000, max difference
  ~1e-5 on values up to ~10. The embedding against Essentia + ONNX Runtime: cosine
  1.000000000000. A track alone and in a batch: ≤ 1e-6.
- A run (64 patches, two tracks): 18.4 ms of GPU time — depthwise 7.0, project 4.4,
  expand 2.9, DFT 1.9, excite 1.3, stem 0.4; ~19 ms per run with two in flight,
  ~108 tracks/s for the model alone (tract-metal did ~38 with three threads). The CPU
  decoding is the limit now. `open` takes ~0.1–0.4 s with the probe.
- Real files: 52 `music-test` tracks of every format (FLAC 44.1/48/96 kHz on the
  windows path, MP3, AAC, ALAC, Opus, APE images with CUE, DSF, WAV) against the old
  path — CPU mel + tract on the CPU — cosine 1.000000000 each
  (`real_tracks_match_the_tract_cpu_path`, `PAWSE_MUSIC_DIR`); the whole library
  (2184 tracks) analysed by the app, against the old app's vectors: the same.
- The whole app on `music-test` (2184 tracks, isolated `HOME`, M3 Max, release, no App
  Nap in either run): 58 s against 86 s with tract-metal (~38 against ~25 tracks/s),
  peak RSS ~1.32 GB against ~1.36 GB. The quality metric (same artist from other albums
  in the top 10, centred) is 0.479 for both.
- The release binary went from 97.6 MB to 75.1 MB (tract out, wgpu in).

## Tests

- Unit tests (no GPU): architecture, the file format, packing, rates, adapter order,
  the probe maths, every depthwise layer fits its tile.
- GPU tests are `#[ignore = "needs a GPU"]`: resampling, mel against the CPU and
  Essentia. With `PAWSE_EFFNET_MODEL` (the ONNX file) also every block against tract,
  the device probe + Essentia's embedding, alone vs batched. They build the weights in
  memory from the ONNX, so no converted file is needed:
  `PAWSE_EFFNET_MODEL=… cargo test --release -p audio_embedding_gpu -- --include-ignored`.
- CI has no GPU (no lavapipe, owner's decision), so it runs the unit tests only.
  `PAWSE_GPU_ALLOW_SOFTWARE` lets tests use a software adapter.
- `tract-onnx` is a dev-dependency only (the reference); the app does not link it.

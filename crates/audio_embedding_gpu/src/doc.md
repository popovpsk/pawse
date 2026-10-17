# audio_embedding_gpu

The sound analysis on the graphics card: resampling to 16 kHz, the mel spectrogram and two
Discogs-EffNet networks as wgpu compute shaders on every platform — "multi" (the 1280-number
vector) and the style classifier (400 Discogs styles, the source of genres by sound), both on the
same mel in the same run. Input is `audio_embedding::Prepared`, output `audio_embedding::Embedding`.
GPUI-free; knows nothing about decoding, files or the library, and reads one file, its weights
file. Owns no threads: the caller's thread submits and waits.

Not here: decoding, windows, patch choice, the CPU reference front end, `EMBEDDING_VERSION` and
the model download (`audio_embedding/src/doc.md`); when a `Device` is opened, retried or dropped
(`pawse/src/similar_tracks/doc.md`).

## Files

- `lib.rs` — the API: `open(weights)` → `Availability` (`Ready(Device)`, `NoGpu`, `Failed`),
  `adapter_name()` (the cheap check, no weights), `Device` (`submit`, `embed`), `Submitted::wait`,
  the per-track means, the periodic check and giving up.
- `gpu.rs` — instance, adapter order, device creation with the error and device-lost handlers.
- `arch.rs` — the network, hardcoded (`BLOCKS`, plane sizes, `Net::{Multi, Styles}`: one
  architecture, two weight sets) and `tensors()`, the single source of the weights file's order.
- `weights.rs` — the file format (`encode` is the converter's, `Weights::parse`).
- `input.rs` — packing patches into runs, per-patch resampling parameters, `supported` rates.
- `kernels.rs` — shader modules, bind group layouts, pipelines (override constants).
- `engine.rs` — buffers, the fixed list of dispatches (`Step`s), upload, submit, readback; tests
  read any buffer after any step through `Tap`s.
- `probe.rs` — the probe input and the agreement maths.
- `shaders/*.wgsl` — `patch.wgsl` (the per-patch struct, prepended to the shaders that need it),
  `resample`, `gemm`, `mel`, `stem`, `depthwise`, `excite`, `pool`. The front-end sizes and the
  depthwise tile come from Rust constants (`audio_embedding`, `DEPTHWISE_TILE`), prepended by
  `Kernels::new`, so the two sides cannot disagree.
- `onnx.rs` — test-only, also `#[path]`-included by `examples/convert.rs`: ONNX initializers and
  tract reference runs. The style model has a dynamic batch and its graph annotates intermediate
  shapes with that symbol, which tract cannot unify with the fixed 64; `run` clears `value_info` and
  the output types before loading.
- `tests.rs` — the GPU tests.

## Adapters

- `wgpu` is the version GPUI pulls (`gpui-pre-wgpu`; one copy in `Cargo.lock`), with `metal`,
  `dx12`, `vulkan`, no GL. On macOS only this crate uses it.
- Adapters of `DeviceType::Cpu` (WARP, llvmpipe, lavapipe) are never used. The rest are tried
  discrete first (within a kind Metal, DX12, Vulkan); `open` takes the first whose kernels build
  (inside a validation error scope) and whose probe agrees, and logs the others. None → `Failed`
  with every reason; no hardware adapter → `NoGpu`. A weights file that does not parse is `Err`
  (the caller treats it as a corrupt download).
- `Limits::downlevel_defaults()` except 8 storage buffers per stage (GEMM and excite bind 6): only
  compute is used, and `Limits::default()` asks for things a compute device may lack (8 colour
  attachments), which makes weaker GPUs refuse the whole device. Depthwise's workgroup memory
  (`DEPTHWISE_TILE` + taps + partial) must stay under downlevel's 16352 bytes; the PCM buffer is
  capped just under the default 128 MiB storage binding (`PCM_CAP`).
- `on_uncaptured_error` and the device-lost callback record a fault; every later submit or wait
  returns it and never panics (wgpu's default handler would).
- DX12 shaders compile with FXC unless wgpu finds a `dxcompiler.dll` (its `Auto` default); none
  is shipped. Windows and Linux are covered only by CI builds and manual runs; the probe catches a
  backend that builds but computes wrong.

## The weights file

- `examples/convert.rs` writes it from MTG's two models, both from
  <https://essentia.upf.edu/models/feature-extractors/discogs-effnet/> (`pawse-models` hosts neither
  ONNX): `discogs_multi_embeddings-effnet-bs64-1.onnx` (SHA-256 `65cfde30…`) and
  `discogs-effnet-bsdynamic-1.onnx` (`a280825b…`; the style model is published as ONNX only with a
  dynamic batch). `cargo run --release -p audio_embedding_gpu --example convert -- <multi> <styles>
  <out>`. It reads the raw ONNX initializers, not a tract-optimised graph, so the numbers are the
  original ones, and gives the same bytes for the same inputs. CC BY-NC-SA 4.0 like the models
  (`audio_embedding/src/doc.md`, "The model file").
- The two graphs have the same 81 convolutions in the same order and shapes (`encode` checks every
  length against `arch::tensors`); they differ in form only: padding sits in separate `Pad` nodes in
  the multi graph and inside each `Conv` in the style graph (symmetric `kernel / 2` in both), and
  the style graph ends in `GlobalAveragePool → Flatten → Gemm (1280 → 400) → Sigmoid`. BatchNorm is
  already folded into both.
- Layout: magic `PWSEFNET`, format (2; format 1, the multi model alone, is refused), a tensor table,
  tensors in `arch::tensors()` order: both networks' convolutions in graph order (weight, bias),
  the classifier, then the probe rows (64 × 1280 vectors, 64 × 400 sigmoid outputs). Every tensor
  starts at a multiple of 256 bytes (the storage-binding offset alignment), so the whole file is one
  buffer and each layer binds its range. `parse` checks everything against `arch::tensors`.
- `encode` stores every 1×1 convolution (expand, squeeze, excite, project, head) and the classifier
  input-major `[in][out]`, the layout the GEMM tile and the SE loops read coalesced; depthwise
  `[C][k][k]` and the stem `[32][9]` stay as in ONNX.
- The DFT and mel tables are not in the file: `Engine::new` builds them from
  `audio_embedding::hann_window` / `mel_filters`, the CPU reference's own functions, with the DFT
  phases in f64.
- Any change to the arrangement, the probe, or `prepare` (which builds the probe input) needs a new
  file, and with it a new release in `pawse-models`, `MODEL` and `EMBEDDING_VERSION`.

## A run

The batch is fixed at 64 patches; a partial run computes zeros for the rest. Activations are
`[C][B][H][W]`, so every 1×1 convolution is one GEMM over all 64 images. A row's output depends
neither on the padding nor on the other rows (no batch statistics).

- **Upload.** `pack` fills runs with up to 64 patches and `PCM_CAP` samples, in order; a track may
  continue in the next run. A track with no patches is `TooShort`; an unsupported rate or a window
  over the cap is a `Decode` error for that track alone.
- **Resample** (`resample.wgsl`) follows `audio_embedding`'s `resampled_at`. WGSL has no f64 and
  `i·step` in f32 is useless at `i` ~ 28.8 M, so positions are exact integers: `from/16000 = P/Q`
  reduced, the CPU gives per patch `base` and `r0` (i128), the shader adds `j·P` as `(j/Q)·P` and
  `(j%Q)·P` so nothing overflows `u32`. A rate whose fraction would overflow (a prime rate above
  ~268 kHz) is refused per track (`supported`).
- **DFT** as a GEMM `[514 × 512] · frames`, reading frames straight from the resampled patches;
  **mel** (`log10(1 + 10000·e)` as `log2 · log10(2)`) into `M`, which both networks read. Mel rows
  past `PatchInput::frames` are zero, as in the CPU reference.
- **Networks**, multi first, then styles. Styles runs the classifier GEMM (1280 → 400) on every one
  of the 4×3 positions and only then the mean — swapped against the ONNX graph (pool, then
  `Gemm`) because the `Gemm` is linear, so the existing GEMM and pool kernels need no new layout.
- **Means** per track on the CPU in f64: the vector, and the style scores as the mean of
  `sigmoid(logit)` over the patches (Essentia's per-patch activations, averaged over the track). A
  non-finite number in either is `NonFinite` for that track. Waits time out after 60 s.

Kernel rules:

- GEMM sums are two-level (16 products per k-step, then the k-steps), which halved the mel error
  against the CPU.
- No per-thread arrays with dynamic indices in hot loops: on Metal they spill to memory.
- Depthwise loads its input planes zero-padded into shared memory, so the inner loop has no bounds
  checks; `depthwise_lanes` picks threads per plane so the planes still fit the tile.

## Buffers and runs in flight

One set of activation buffers serves every run and both networks (the frontend borrows `D` for the
resampled patches and `E` for the spectrum, the classifier `D` for its output); the mel in `M` is
all the second network needs from before. wgpu orders dispatches in a pass and submissions on the
queue, so a run waits for the previous one on the GPU while the CPU prepares and encodes the next,
and a caller can keep runs in flight with `submit` / `wait`. Uploads, encoding and the queue submit
happen under the PCM mutex, so concurrent `submit` calls cannot interleave. Only the readback
buffers are per run. A `Device` holds ~240 MB of GPU memory.

## Checks

- At `open` each adapter runs the probe: `probe_input()` is two synthetic tracks through
  `audio_embedding::prepare` (130 s at 44.1 kHz on the windows path, so the GPU resamples; 80 s at
  16 kHz on the full pass), 64 patches, one run. Its signal uses no libm, so it is the same on
  every platform. The `Device` keeps it for the re-checks.
- Rows are compared with the file's probe rows (CPU mel + tract, written by the converter),
  vectors and style scores alike: every row's cosine at least `AGREEMENT` (0.999), a NaN fails;
  `agreement()` is the worse of the two. Style scores are positive and mostly near zero, so their
  cosine says little; every one must also be within `STYLE_TOLERANCE` (0.01), below the 0.05
  `GENRE_FLOOR` the app keeps genres from.
- Every `CHECK_EVERY` (64) runs the probe runs again. A failure, a GPU fault, a failed wait or a
  non-finite track whose re-check fails makes the device give up: logged once, every later submit
  is `EmbedError::Model`, and so is every result a `wait` returns from then on — runs already in
  flight included. The app retries with a new `Device` (`pawse/src/similar_tracks/doc.md`,
  "Failures").
- A non-finite track alone (absurd float samples overflow the power spectrum) is the track's
  problem when the re-check passes: `NonFinite`, the pass goes on.

## Tests

- Unit tests need no GPU: architecture, the file format, packing, rates, adapter order, the probe
  maths, every depthwise layer fits its tile. CI has no GPU, so it runs only these.
- GPU tests are `#[ignore = "needs a GPU"]`: resampling against the CPU formula (≤ 1e-6, many
  rates, windows across a 30-minute track), mel against the CPU reference (≤ 2e-5) and Essentia
  (< 1e-4). With `PAWSE_EFFNET_MODEL` and `PAWSE_STYLES_MODEL` (the two ONNX files) also every
  block of each network against tract, the device probe plus Essentia's embedding and style scores
  (`embedding.f32`, `styles.f32`), a track alone vs batched (≤ 1e-6), and with `PAWSE_MUSIC_DIR`
  real files of every format against CPU mel + tract. They build the weights in memory from the
  ONNX files, so no converted file is needed:
  `PAWSE_EFFNET_MODEL=… PAWSE_STYLES_MODEL=… cargo test --release -p audio_embedding_gpu --
  --include-ignored`.
- `PAWSE_GPU_ALLOW_SOFTWARE` lets the engine-level tests (resampling, mel, blocks) use a software
  adapter; the `Device` tests still need hardware.
- `tract-onnx` is a dev-dependency only (the reference); the app does not link it. The crate is in
  neither `SAN_CRATES` nor `MIRI_CRATES`.

## Decisions

Owner's calls; ask before changing.

- No CPU inference and no software adapters: without a hardware adapter the feature is unavailable.
- Batch fixed at 64 (the model's `bs64`).
- `AGREEMENT` is set loose on purpose: GPUs round differently (FMA contraction, `exp`/`log`
  precision, denormals) and a card that misses the threshold loses the feature, so the probe is
  meant to catch garbage, not rounding.
- Vectors saved since the previous periodic check cannot be told apart from bad ones when a check
  fails; they stay (for now).
- CI gets no software GPU (no lavapipe).

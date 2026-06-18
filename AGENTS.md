## Documentation

Always start by reading `.docs/project.md` to understand the project architecture, crate responsibilities, audio pipeline, UI patterns, database schema, and known limitations.

For icon usage, embedding, and SVG conventions, see `.docs/icons.md`.

Each folder-style module (and crate) carries a `doc.md` next to its sources. It
states what the module is responsible for, which file holds what (briefly), and any
non-obvious behavior. Read a module's `doc.md` before changing it, and keep it in
sync when you move or rename things (e.g. `crates/pawse/src/library_views/doc.md`).

## File Operations

The model must not create or modify any files without explicit user instruction.


## Build & Lint

```bash
cargo fmt && cargo build && cargo clippy
```

No warnings allowed. If any appear — remove them.

## Test

```bash
cargo test
```

All tests must pass.


### Testing

- Test without GUI first (console main)
- Log in callback for debugging
- Check that buffer is filled before requesting callback

## Conventions

- Crates: lowercase with underscores
- `publish = false` for internal crates
- Versions and edition from workspace

### Architecture

- **Trait-based abstraction**: Define traits for core functionality (`AudioSource`, `AudioOutput`), implement concrete structs behind them
- **Service pattern**: Global services via `gpui::Global` trait (e.g., `Services` with `audio_engine`, `output`)
- **Event-driven UI**: Use `EventEmitter<T>` + `EventBus` entity for cross-component communication
- **Lock-free audio**: SPSC ring buffer (`rb` crate) for real-time audio data transfer

### Audio Pipeline

- **Planar → Interleaved conversion**: Symphonia outputs planar samples; always convert to interleaved for output
- **Dynamic stream recreation**: Recreate output stream when metadata (sample rate, channels, bit_depth) changes
- **Buffer duration**: Use `BUFFER_DURATION_MS = 128` for audio buffering
- **Volume curve**: Apply cubic curve for volume < 10%, linear otherwise

### Error Handling

- **`anyhow` + `thiserror`**: `thiserror` for library error types, `anyhow` for applications
- **Graceful degradation**: Continue on non-fatal errors (e.g., `DecodeError` in decoder loop). Output init failures (no device / stream) don't panic — the app launches silent, logs, and surfaces an `OutputEvent::Failure`; unexpected ring-buffer write errors are logged and the chunk is dropped
- **Fail-fast on invariants**: `panic!()` / `unwrap()` only where failure means a real bug (a decoder guaranteed `Some` by local flow, a poisoned mutex, DB / window open). The `diagnostics` crate installs a panic hook so even these leave a log-file artifact
- **Diagnostics sink**: all crates log via the `log` facade into the `diagnostics` crate (rolling log file + optional user notification); no scattered `eprintln!` / `println!`

### GPUI Patterns

- **Module structure**: `main.rs` (entry) → `main_view.rs` (root view) → component files (`footer.rs`)
- **Entity state**: Encapsulate state in `Entity<T>`, use `cx.notify()` for re-render
- **Global access**: `cx.global::<Services>()` for shared state
- **Async spawning**: `cx.spawn(async move |cx| { ... })` for background tasks
- **Event forwarding**: Spawn task to forward engine events to UI event bus
- **Theme colors**: Always use `cx.theme()` properties (e.g., `cx.theme().border`, `cx.theme().primary`, `cx.theme().muted`) instead of hardcoded `Hsla` literals

### Testing

- **Fixture-based tests**: Use `CARGO_MANIFEST_DIR` + relative paths for test fixtures
- **Parameterized tests**: `#[rstest]` with `#[case::name(...)]` for multiple scenarios
- **Test coverage**: Validate params, non-empty buffers, sample ranges, seek behavior, duration accuracy

### Memory & Concurrency

- **`Arc` for shared state**: Audio buffer, volume, playback state
- **`AtomicU8` for state flags**: `STATE_IDLE`, `STATE_PLAYING`, `STATE_PAUSED`
- **`RwLock` for optional streams**: `RwLock<Option<OutputStream>>`
- **Blocking with timeout**: `write_blocking_timeout()` for ring buffer writes
- **Incidental background work** uses GPUI's background thread pool (`cx.background_spawn` / stored `BackgroundExecutor`), not `std::thread::spawn`. The carve-outs that still own their threads are the audio engine render loop, exclusive audio output, and the indexer worker pool.

### Migrations

There are users now — preserve their data. A schema change must be a versioned
step in `MIGRATIONS` (`ALTER`/`CREATE`, bumping `user_version`), never a DB wipe.
Re-deriving track data from files is fine via an `INDEXER_FORMAT_VERSION` bump; the
reindex keeps playlists/likes through the content-key snapshot.

### User Interaction

If the user asks a question, you should answer only the question without unnecessary initiative.
If the user has not explicitly asked to rewrite code, do not touch anything.
If the user asks a question and has not explicitly asked to see a rewritten version, just answer the question.

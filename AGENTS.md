## Documentation

For detailed project architecture, crate responsibilities, audio pipeline, UI patterns, database schema, and known limitations, see `.docs/project.md`.

## File Operations

The model must not create or modify any files without explicit user instruction.


## Build & Lint

```bash
cargo build && cargo clippy
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
- **Graceful degradation**: Continue on non-fatal errors (e.g., `DecodeError` in decoder loop)
- **Panic on critical failures**: `unwrap()` on stream creation, `panic!()` on unexpected ring buffer errors

### GPUI Patterns

- **Module structure**: `main.rs` (entry) → `main_view.rs` (root view) → component files (`footer.rs`)
- **Entity state**: Encapsulate state in `Entity<T>`, use `cx.notify()` for re-render
- **Global access**: `cx.global::<Services>()` for shared state
- **Async spawning**: `cx.spawn(async move |cx| { ... })` for background tasks
- **Event forwarding**: Spawn task to forward engine events to UI event bus

### Testing

- **Fixture-based tests**: Use `CARGO_MANIFEST_DIR` + relative paths for test fixtures
- **Parameterized tests**: `#[rstest]` with `#[case::name(...)]` for multiple scenarios
- **Test coverage**: Validate params, non-empty buffers, sample ranges, seek behavior, duration accuracy

### Memory & Concurrency

- **`Arc` for shared state**: Audio buffer, volume, playback state
- **`AtomicU8` for state flags**: `STATE_IDLE`, `STATE_PLAYING`, `STATE_PAUSED`
- **`RwLock` for optional streams**: `RwLock<Option<OutputStream>>`
- **Blocking with timeout**: `write_blocking_timeout()` for ring buffer writes

### Migrations

There are no users. When changing data handling, migrations are not needed.

### User Interaction

If the user asks a question, you should answer only the question without unnecessary initiative.
If the user has not explicitly asked to rewrite code, do not touch anything.
If the user asks a question and has not explicitly asked to see a rewritten version, just answer the question.

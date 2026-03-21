## File Operations

The model must not create or modify any files without explicit user instruction.


## Build & Lint

```bash
cargo build && cargo clippy
```

Не должно быть warning'ов. Если появляются — убрать.

## Test

```bash
cargo test
```

Все тесты должны проходить.


### Тестирование

- Тестируй без GUI сначала (консольный main)
- Логируй в callback для отладки
- Проверяй что буфер заполнён до запроса callback

## Conventions

- Crates: lowercase with underscores
- `publish = false` для внутренних крейтов
- Версии и edition из workspace

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

### Взимаодействие с пользователем.

Если пользователь задает вопрос ты должен отвечать только на вопрос без лишней инициативы.
Если пользователь явно не просил переписывать код ничего не трогай.
Если пользователь задает вопрос и явно не просил показать ему переписанную версию просто отвечай на вопрос.

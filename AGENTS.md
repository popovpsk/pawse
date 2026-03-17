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

## Audio Architecture

### Кратко

```
UI → AudioEngine (один на трек) → Arc<dyn AudioOutput> → Output (один на приложение)
```

### Компоненты

**`audio_output` crate:**
- `Output` — создаётся при старте приложения, управляет cpal stream
- `AudioOutput` trait — единый API для записи и управления
- `pause/resume` мгновенные, fade logic будет в `AudioEngine`

**`audio_engine` crate:**
- `AudioEngine` — декодирование, позиция, события
- Получает `Arc<dyn AudioOutput>` для вывода
- Не зависит от cpal напрямую

### Правила для cpal

1. **Output создаётся в главном потоке** — иначе crash на macOS
2. **cpal callback в том же потоке где stream** — иначе не вызовется
3. **Decode + write в одном потоке** — cpal требует данные сразу
4. **Stream реализует Send (cpal 0.17+)** — можно хранить в структурах

### Тестирование

- Тестируй без GUI сначала (консольный main)
- Логируй в callback для отладки
- Проверяй что буфер заполнён до запроса callback

## Conventions

- Crates: lowercase with underscores
- `publish = false` для внутренних крейтов
- Версии и edition из workspace

## Music Library & Indexer

### Компоненты

**`music_library` crate:**
- `MusicLibrary` — async API для управления музыкальной коллекцией
- SQLite (bundled) хранит треки, артистов, альбомы
- Все операции асинхронные через `tokio::spawn_blocking`
- Не блокирует UI

**`music_indexer` crate:**
- `MusicIndexer` — сканирует директории, извлекает метаданные через symphonia
- Поддерживает incremental updates (только изменённые файлы)
- Удаляет треки для несуществующих файлов

### Пример использования

```rust
// CLI тест:
cargo run -p music_indexer --example index /path/to/music
```

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

### Взимаодействие с пользователем.

Если пользователь задает вопрос ты должен отвечать только на вопрос без лишней инициативы.
Если пользователь явно не просил переписывать код ничего не трогай.
Если пользователь задает вопрос и явно не просил показать ему переписанную версию просто отвечай на вопрос.

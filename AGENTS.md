

## Build

```bash
cargo build
cargo run -p gpui-test
```

Не должно быть ворнингов компиляции. Если появляются ворнинги, то нужно их убрать.

## Test

```bash
cargo test
```

Все тесты должны проходить без ошибок. 
Если какие-то тесты не проходят, то нужно их исправить. 

## Lint

```bash
cargo clippy
```

Все линтеры должны проходить без ошибок. 
Если появляются ошибки, то нужно их исправить. 

## Conventions

- Crates: lowercase with underscores
- `publish = false` for internal crates
- Platform-specific code via `cfg` attributes
- Version and edition from workspace

## Audio Programming on macOS (cpal + symphonia)

### Используй cpal 0.17+

В версии 0.17+ на macOS:
- Stream реализует Send - можно хранить в структурах без unsafe
- Используй `description()` вместо deprecated `name()`

### Правила, которые НЕЛЬЗЯ нарушать

1. **Не создавай audio device в static/const контексте**
   - Audio device должен создаваться в главном потоке приложения
   - Иначе: segmentation fault

2. **cpal callback работает только в том потоке, где создан stream**
   - На macOS cpal callback работает только в том потоке, где создан stream
   - Иначе: callback никогда не вызовется, звука не будет

3. **Не используй worker thread для decode + write**
   - cpal callback требует чтобы данные были доступны в том же потоке
   - Решение: делай decode + write в одном потоке (синхронно)

4. **Избегай статической инициализации аудио компонентов**
   - Инициализируй AudioEngine только при реальном использовании (по кнопке)
   - Иначе: crash до запуска GUI

### Тестирование аудио

- Тестируй сначала без GUI (простой консольный main)
- Добавь дебаг-логирование в callback чтобы видеть вызывается ли он
- Проверяй что буфер заполняется данными до того как callback их запрашивает

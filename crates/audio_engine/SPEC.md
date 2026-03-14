# AudioEngine Specification

## Overview

AudioEngine - высокоуровневый API для аудио воспроизведения с поддержкой очереди команд, асинхронного декодирования и real-time audio output.

**Важно:** AudioEngine создаётся и используется в собственном потоке (не в UI thread). UI взаимодействует с ним через thread-safe каналы (mpsc). Это позволяет не блокировать UI при load() и других операциях.

## Архитектура

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              UI Thread                                       │
│  - GUI (gpui)                                                               │
│  - Слаёт команды: Load, Play, Pause, Stop, Seek                            │
│  - Получает события: Loaded, Playing, Paused, TrackEnded, Position         │
└────────────────────────────────┬────────────────────────────────────────────┘
                                 │ mpsc::Sender<Command>
                                 ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                         Output Thread (long-lived)                           │
│                                                                             │
│  - Создаётся при инициализации AudioEngine                                  │
│  - Живёт всё время работы приложения                                        │
│  - Владеет cpal::Stream                                                     │
│  - ringbuf::Consumer (читает семплы)                                        │
│  - Слушает команды: Pause, Resume                                           │
│                                                                             │
│  Структура:                                                                 │
│  ┌─────────────────────────────────────────┐                               │
│  │ OutputState (shared)                     │                               │
│  │  - is_playing: AtomicBool                │                               │
│  │  - command_rx: Receiver<Cmd> (owned)    │                               │
│  │  - shutdown: Arc<AtomicBool>            │                               │
│  └─────────────────────────────────────────┘                               │
│                         ▲                                                   │
│                         │ ringbuf (Consumer передаётся в Engine)            │
└─────────────────────────┼───────────────────────────────────────────────────┘
                          │
                          │ Producer
                          ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                      Engine Thread (per-track)                              │
│                                                                             │
│  - Создаётся при вызове load()                                              │
│  - Умирает при TrackEnded                                                   │
│  - Владеет Decoder (Symphonia)                                             │
│  - Получает Producer от Output при старте                                   │
│                                                                             │
│  Loop:                                                                      │
│  1. try_recv commands → обработай                                          │
│  2. decode next_buffer()                                                    │
│  3. push samples to ringbuf                                                 │
│                                                                             │
│  Структура:                                                                 │
│  ┌─────────────────────────────────────────┐                               │
│  │ EngineState                              │                               │
│  │  - command_rx: Receiver<Cmd> (owned)    │                               │
│  │  - event_tx: Sender<EngineEvent>        │                               │
│  │  - params: StreamParams                  │                               │
│  │  - duration: Duration                    │                               │
│  │  - position: u64 (в СЕМПЛАХ)             │                               │
│  │  - shutdown: Arc<AtomicBool>            │                               │
│  └─────────────────────────────────────────┘                               │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Типы данных

### Command (UI → Engine)

```rust
#[derive(Debug, Clone)]
pub enum Command {
    /// Загрузить трек (создаёт новый Engine thread)
    Load(PathBuf),
    /// Начать воспроизведение
    Play,
    /// Приостановить
    Pause,
    /// Остановить и сбросить позицию
    Stop,
    /// Seek к позиции (0.0 - 1.0)
    Seek(f32),
}
```

### EngineEvent (Engine → UI)

```rust
#[derive(Debug, Clone)]
pub enum EngineEvent {
    /// Трек загружен
    Loaded {
        params: StreamParams,
        duration: Duration,
    },
    /// Началось воспроизведение
    Playing,
    /// Приостановлено
    Paused,
    /// Остановлено
    Stopped,
    /// Позиция изменилась
    PositionChanged(Duration),
    /// Трек закончился
    TrackEnded,
    /// Ошибка
    Error(AudioError),
}
```

### PlaybackState

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PlaybackState {
    Stopped = 0,
    Playing = 1,
    Paused = 2,
}
```

### TrackInfo

```rust
#[derive(Debug, Clone)]
pub struct TrackInfo {
    pub params: StreamParams,
    pub duration: Duration,
}
```

## Компоненты

### AudioEngine (публичный API)

Точка входа для UI. Создаёт Output thread и управляет Engine threads.

```rust
pub struct AudioEngine {
    /// Команды для AudioEngine (Engine thread создаётся и управляется внутри)
    command_sender: mpsc::Sender<Command>,
    
    /// Состояние (PlaybackState repr(u8))
    state: Arc<AtomicU8>,
    
    /// Позиция - шарится через события от Engine
    /// Engine отправляет PositionChanged при каждой секунде
    position: Arc<AtomicU64>,  // в семплах
    
    /// События от Engine (UI может читать в своём loop)
    event_receiver: mpsc::Receiver<EngineEvent>,
}

impl AudioEngine {
    /// Создать новый AudioEngine (запускает Output thread)
    pub fn new() -> Self;
    
    /// Загрузить трек (async - возвращает сразу, Loaded приходит через events)
    /// При повторном load() старый Engine thread корректно завершается
    /// Если старый не завершился за 500ms - принудительно завершаем
    pub fn load(&self, path: &Path) -> Result<(), AudioError>;
    
    /// Получить TrackInfo (доступно после Loaded события)
    pub fn track_info(&self) -> Option<TrackInfo>;
    
    /// Начать воспроизведение
    pub fn play(&self) -> Result<(), AudioError>;
    
    /// Приостановить
    pub fn pause(&self) -> Result<(), AudioError>;
    
    /// Остановить
    pub fn stop(&self) -> Result<(), AudioError>;
    
    /// Seek (0.0 - 1.0) - работает в любом состоянии
    pub fn seek(&self, position: f32) -> Result<(), AudioError>;
    
    /// Текущая позиция в семплах (читается из AtomicU64)
    pub fn position_samples(&self) -> u64;
    
    /// Текущая позиция в Duration
    pub fn position(&self) -> Duration;
    
    /// Получить канал событий (для polling в UI loop)
    /// Примечание: достаточно просто Receiver, не нужен Arc<Mutex>
    pub fn events(&self) -> &mpsc::Receiver<EngineEvent>;
}
```

### OutputThread

Запускается в отдельном потоке, владеет cpal stream.

```rust
/// Output - долгоживущий поток
struct Output {
    /// Ringbuffer (создаётся при старте, НЕ пересоздаётся)
    ringbuf: HeapRb<f32>,
    
    /// cpal stream (owned)
    stream: Stream,
    
    /// Команды (owned)
    command_rx: mpsc::Receiver<OutputCommand>,
    
    /// Signal для graceful shutdown
    shutdown: Arc<AtomicBool>,
    
    /// Отдаёт Producer новому Engine при load()
    give_producer_tx: mpsc::SyncSender<Producer<HeapRb<f32>>>,
    give_producer_rx: mpsc::Receiver<Producer<HeapRb<f32>>>,
    
    /// Получает Producer обратно когда Engine завершился
    take_producer_rx: mpsc::Receiver<Producer<HeapRb<f32>>>,
}

enum OutputCommand {
    Pause,
    Resume,
    Shutdown,
}
```

### EngineThread

Запускается на каждый трек, владеет Decoder.

```rust
/// Engine - создаётся на каждый трек
struct Engine {
    /// Decoder (Symphonia)
    decoder: Box<dyn AudioSource>,
    
    /// Producer ringbuf - owned, НЕ в Arc (Producer не Sync)
    producer: Producer<HeapRb<f32>>,
    
    /// Канал команд (owned)
    command_rx: mpsc::Receiver<EngineCommand>,
    
    /// Канал событий - отправляет в AudioEngine
    event_tx: mpsc::Sender<EngineEvent>,
    
    /// Параметры трека
    params: StreamParams,
    duration: Duration,
    
    /// Текущая позиция в СЕМПЛАХ
    position_samples: u64,
    sample_rate: u32,
    channels: u8,
    
    /// Signal для shutdown (от AudioEngine при новом load)
    shutdown: Arc<AtomicBool>,
    
    /// Вернуть Producer в Output при завершении
    return_producer_tx: mpsc::SyncSender<Producer<HeapRb<f32>>>,
    
    /// Сигнал завершения (отправляет () перед смертью)
    completion_tx: mpsc::SyncSender<()>,
}

/// Engine отправляет позицию в AudioEngine через события:
/// - PositionChanged(Duration) - каждую секунду
/// - AudioEngine обновляет свой position: Arc<AtomicU64>
enum EngineCommand {
    Play,
    Pause,
    Stop,
    Seek(f32),
}
```

## Потоки данных

### Инициализация

```
1. UI: AudioEngine::new()
   → AudioEngine создаёт Output thread (долгоживущий)
   → Output: создаёт ringbuf, создаёт cpal stream
   → Output: создаёт oneshot канал для передачи Producer
   → Output: возвращает (oneshot_sender, consumer) в AudioEngine
   → AudioEngine: хранит oneshot_sender
```

### Load трека (повторный)

```
1. UI: engine.load(path)
   → AudioEngine: если есть старый Engine 
        → шлёт EngineCommand::Shutdown
        → ждёт completion через completion_rx с таймаутом 500ms
        → если таймаут — ничего не делаем, поток умрёт сам
   
   → AudioEngine: запрашивает Producer у Output через give_producer_tx
   → Output: producer.clear()  // сбрасываем старые данные
   → Output: отправляет Producer через give_producer_tx
   → AudioEngine: получает Producer, создаёт Engine thread
   → Engine: открывает Decoder, отправляет Loaded событие
   → Engine: НЕ запускает playback сразу (ждёт Play команды)
```

**Важно:** Ringbuf создаётся ОДИН раз при AudioEngine::new(), никогда не пересоздаётся.

### Воспроизведение

```
1. UI: engine.play()
   → AudioEngine → Engine: Play
   → Engine: resume cpal stream через Output state
   → Engine: loop { decoder.next_buffer() → producer.push() }
   → Output: callback читает из ringbuf → железо
```

### Пауза

```
1. UI: engine.pause()
   → AudioEngine → Output: Pause
   → Output: stream.pause() + is_playing = false
   → Engine продолжает писать в ringbuf (или останавливает decoder)
```

### Stop

```
1. UI: engine.stop()
   → AudioEngine → Engine: Stop
   → Engine: 
      a) producer.clear()  // ФЛАШИМ БУФЕР
      b) decoder.seek(Duration::ZERO)
      c) position = 0
   → AudioEngine → Output: Pause
   → Output: stream.pause()
```

### Seek

```
1. UI: engine.seek(0.5)
   → AudioEngine → Engine: Seek(0.5)
   → Engine: 
      a) ringbuf.producer.clear()  // ФЛАШИМ БУФЕР - сбрасываем старые данные
      b) decoder.seek(position)    // Перематываем decoder
      c) position = new_position
```

**Важно:** После seek нужно очистить ringbuf, иначе будут слышны старые данные (до 3 сек).

### TrackEnded

```
1. Engine: decoder.next_buffer() → None
   → Engine отправляет TrackEnded
   → Engine thread умирает
   → UI получает событие, решает что делать
```

## Детали реализации

### RingBuffer

```rust
// 5 секунд @ 192000Hz stereo — с запасом, один раз навсегда
// 192000 * 2 * 5 = 1,920,000 семплов ≈ 7.7MB
const BUFFER_CAPACITY: usize = 192_000 * 2 * 5;

let ringbuf = HeapRb::<f32>::new(BUFFER_CAPACITY);
let (producer, consumer) = ringbuf.split();

// Producer - в Engine thread (owned, не Arc)
// Consumer - в Output thread (cpal callback)
```

### Producer Lifecycle

Producer передаётся между Output и Engine через oneshot каналы:

```
1. Output создаётся → держит producer у себя
2. load() → Output: producer.clear(), отдаёт producer в Engine через oneshot
3. Engine работает → владеет producer, пишет сэмплы
4. Engine завершается → возвращает producer в Output через обратный oneshot
5. Output снова владеет producer → готов к следующему load()
```

**Два oneshot канала в Output:**

```rust
struct Output {
    ringbuf: HeapRb<f32>,
    stream: Stream,
    command_rx: mpsc::Receiver<OutputCommand>,
    shutdown: Arc<AtomicBool>,
    
    // Отдаёт Producer новому Engine при load()
    give_producer_tx: mpsc::SyncSender<Producer<HeapRb<f32>>>,
    give_producer_rx: mpsc::Receiver<Producer<HeapRb<f32>>>,
    
    // Получает Producer обратно когда Engine завершился
    take_producer_rx: mpsc::Receiver<Producer<HeapRb<f32>>>,
}

struct Engine {
    decoder: Box<dyn AudioSource>,
    producer: Producer<HeapRb<f32>>,
    command_rx: mpsc::Receiver<EngineCommand>,
    event_tx: mpsc::Sender<EngineEvent>,
    shutdown: Arc<AtomicBool>,
    
    // Вернуть Producer в Output при завершении
    return_producer_tx: mpsc::SyncSender<Producer<HeapRb<f32>>>,
    
    // Сигнал завершения (отправляет () перед смертью)
    completion_tx: mpsc::SyncSender<()>,
}

### cpal Callback (Output thread)

```rust
let callback = move |data: &mut [f32], _: &OutputCallbackInfo| {
    for sample in data.iter_mut() {
        *sample = consumer.try_pop().unwrap_or(0.0);
    }
};

// Важно: никаких lock, никаких alloc в callback!
```

### Command processing (non-blocking, обрабатывает все накопившиеся команды)

```rust
// Engine thread
fn try_process_commands(state: &EngineState) {
    // Обрабатываем ВСЕ команды, которые накопились
    while let Ok(cmd) = state.command_rx.try_recv() {
        match cmd {
            EngineCommand::Seek(pos) => {
                let target = duration.mul_f32(pos);
                decoder.seek(target).ok();
                position_samples = target.as_secs() * sample_rate as u64;
            }
            EngineCommand::Stop => {
                decoder.seek(Duration::ZERO).ok();
                position_samples = 0;
            }
            // ...
        }
    }
}

// Output thread  
fn try_process_commands(output: &OutputState) {
    // Обрабатываем ВСЕ команды
    while let Ok(cmd) = output.command_rx.try_recv() {
        match cmd {
            OutputCommand::Pause => {
                stream.pause().ok();
                is_playing.store(false, Ordering::SeqCst);
            }
            OutputCommand::Resume => {
                stream.play().ok();
                is_playing.store(true, Ordering::SeqCst);
            }
            // ...
        }
    }
}
```

## Ownership и Lifetime

| Компонент | Владелец | Lifetime | Notes |
|-----------|----------|----------|-------|
| cpal::Stream | Output thread | Приложение | Создаётся один раз |
| ringbuf | Output thread | Приложение | ~7.7MB, 192000Hz * 2ch * 5sec |
| Producer (active) | Engine thread | Track | Owned, НЕ в Arc, не Sync |
| Producer (idle) | Output thread | Между треками | Хранится пока нет активного Engine |
| Consumer | Output thread | Приложение | В cpal callback |
| Decoder | Engine thread | Track | Owned |

## AudioEngine Thread Model

```
┌─────────────────┐      mpsc       ┌─────────────────┐
│   UI Thread     │ ─── commands ──▶│ AudioEngine     │
│                 │ ←─ events ──────│ (owns Output)   │
└─────────────────┘                 └────────┬────────┘
                                             │
                                             │ spawn
                                             ▼
                                    ┌─────────────────┐
                                    │  Engine Thread  │
                                    │ (per-track)     │
                                    └─────────────────┘
```

## Ограничения

- Engine создаётся заново при каждом load()
- При load() старый Engine thread корректно завершается
- Одновременно только один трек
- Seek работает в любом состоянии (Playing, Paused, Stopped)
- Нет gapless playback (пока)
- Нет DSP (пока)

## Пример использования

```rust
// UI side
let engine = AudioEngine::new();

// Load (async - возвращает сразу, Loaded придёт через events)
engine.load(&path).expect("Failed to send load command");

// В UI loop (polling events)
for event in events.lock().unwrap().try_iter() {
    match event {
        EngineEvent::Loaded { params, duration } => {
            eprintln!("Loaded: duration={:?}", duration);
        }
        EngineEvent::TrackEnded => {
            eprintln!("Track ended!");
        }
        EngineEvent::PositionChanged(pos) => {
            update_progress_bar(pos);
        }
        _ => {}
    }
}

// Play
engine.play().expect("Failed to play");

// Pause
engine.pause().ok();

// Resume
engine.play().ok();

// Seek (работает даже на паузе!)
engine.seek(0.5).ok();

// Stop
engine.stop().ok();

// Новый трек - load() корректно завершит старый engine
engine.load(&new_path).ok();
```

## Файлы

```
crates/audio_engine/
├── SPEC.md              # Этот документ
├── Cargo.toml
└── src/
    ├── lib.rs           # Публичный API (AudioEngine)
    ├── engine.rs        # Engine thread логика
    ├── output.rs        # Output thread логика (существующий)
    ├── types.rs         # Command, EngineEvent, etc
    └── test/
        └── main.rs      # Тестовый binary
```

## Зависимости

```toml
[dependencies]
audio_common = { path = "../audio_common" }
audio_decoder = { path = "../audio_decoder" }
audio_output = { path = "../audio_output" }
ringbuf = "0.4"
anyhow.workspace = true
```
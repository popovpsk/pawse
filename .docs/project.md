# Project Overview: Home Audio Player

A local audio player built with Rust and [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) (Zed's UI framework). The project uses a workspace-based architecture with trait abstractions for the audio pipeline and GPUI's entity/event system for the UI.

## Workspace Structure

| Crate | Path | Purpose |
|-------|------|---------|
| `audio_common` | `crates/audio_common/` | Shared types: `AudioSource` trait, `AudioSamples`, `StreamParams`, `AudioError` |
| `audio_decoder` | `crates/audio_decoder/` | Symphonia-based decoder. Reads FLAC, MP3, WAV, OGG, etc. Outputs interleaved `f32` |
| `audio_output` | `crates/audio_output/` | CPAL-based output. Lock-free SPSC ring buffer (`rb` crate). Dynamic stream recreation on format change |
| `audio_engine` | `crates/audio_engine/` | Playback engine thread + state machine. Commands via `flume` channels. Emits `EngineEvent` |
| `music_library` | `crates/music_library/` | SQLite repository. Artists, albums, tracks with many-to-many relationships |
| `music_indexer` | `crates/music_indexer/` | Directory scanner (`jwalk`) + metadata reader (`lofty`). Emits `ScanEvent` |
| `ui` | `crates/ui/` | GPUI application. Views, service globals, event buses |

## Audio Pipeline

```
File → Decoder (Symphonia) → Engine Thread → Ring Buffer → CPAL Callback → Device
```

- **Decoder**: Symphonia outputs planar buffers; the decoder interleaves them into `AudioSamples::F32`
- **Engine**: `AudioEngineLoop` is a state machine (`TrackNotSet` / `Paused` / `Playing`). Decodes in chunks, writes to ring buffer
- **Output**: `OutputStream` wraps a live CPAL stream. Recreation happens automatically when sample rate, channels, or bit depth change
- **Ring buffer**: `rb::SpscRb<f32>`, capacity sized for `BUFFER_DURATION_MS = 128`
- **Volume curve**:
  - `volume >= 0.99`: `1.0`
  - `volume > 0.1`: exponential (`exp(3.912... * volume) / 50.0`)
  - `volume <= 0.1`: linear (`volume * 0.295...`)

## UI Architecture (GPUI)

### Services & Globals

`Services` is registered as a `gpui::Global` and accessed via `cx.global::<Services>()`:

```rust
pub struct Services {
    pub engine_manager: Rc<EngineManager>,
    pub output: Arc<Output>,
    pub engine_event_bus: Entity<EngineEventsBus>,
    pub library: Arc<LibraryService>,
    pub library_event_bus: Entity<LibraryEventsBus>,
}
```

- `Rc` for GPUI-thread-only globals
- `Arc` for shared state that may cross threads

### Event Bus Pattern

Background threads emit events into a `flume` channel. An async GPUI task forwards them into an `EventEmitter` entity:

```rust
// In services.rs
cx.spawn(async move |cx| {
    while let Ok(event) = rx.recv_async().await {
        cx.update(|cx| bus.update(cx, |_, cx| cx.emit(event)))
    }
})
```

Components subscribe with `cx.subscribe(&bus, |this, _, event, cx| { ... })`. The returned `Subscription` must be kept alive (stored in the struct).

### Entity Lifecycle

- State lives in `Entity<T>` (e.g., `Entity<SliderState>`)
- Re-render triggered by `cx.notify()`
- Child views created via `cx.new(|cx| ChildView::new(window, cx))`

## Music Library

### Database Schema (SQLite)

```
artists (id, name, sort_name)
albums (id, title, year, cover_art_path)
album_artists (album_id, artist_id, position)  -- supports compilations
tracks (id, path, title, album_id, track_number, disc_number, duration_ms, year, cover_art_path)
track_artists (track_id, artist_id, role, credited_as, position)  -- supports feat., multiple artists
```

### Cover Art Storage

Extracted by `lofty` during scan, saved to `dirs::cache_dir()/gpui-test/covers/{hash}.jpg`. Only the path is stored in the database.

### Album Sorting

Sorted by `artist.sort_name` → `year` → `title`. `sort_name` strips leading articles: "The Beatles" → "Beatles, The".

### Album Grouping Key

Albums are matched by `(title, year)`. Two folders with the same album title but different years create separate albums.

## Key Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `gpui` | `0.2.2` | UI framework |
| `gpui-component` | `0.5.1` | Component library (sliders, buttons, etc.) |
| `symphonia` | latest | Audio decoding |
| `cpal` | latest | Cross-platform audio output |
| `rb` | latest | Lock-free SPSC ring buffer |
| `lofty` | `0.22` | Metadata / tag reading |
| `rusqlite` | `0.34` | SQLite (bundled) |
| `jwalk` | `0.8` | Parallel directory walking |
| `flume` | `0.12` | Async channels |
| `rstest` | dev | Parameterized tests |

## Testing Patterns

- **Fixture-based**: Audio files in `fixtures/` directory. Tests resolve paths via `CARGO_MANIFEST_DIR`
- **Parameterized**: `#[rstest]` with `#[case::name(...)]` for multiple file formats
- **DB tests**: Each test creates a unique temp database to avoid SQLite locking conflicts
- **No GUI tests**: Audio pipeline tested without GPUI; UI tested manually

## State & Concurrency Primitives

| State | Primitive | Where |
|-------|-----------|-------|
| Playback state | `AtomicU8` (`STATE_IDLE`/`PLAYING`/`PAUSED`) | `audio_output` callback |
| Volume | `AtomicF32` | Shared between UI and audio callback |
| Output stream | `RwLock<Option<OutputStream>>` | Dynamic recreation |
| Audio buffer | `Arc<AudioRingBuffer>` | Shared between engine thread and callback |
| Library DB | `Mutex<Connection>` | Single-threaded access to SQLite |

## Error Handling Strategy

- **`thiserror`** in library crates (`audio_common`, `music_library`)
- **`anyhow`** in application-level code (`ui`, `music_indexer`)
- **Graceful degradation**: Non-fatal decode errors are logged and skipped; scan errors skip the file
- **Panic on critical failures**: `unwrap()` on stream creation, ring buffer errors

## File Locations

- **Entry point**: `crates/ui/src/main.rs`
- **Audio fixtures**: `fixtures/` (WAV, FLAC test files)
- **Database**: `dirs::data_dir()/gpui-test/library.db`
- **Cover cache**: `dirs::cache_dir()/gpui-test/covers/`

## Known Limitations & Behaviors

- **Rescan on startup**: Not automatic. First run shows "Select Music Folder" button. Subsequent runs require manual rescan.
- **Album view**: Currently a vertical list (not a grid). No cover thumbnails rendered yet.
- **No playlist support**: Library is browse-only (albums → tracks). No queue or playlist editing.
- **No search UI**: `LibraryRepository::search()` exists but no search input in the UI.
- **Track title fallback**: If `lofty` finds no title tag, the filename stem is used.
- **Multiple artists**: `music_indexer` reads `TrackArtists` tag from `lofty`; falls back to single `Artist` tag.

## Build Environment

- **Platform**: macOS (Metal renderer required for GPUI)
- **Edition**: Rust 2024
- **Lint**: `#![forbid(unsafe_code)]` at workspace level
- **Note**: GPUI requires Xcode Metal Toolchain. If `cargo build` fails with "missing Metal Toolchain", run: `xcodebuild -downloadComponent MetalToolchain`

## Playback Queue

- The queue is a UI-side ordered list of `Track`s, decoupled from the library view.
- Any component can load tracks into the queue (e.g., clicking a track in an album view replaces the queue with that album's tracks).
- **Previous track behavior**: If more than 3 seconds have elapsed, seek to the start of the current track. Otherwise, go to the previous track in the queue (or seek to start if already at the first track).
- **Auto-advance**: When the engine emits `TrackEnded`, the footer automatically loads and plays the next track in the queue.
- **Future**: playlist editing, persistent queues, shuffle/repeat modes.

# Project Overview: Pawse

A local audio player built with Rust and [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) (Zed's UI framework). The project uses a workspace-based architecture with trait abstractions for the audio pipeline and GPUI's entity/event system for the UI.

## Workspace Structure

| Crate | Path | Purpose |
|-------|------|---------|
| `audio_common` | `crates/audio_common/` | Shared types: `AudioSource` trait, `AudioSamples`, `Metadata`, `AudioBatch`, `AudioError` |
| `audio_decoder` | `crates/audio_decoder/` | Symphonia-based decoder. Reads FLAC, MP3, WAV, OGG, etc. Outputs interleaved `f32` |
| `audio_output` | `crates/audio_output/` | CPAL-based shared output **and** macOS CoreAudio exclusive (hog) output. Lock-free SPSC ring buffer (`rb` crate). Dynamic stream recreation on format change. Device selection, bit-perfect status tracking |
| `audio_engine` | `crates/audio_engine/` | Playback engine thread + state machine. Commands via `flume` channels. Emits `EngineEvent`. Supports CUE-style start offset + clamped duration |
| `music_library` | `crates/music_library/` | SQLite repository. Artists, albums, tracks with many-to-many relationships. Cover art stored as DB blobs (small + large thumbnails) |
| `music_indexer` | `crates/music_indexer/` | Directory scanner (`jwalk`) + metadata reader (`lofty`) + CUE-sheet expansion. Emits `ScanEvent` |
| `cue_parser` | `crates/cue_parser/` | Plain-Rust CUE sheet parser (no audio dependencies). Returns `CueSheet` with tracks, FILE references, indexes |
| `media_integration` | `crates/media_integration/` | System media integration: platform-agnostic facade (`SystemMediaIntegration`, `NowPlayingInfo`, `MediaCommand`) plus a per-OS implementation. macOS = native `objc2` (Now Playing widget, remote media-key commands, Dock icon) under `src/macos/`; Windows (SMTC) + Linux (MPRIS) share a `souvlaki`-based backend in `src/souvlaki_backend.rs` |
| `ui_components` | `crates/ui_components/` | Reusable GPUI components (custom `slider`, `fade` overlay) not provided by `gpui-component` |
| `ui_resources` | `crates/ui_resources/` | UI data layer (not UI logic): embedded icon `Assets` (`rust_embed`), bundled `themes` JSON, and `i18n` — compile-time-checked localization tables for 20 languages. See `crates/ui_resources/src/doc.md` |
| `diagnostics` | `crates/diagnostics/` | GPUI-free error/diagnostics sink. `log::Log` backend writing a rolling log file, a panic hook, and a user-notification channel the app drains into toasts. See `crates/diagnostics/src/doc.md` |
| `pawse` | `crates/pawse/` | GPUI application binary. Views, service globals, event buses, settings store, theme management, active-language resolution (`localization::tr`) |

## Audio Pipeline

```
File → Decoder (Symphonia) → Engine Thread → Ring Buffer → CPAL / CoreAudio Exclusive Callback → Device
```

- **Decoder**: Symphonia outputs planar buffers; the decoder interleaves them into `AudioSamples::F32`. Supports per-track start offset (`start_offset_ms`) and clamped duration for CUE-sheet tracks
- **Engine**: `AudioEngineLoop` is a state machine (`TrackNotSet` / `Paused` / `Playing`). Decodes in chunks, writes to ring buffer
- **Output (`Output`)**: an enum-dispatched wrapper over two modes
  - `OutputMode::Shared` — CPAL stream on the selected (or system-default) device
  - `OutputMode::Exclusive` — macOS-only CoreAudio "hog" mode with a custom IOProc; restores device sample rate on drop
  - Auto-recreates the stream when sample rate / channels / bit depth change
  - On failure, gracefully falls back: exclusive → shared on same device → shared on system default
  - Emits `OutputEvent::Recovered` / `OutputEvent::Failure` for UI notifications
- **Ring buffer**: `rb::SpscRb<f32>`, capacity sized for `BUFFER_DURATION_MS = 128`
- **Volume curve**:
  - `volume >= 0.99`: `1.0`
  - `volume > 0.1`: exponential (`exp(3.912... * volume) / 50.0`)
  - `volume <= 0.1`: linear (`volume * 0.295...`)
- **Bit-perfect status**: `Output::bit_perfect_status()` reports issues (not exclusive, system / app volume below unity, sample-rate mismatch, bit-depth exceeds f32 container, system muted, no source). All reads are atomic-only — no syscalls

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
    pub playback_queue: Rc<RefCell<PlaybackQueue>>,
    pub cover_art_cache: Rc<RefCell<CoverArtCache>>,
    pub current_position_ms: Arc<AtomicU64>,
}
```

- `Rc` for GPUI-thread-only globals
- `Arc` for shared state that may cross threads
- `current_position_ms` is updated inside the engine-events forwarding loop so that playback position can be snapshotted into settings without UI involvement

`SettingsStore` is a separate `Global` holding the persisted `UserSettings` (theme, music folders, volume, last `PlaybackState`, `show_hog_button`).

### Event Bus Pattern

Background threads emit events into a `flume` channel. An async GPUI task forwards them into an `EventEmitter` entity:

```rust
cx.spawn(async move |cx| {
    while let Ok(event) = rx.recv_async().await {
        cx.update(|cx| bus.update(cx, |_, cx| cx.emit(event)))
    }
})
```

Components subscribe with `cx.subscribe(&bus, |this, _, event, cx| { ... })`. The returned `Subscription` must be kept alive (stored in the struct).

The two buses are `EngineEventsBus` (audio playback) and `LibraryEventsBus` (scan progress).

### System Media Bridge (`MediaBridge`)

The bridge listens to `EngineEvent`s via the `EngineEventsBus` and forwards them to the platform's system media UI (via the shared `apply_engine_event`), while a `flume`-fed command loop drives the `EngineManager` / `PlaybackQueue`. It calls `media_integration::create_integration(command_tx, hwnd)` and, if an integration exists for the platform, installs both; otherwise it is a no-op.

- On **macOS**: there is **no** `MediaBridge` entity. `media_bridge::setup(cx)` runs once at startup from `main()` and keeps the integration **app-lived** (an `App::subscribe(...).detach()` plus a detached command loop), because the last window can close while audio keeps playing — a window-owned bridge would die with the window and leave the Now-Playing panel's buttons / state dead. `MacOsIntegration` updates the Now Playing widget, status bar menu, and remote media-key commands via `objc2`. `seed_from_services` publishes the current track/state on construction (see decision 12 in `macos-media-integration.md`)
- On **Windows / Linux**: `MediaBridge` is a GPUI entity instantiated in `MainView` (`#[cfg(not(target_os = "macos"))]`) — fine because these platforms quit when the last window closes. The `souvlaki`-based backend drives SMTC (Windows) / MPRIS (Linux); Windows needs the native window handle (`hwnd`), which `MediaBridge::new` extracts from the GPUI `Window` (it implements `raw_window_handle::HasWindowHandle`)
- Commands from the OS (play / pause / next / previous / seek) flow through a `flume` channel into an async task that drives the `EngineManager` and `PlaybackQueue`

> See `.docs/macos-media-integration.md` for a detailed explanation of the macOS design decisions (why `OnceLock`, `flume`, `RcBlock`, `RefCell` caching, and the trait split were chosen).

### MainView Layout

`MainView` is the window root and owns long-lived entities:

- Header: back button, search input (`InputState` from `gpui-component`), settings gear, `AudioSettings` (bit-perfect indicator, exclusive toggle, device picker)
- Body: either `LibraryView` (albums → tracks) or the `SettingPage` list (themes, folders, audio toggles)
- Footer: `NowPlaying`, transport controls (`PrevButton`, `PlayButton`, `NextButton`, `ShuffleButton`, `RepeatButton`), `TrackProgressSlider`, `Volume`
- Top/bottom fade overlays from `ui_components::fade`

### Entity Lifecycle

- State lives in `Entity<T>` (e.g., `Entity<SliderState>`)
- Re-render triggered by `cx.notify()`
- Child views created via `cx.new(|cx| ChildView::new(window, cx))`

### Window Lifecycle (platform-specific)

- `main.rs` factors window creation into `build_window_options` + `open_main_window(cx, run_startup_tasks)`. Startup calls it with `true` (restores engine state + schedules the launch rescan); reopen calls it with `false`
- **Windows / Linux**: closing the last window quits the app (`on_window_closed` → `cx.quit()`)
- **Single instance (Windows / Linux)**: `single_instance::acquire()` runs at the top of `main()` (before `Application::new()`) and claims a `GenericNamespaced` local socket `dev.pawse.app.sock` via `interprocess` (Windows named pipe / Linux abstract-namespace socket). A second launch connects to that socket, writes one byte, and returns from `main()` before any window is created. The first instance owns a small listener thread whose accepted connections are forwarded through a `flume` channel into a `cx.spawn` task that calls `cx.activate(true)` and raises the existing window (`Window::activate_window`), so a duplicate launch focuses the running app instead of opening a second window. macOS is excluded by `#[cfg]` — Launch Services already enforces single-instance for `.app` bundles, and `on_reopen` handles Dock relaunch
- **macOS**: closing the last window keeps the process and audio alive (the engine runs on its own thread and `Services` is an app-lived `Global`). The window's `MainView`/`Root` entity tree is genuinely destroyed (gpui 0.2.2 exposes no hide/order-out path), so transient navigation state (current view/album, search, scroll) resets to the default library view. `Application::on_reopen` (Dock-icon click with no visible window) calls `open_main_window(cx, false)` to rebuild a fresh window. `open_main_window` opens the window synchronously (no `cx.spawn`), so `cx.windows()` is non-empty before the next reopen event — a rapid double-click can't spawn two windows. Cmd-Q / menu Quit still fully exits via `cx.quit()`, firing the `on_app_quit` snapshot
- **Mid-playback view construction**: because a window can be rebuilt while audio keeps playing (no fresh `EngineEvent::Loaded` will arrive), the footer components seed their state from live `Services` on `new()` instead of waiting for events: `NowPlaying` reads the current queue track + `Output::source_format()` for specs, `PlayButton` reads `Services::is_playing`, and `TrackProgressSlider` reads `Services::current_duration_ms` + `current_position_ms`. These mirrors (`is_playing`, `current_position_ms`, `current_duration_ms`) are kept current by the app-lived engine-events forwarder, not the window, so they survive a closed window. `current_duration_ms` holds the engine's *decoded* duration (set on `Loaded`), so the reopened slider matches the live path rather than the queue's metadata estimate. This same seeding also makes the footer show the restored track on normal startup. The macOS system media bridge follows the same principle but goes further — it is app-lived rather than re-seeded per window (see "System Media Bridge"), so the Now-Playing panel keeps working with no window at all

### Settings & Themes

- `SettingsStore` (in `pawse::settings_store`) persists `UserSettings` as JSON via atomic temp-file rename to `dirs::config_dir()/pawse/settings.json`
- Loading is fault-tolerant: missing or corrupt file falls back to `UserSettings::default()`
- Themes are bundled as JSON in `crates/pawse/themes/`, written to `dirs::data_dir()/pawse/themes/` at startup, and watched by `ThemeRegistry::watch_dir`. The active theme is applied via `gpui-component::theme::Theme`
- `ThemeChoice` is `System` or `Named(String)`. On macOS, `System` syncs with appearance changes

## Music Library

### Database Schema (SQLite)

```
cover_art (id, hash, small BLOB, large BLOB)        -- deduped by content hash
artists (id, name, sort_name)
albums (id, title, year, cover_art_id)
album_artists (album_id, artist_id, position)        -- supports compilations
tracks (id, path, title, album_id, track_number, disc_number,
        duration_ms, year, cover_art_id, start_offset_ms)
track_artists (track_id, artist_id, role, credited_as, position)
                                                     -- supports feat., multiple artists
```

- Unique index `idx_tracks_path_offset` on `(path, start_offset_ms)` — lets one audio file back multiple logical tracks (CUE sheets)
- `track_artists` PRIMARY KEY is `(track_id, artist_id, role, position)` so the same artist can appear in different roles/positions on a single track
- `cover_art.hash` is unique — identical artwork bytes share a single row across albums and tracks
- DB lives at `dirs::data_dir()/pawse/library.db`. On open, if a pre-existing DB lacks the `cover_art` table the file is deleted and re-created (there are no users, so migrations are not maintained — see "Migrations" below)

### Cover Art Storage

- Extracted by `lofty` from embedded tags, or discovered as adjacent image files (e.g. `cover.jpg`, `front.png`, `Artwork/Front.jpg`, RED/OPS `*_01.jpg`) by `music_indexer::metadata`
- Saved into SQLite as **two pre-sized JPEG thumbnails**: `small` (≤128px) and `large` (≤320px). Generated by `music_library::thumbnail`
- Tracks and albums reference cover art by `cover_art_id`. The same image is reused across the album and all its tracks
- The UI reads JPEG bytes via `LibraryService::get_cover_art_small / _large` and decodes them through `CoverArtCache`, which holds `Arc<gpui::Image>` keyed by id
- For the macOS Now Playing widget, large JPEG bytes are written to `temp_dir()/pawse-artwork/{id}.jpg` because `MPMediaItemArtwork` needs a file path

### Album Sorting

Sorted by `artist.sort_name` → `year` → `title`. `sort_name` strips leading articles: "The Beatles" → "Beatles, The".

### Album Grouping Key

Albums are matched by `(title, year)`. Two folders with the same album title but different years create separate albums.

### Search

- Free-text search runs over a precomputed haystack (album title + artist + tracks) using `nucleo-matcher` for fuzzy matching
- The search input lives in `MainView`'s header; queries are pushed to whichever child view is active (`AlbumsView` or `TracksView`)
- `LibraryRepository::search()` also exposes a SQL-`LIKE` fallback used by tests

### CUE Sheets

- `cue_parser` is a standalone parser: tokenizes `PERFORMER`, `TITLE`, `FILE`, `TRACK`, `INDEX`, `REM DATE`, etc.
- `music_indexer::scanner` walks each directory, parses any `.cue` files, opens the referenced audio file, and emits one `ScannedTrack` per CUE `TRACK`. The audio file's full duration is read once and used to compute the *last* track's duration. Referenced audio files are removed from the standalone-file scan so they aren't double-indexed
- CUE tracks share the audio file's `path`, distinguished by `start_offset_ms`. Embedded cover art is inherited from the audio file

## Playback Queue

- `PlaybackQueue` lives in `Services` (interior-mutable via `Rc<RefCell<_>>`) and is decoupled from the library view
- Any component can call `set_tracks(...)` to replace the queue (e.g., clicking a track in an album replaces the queue with that album)
- **Previous track behavior**: If more than 3 seconds have elapsed in the current track, seek to its start. Otherwise, go to the previous track in the queue (or seek to start if already at the first track)
- **Auto-advance**: When the engine emits `TrackEnded`, the app-lived engine-events forwarder (`run_engine_events_bus`) advances `queue.next_track()` and plays it gapless. It lives there rather than in a view so it keeps working on macOS while the window is closed; at end-of-queue `next_track()` returns `None`, clearing `current_index` so a reopened window shows a stopped state instead of a stale track
- **Shuffle**: `set_shuffle(true)` stores the original order so it can be restored exactly when shuffle is disabled. The currently-playing track is pinned to index 0 after shuffling so playback continues seamlessly. New `set_tracks()` while shuffle is on reshuffles and replaces the saved original order
- **Repeat**: `Off` / `All` / `One`. `cycle()` rotates through them in that order. `RepeatMode::One` makes `next_track()` return the current track again
- **Custom queue**: `append_track` (add-to-queue buttons), `move_track` (manual reorder in the queue view) and `remove_track_at` (removing a row) mark the queue custom (`is_custom()`); removing the last track or any `set_tracks*` / `refresh_keeping_current` resets it. Shuffle does not count. While custom, clicking a track in any library view goes through `track_list::play_replacing_queue`, which asks for confirmation (add to queue / cancel / replace) instead of silently replacing the queue. The flag is persisted with the rest of the playback state
- **Add-to-queue indication**: when tracks are appended (`LibraryEvent::QueueChanged`) and the queue panel is open, `QueueView` scrolls to the appended track at the end (`MainView` keeps the view's `visible` flag in sync on both toggle paths)
- **Persistence**: The full queue, original order, current index, position (ms), shuffle flag, and repeat mode are snapshotted into `SettingsStore` on app quit and on every track change. They are restored on startup, including resuming playback position via `EngineManager::seek(fraction)`

## Key Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `gpui` | `0.2.2` | UI framework |
| `gpui-component` | `0.5.1` | Component library (inputs, buttons, popovers, settings layout, etc.) |
| `symphonia` | latest | Audio decoding |
| `cpal` | latest | Cross-platform audio output (shared mode) |
| `objc2-core-audio` | latest | macOS CoreAudio (exclusive/hog mode) |
| `objc2`, `objc2-app-kit`, `objc2-media-player` | latest | macOS Now Playing / status bar / remote commands |
| `rb` | latest | Lock-free SPSC ring buffer |
| `lofty` | `0.22` | Metadata / tag reading |
| `rusqlite` | `0.34` | SQLite (bundled) |
| `jwalk` | `0.8` | Parallel directory walking |
| `flume` | `0.12` | Async channels |
| `nucleo-matcher` | `0.3` | Fuzzy search |
| `rand` | `0.9` | Queue shuffling |
| `rust-embed` | `8` | Embedded SVG icons and theme JSON |
| `image` | latest | Cover-art thumbnail generation |
| `rfd` | `0.15` | Native folder picker |
| `dirs` | `5` | XDG-style config/data/cache paths |
| `rstest` | dev | Parameterized tests |

## Testing Patterns

- **Fixture-based**: Audio files in `fixtures/` directory. Tests resolve paths via `std::env::var("CARGO_MANIFEST_DIR")` (not the `env!` macro — see `feedback_music_indexer_env_var`)
- **Parameterized**: `#[rstest]` with `#[case::name(...)]` for multiple file formats
- **DB tests**: Each test creates a unique temp database to avoid SQLite locking conflicts
- **No GUI tests**: Audio pipeline tested without GPUI; UI tested manually
- **Sanitizer / Miri targets** in `Makefile`: `test-careful`, `test-asan`, `test-tsan`, `test-miri`, `test-leaks`. The GUI crates (`pawse`, `ui_components`, `audio_engine`) are excluded from nightly sanitizer runs because `gpui` pulls in `pathfinder_simd` which doesn't compile on current nightly

## State & Concurrency Primitives

| State | Primitive | Where |
|-------|-----------|-------|
| Playback state | `AtomicU8` (`STATE_IDLE`/`PLAYING`/`PAUSED`) | `audio_output` callback |
| App volume | `AtomicF32` | Shared between UI and audio callback |
| Source rate / bit-depth / present | `AtomicU32` / `AtomicU8` / `AtomicBool` | `Output` for lock-free bit-perfect status |
| Output mode | `RwLock<Option<OutputMode>>` | `None` only briefly between teardown and install |
| Device manager | `RwLock<DeviceManager>` | Selected device, default tracking |
| Event queue | `Mutex<Vec<OutputEvent>>` | UI drains on every tick |
| Audio buffer | `Arc<AudioRingBuffer>` | Shared between engine thread and callback |
| Library DB | `Mutex<Connection>` | Single-threaded access to SQLite |
| Current position | `Arc<AtomicU64>` (ms) | Updated by event forwarder, read by settings snapshot |
| Current duration | `Arc<AtomicU64>` (ms) | Decoded duration set on `Loaded`; seeds a slider built mid-playback |
| Playback state mirror | `Arc<AtomicBool>` | `is_playing`, updated by forwarder; seeds a play button built mid-playback |
| Media commands | `OnceLock<Sender<MediaCommand>>` | AppKit callbacks → player command channel (macOS only) |

## Error Handling Strategy

- **`thiserror`** in library crates (`audio_common`, `music_library`, `cue_parser`)
- **`anyhow`** in application-level code (`pawse`, `music_indexer`)
- **Graceful degradation**: Non-fatal decode errors are logged and skipped; scan errors skip the file; cover-art save failures don't abort the track insert
- **Graceful shutdown**: `cx.on_app_quit` snapshots playback state to settings, then `Services::shutdown()` quiesces audio output and the engine
- **Audio recovery**: device disconnect → drop exclusive output, restore device state, install shared on system default, push a `Recovered` event for a notification toast. `Output::new` is infallible — no device/stream at startup launches the app silent with an `OutputEvent::Failure`; selecting a device (or a later format change) installs a stream
- **Diagnostics sink** (`diagnostics` crate): every crate logs through the `log` facade into a rolling log file (`dirs::data_dir()/pawse/logs/pawse.log`). `diagnostics::init` runs first in `main`, installs the logger + a panic hook (so even fail-fast panics leave a file artifact), and returns a `Notice` channel that `error_bridge::spawn_notice_forwarder` drains into toasts
- **User-visible errors**: surfaced via `gpui-component::notification::Notification` — from `OutputEvent` drained on each render of `AudioSettings`, from `settings_store::notify_save_error`, and from `diagnostics::notify_error/notify_warning` via the notice forwarder

## File Locations

- **Entry point**: `crates/pawse/src/main.rs`
- **Audio fixtures**: `fixtures/` (WAV, FLAC, MP3, OGG test files; generated by `make generate`)
- **Database**: `dirs::data_dir()/pawse/library.db`
- **Settings**: `dirs::config_dir()/pawse/settings.json`
- **Log file**: `dirs::data_dir()/pawse/logs/pawse.log` (rolls to `pawse.log.1` past 5 MiB)
- **Bundled themes**: staged at `dirs::data_dir()/pawse/themes/`
- **Temp cover art** (for macOS Now Playing widget): `std::env::temp_dir()/pawse-artwork/`

## Known Limitations & Behaviors

- **Rescan**: Triggered manually via the settings folder picker or the `Rescan` menu action; not automatic on filesystem changes. If `music_folders` is configured but the DB is empty on startup, a rescan runs automatically
- **Album view**: Vertical virtualized list (`v_virtual_list`) with small cover thumbnails. Not a grid
- **No playlists**: Library is browse-only (albums → tracks). The queue is implicit (set by clicking a track). Saved playlists are not implemented. Direction is *generated* playlists only (random-from-library is the next planned feature) — no user-curated playlists
- **Track title fallback**: If `lofty` finds no title tag, the filename stem is used
- **Multiple artists**: `music_indexer` reads `TrackArtists` tag from `lofty`; falls back to single `Artist` tag
- **Exclusive mode is macOS-only**: On other platforms the toggle is hidden (controlled by `show_hog_button` in settings) and `audio_output::exclusive::unsupported` returns errors
- **No skip-15s media-key handlers**: only `changePlaybackPositionCommand` is registered. See `.docs/macos-media-integration.md`

## Build Environment

- **Platform**: macOS (Metal renderer required for GPUI)
- **Edition**: Rust 2024
- **Lint**: `#![forbid(unsafe_code)]` at workspace level; `macos_integration` overrides it to `allow` because it uses `objc2` for AppKit interop
- **Note**: GPUI requires Xcode Metal Toolchain. If `cargo build` fails with "missing Metal Toolchain", run: `xcodebuild -downloadComponent MetalToolchain`

## Migrations

There are no users. When changing data handling, do **not** write migrations — bump the schema, and the bootstrap path will either pick it up via the empty `migrations` table or recreate the DB. Old schema detection in `SqliteLibrary::open()` (e.g. the `cover_art` table presence check) is a one-off and may be removed once it has served its purpose.

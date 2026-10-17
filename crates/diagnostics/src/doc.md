# diagnostics

The single log sink of the app, behind the `log` facade: a rolling `pawse.log`, user-facing
notices, and panics captured into the same file. GPUI-free on purpose, so any crate
(including the `forbid(unsafe_code)` libraries) can log through `log` without pulling in the
UI framework.

Not here: showing a `Notice` as a toast and the other background-error path
(`pawse/src/error_bridge.rs`, `pawse/src/doc.md`); what gets logged is up to each caller.

## Files

- `lib.rs` — everything: config, `init` / `flush`, `notify_*`, `FileLogger`, the writer
  thread, the panic hook.

## Logging

- `init(Config) -> Receiver<Notice>` opens the log, spawns the writer thread, installs the
  logger and the panic hook, and returns the receiver the app forwards into notifications.
  Call it once at startup: the statics are `OnceLock`s, so a second call installs no logger
  and its receiver never gets a notice. Tests drive `write_line` directly instead of `init`.
- `FileLogger::log` only formats the line and sends it down a channel, so the caller never
  takes a file lock or does I/O: the audio callback's rare error path must not block.
- The `pawse-diagnostics` writer thread owns the file and flushes every line. Rotation is
  size-based: past `Config.max_bytes` the file becomes `pawse.log.1` (one generation).
- The writer thread is a deliberate exception to "background work on GPUI's executor": this
  crate cannot depend on GPUI. It is never joined (the logger is leaked); `flush()` polls
  the channel until it is empty (bounded, about 200 ms) instead, and the app calls it on
  quit so shutdown-path lines reach disk. Each line is already flushed on its own, so this
  only closes the in-flight-at-exit gap.
- `Config.log_dir` comes from the caller (the app passes `dirs::data_dir()/pawse/logs`): the
  crate holds no path conventions. `also_stderr` defaults to `cfg!(debug_assertions)`:
  console echo in dev, file only in release.

## Levels

- `Config.level` is the global level (Info by default). `Config.target_levels` caps single
  targets; the longest prefix on a `::` boundary wins. The default is `NOISY_TARGETS`:
  `gpui::window::a11y` at `Warn`, because gpui logs "focused element has no accessibility
  node" at `Info` on every focus change and pawse sets no roles; it flooded the log on
  Windows, where accessibility activates. Muted here rather than disabling accessibility
  (`Application::new_inaccessible`), which would also drop gpui-component's own roles.
- `log::set_max_level` is the most permissive of the global level and every override: an
  override above the global level would otherwise be filtered out by the facade before
  `enabled` runs.

## Notices

- `notify_error` / `notify_warning` log (`"{title}: {message}"`) **and** push a `Notice`
  into an unbounded channel. Only these reach the user; a plain `log::error!` never does.
  Title and message are the final user-facing text, so the caller localizes them.

## Panics

- The hook writes the record (location, payload, backtrace when captured) **synchronously**
  to the log file and to stderr, bypassing the writer channel, so it lands before the process
  dies; then it chains the previous hook.

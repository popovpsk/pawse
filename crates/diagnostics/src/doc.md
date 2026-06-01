# diagnostics

The single error/diagnostics sink for the app. GPUI-free on purpose, so any crate
(including the `#![forbid(unsafe_code)]` library crates) can route diagnostics here
through the `log` facade without pulling in the UI framework.

## Responsibilities

- Back the `log` facade: install a `log::Log` implementation that writes a rolling
  log file on the client machine, so a user hitting a bug leaves an artifact.
- Surface a subset of diagnostics to the user as notification toasts, via an
  out-of-band channel the app drains and renders.
- Capture panics (even the deliberately-kept fail-fast ones) into the same log file.

## Files

- `lib.rs` — everything:
  - `init(Config) -> Receiver<Notice>`: opens the log file, spawns the writer thread,
    installs the logger + `log::set_max_level`, installs the panic hook, and returns
    the receiver the app forwards into GPUI notifications.
  - `notify_error` / `notify_warning`: log **and** push a user-facing `Notice`.
  - `flush()`: bounded drain of the writer channel — the app calls it on `on_app_quit`
    so shutdown-path log lines (e.g. CoreAudio teardown warnings) reach disk before
    exit. Each pulled line is already flushed individually; this only closes the
    in-flight-at-exit gap. Panics don't need it (`write_sync` is synchronous).
  - `FileLogger` (`log::Log`): formats `{rfc3339} {LEVEL} {target}: {msg}` and sends
    the line down a channel — no file lock on the caller, so the audio callback's
    rare error path never blocks.
  - writer thread (`spawn_writer` / `write_line`): owns the `BufWriter<File>`, flushes
    every line, and does size-based rotation to `pawse.log.1`.
  - panic hook (`install_panic_hook` / `format_panic` / `write_sync`): writes the
    panic record **synchronously** (not via the async channel) so the artifact lands
    on disk before the process dies, then chains the previous hook.

## Non-obvious behavior

- The dedicated writer thread is a deliberate carve-out from the "use GPUI's
  background pool" rule (this crate must not depend on GPUI). It mirrors the audio /
  indexer thread carve-outs documented in `AGENTS.md`.
- `Config.log_dir` is supplied by the caller (the app passes
  `dirs::data_dir()/pawse/logs`) — the crate stays free of path conventions.
- `also_stderr` defaults to `cfg!(debug_assertions)`: dev keeps a console echo,
  release writes file-only.
- `log::set_boxed_logger` can only succeed once per process; `init` is a no-op for the
  logger on a second call (tests exercise `write_line` directly rather than `init`).
- The writer thread is never joined (the logger is intentionally `'static`/leaked);
  `flush()` polls the send channel's queue instead of joining, so it stays decoupled
  from the leaked logger.

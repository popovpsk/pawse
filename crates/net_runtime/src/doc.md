# net_runtime

The app's one tokio runtime, started on first use and kept for the life of the process. Its
workers run the async code (`torrent`'s librqbit session, `pawse_remote`'s server); its
blocking pool runs the blocking network calls of every other crate and of the app, so a
request waiting on a slow server never holds a thread of GPUI's executor. GPUI-free and
knows nothing of what runs on it.

Not here: the blocking clients themselves (`server_http/src/doc.md` and the client crates);
which app work goes to the pool (`pawse/src/doc.md`, CLAUDE.md "Concurrency").

## Files

- `lib.rs` — `handle`, `try_handle`, `unblock`, the runtime's shape.
- `tests.rs` — `unblock` awaited by a plain thread-parking executor (the way GPUI awaits
  it), `block_on` from the blocking pool, a panic reaching the awaiter.

## Rules

- `unblock(work)` starts `work` on the blocking pool at once and returns a future of its
  result. The future is runtime-agnostic: GPUI tasks await it. Dropping the future does not
  stop the work, it only drops the result; cancellation stays the work's own (a stop flag,
  a timeout).
- A panic in `work` is re-raised in whoever awaits `unblock`, as if the work had run
  inline. A cancelled task cannot happen: the runtime is a static and never shuts down.
- `handle().spawn_blocking(work)` with the handle dropped is the fire-and-forget form.
- Blocking-pool threads may call `Handle::block_on` (`torrent` does on every call); worker
  threads and async code never may, and never run blocking calls — that would stall the
  librqbit session and the web remote.
- Threads are named `net-runtime`, workers and blocking pool alike. The blocking pool grows
  on demand (tokio's default cap, 512) and drops a thread after 10 s idle; it is shared with
  librqbit's disk work (`block_in_place`), so it is not capped lower.
- `try_handle` reports a runtime that failed to build; `handle` panics on it, like
  `std::thread::spawn` does when no thread can start.

## Decisions

Owner's calls; ask before changing.

- One runtime for the whole app, not one per crate.
- Two worker threads: librqbit's disk work leaves the workers through `block_in_place`, and
  the web remote is light.

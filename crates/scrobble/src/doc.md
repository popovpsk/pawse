# scrobble

Self-contained Last.fm scrobbling. No GPUI, no pawse dependencies — a pure
library the app drives through `ScrobbleHandle`. The GPUI/event wiring lives in
`pawse::scrobble_bridge`.

## Responsibilities

- Talk to the Last.fm audioscrobbler 2.0 API (auth, Now Playing, batched
  scrobble submit, love/unlove) with MD5 request signing.
- Decide *when* a play counts as a scrobble (Last.fm's 30s / half-or-4-minute
  rule) from wall-clock playing time.
- Never lose a scrobble: a submit that fails (offline) stays in a disk-backed
  queue and is retried on the next submit and on the next launch.

## Files

- `lib.rs` — public data types (`Session`, `NowPlaying`, `Scrobble`), credential
  resolution (`creds` / `is_available`, runtime env then compile-time
  `option_env!`), and re-exports.
- `client.rs` — `LastfmClient` over blocking `ureq`. `sign` hashes the
  sorted `key+value` pairs then the shared secret (`format`/`api_sig` excluded).
  Errors returned by Last.fm as either a JSON `error` object or a non-2xx body
  are both surfaced (`read` reads the body out of `ureq::Error::Status`).
- `accumulator.rs` — `PlayAccumulator`: wall-clock playing-time accumulation
  (`on_play`/`on_pause`/`played`), plus the `should_scrobble` policy. The clock
  instant is passed in, so it is deterministic and rate-independent (position
  ticks are irrelevant). Seeks are ignored; only real playing time counts.
- `queue.rs` — `ScrobbleQueue`: append + atomic JSON persist, `peek_batch` /
  `drop_front` (items leave only after a confirmed submit), bounded by `cap`
  (oldest dropped) so it can't grow without limit.
- `worker.rs` — `ScrobbleHandle` + a dedicated worker thread owning the client
  and queue, fed `Msg`s over a `flume` channel. `flush` drains the queue in
  batches of up to 50 and stops on the first failure, leaving the rest queued.

## Non-obvious behavior

- **Only 2xx removes items from the queue.** `flush` calls `drop_front` solely
  on `Ok`; any error breaks the loop so nothing is lost when offline.
- **Now Playing and love/unlove are fire-and-forget** — never queued (they are
  ephemeral / idempotent); only scrobbles are persisted and retried.
- **No session ⇒ no network.** The worker no-ops submission while `session` is
  `None` but retains the queue, so scrobbles captured around a sign-out are not
  dropped.
- **Signature order matters.** Params live in a `BTreeMap`, so signing iterates
  in the lexicographic order Last.fm requires (including batch keys like
  `artist[0]`).

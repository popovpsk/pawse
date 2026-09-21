# scrobble

Self-contained scrobbling engine. No GPUI, no pawse dependencies — a pure
library the app drives through `ScrobbleHandle`. The GPUI/event wiring lives in
`pawse::scrobble_bridge`, the loved-tracks import in `pawse::scrobble_import`,
and the UI in `pawse::scrobble_settings` — its own Settings tab, one
`SettingGroup` per target plus a shared options group.

The engine is service-agnostic: policy (when a play counts, what is queued, how
failures are retried) lives here once, and every destination is a
`ScrobbleTarget` implementation. One play fans out to every configured target.

## Responsibilities

- Decide *when* a play counts as a scrobble (Last.fm's 30s / half-or-4-minute
  rule) from wall-clock playing time.
- Fan a scrobble, a Now Playing update or a love/unlove out to every configured
  target, each with its own batch size, error handling and retry schedule.
- Never lose a scrobble: anything a target has not confirmed stays in the
  store and is retried on a timer and on the next launch.
- Keep the listening history. Every play past `HISTORY_MIN_SECS` is recorded
  with how long it was actually played, whether or not it qualified as a
  scrobble and whether or not any target is configured. The history is the
  local record a future recommendations feature reads; the queue is derived
  from it.

## Files

- `lib.rs` — public data types (`Session`, `NowPlaying`, `Scrobble`), Last.fm
  credential resolution (`creds` / `is_available`, runtime env then compile-time
  `option_env!`), the client constructors, `primary_artist`, and re-exports.
- `accumulator.rs` — `PlayAccumulator`: wall-clock playing-time accumulation
  (`on_play`/`on_pause`/`played`), plus the `should_scrobble` policy. The clock
  instant is passed in, so it is deterministic and rate-independent (position
  ticks are irrelevant). Seeks are ignored; only real playing time counts.
- `target.rs` — `TargetId`, the `ScrobbleTarget` trait, and `SubmitError`, the
  four-way error classification every target maps its transport onto.
- `store.rs` — the `ScrobbleStore` trait plus the types crossing it (`Play`,
  `Love`, `Outcome`, `StoreError`). The engine defines the storage contract and
  implements none of it: the crate stays free of rusqlite and of any path.
  `pawse::scrobble_store::LibraryScrobbleStore` is the implementation, backed by
  the `plays` / `loves` / `play_deliveries` / `love_deliveries` tables in
  `library.db` (migration 8).
- `worker.rs` — `ScrobbleHandle` + a dedicated worker thread owning the targets
  and sending, fed `Msg`s over a `flume` channel and reporting back over a
  `StatusEvent` channel (`AuthFailed` / `Rejected` carry the server's own text).
  Holds the per-target backoff schedule. The store is shared with the handle,
  which writes into it directly.
- `targets/audioscrobbler.rs` — the AudioScrobbler 2.0 client over blocking
  `ureq`, parameterized by `Profile` (endpoint, auth URL, key, secret) so
  Last.fm and Libre.fm/GNU FM share one implementation. `sign` hashes the sorted
  `key+value` pairs then the shared secret (`format`/`api_sig` excluded). Also
  hosts the auth flow (`auth_url`/`get_session` over `SessionError`, plus the
  loopback callback listener `bind_callback_listener`/`wait_for_callback` pawse
  drives sign-in through) and `loved_tracks` for the like import.
- `targets/listenbrainz.rs` — `submit-listens` over `Authorization: Token`, plus
  `validate` for the login screen. Works against any ListenBrainz-compatible
  root, so a self-hosted instance is just a different `api_root`.
- `targets/csv_log.rs` — appends one RFC 4180 row per event to a local file.
  The header matches Pano Scrobbler's file format, so its converter reads ours.
  `is_pawse_log` is the guard the settings UI runs before pointing the log at a
  file the user already has.
- `targets/mod.rs` — the shared `ureq` agent config and response reader.

## Storage

Migration 8 in `music_library`, in `library.db` next to the rest of the library
so a future recommendations query can join `plays.track_id` against `tracks`
directly. `track_id` is `ON DELETE SET NULL`: the history outlives the file it
came from, and the artist/title text on the row is what survives.

```
plays            id, track_id?, artist, title, album?, album_artist?,
                 track_number?, duration_secs?, played_secs?, started_at,
                 qualified            unique (started_at, artist, title)
loves            id, track_id?, artist, title, loved, at
play_deliveries  (play_id, target) pk, state, attempts, last_error?, updated_at
love_deliveries  (love_id, target) pk, state, attempts, last_error?, updated_at
```

`target` holds `TargetId::key()` verbatim (`lastfm`, `librefm`, `listen_brainz`,
`csv_log`) — the same strings the old JSON file used, so the import needed no
mapping table. `state` is 0 pending / 1 sent / 2 dropped, and every settle is
guarded on `state = 0` so a finished row can never be reopened. `played_secs` is
NULL only for rows imported from that file.

Both `track_id` columns are foreign keys, so both need an index: a rescan deletes
every track, and SQLite enforces `ON DELETE SET NULL` by looking for children of
each deleted parent. Without `idx_loves_track` that is a full scan of `loves` per
deleted track — 2.3s for 20k tracks against 0.15s with it, inside the scan
writer's transaction with the UI waiting behind `busy_timeout`.

The scrobble tables are reached through a **second `Connection`**
(`SqliteLibrary::scrobble_conn`), the same trick `ScanSession` uses. The worker
thread writes here while the UI thread reads `library.db` through `conn`; sharing
one `Mutex<Connection>` would mean a settle that waits out the 5s `busy_timeout`
against the scan writer holds the UI's mutex for those 5 seconds, which is a
freeze rather than a dropped frame.

One consequence of sharing `library.db`: the pre-migration schema probe in
`SqliteLibrary::open` used to delete the database whenever it failed to find six
expected artifacts. It now runs only when `user_version == 0`, because above
that the migrations own the schema — and because the history, unlike tracks,
cannot be rebuilt by rescanning the disk.

## Non-obvious behavior

- **The history and the queue are different things.** A `plays` row is the fact
  that something was listened to; a `play_deliveries` row is one target's debt
  against it. The payload is stored once and joined per target, so a scrobble
  that reached Last.fm is never resubmitted there because ListenBrainz was down.
  Delivery state is `pending` / `sent` / `dropped` with an attempt count and the
  last error, so "did not arrive" can be told apart from "was refused, here is
  why". **`plays` and `loves` rows are never deleted** — not on success, not on a
  permanent rejection, not when the queue overflows. Only delivery rows change
  state.
- **Errors are classified so a global failure never eats the queue.** `Transient`
  keeps the batch and backs the target off (60s, doubling, capped at 30 min,
  reset on the next success). `Auth` — which covers everything that is wrong with
  the account, the API key or the signature, not just an expired session — keeps
  the items, disables the target until the next `configure`, and raises
  `StatusEvent::AuthFailed`; re-logging in delivers the backlog. `Permanent` is
  reserved for a payload the server will never take (Last.fm 6/7): the batch is
  dropped, but the run stops and the target backs off, so a systemic `Permanent`
  can cost at most one batch per backoff window instead of the whole queue.
  `Unsupported` resolves the item for that target as `dropped`, not as `sent` —
  the row has to say the delivery never happened, or the table lies about every
  love ListenBrainz was asked for.
- **A delivery that cannot be recorded stops the run.** The store is the one
  thing in the send loop that can now fail *between* "the server took it" and
  "the row says so". If `settle` errors, `flush_target` backs the target off and
  returns instead of looping: re-reading the still-pending rows would resubmit
  the same batch up to `RUN_LIMIT / max_batch` times per flush and then again
  every `RESUME_SOON`, which duplicates rows in the CSV log and looks like abuse
  to Last.fm. The old in-memory `resolve` could not fail, so this failure mode
  is new with the store.
- **The worker owns the retry timer.** Its loop is `recv_timeout` against the
  earliest backoff deadline, so a queue filled while offline drains on its own,
  without waiting for the next track to end. A run is capped at `RUN_LIMIT`
  items per target and resumes `RESUME_SOON` later, so a large backlog cannot
  hold the thread. `configure` clears the backoff along with the disabled set:
  it only ever runs because the user changed a setting, and leaving a 30-minute
  deadline standing would make the fix look like it did nothing.
- **Nothing that must survive goes through the channel.** The store is an
  `Arc<dyn ScrobbleStore>` shared with the handle, so `scrobble` / `love` /
  `persist` write on the caller's thread and only then poke the worker with a
  `Flush`. A `Msg` still in the channel dies with the process, and the worker can
  sit inside a 35s HTTP timeout, so anything queued as a message could be lost on
  quit. The cost is one `INSERT` plus one row per target in a single transaction
  on the caller's thread (the UI thread) per track change or like — measured at
  0.098 ms median / 0.148 ms p95 / 1.8 ms worst on an APFS SSD with three targets
  configured. Against the JSON file it replaced (0.15 ms median / 0.33 ms worst)
  the typical case is cheaper, because `library.db` runs `journal_mode = WAL`
  with `synchronous = NORMAL` and a commit does not fsync; the tail is worse,
  because a WAL checkpoint occasionally lands on one of these writes. That
  trade is accepted for the same reason the fsync was: the alternative is the
  loss window above. Decoding is not a fair comparison — opening the decoder is
  a `Command` to the engine thread — so this remains the heaviest blocking call
  on the UI thread at a track change, and a hitch there is the first thing to
  look at. `NowPlaying` stays a message: it is ephemeral and losing it costs
  nothing.
- **Only the targets currently configured count as pending.** Deliveries left
  over for a service the user switched off stay pending in the table (a re-login
  should still deliver them) but are excluded from `pending_count` and never wake
  the worker. `pending_count` counts items, not delivery rows: something owed to
  two services counts once. Nothing surfaces it in the UI — a number that reads
  zero almost always tells the user nothing, and the failures worth knowing about
  arrive as notifications instead. It exists for the worker, which uses it to
  decide whether a target is worth flushing and whether to arm the retry timer at
  all.
- **The queue is bounded, the history is not.** `trim` counts **items**, the
  same unit `pending_count` does, and when there are more than `CAP`
  pending it marks every delivery row of the oldest ones `dropped` with
  `queue overflow` as the reason. Counting rows instead would silently divide
  the capacity by the number of configured services. The `plays` row survives,
  so an overflow costs the delivery, not the record that it happened — which is
  what the old file-backed queue used to lose.
- **A play is written once, at the end, but a qualified one is written early.**
  Two different moments can commit it. `Paused` commits *only* if the play
  already qualifies, so a scrobble survives a crash while paused — that is what
  the old queue's pause handling bought and it is kept. Every terminal
  transition (`Loaded` of the next track, `TrackEnded`, `Stopped`, quit) commits
  whatever has not been committed yet, and if the early commit already happened
  it simply commits again and the upsert raises `played_secs` to the real total
  rather than leaving the figure at the pause. The distinction matters: marking the play
  consumed at the 15s floor instead of at qualification silently destroys the
  scrobble of every track the user pauses in its first half.
- **History does not depend on scrobbling being set up.** `ScrobbleHandle` is
  created unconditionally and `on_engine_event` no longer returns early when no
  target is configured; only `qualified` (and therefore whether any delivery row
  is written) is gated on it. `HISTORY_MIN_SECS` (15s, in
  `pawse::scrobble_bridge`) is the floor that keeps click-throughs out of the
  table — it is deliberately far below the scrobble threshold, because a track
  that was started and skipped is signal worth keeping.
- **Now Playing is never queued** — it is ephemeral, sent to every target
  fire-and-forget. Loves *are* queued: they are idempotent and worth retrying,
  and they carry the moment they happened. The CSV target writes that timestamp
  rather than the flush time, so a like made offline does not land in the log
  with the time the app next came online.
- **ListenBrainz cannot love.** `feedback/recording-feedback` needs a recording
  MBID and the library stores none, so `love` returns `Unsupported`. The CSV log
  returns `Unsupported` for Now Playing instead, because a "currently playing"
  line is not something an append-only listening log should carry.
- **Libre.fm needs no configured keys.** GNU FM does not validate `api_key` /
  `api_secret`, so `Profile::librefm` ships a constant — Libre.fm works in
  builds with no `LASTFM_API_KEY`.
- **Signature order matters.** Params live in a `BTreeMap`, so signing iterates
  in the lexicographic order the API requires (including batch keys like
  `artist[0]` and `albumArtist[0]`). `scrobble_params` / `now_playing_params` /
  `love_params` are free functions precisely so the wire shape can be asserted
  without a session or a network.
- **Imported likes are not scrobbled back.** `pawse::scrobble_import` marks
  matches with `LibraryService::like_many`, which emits one
  `LibraryEvent::LikesImported`; the bridge only reacts to `TrackLikedChanged`,
  so an import never bounces back out as a `track.love` — to Last.fm or to any
  other target.
- **Sign-in never calls `auth.getToken`.** `auth_url` only ever sends
  `api_key` and `cb=http://127.0.0.1:<port>/pawse-auth?state=<state>` — no
  pre-fetched token. Last.fm/Libre.fm mint the token themselves and hand it
  back as a query param on the redirect to that loopback URL once the user
  approves, which `wait_for_callback` is waiting on — this is the API's "web
  application" flow, not its "desktop" one. Mixing this with a separately
  fetched `auth.getToken` token (the previous, desktop-flow design) silently
  breaks the redirect — the two flows aren't meant to be combined, and the
  failure mode is the browser just never coming back, not an error.
- **The callback listener trusts nothing that doesn't carry its own random
  `state`.** `bind_callback_listener` mints one per sign-in attempt
  (`random_state`, two independently-seeded `RandomState` hashes — no `rand`
  dependency needed for a value that only has to be unguessable by another
  local process for up to `CALLBACK_TIMEOUT`) and `handle_callback_connection`
  only accepts a request whose path is `CALLBACK_PATH` *and* whose `state`
  matches; anything else (a stray favicon fetch, another localhost process
  probing the ephemeral port) is answered and ignored rather than treated as
  the real callback. A matching request with no `token` is Last.fm/Libre.fm's
  own denial redirect, not noise — it fails the sign-in immediately
  (`SessionError::Denied`) instead of leaving the UI parked on "Connecting…"
  for the full `CALLBACK_TIMEOUT`.
  `SessionError::NotAuthorized`/`TokenExpired` still exist because
  `get_session` can in principle report either, but the ordinary path never
  hits them: `get_session` only runs after the redirect already proves
  approval. `Cancelled` and `TimedOut` come from `wait_for_callback` itself
  (a "Cancel" click, or nothing arriving inside `CALLBACK_TIMEOUT`) and reset
  the UI to `AuthPhase::Idle` rather than staying stuck waiting; the settings
  UI folds `NotAuthorized`/`Denied` into the same localized "link expired,
  sign in again" message rather than leaving them as untranslated Rust text.
- **The callback socket is closed gracefully, or Windows eats the page.**
  `respond` ends in `close_gracefully`: `shutdown(Write)` to send a FIN, then
  drain whatever is still in the receive buffer until EOF or
  `CALLBACK_DRAIN_TIMEOUT`. Dropping the socket with unread request bytes still
  queued makes the stack close abortively (RST) instead, and Windows then
  discards the already-delivered response sitting in the browser's receive
  buffer — the browser shows a connection error while the app has the token and
  reports a successful sign-in. macOS hands buffered data to the app before
  reporting the reset, which is why the same code looked fine there.
  `read_request_line` reads to `\r\n\r\n` rather than the first `\r\n` for the
  same reason (a real browser request does not fit in one 512-byte chunk), and
  falls back to whatever it has on a read error so a truncated stray request is
  still answered.
- **The accepted socket is put back into blocking mode explicitly.** The
  listener is non-blocking so `wait_for_callback` can poll `cancel`/the
  deadline; whether `accept` hands that mode down to the connection is
  platform-dependent, and an inherited non-blocking socket turns the
  read-then-respond exchange into silently-swallowed `WouldBlock`s.
- **The wait runs on its own OS thread, not gpui's background-executor
  pool.** `wait_for_callback_async` spawns a plain `std::thread` and hands
  the result back over a `flume` channel the caller `.recv_async().await`s,
  so a sign-in attempt left dangling for up to five minutes never occupies a
  pool worker that other incidental background work (thumbnail decode,
  library scans) shares. `wait_for_callback` itself stays synchronous and
  independently testable; only the call site changed.
- **`apply_session` rechecks the cancel flag, not just the error.** The gap
  between `wait_for_callback` returning a token and `get_session` completing
  (up to `targets/mod.rs`'s `ureq` timeouts, tens of seconds in the worst
  case) is a window where a "Cancel" click can't abort an in-flight request,
  so `start_web_sign_in` checks `cancel` again right before applying the
  result — a session that arrives after Cancel was pressed is discarded
  locally rather than silently signing the account in.
- **A failure the server explained reaches the user, once.** `AuthFailed` and
  `Rejected` carry the server's message, not just a target id, because the useful
  part is usually an instruction ("verify your email at metabrainz.org"). The
  bridge raises it as a notification and remembers the text per target, so a
  rejection repeating every backoff window notifies once, not every window; the
  memory is cleared on `configure`, so a retry after the user changed something
  speaks up again. `Transient` stays silent — it is the offline case and heals
  itself.
- **The CSV target never overwrites and never adopts a stranger's file.** It
  opens with `create(true).append(true)` and writes the header only when the file
  is missing or zero-length, so pointing it at a log from an earlier run just
  continues that log. Because "open an existing file" therefore has to be
  offered, `is_pawse_log` gates it: empty is fine, the exact header is fine,
  anything else is refused, so a mis-click on a document cannot get scrobble rows
  appended to it. It reads at most `HEADER.len() + 1` bytes, so picking a huge or
  binary file costs nothing. A file that is damaged *after* it was chosen is not
  a problem worth guarding: appending still works, and if it is deleted the next
  write recreates it with a header.
- **No artificial delay between batches.** Settling a batch is one small
  transaction and nothing sleeps on the worker thread, so a flush is bounded by
  the HTTP timeouts alone.
- **The old JSON queue is imported once, and the file is claimed first.**
  `pawse::scrobble_bridge` renames `<config>/pawse/scrobble_queue.json` to
  `.json.imported` **before** inserting anything, and gives up without importing
  if the rename fails. Inserting first would mean a failed rename (a stale
  destination on Windows, an AV lock) re-imports the whole file on every launch,
  i.e. N duplicate scrobbles per start. A file that parses to zero items is set
  aside as `.json.unreadable` instead, so "imported" never names a file whose
  contents were dropped, and one malformed entry only costs that entry rather
  than the whole queue. Imported rows have a NULL `played_secs`: the file never
  recorded how long anything was played, and guessing would poison the history.

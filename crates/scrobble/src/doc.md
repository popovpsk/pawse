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
- Never lose a scrobble: anything a target has not confirmed stays in a
  disk-backed queue and is retried on a timer and on the next launch.

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
- `queue.rs` — `PendingStore`: one queue of `Pending` items, each carrying the
  set of targets that still owe it, plus atomic JSON persist and the v1 → v2
  file migration. Bounded by `cap` (oldest dropped) so it can't grow unbounded.
- `worker.rs` — `ScrobbleHandle` + a dedicated worker thread owning the targets
  and sending, fed `Msg`s over a `flume` channel and reporting back over a
  `StatusEvent` channel (`AuthFailed` / `Rejected` carry the server's own text).
  Holds the per-target backoff schedule. The queue is shared with the handle,
  which writes into it directly.
- `targets/audioscrobbler.rs` — the AudioScrobbler 2.0 client over blocking
  `ureq`, parameterized by `Profile` (endpoint, auth URL, key, secret) so
  Last.fm and Libre.fm/GNU FM share one implementation. `sign` hashes the sorted
  `key+value` pairs then the shared secret (`format`/`api_sig` excluded). Also
  hosts the auth flow (`get_token`/`auth_url`/`get_session`, the latter over
  `SessionError`) and `loved_tracks` for the like import.
- `targets/listenbrainz.rs` — `submit-listens` over `Authorization: Token`, plus
  `validate` for the login screen. Works against any ListenBrainz-compatible
  root, so a self-hosted instance is just a different `api_root`.
- `targets/csv_log.rs` — appends one RFC 4180 row per event to a local file.
  The header matches Pano Scrobbler's file format, so its converter reads ours.
  `is_pawse_log` is the guard the settings UI runs before pointing the log at a
  file the user already has.
- `targets/mod.rs` — the shared `ureq` agent config and response reader.

## Non-obvious behavior

- **A queue item belongs to several targets at once.** Success removes only the
  target that confirmed it; the item is deleted when its set empties. A scrobble
  that reached Last.fm is never resubmitted there because ListenBrainz was down,
  and the payload is stored once, not per service.
- **Errors are classified so a global failure never eats the queue.** `Transient`
  keeps the batch and backs the target off (60s, doubling, capped at 30 min,
  reset on the next success). `Auth` — which covers everything that is wrong with
  the account, the API key or the signature, not just an expired session — keeps
  the items, disables the target until the next `configure`, and raises
  `StatusEvent::AuthFailed`; re-logging in delivers the backlog. `Permanent` is
  reserved for a payload the server will never take (Last.fm 6/7): the batch is
  dropped, but the run stops and the target backs off, so a systemic `Permanent`
  can cost at most one batch per backoff window instead of the whole queue.
  `Unsupported` silently resolves the item for that target.
- **The worker owns the retry timer.** Its loop is `recv_timeout` against the
  earliest backoff deadline, so a queue filled while offline drains on its own,
  without waiting for the next track to end. A run is capped at `RUN_LIMIT`
  items per target and resumes `RESUME_SOON` later, so a large backlog cannot
  hold the thread. `configure` clears the backoff along with the disabled set:
  it only ever runs because the user changed a setting, and leaving a 30-minute
  deadline standing would make the fix look like it did nothing.
- **Nothing that must survive goes through the channel.** The queue lives behind
  an `Arc<Mutex<..>>` shared with the handle, so `scrobble` / `love` / `persist`
  write to disk on the caller's thread and only then poke the worker with a
  `Flush`. A `Msg` still in the channel dies with the process, and the worker can
  sit inside a 35s HTTP timeout, so anything queued as a message could be lost on
  quit. The cost is one small `write` + `fsync` + `rename` on the caller's thread
  (the UI thread) per track change or like — measured at 0.15 ms median / 0.33 ms
  worst on an APFS SSD for the usual near-empty file. Decoding is not a fair
  comparison: opening the decoder is a `Command` to the engine thread, so the
  only other work on the UI thread at a track change is a few indexed WAL reads
  with cached statements. That makes this the heaviest blocking call there, and
  it is still accepted: the alternative is the loss window above, and an fsync
  this small is sub-frame. It is unbounded under filesystem pressure, so a hitch
  on track change is the first thing to look at here. `NowPlaying` stays a
  message — it is ephemeral and losing it costs nothing.
- **Only the targets currently configured count as pending.** Items left over
  for a service the user switched off stay in the file (a re-login should still
  deliver them) but are excluded from `StatusEvent::Pending` and never wake the
  worker.
- **Now Playing is never queued** — it is ephemeral, sent to every target
  fire-and-forget. Loves *are* queued: they are idempotent and worth retrying.
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
- **The browser auth flow reports its own failures.** `get_session` returns
  `SessionError`, not a flat string, because the two common outcomes are things
  the user has to fix in the browser: `NotAuthorized` (Last.fm 14 — Confirm was
  pressed before approving the page) and `TokenExpired` (4/15 — the token went
  stale, or the page was closed and a new one is needed). The settings UI turns
  those into an instruction and keeps `AuthPhase::Awaiting` so its "open again"
  button, which just re-runs `get_token` + `auth_url`, stays reachable. Anything
  else keeps its server text.
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
- **No artificial delay between batches.** The queue write is `write` + `fsync`
  + `rename`, and nothing sleeps on the worker thread, so a flush is bounded by
  the HTTP timeouts alone.

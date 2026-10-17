# scrobble

Self-contained scrobbling engine: no GPUI, no pawse dependencies. It decides when a play counts, keeps every delivery owed to every destination in a store the app supplies, and sends from one worker thread with per-target retry. Every destination (Last.fm, Libre.fm, ListenBrainz, CSV log, media servers) is a `ScrobbleTarget`; the app drives the crate through `ScrobbleHandle`.

Not here: turning engine events into plays, the Settings tab, the loved-tracks import and the media-server target (`crates/pawse/src/doc.md`: `scrobble_bridge.rs`, `scrobble_settings.rs`, `scrobble_rules_settings.rs`, `scrobble_import.rs`, `server_scrobble.rs`); the SQLite tables behind `ScrobbleStore` (`crates/music_library/src/doc.md`).

## Files

- `lib.rs` — `Session`, `NowPlaying`, `Scrobble`; `creds` / `is_available` (Last.fm key and secret: runtime env first, then the value baked in at compile time); client constructors; `primary_artist` (the first credited artist, or all joined by ", "); re-exports.
- `build.rs` — bakes `LASTFM_API_KEY` / `LASTFM_API_SECRET` from the repo-root `.env` into the binary (same scheme as `discord/build.rs`).
- `accumulator.rs` — `PlayAccumulator` and `should_scrobble`.
- `target.rs` — `TargetId`, the `ScrobbleTarget` trait, `SubmitError`.
- `store.rs` — the `ScrobbleStore` trait and the types crossing it. The crate defines the contract and implements none of it (no rusqlite, no paths); `pawse::scrobble_store::LibraryScrobbleStore` does.
- `worker.rs` — `ScrobbleHandle`, the `scrobble` worker thread, backoff, `StatusEvent`.
- `targets/mod.rs` — the shared `ureq` agent (HTTP error statuses are returned, not raised) and the response reader.
- `targets/audioscrobbler.rs` — AudioScrobbler 2.0 client for Last.fm and Libre.fm (`Profile`), sign-in (`auth_url`, `get_session`, loopback callback listener) and `loved_tracks`.
- `targets/listenbrainz.rs` — ListenBrainz client: `submit-listens`, `validate` for the login screen, `loved_tracks` (paged `get-feedback`).
- `targets/csv_log.rs` — `CsvLog` and the `is_pawse_log` guard.
- `rewrite/` — `mod.rs` (`RewriteConfig`, `Rewriter`, `preview`), `presets.rs` (the built-in presets), `tests.rs`, `LICENSE-metadata-filter`.

## Plays, history, queue

- A `plays` row is the fact that something was listened to; a `play_deliveries` row is one target's debt against it. The payload is stored once, so a scrobble that reached Last.fm is never resubmitted because ListenBrainz was down. Delivery state is pending / sent / dropped with an attempt count and the last error, so "did not arrive" differs from "was refused, and why".
- `plays` and `loves` rows are never deleted — not on success, rejection or queue overflow. Only delivery rows change state.
- `persist` records any play; an unqualified one gets no delivery rows, and recording does not depend on a target being configured. `scrobble` is `persist` plus a `Flush` poke when the play qualified. A love is recorded only when some target accepts loves.
- `should_scrobble`: track at least 30 s, played at least half of it or 240 s. `PlayAccumulator` adds up wall-clock time between `on_play` and `on_pause` (the instant is passed in); seeks and position ticks do not count.
- The same listen committed twice (early at a pause, then at the end) must land on one row: the store keys plays by `(started_at, artist, title)` and a repeat raises `played_secs`. The bridge relies on it.
- Nothing that must survive goes through the channel. `scrobble` / `love` / `persist` write the store on the caller's thread and only then poke the worker, because a `Msg` still queued dies with the process and the worker can sit inside a long HTTP timeout. Each call is one transaction on the caller's thread — call them off the UI thread. `NowPlaying` stays a message: it is ephemeral.
- The queue is bounded, the history is not. `trim(CAP)` counts items, not delivery rows (something owed to two services counts once), and marks the oldest as dropped with `queue overflow`. It runs at launch and after every flush.
- Only configured targets count as pending. Deliveries left for a switched-off service stay pending (a re-login still delivers them) but are excluded from `pending_count` and never wake the worker. `pending_count` counts items.

## Delivery

- The worker owns the targets and the retry timer: `recv_timeout` against the earliest backoff deadline, so a queue filled offline drains without waiting for the next track. At start it trims and flushes the previous session's backlog before it reads its first message.
- The initial `Rewriter` is an argument of `ScrobbleHandle::spawn`, not a `set_rewriter` afterwards: a rewriter queued behind that startup flush would let the whole offline backlog go out unrewritten.
- A flush goes target by target: loves first, then scrobbles in batches of `max_batch`. A run is capped at `RUN_LIMIT` items per target and resumes `RESUME_SOON` later, so a big backlog cannot hold the thread.
- Errors are classified so a global failure never eats the queue:
  - `Transient` keeps the batch and backs the target off (60 s, doubling to 30 min, reset on that target's next success).
  - `Auth` keeps the items, disables the target until the next `configure`, and raises `StatusEvent::AuthFailed`. It covers everything wrong with the account, key or signature, not only an expired session; re-login delivers the backlog.
  - `Permanent` is for a payload the server will never take. The batch is dropped, `Rejected` is raised, the run stops and the target backs off — a systemic `Permanent` costs at most one batch per backoff window.
  - `Unsupported` settles the item as `dropped`, never `sent`: the row must say the delivery never happened.
- A delivery that cannot be recorded stops the run. If `settle` fails after the server took the batch, the worker backs off instead of re-reading the still-pending rows; looping would resubmit the same batch (duplicate CSV rows, abuse to Last.fm).
- `configure` clears the backoff along with the disabled set: it only runs because the user changed a setting, and a standing 30-minute deadline would make the fix look like it did nothing. It also updates the handle's target lists at once, so a play recorded right after sees the new set.
- `AuthFailed` and `Rejected` carry the server's own text, because the useful part is often an instruction ("verify your email"). `Transient` raises nothing.
- Now Playing is never queued: it goes to every non-disabled target, fire-and-forget; an `Auth` failure there disables the target too. Loves are queued (idempotent, worth retrying) and carry the moment they happened; the CSV log writes that time, not the flush time.

## Targets

- `ScrobbleTarget`: `accepts_scrobbles` / `accepts_loves` — a target that accepts none of a kind gets no delivery rows for it, not rows that would all be dropped. A batch settles as one outcome, so a target that can lose single items (the media-server one) uses `max_batch` 1. `now_playing_track` / `submit_tracks` / `love_track` carry the library track id and by default forward to the id-less methods, so only server targets ever see it. `rewrites()` is false for a target that matches by id.
- `TargetId::key()` strings are stored in the `target` column: `lastfm`, `librefm`, `listen_brainz`, `csv_log`, `server:<sources.id>`. Changing one orphans stored deliveries.

### Last.fm and Libre.fm

- One client, `Profile` picks endpoint, auth URL, key and secret. Libre.fm ships a constant key and secret (GNU FM does not validate them), so it works in builds without `LASTFM_API_KEY`. `with_loves(false)` makes `accepts_loves` false.
- Signing hashes the sorted `key+value` pairs, then the secret; `format` and `api_sig` are not in it. Params live in a `BTreeMap`, which gives the lexicographic order the API requires, indexed batch keys included (`artist[0]`, `albumArtist[0]`). `scrobble_params` / `now_playing_params` / `love_params` are free functions so the wire shape is tested without a session.
- Error codes: 6, 7 (invalid parameters) → `Permanent`; the account/key/signature family (4, 9, 10, 13, 14, 15, 26) → `Auth`, never a drop; any other code (rate limit included), an HTTP error without a JSON body and transport failures → `Transient`.
- Sign-in is the API's web flow. `auth_url` sends only `api_key` and `cb=http://127.0.0.1:<port>/pawse-auth?state=<state>`; the service mints the token and returns it on the redirect, and `get_session` exchanges it. Never mix in a token from `auth.getToken` (the desktop flow): the redirect then silently never comes back.
- The callback listener trusts only a request on `CALLBACK_PATH` whose `state` matches the random per-attempt one (two `RandomState` hashes — no `rand` dependency). Anything else is answered with the waiting page and ignored. A matching request without `token` is the service's denial redirect and fails at once (`SessionError::Denied`) rather than leaving the UI on "Connecting…" until `CALLBACK_TIMEOUT`.
- Callback socket details that look like noise: the response is closed gracefully (`shutdown(Write)`, drain until EOF or `CALLBACK_DRAIN_TIMEOUT`), because dropping the socket with unread request bytes resets the connection and Windows then discards the page already delivered (macOS looks fine either way); `read_request_line` reads to `\r\n\r\n` for the same reason. The accepted socket is set to blocking explicitly — the listener is non-blocking so the wait can poll `cancel`, and whether `accept` passes that on is platform-dependent. `wait_for_callback_async` waits on its own OS thread, not GPUI's pool, so a sign-in left dangling for up to `CALLBACK_TIMEOUT` holds no shared worker; `wait_for_callback` stays synchronous and testable.
- `Cancelled` / `TimedOut` come from the wait; `NotAuthorized` / `TokenExpired` (error codes 14 / 4, 15) only from `get_session`, which the ordinary path calls after the redirect already proved approval.

### ListenBrainz

- Works against any ListenBrainz-compatible root (self-hosted = another `api_root`; blank = default). Errors are classified by HTTP status: 401 / 403 → `Auth`; 400 / 404 / 413 → `Permanent`; the rest `Transient`.
- It cannot love: feedback needs a recording MBID and the library stores none, so `accepts_loves` is false and `love` is `Unsupported`. Album artist is not sent.

### CSV log

- One RFC 4180 row per event (`scrobble`, `love`, `unlove`), timestamps in UTC; the header is Pano Scrobbler's file format so its converter reads ours. No Now Playing (`Unsupported`): an append-only listening log should not carry "currently playing".
- Never overwrites and never adopts a stranger's file: create + append, header only when the file is missing or empty. `is_pawse_log` gates "use an existing file" — empty or the exact header passes, anything else is refused (it reads at most `HEADER.len() + 1` bytes) — so a mis-click on a document cannot get rows appended to it. A file damaged after it was chosen is not guarded against.

## Rewrite rules

Rules that rewrite artist / title / album / album artist on the way to a service: built-in presets switched on one by one (all off by default), then the user's find-and-replace rules. `RewriteConfig` lives in `settings.json` under `scrobble.rewrite`; unknown preset ids are ignored.

- Applied at delivery, not at recording. `plays` / `loves` keep the text the file has, so the history stays a fact about the file, the `(started_at, artist, title)` key stays stable, and a rule changed while items are queued applies to them too.
- `ScrobbleHandle::set_rewriter` swaps the compiled `Rewriter` without reconfiguring targets, so editing a rule never resets a backoff.
- Server targets are exempt (`rewrites()` is false): a server matches by song id. The CSV log is rewritten — it records what the services got.
- Order is fixed: presets in `PRESETS` order whatever order they were switched on, then custom rules in list order, so a custom rule sees text the presets already cleaned.
- Presets replace the first match, custom rules every match. Presets are ported from Web Scrobbler's `metadata-filter` (MIT, `rewrite/LICENSE-metadata-filter`), where each rule is a JS `replace` without `/g`; porting that exactly keeps their test cases valid. `single_ep` follows Pano Scrobbler's presets, whose GPL code is not copied.
- A non-regex custom rule is escaped and its replacement is literal (`NoExpand`), so `$1` is two characters. A regex rule's replacement goes through `brace_group_numbers` (`$<digits>` → `${<digits>}`, `$$` stays a dollar): the `regex` crate reads `$1abc` as a group named `1abc`, which is empty, and the text silently vanishes. Presets are written with `${1}`.
- Bracket rules stay inside one bracket (`[^()\[\]]*`): `metadata-filter`'s greedy `.*` also removed `(feat. X)` from `Song (feat. X) (2011 Remaster)`. Its dedicated `[Remastered] (Remastered Version)` rule is kept and runs first.
- Fields never become empty. A changed field is trimmed; an artist or title rewritten to nothing falls back to the original (every service rejects an empty one); an album or album artist rewritten to nothing is sent as absent. `preview` follows the same rule, so a rule that only empties an artist or title shows no matches instead of promising a change that is never sent.
- A broken custom rule is skipped, not fatal: `Rewriter::compile` returns the failed rules next to the working set. The UI refuses to save a pattern that does not compile, so this only guards a hand-edited `settings.json`.
- `Rewriter::artist_title` is what the likes import uses to find a library track under the name a love went out with.

## Decisions

Owner's calls; ask before changing.

- A love and its unlove use the rules of their own send, so a track loved with a preset on and unliked after it was switched off sends the unlove under the other name. Not fixing: it would need the sent name stored per love delivery (a migration) for a case that needs a rules change between two clicks.
- No typography preset (curly quotes, NBSP, zero-width, double spaces) — tried and dropped; the fixes are invisible to most people and a custom rule covers them. No YouTube title parsing: the library is files.
- The pending count is not shown in the UI — a number that reads zero almost always tells nothing; failures that matter arrive as notifications. It exists for the worker.

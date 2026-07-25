# discord

Self-contained Discord Rich Presence ("now playing"). No GPUI, no pawse
dependencies — a pure library the app drives through `DiscordHandle`. The
GPUI/event wiring lives in `pawse::discord_bridge`.

## Responsibilities

- Hold the Discord IPC socket and publish a `Presence` (title / artist / album /
  cover / progress) via `discord-rich-presence`, reconnecting when the Discord
  client is not running.
- Resolve album art to a public URL **by lookup only** (Apple iTunes Search,
  `artist album` → `artworkUrl100` upscaled to 512), cached on disk. Local files
  are never uploaded.

## Files

- `lib.rs` — `Presence` data type, `client_id()` (runtime `DISCORD_CLIENT_ID`
  env var, else the compile-time `option_env!` baked in by `build.rs` — same
  precedence as `scrobble::creds()`), re-export of `DiscordHandle`. Resolved once
  into a `OnceLock`, so `is_available()` is allocation-free and the id can't
  change under a running process.
- `build.rs` — reads the repo-root `.env` and injects `DISCORD_CLIENT_ID` via
  `cargo:rustc-env` so `option_env!` bakes it into the binary (mirrors
  `scrobble/build.rs`); reruns when `.env` or the env var changes.
- `art.rs` — `ArtCache`: `resolve(artist, album)` over blocking `ureq`, backed by
  a `"artist|album" -> url` JSON cache (empty string = negative cache, so a miss
  is never re-queried). `lookup`/`parse_artwork` return `Result<Option<_>, ()>`:
  only a **definitive** answer (HTTP responded, JSON parsed) is cached — a request
  failure (offline / 5xx / throttle) returns `Err` and is never persisted, so a
  transient outage can't poison an album forever. `parse_artwork` upscales
  `100x100bb` → `512x512bb` and rejects URLs over 254 bytes (Discord's limit).
- `ipc.rs` — `DiscordHandle` + a dedicated worker thread owning the IPC client
  and art cache, fed `Msg`s over a `flume` channel. Builds the `Listening`
  activity (details = title, state = artist, `large_image` = cover URL when
  found — otherwise omitted so Discord shows the application icon). `Presence`
  carries an absolute `started_at` (unix secs) computed by the bridge, so the
  progress bar is correct even when the worker applies the update later (rate
  limit / reconnect). `spawn` returns `None` if the worker thread can't start.

## Non-obvious behavior

- **Progress is anchored, not ticked.** Discord rate-limits `set_activity`, so
  the bridge sends a `Presence` only on real changes and the worker applies at
  most once per `MIN_INTERVAL`; the progress bar is conveyed as
  `start`/`end` timestamps the Discord client animates itself.
- **Latest-wins coalescing.** The worker keeps only the newest desired presence
  and dedupes identical ones (`Presence: Eq`); bursts collapse to one update.
- **Discord-not-running is normal.** `connect` failures back off (~15s) and
  retry while a presence is desired; the app is never blocked.
- **Paused ⇒ status is cleared** (like Spotify's own integration). Discord
  timestamps always animate client-side and can't be frozen, and a `Listening`
  activity with no timestamps shows a bogus ticking counter — so the bridge
  hides the presence while paused and re-publishes on resume. `Presence` is only
  ever sent while playing. Cost of that choice: the clear spends the rate-limit
  budget, so resuming after a long pause can leave the profile blank for up to
  `MIN_INTERVAL` before the track reappears. Inherent to hide-on-pause — both
  the clear and the re-publish are `set_activity` calls.
- **Unavailable ⇒ no UI.** `pawse::settings_view` omits the whole Discord group
  when `is_available()` is false, rather than showing a dead toggle. Release
  builds bake the id in, so this only affects local builds without a `.env`;
  there is no user-facing "not configured" string to translate.
- **Album required for art.** Art lookup needs a non-empty album; without one (or
  on a lookup miss) `large_image` is omitted and Discord shows the application
  icon (no `logo` asset needed, no network call).

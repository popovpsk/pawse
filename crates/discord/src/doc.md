# discord

Self-contained Discord Rich Presence ("Listening to"): no GPUI, no pawse dependencies. The app drives it through `DiscordHandle`; one worker thread holds the Discord IPC socket, resolves album art to a public URL and publishes a `Presence`.

Not here: when to publish, pause/seek handling, the Settings switch and quit handling (`crates/pawse/src/doc.md`: `discord_bridge.rs`, `settings_view.rs`).

## Files

- `lib.rs` — `Presence`, `client_id()` / `is_available()`: runtime `DISCORD_CLIENT_ID`, else the value baked in at compile time (same precedence as `scrobble::creds()`), resolved once.
- `build.rs` — reads the repo-root `.env` and bakes `DISCORD_CLIENT_ID` in (mirrors `scrobble/build.rs`).
- `art.rs` — `ArtCache::resolve(artist, album)`: iTunes lookup plus the on-disk cache.
- `ipc.rs` — `DiscordHandle` and the `discord-rpc` worker thread (`Msg::Set` / `Clear` / `Shutdown`). `spawn` returns `None` if the thread cannot start.

## Presence contract

- Progress is anchored, not ticked. `Presence` carries an absolute `started_at` (unix seconds) computed by the caller and a `duration`; the worker turns them into start/end timestamps that the Discord client animates itself, so the bar is right even when the update is applied late (rate limit, reconnect). No end timestamp when the duration is unknown.
- `Presence` is only sent while playing; pausing is `clear`. Discord timestamps always animate and cannot be frozen, and an activity with no timestamps shows a bogus ticking counter. Cost: the clear spends the rate-limit budget, so resuming after a long pause can leave the profile blank for up to `MIN_INTERVAL`.
- The worker keeps only the newest desired state and drops an identical `Presence` (`Eq`); bursts collapse to one update, applied at most once per `MIN_INTERVAL`.
- Discord not running is normal. A failed connect or a failed `set_activity` backs off (`RECONNECT_BACKOFF`) and retries while a presence is desired; with none desired nothing connects. The app is never blocked. `Shutdown` clears the activity and closes the socket.
- Activity layout: details = title, state = artist, small image = the application's `play` asset ("Playing").

## Album art

- By lookup only: Apple iTunes Search (`cover_search::itunes`) for `artist` + `album`, top result, requested at 512 px. Local files are never uploaded.
- Needs a non-empty artist and album. Without one, or on a miss, `large_image` is omitted and Discord shows the application icon (no network call, no `logo` asset). The lookup is blocking `ureq` on the worker thread.
- The cache is a JSON map `"artist|album"` → URL, written atomically. An empty string is a negative entry, so a miss is not re-queried. Only a definitive answer (HTTP responded, JSON parsed) is stored: a request failure returns `Err` and is never persisted, so a transient outage cannot poison an album forever.
- A URL over 254 bytes (Discord's limit) counts as a miss.

## Decisions

Owner's calls; ask before changing.

- Hide the presence while paused, like Spotify's own integration, rather than show a frozen or bogus timer.
- Art by lookup, never by uploading local files.
- When no client id is baked in (local builds without `.env`), the Settings row is omitted rather than shown dead; there is no "not configured" string to translate.

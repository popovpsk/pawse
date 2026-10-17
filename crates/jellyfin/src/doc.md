# jellyfin

A blocking client for the Jellyfin API. It knows nothing about the library database.

Not here: `Item` → `RemoteSong`, `Error` → `RemoteError` and the lyrics conversion
(`pawse/src/servers/doc.md`); agent, status classification, ranges, lenient JSON and `redact`
(`server_http/src/doc.md`).

## Files

- `lib.rs` — `authenticate`, `Client`, `Config`, `Item`, `Playlist`, `Lyrics` (+ `LyricsLine`,
  `LyricsCue`), `Error`.
- `tests.rs` — a `TcpListener` stub server.

## Auth and errors

- `authenticate` posts the password to `/Users/AuthenticateByName` once and returns a `Config`
  with the access token and user id; the password is never kept. Every request carries the
  token in the `Authorization: MediaBrowser …` header, never in a URL.
- Jellyfin ties tokens to the device id and may revoke a device's older tokens on a new login,
  so the caller passes a fresh `device_id` for every added server
  (`pawse/src/jellyfin_settings.rs`).
- `Error`: `Auth` (HTTP 401/403: wrong password or revoked token), `Transient` (transport,
  5xx), `NotFound` (a playlist or item the server will not show), `Server` (the rest, including
  a non-JSON reply = "not a Jellyfin server"). The UI tells "wrong password" from "server is
  down".
- `ping` checks `/System/Info/Public` (is this Jellyfin at all?), then `/Users/Me` (is the token
  still valid?).

## Listing

- `songs`: `/Items` with `IncludeItemTypes=Audio`, paged by `StartIndex` until
  `TotalRecordCount` or an empty page. A page that adds no new ids means the server ignores
  `StartIndex`: an error.
- Paging sorts by name, which can tie across pages, so a paged listing that ends with fewer
  unique ids than the reported total is redone as one unpaged request instead of applied short.
- Any failure fails the whole listing: a partial enumeration applied by the caller would retire
  every song past the failure. An item without an id is skipped and logged.
- `favorites` is the same listing with `Filters=IsFavorite`, so the import never walks the whole
  library; the main listing does not ask for user data.

## Playlists and favorites

- `playlists` lists the user's own, shared and open-access playlists, plus `.m3u` / `.pls` files
  found in music libraries (open access, no owner).
- The listing carries no owner, so `owns_playlist` asks `/Playlists/{id}/Users`: 10.9+ answers
  only the owner (403 otherwise); a 404 is a server without the route, where playlists were
  never shared, so it counts as owned.
- `playlist_song_ids` keeps the `Id` of `Audio` entries in order, repeats included (a playlist
  can mix in videos and episodes). A 404 or 403 is `NotFound`, so one playlist the server will
  not show skips that playlist instead of reading as a failed login.
- `set_favorite` uses `/UserFavoriteItems/{id}` (10.9+); a 404/405 means an older server, which
  gets `/Users/{userId}/FavoriteItems/{id}`. A 404 on both is `NotFound`, any other refusal a
  `Server` error carrying its status.

## Lyrics

- `lyrics` reads `/Audio/{id}/Lyrics` (10.9+); a 404 (no lyrics, or an older server) is
  `Ok(None)`.
- `Start` / `End` are ticks (100 ns). Cue `Position` / `EndPosition` are UTF-16 indexes into
  `Text` (C# string indexes); the crate hands them out raw and the adapter converts them to byte
  ranges.

## Covers and audio

- `Item::cover_key` is the album id when the album has a primary image, else the item's own id,
  so an album's tracks share one download. `cover_art` sends `maxWidth` / `maxHeight` =
  `max_size` so the server scales; capped at 32 MB.
- `fetch_range` reads `/Audio/{id}/stream?static=true`: the original file, never a transcode,
  and unlike `/Items/{id}/Download` it needs no download permission. A `200` means the range was
  ignored (`ranged: false`). Timeouts are `server_http::with_range`'s.

## Not implemented

Reporting plays back to the server, picking individual music libraries, Quick Connect, logging
the session out when a server is removed.

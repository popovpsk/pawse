# subsonic

A blocking client for the Subsonic API (Navidrome, Gonic, Airsonic, …) with the OpenSubsonic
extensions it relies on. It knows nothing about the library database.

Not here: `Song` → `RemoteSong` and `Error` → `RemoteError` (`pawse/src/servers/doc.md`);
what the app does with a placeholder cover (`pawse/src/remote_media/doc.md`); agent, status
classification, ranges, lenient JSON and `redact` (`server_http/src/doc.md`).

## Files

- `lib.rs` — `Client`, `Config`, `Song`, `Playlist`, `StructuredLyrics` (+ `LyricsLine`,
  `CueLine`, `Cue`, `Agent`), `Error`, `is_placeholder_cover`.
- `tests.rs` — a `TcpListener` stub server.

## Requests and errors

- Token auth: `t = md5(password + salt)`, a fresh salt per request. Error 41 (token auth
  unsupported, e.g. LDAP-backed) switches that `Client` to `p=enc:<hex>` for good; a second
  41 is `Auth`.
- `Error`: `Auth` (40, 50, HTTP 401/403), `Transient` (transport, 5xx), `NotFound` (70: the
  song or album is gone), `Server` (the rest, including "not a Subsonic server"). The UI
  tells "wrong password" from "server is down", so the split matters more than the message.
- The token or password travels in the URL: a request's transport error goes through `redact`.
- JSON is read through `into_reader` (no ureq 10 MB cap); covers are capped at 32 MB.
- An odd field becomes `None` instead of failing the page; a list entry that still does not
  parse (a song without an id) is skipped and logged.

## Listing

- `songs`: `search3` with an empty query, paged (the OpenSubsonic convention Navidrome
  follows). On `Server` / `NotFound` or an empty result the album walk (`getAlbumList2` +
  `getAlbum`) runs instead; `Auth` and `Transient` never fall back.
- Paging stops at an empty page, not a short one (servers clamp the page size). A page that
  adds no new ids means the server ignores the offset: an error.
- Any failure mid-listing fails the whole listing: a partial enumeration applied by the
  caller would retire every song past the failure.
- OpenSubsonic sends the full genre list as `genres: [{name}]` and in `genre` only the
  primary one (Navidrome: the first). `Song` keeps both; the adapter prefers the list.

## Reporting back and playlists

- `scrobble` sends `submission=true` with `time` in ms (when the play started);
  `now_playing` is `submission=false`. Streaming goes through `download`, which Navidrome
  does not count as a play, so without `scrobble` play counts never move.
- `is_mine`: owner is this user (or absent) and not `readonly`. Navidrome marks smart, other
  users' and file-synced playlists `readonly`, so this separates hand-made ones even for the
  admin who owns the imported files.
- `playlist_song_ids` keeps order and repeats and drops `isVideo`; a vanished playlist is
  `NotFound`.

## Lyrics

- `lyrics` is `getLyricsBySongId` with `enhanced=true` (songLyrics v2: word timing, backing
  agents, translations; a v1 server ignores it and sends lines only), returned as sent.
- No capability check first: a server without the method answers `Server`, which the app
  reads as "no lyrics".

## Covers

- `cover_art(id, max_size)` sends `size` so the server scales: without it Navidrome sends the
  original embedded picture for every track.
- Navidrome serves its placeholder (a blue vinyl) as HTTP 200 when a cover cannot be served
  right now; for a size it has not scaled yet it can be the placeholder while a smaller size
  still works, and the next request usually succeeds. `cover_art` returns the picture as is;
  the caller decides.
- The placeholder is sent unscaled whatever size was asked, so `is_placeholder_cover` matches
  it by length and MD5 against the three files Navidrome has shipped; a new one reads as real
  art until its hash is added.
- `Cache-Control: no-store` (set on the placeholder since 0.64) is not the signal: a proxy
  can send it for real covers too.

## Audio

- `fetch_range` asks `download` (the original file, never a transcode); a `200` means the
  range was ignored (`ranged: false`). Timeouts are `server_http::with_range`'s.

# subsonic

A blocking client for the Subsonic API (Navidrome, Gonic, Airsonic, …). It knows
nothing about the library database; `pawse::remote_sync` turns what it returns
into `music_library::RemoteSong`s. Blocking `ureq` on the caller's thread, like
`scrobble` and `lyrics` — there is no async runtime outside `pawse_remote`.

## Files

- `lib.rs` — `Client`, `Config`, `Song`, `Error`. Ranges, status
  classification, lenient JSON and `redact` come from `server_http`.
- `tests.rs` — a `TcpListener` stub server; no new dependencies.

## Behaviour worth knowing

- **Auth.** Token auth (`t = md5(password + salt)`, a fresh salt per request).
  A server that answers error 41 (token auth unsupported, e.g. LDAP-backed)
  gets `p=enc:<hex>` from then on, for the life of that `Client`.
- **Errors.** `Auth` (40, 50, HTTP 401/403), `Transient` (transport failures and
  HTTP 5xx), `NotFound` (70: the song or album is gone), `Server` (everything
  else, including "this is not a Subsonic server"). Callers show different things for "wrong password" and "server is
  down", so the split matters more than the message.
- **Listing all songs.** `search3` with an empty query and paging (the
  OpenSubsonic convention Navidrome and most servers follow); if it errors or
  returns nothing, the album walk (`getAlbumList2` + `getAlbum`) runs instead.
  Paging continues until an empty page (a server may return fewer than asked
  for); a page that adds no new ids means the server ignores the offset, which
  is an error rather than a complete listing. Any failure mid-listing fails the
  whole listing — the caller must never apply a partial enumeration, or every
  song past the failure would be retired.
- **Genres.** OpenSubsonic servers send the full list as `genres: [{name}]`
  and, in `genre`, only what they pick as the primary one (Navidrome: the
  first). `Song` keeps both; the app's adapter prefers the list and falls back
  to the single string for servers that have no list.
- **Lenient fields.** Numbers may come as floats or strings, lists as something
  else; an odd field becomes `None` instead of failing the page (and with it the
  whole server). A list entry that still does not parse (a song without an id)
  is skipped and logged, like Jellyfin's items.
- **No secrets in errors.** Transport error texts can contain the request URL,
  which carries the token or the encoded password; `redact` strips query
  strings before any message leaves the crate.
- **Ids** can be strings or numbers depending on the server; both deserialize
  to `String`.
- **Reporting back.** `scrobble` sends `submission=true` with `time` in
  milliseconds (when the play started); `now_playing` is the same call with
  `submission=false`, which is what Navidrome shows as "now playing".
  `set_starred` is `star` / `unstar`. One song per request: the caller settles
  each play on its own. Streaming goes through `download`, which Navidrome does
  not count as a play, so without `scrobble` a server's play counts never move.
- **Playlists.** `playlists` is `getPlaylists` (every playlist the server shows
  this user: their own, other users' public ones — every user's, private too, when
  the user is a Navidrome admin — and on Navidrome the ones it imported from
  `.m3u`/`.nsp` files); `playlist_song_ids` is `getPlaylist`'s
  `entry` ids in order, repeats kept, `isVideo` entries dropped. `is_mine` is
  "owner is this user (or not given) and not `readonly`": OpenSubsonic's
  `readonly` is how Navidrome marks smart playlists, other users' playlists and
  file-synced ones, so it tells a hand-made playlist from the rest even for the
  admin who owns the imported files. A playlist that is gone is `NotFound`.
- **Lyrics.** `lyrics` is OpenSubsonic `getLyricsBySongId` (the `songLyrics`
  extension) with `enhanced=true`, returned as the server sends it: every
  `structuredLyrics` entry with its `kind`, `offset`, `line`s, `cueLine`s and
  `agents`. `enhanced` asks for songLyrics v2 (word timing, backing-vocal
  agents, translations); a v1 server ignores the parameter and sends lines
  only. No extension check first: a server without the method answers with an
  error (`Server`), which the app reads as "no lyrics". Navidrome parses TTML,
  Enhanced LRC, SRT and the rest itself, so the client only ever sees this
  structure.
- **Bodies.** JSON is read through `into_reader` (no ureq 10 MB cap). Covers are
  capped at 32 MB.
- **Covers** are asked for with `size` (the caller's `max_size`; the sync passes its
  largest thumbnail, 320), so the server scales them: Navidrome otherwise sends the
  original embedded picture for every track — ~470 MB for a 1.7k-song test library
  against ~31 MB scaled. Navidrome ignores the `_<timestamp>` suffix of a cover id
  when looking it up, so an older id still returns the current picture.
- **Audio.** `fetch_range` asks `download` (the original file, never a
  transcode) for a byte range and reports where the body starts and the file's
  total size from `Content-Range`. A `200` reply means the server ignored the
  range and sends the whole file (`ranged: false`). Range requests use a 15 s
  response and body timeout instead of the listing's long ones, so a stalled
  connection turns into a retry quickly; `media_stream` keeps what arrived.

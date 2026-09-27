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
  HTTP 5xx), `Server` (everything else, including "this is not a Subsonic
  server"). Callers show different things for "wrong password" and "server is
  down", so the split matters more than the message.
- **Listing all songs.** `search3` with an empty query and paging (the
  OpenSubsonic convention Navidrome and most servers follow); if it errors or
  returns nothing, the album walk (`getAlbumList2` + `getAlbum`) runs instead.
  Paging continues until an empty page (a server may return fewer than asked
  for); a page that adds no new ids means the server ignores the offset, which
  is an error rather than a complete listing. Any failure mid-listing fails the
  whole listing — the caller must never apply a partial enumeration, or every
  song past the failure would be retired.
- **Lenient fields.** Numbers may come as floats or strings, lists as something
  else; an odd field becomes `None` instead of failing the page (and with it the
  whole server). A list entry that still does not parse (a song without an id)
  is skipped and logged, like Jellyfin's items.
- **No secrets in errors.** Transport error texts can contain the request URL,
  which carries the token or the encoded password; `redact` strips query
  strings before any message leaves the crate.
- **Ids** can be strings or numbers depending on the server; both deserialize
  to `String`.
- **Bodies.** JSON is read through `into_reader` (no ureq 10 MB cap). Covers are
  capped at 32 MB.
- **Audio.** `fetch_range` asks `download` (the original file, never a
  transcode) for a byte range and reports where the body starts and the file's
  total size from `Content-Range`. A `200` reply means the server ignored the
  range and sends the whole file (`ranged: false`). Range requests use a 15 s
  response and body timeout instead of the listing's long ones, so a stalled
  connection turns into a retry quickly; `media_stream` keeps what arrived.

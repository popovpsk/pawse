# jellyfin

A blocking client for the Jellyfin API. Like `subsonic`, it knows nothing about
the library database; `pawse::remote_sync` turns its `Item`s into
`music_library::RemoteSong`s. Blocking `ureq` on the caller's thread.

## Files

- `lib.rs` — `authenticate`, `Client`, `Config`, `Item`, `Error`. Ranges,
  status classification, lenient JSON and `redact` come from `server_http`.
- `tests.rs` — a `TcpListener` stub server; no new dependencies.

## Behaviour worth knowing

- **Auth.** `authenticate` posts the username and password to
  `/Users/AuthenticateByName` once and returns a `Config` with the access token
  and user id; the password is never kept. Every request carries
  `Authorization: MediaBrowser Client=…, Device=…, DeviceId=…, Version=…, Token=…`
  — the token lives in a header, never in a URL. Jellyfin ties tokens to the
  device id and may revoke a device's older tokens on a new login, so `pawse`
  mints a fresh device id per added server (`jellyfin_settings::new_device_id`).
  The session shows up in the Jellyfin dashboard as a "Pawse" device.
- **Errors.** Same split as `subsonic`: `Auth` (HTTP 401/403 — a wrong password
  or a revoked token), `Transient` (transport failures and HTTP 5xx), `Server`
  (everything else, including "this is not a Jellyfin server").
- **Ping** checks `/System/Info/Public` (is this Jellyfin at all?) and then
  `/Users/Me` (is the token still valid?).
- **Listing all songs.** `/Items?userId=…&IncludeItemTypes=Audio&Recursive=true`
  in pages of 500 until `TotalRecordCount` or an empty page. A page that adds no
  new ids means the server ignores `StartIndex`, which is an error. Paging sorts
  by name, which can tie, so a paged listing that ends up with fewer unique ids
  than the reported total is redone as one unpaged request rather than applied
  short. Any failure fails the whole listing — the caller must never apply a
  partial enumeration, or every song past the failure would be retired. An item
  without an id is skipped.
- **Favorites** are the same listing with `Filters=IsFavorite` — only the
  favorites come back, so the import never walks the whole library. The main
  listing does not ask for user data.
- **Lenient fields.** Numbers may come as floats or strings, lists and objects as
  something else; an odd field becomes `None`/empty instead of failing the page.
- **Covers.** `Item::cover_key` is the album id when the album has a primary
  image, otherwise the item's own id — so an album's tracks share one download.
  Fetched from `/Items/{id}/Images/Primary?maxWidth=1200`, capped at 32 MB.
- **Audio.** `fetch_range` reads `/Audio/{id}/stream?static=true`: the original
  file, never a transcode, and unlike `/Items/{id}/Download` it does not need the
  download permission. A byte range is asked for and `Content-Range` gives where
  the body starts and the file's total size; a `200` reply means the server
  ignored the range (`ranged: false`). Range requests use a 15 s response and
  30 s body timeout, as in `subsonic`.
- **No secrets in errors.** `server_http::redact` strips query strings from
  transport error texts anyway.
- **Not done:** reporting plays or favorites back to the server, picking
  individual music libraries, Quick Connect, logging the session out when a
  server is removed.

# remote_media

Getting the bytes of a track that is not a local file. `RemoteMedia` is the facade (used
through `playback_opener::LibraryBackend`, which `crate::cast` shares for renderers, and by
the engine's resolver, gapless prefetch, the cache settings, `similar_tracks`, `cache_fill`,
`album_export`); behind it each source id maps to a `SourceMedia`, and every source writes
into the one on-disk cache.

Not here: protocols (`servers/doc.md`); the download machinery (`media_stream/src/doc.md`);
opening a track for playback, saving or moving albums, the cache group in Settings
(`pawse/src/doc.md`).

## Files

- `mod.rs` — `RemoteMedia`, the `SourceMedia` trait, `PendingStream`, `StreamControl`.
- `cache.rs` — `CacheStore`: layout, lookup, trimming, clearing.
- `http.rs` — `HttpMedia`, the `SourceMedia` for every kind (`set_servers` builds one per
  configured server): a `media_stream` download fed by `ServerClient::fetch_range`
  (`ServerFetch`), `cover` = `ServerClient::cover_art`.
- `tests.rs`.

## Cache

- Path: `<cache>/pawse/media/<source_id>/<key>-<digest>.<ext>`. The key is sanitized and cut
  to 96 characters (a DLNA key is a URL path); the digest of the whole key keeps keys that
  sanitize alike apart. The facade decides the path: a source gets `dest` and must leave the
  finished file there, which keeps one cache, one limit and one lookup for every kind.
- A torrent's finished file lands in this cache too; the torrent engine keeps only its own
  piece data and deletes it itself.
- LRU by mtime. `cached` and the engine's resolver refresh the mtime (a lookup means the
  track is being played); `peek_cached` does not — `similar_tracks` walks the whole library
  and must not reorder what gets evicted.
- Trimming runs when a download completes and never removes that file; lowering the limit
  trims at once on the background executor. Trim and `clear` skip a `.partial` younger than
  a day (a download in progress); an older one is an ordinary file to them.
- `<cache>/pawse/subsonic` is renamed to `media` once, when `media` does not exist.

## Sources

- Locators are parsed once with `music_library::remote::location`: a file path, a server
  track or an invalid locator, which is never treated as a local path.
- A source that is not configured (removed server, stale locator) is an error on open and
  `None` from `ping`; a file already in the cache still plays.
- `can_stream` decides by the locator's suffix; formats that cannot stream go through
  `resolve`, a whole download into the cache. `prefetch` (gapless) keeps its download alive
  in a single slot: the next prefetch replaces it, and dropping the last interest cancels an
  unfinished download.
- `ServerFetch` retries only `RemoteError::Unreachable`; a wrong password or a server error
  fails the download at once. For a torrent a range blocks until its pieces arrive
  (`torrent/src/doc.md`).
- `StreamControl` (abort and failure reason of an opening stream) keeps the handle from being
  tied to `media_stream`: a source that streams from its own engine can supply one.
- `lyrics(locator)` → `SourceMedia::lyrics` → `ServerClient::lyrics`; a locator that is not
  a server track, or whose source is not configured, is `Ok(None)`.
- `cover(source_id, key, max_size)` → `SourceMedia::cover`. `HttpMedia::cover` asks
  `is_placeholder_cover` and turns the server's placeholder picture (HTTP 200, see
  `subsonic/src/doc.md`) into `NotFound`, so the caller falls back instead of drawing it.
  `remote_sync` does not ask: a key the server has no picture for keeps the server's
  placeholder as its thumbnail, as in the server's own UI.

## For the sound analysis

`similar_tracks` reads server tracks without touching what was cached for listening:

- `download_to(reference, dest, abandoned)` is `SourceMedia::fetch_whole` into a path the
  caller picks (its scratch folder). It still goes through the cache's `Downloads` registry
  and its trim-on-complete, which only looks at the cache folder. A different destination is
  a different download, so `is_downloading(reference)` tells the analysis that playback is
  already fetching the file into the cache; it then joins that one through `resolve`.
- `read_ranges(reference, stop)` opens a `media_stream::RangeReader` on
  `SourceMedia::range_fetch` (the same `ServerFetch`; the default `None` is
  `RangeError::Unsupported`). Nothing reaches the cache.
- `is_torrent(source_id)` comes from the kinds `set_servers` was given (sources set with
  `set_sources`, as in tests, are never torrents).

## Decisions

Owner's calls; ask before changing.

- Covers bypass the cache: every cover asked through `RemoteMedia::cover` is a request, as a
  local cover is a disk read.

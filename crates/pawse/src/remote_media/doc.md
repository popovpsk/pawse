# remote_media

Getting the bytes of a track that is not a local file. `RemoteMedia` is the
facade the app uses (`Services::start_track`, the engine's resolver, gapless
prefetch, the cache settings); behind it each source id maps to a
`SourceMedia`.

## Files

- `mod.rs` — `RemoteMedia`, the `SourceMedia` trait, `PendingStream`,
  `StreamControl` (abort and failure reason of an opening stream).
- `cache.rs` — `CacheStore`: the one on-disk cache every source writes into
  (`<cache>/pawse/media/<source_id>/<key>-<digest>.<ext>`), LRU by mtime, the
  size limit, clearing, and the move of the old `<cache>/pawse/subsonic` dir.
- `http.rs` — `HttpMedia`, the `SourceMedia` for servers that serve byte ranges
  (Subsonic, Jellyfin; DLNA would fit too): a `media_stream` download fed by the
  source's `ServerClient::fetch_range` (`ServerFetch`).
- `tests.rs`.

## Behaviour worth knowing

- **Locators are parsed once**, with `music_library::remote::location`: a file
  path, a server track, or an invalid locator. An invalid one is never treated
  as a local path.
- **The cache path is decided by the facade**, not by the source: a source gets
  `dest` and must leave the finished file there. That keeps one cache, one
  limit and one lookup for every kind of source.
- **A source that is not configured** (removed server, stale locator) is an
  error on open and `None` from `ping`; a file already in the cache still plays.
- **Retries.** `ServerFetch` retries only `RemoteError::Unreachable`; a wrong
  password or a server error fails the download at once.
- **`StreamControl`** exists so an opening stream's abort handle is not tied to
  `media_stream`: a source that streams from its own engine can supply one.

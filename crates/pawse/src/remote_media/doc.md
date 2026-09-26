# remote_media

Getting the bytes of a track that is not a local file. `RemoteMedia` is the
facade the app uses (`crate::playback_opener` through its `LibraryBackend`, the engine's resolver, gapless
prefetch, the cache settings); behind it each source id maps to a
`SourceMedia`.

## Files

- `mod.rs` — `RemoteMedia`, the `SourceMedia` trait, `PendingStream`,
  `StreamControl` (abort and failure reason of an opening stream).
- `cache.rs` — `CacheStore`: the one on-disk cache every source writes into
  (`<cache>/pawse/media/<source_id>/<key>-<digest>.<ext>`), LRU by mtime, the
  size limit, clearing, and the move of the old `<cache>/pawse/subsonic` dir.
- `http.rs` — `HttpMedia`, the `SourceMedia` for every source that serves byte
  ranges (Subsonic, Jellyfin, torrents; DLNA would fit too): a `media_stream`
  download fed by the source's `ServerClient::fetch_range` (`ServerFetch`). For
  a torrent a range blocks until its pieces arrive — as long as the torrent
  keeps receiving bytes, up to 5 minutes; without any, 20 s for opening a range
  and its first byte and 30 s later in the body, then it is `Unreachable` and
  retried, and the finished file lands in this
  same cache — the torrent engine keeps only its own piece data, which it
  deletes itself.
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

## Saving albums to the cache

`crate::cache_fill` (outside this module) saves an album's or artist's network
tracks ahead of listening: it lists the tracks' remote files not in the cache
yet (a cue image once), takes their sizes from the library
(`remote_file_sizes`), and fetches them one at a time with `resolve` — the
same path playback uses, so one download at a time never starves the track
that is playing. If everything is larger than the cache limit, a dialog says so
and only the files that fit, in album order, are saved; otherwise the LRU makes
room silently, as it does while listening. Saved tracks are ordinary cache
entries and can be evicted later — there is no pinning. The largest limit
choice, "Unlimited", is 100 TB (`UNLIMITED_CACHE_GB`), so no code path needs a
special case for it.

## Moving an album to a music folder

`crate::album_export` (outside this module) moves an album's network tracks out
of the cache into a configured music folder: one folder is used directly,
several ask which one, and without any music folder the button is hidden:
a file moved outside the library folders would leave the cache and never be
scanned, so the album would vanish with its likes. Only the album header has the button — per artist it
would depend on the artist grouping. Files are named from the tags
(`Album Artist/Year - Album/NN - Title.ext`, `D-NN - …` for several discs) —
the cache names are digests and Subsonic's own paths are not stored. Each file
goes through `resolve` (download if needed) and is renamed out of the cache
right away (copy + delete across volumes), so the LRU never evicts a file of
the album before it is moved and the cache does not grow. A cue image (a
torrent's) gets a generated `.cue` from its pieces' offsets, in `CDn/` for
several discs; `cue_time` rounds milliseconds back to the exact frames the scan
derived them from, so the local tracks start at the same offsets. A target that
exists with the file's size counts as done, so a stopped move can be restarted;
a taken name gets ` (n)`. `cover.jpg` (the stored large thumbnail) is written
only when the folder has no image. Then a rescan runs, and adoption does the
rest: the moved file has the server binding's size (and offset), so it joins
the same item by the `file` tier — likes, playlists and history stay, and the
local copy plays first. The plan (sizes, artists,
the cover blob) is built on the background executor, not in the click handler.
The album header re-reads its tracks on every `CatalogChanged` (album ids are
stable across rescans), so the buttons follow what the album holds now; right
after a move, `AlbumExport` hides both buttons for that album until the next
`CatalogChanged` — the rescan that makes its tracks local. Cancelling a move
ends quietly, without an error toast. The rescan is a full one: the catalog has
no per-folder refresh yet.

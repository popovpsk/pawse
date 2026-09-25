# servers

Media-server protocols (and torrents) behind one interface. Everything above this module —
sync, playback, settings rows — talks to `ServerClient` and `ServerKind`, never
to `subsonic::` or `jellyfin::` directly.

## Files

- `mod.rs` — `ServerKind` (the closed list of protocols, with its stored name and
  title), `RemoteConfig` (a server's saved connection, one variant per kind),
  `RemoteServer` (a configured server: uri, name, config), `RemoteError`,
  the `ServerClient` trait, and the tag cleanups every adapter shares
  (`real_artist`, `real_album`, `real_track_number`).
- `subsonic.rs` — the Subsonic adapter: `subsonic::Song` → `RemoteSong`,
  `subsonic::Error` → `RemoteError`.
- `jellyfin.rs` — the Jellyfin adapter: `jellyfin::Item` → `RemoteSong`, error
  mapping, and `authenticate` (log in once, get a token).
- `torrent/mod.rs` — the torrent adapter over the `torrent` crate: the one
  process-wide `torrent::Engine` (`configure` at startup from
  `torrent_settings::configure_engine`, created by the first `engine()` call —
  never on the UI thread, never without a torrent), `set_upload`, `state_lock`,
  the index retry backoff, `Config` (just the info hash), error mapping, and
  `fetch_range` = `Engine::read` of one file of the torrent.
- `torrent/index.rs` — turning a torrent into `RemoteSong`s without
  downloading it: the probe plan, the tag scan over a throwaway tree, the saved
  index and covers.
- `torrent/index/tests.rs` — the plan, the metadata bounds, and an end-to-end
  index of a torrent served by a loopback seeder.

## Adding a protocol

1. A client crate (blocking, using `server_http`).
2. A `ServerKind` variant. Every `match` on the kind is exhaustive — settings
   removal, button ids, `RemoteConfig::kind` — so the compiler lists what is left.
3. A `RemoteConfig` variant and an adapter implementing `ServerClient`.
4. If bytes do not come as HTTP ranges, a `remote_media::SourceMedia`
   implementation instead of `HttpMedia`.

## Behaviour worth knowing

- **Servers are keyed `kind:uri`** (`source_key`), in `remote_sync::source_ids`,
  the sync queue, `LibraryEvent`s and the settings rows, so two kinds at the
  same address never share state.
- **`RemoteSong` is the library's model, not a protocol's.** Adapters convert
  units on the way in: duration to ms, bitrate to kbit/s (`bitrate_kbps`;
  Jellyfin sends bit/s), size in bytes, `suffix` = the file's extension (the
  decoder picks a backend by it). `artist` is the first credited artist;
  `artist_aliases` are other names the same recording's artist is credited
  under, used only for matching (Subsonic: the joined display name and the
  other credited artists; Jellyfin: the other entries of its split `Artists`).
- **Jellyfin's extension** comes from the file path, else from the container
  list, preferring a known audio extension (`mov,mp4,m4a,…` → `m4a`).
- **Placeholders** (`[Unknown Artist]`, `[Unknown Album]`) become empty, and a
  track number above 999 is dropped — Navidrome takes one from a leading number
  in an untagged file name.

## Torrents

A torrent is a source like a server: `ServerKind::Torrent`, `uri = btih:<hash>`,
the same settings rows, sync, statuses and playback path. What differs:

- **Nothing to log into.** `resolve` (a `.torrent` file or a magnet link, which
  needs DHT) stores the `.torrent` under the engine's state dir; `ping` only
  checks it is still there. A torrent without peers is not offline until a
  sync or a read fails: then it is `Unreachable`, like a server that is down.
- **Songs are indexed once per torrent**, since its content never changes. The
  plan takes, per audio file, the head (1 MiB) — plus the tail (256 KiB) for
  formats that keep tags or length at the end (mp3, ogg/opus, m4a/aac, ape, wv,
  wma, dsf) — every `.cue`/`.lrc` up to 1 MiB, and one cover per album folder,
  chosen from the file list with `music_indexer`'s own ranking (album folder,
  its artwork folders, then the parent) and at most 8 MiB. Booklet scans are
  never fetched. Then, in up to six more rounds, what the tag reader will need
  beyond that is fetched: FLAC metadata blocks (each block's content except
  PADDING, the next header when it lies outside what arrived), a whole ID3v2
  tag, and an MP4's `moov` found by walking the top-level atoms (it is often at
  the end and larger than the tail). Nothing past 16 MiB is fetched. Only bytes
  known to have arrived are parsed, never the zeros of a sparse file. Ogg/Opus
  art larger than the head is not chased.
- **The scan is the local one.** Hard links to just the planned files go into
  `<work>/<hash>.view` (where hard links fail, a sparse copy of just the
  fetched ranges), and `music_indexer`
  runs over it — tags, cue expansion, external covers — so a torrent track
  reads exactly like the same file in a music folder. Only planned files are
  linked, because every other file of the probe tree exists at full length
  filled with zeros and would otherwise be taken for a cover. The view and the
  probe tree are deleted right after.
- **Keys** are file indexes in the torrent; cue tracks share their image's key
  and carry `start_offset_ms`. The index (`<hash>.index.json`, versioned by
  `INDEX_VERSION`) and the chosen covers (`<hash>.covers/<cover hash>`) live next
  to the `.torrent`; removing the source calls `Engine::forget`, which deletes
  everything named after the hash there.
- **Covers** go through the normal `fetch_covers`: `cover_art(key)` reads the
  saved large thumbnail.
- **Each torrent syncs on its own thread** (`LibraryService::sync_torrent`,
  one at a time per torrent), outside the servers' one-by-one queue: a slow or
  dead swarm must not hold Subsonic/Jellyfin or the other torrents back. A probe
  gives up after 90 s without a byte. Any failed index makes `ping` fail for
  the next 10 minutes with the failure's reason, so the offline watcher does
  not re-download heads every minute. The sync's "running" marks are drop
  guards (`Claim`), so a panic in an index does not leave a torrent syncing.
- **Peers** (`connected/known`) show next to a torrent while it is loaded —
  indexing or playing — polled every 2 s by `LibrarySources`.
- **`state_lock`** serialises what writes or deletes a torrent's files in the
  state dir: `resolve` from the settings, `forget` on removal, saving an index
  (which also checks the `.torrent` still exists, so a sync finishing after a
  removal leaves no orphans).
- **Idle torrents** are unloaded 15 minutes after their last read
  (`torrent_settings::IDLE_UNLOAD`). A track downloads into the media cache
  much faster than it plays, so the clock usually starts early in a track;
  15 minutes keeps a torrent loaded across long tracks, and prefetching the
  next track 30 s before the end reloads it if not.
- **Stars** do not exist; the button is hidden for torrents.

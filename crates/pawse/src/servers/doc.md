# servers

Media-server protocols and torrents behind one interface. Sync, playback and the
settings rows talk to `ServerClient` / `ServerKind`; the protocol crates appear outside
only as config types, DLNA discovery and torrent `resolve` in the settings.

Not here: the protocols (`subsonic/src/doc.md`, `jellyfin/src/doc.md`, `dlna/src/doc.md`,
`torrent/src/doc.md`); the sync, playlist import and connect forms (`pawse/src/doc.md`);
the bytes of a track (`remote_media/doc.md`).

## Files

- `mod.rs` — `ServerKind`, `RemoteConfig` (one variant per kind), `RemoteServer`,
  `RemoteError`, the `ServerClient` trait, `source_key`, and what adapters share:
  `real_artist` / `real_album` / `real_track_number`, `joined_genres`, `server_lyrics`.
- `subsonic.rs`, `jellyfin.rs`, `dlna.rs` — adapters: crate item → `RemoteSong`, crate
  error → `RemoteError`. Jellyfin adds `authenticate` (log in once, keep the token), DLNA
  `describe` (an address typed by the user → `dlna::Device`).
- `torrent/mod.rs` — `TorrentHost`, the adapter over the `torrent` crate, error mapping.
- `torrent/index.rs` — a torrent → `RemoteSong`s without downloading it: probe plan, tag
  scan, saved index and covers. `index/tests.rs` covers the plan, the metadata bounds and an
  end-to-end index of a torrent served by a loopback seeder.

## Adding a protocol

1. A blocking client crate on `server_http`.
2. A `ServerKind` variant; every `match` on the kind is exhaustive, so the compiler lists
   what is left.
3. A `RemoteConfig` variant and an adapter implementing `ServerClient`.
4. If bytes do not come as HTTP ranges, a `remote_media::SourceMedia` instead of
   `HttpMedia` (and `RemoteMedia::set_servers` must build it).

## Kinds and the client

- What differs between kinds is asked of `ServerKind` (`manual_sync`, `imports_favorites`,
  `imports_playlists`, `reports_plays` / `sends_favorites` for what `server_scrobble` may
  send back, `syncs_alone`, `titled_by_name`, `has_peers`), never compared: code outside
  does not test `kind == Torrent`; the per-kind `match`es left are exhaustive maps to a
  value (icon, element ids).
- `as_str` is the stored name: the `sources` table kind, the `kind:uri` key (`source_key`
  in the sync queue, `LibraryEvent`s and settings rows, so two kinds at one address never
  share state) and `<kind>-cover://` locators. Do not rename.
- A method a kind does not support keeps the trait default (an error, or nothing for
  `lyrics`, `forget`, `peers`, `moved`, `is_placeholder_cover`); `ServerKind` must say which
  kinds do support it. `RemoteConfig::web_url` is what "Open in browser" opens (none for DLNA
  and torrents).
- `cover_art(key, max_size)`: servers that can scale get `max_size`; DLNA and torrents
  ignore it. The sync calls it from 4 threads at once, so a client must be safe to share.
- `RemoteError`: `Unreachable` is the only one downloads retry. `NotFound` is a song the
  server says is gone: shown and fetched like `Other`, but a report back (scrobble, like)
  drops it quietly. `NotSynced` / `Syncing` never come from a client: `LibraryService`'s
  favorites and playlist imports return them (no enabled source row; mid-sync);
  `library_sources::describe_error` localizes them and every other `match` treats them as
  a plain failure.
- `moved()` (DLNA only) is a config to save because the server was found elsewhere: a sync
  sends it as `LibraryEvent::RemoteMoved`, `remote_settings::server_moved` stores it.

## RemoteSong

The library's model, not a protocol's; adapters convert on the way in.

- Units: duration ms, bitrate kbit/s (`bitrate_kbps`; Jellyfin sends bit/s), size bytes;
  `suffix` is the file's extension, since the decoder picks a backend by it (Jellyfin: from
  the file path, else the container list preferring a known audio extension, `mov,mp4,m4a,…`
  → `m4a`).
- `artist` is the first credited artist; `artist_aliases` are other names the recording is
  credited under, used only for matching (Subsonic adds the joined display name).
- `[Unknown Artist]` / `[Unknown Album]` become empty and a track number above 999 is
  dropped — Navidrome takes one from a leading number in an untagged file name.
- `genre` is one raw string: an adapter whose source lists several genres joins them with
  `; ` (`joined_genres`); splitting, dedup and junk filtering are `normalize_genres`'s at
  projection, so a server track lands on the same genres as the file would. A Subsonic
  server without `genres[]` keeps its single `genre` as it comes.

## Playlists

`playlists(scope)` returns name + song keys in server order, repeats included. `Mine` is
"made by hand by this user", `All` everything the server lists for them; how each protocol
tells is in the client crates' docs. A playlist that vanishes between the listing and its
own request (`NotFound`) is skipped; any other error fails the call. What an import does:
`pawse/src/doc.md`, "Server playlist import".

## Lyrics

`server_lyrics` is the shared finish for Subsonic and Jellyfin: line breaks inside a line
become spaces; timed lyrics keep only timed lines, sorted (blank ones stay as gaps); plain
lyrics drop blank lines; all-blank is `None`. Word ranges point into the text, so adapters
flatten with `one_line` before placing words with `lyrics::locate_words`, and a text the
flattening changes (or a range outside it) loses its words.

- Subsonic: only `kind` main (or none) counts — synced first, then plain, the first with any
  words; `offset` is applied (positive = earlier). A line is split into front text and
  `background` only when `cueLine`s exist for a `bg`-role agent and another agent, every one
  has a `value`, and the front values are not blank; otherwise the server's `line` stays
  whole — never glued from word cues, whose spacing is unreliable. Front cues become words of
  the line, `bg` cues of the backing text; unsynced entries get no words. An unsupported method
  or a missing song is `Ok(None)`; only unreachable and auth are errors.
- Jellyfin: `Start` ticks → ms; a cue's slice of `Text` (UTF-16 positions) is placed in the
  flattened text, so blank cues are skipped and line breaks do not matter; a cue outside
  the text drops the line's words. Jellyfin reports no backing vocals and no offset.

## DLNA

`uri` = the UDN (lowercased), titled by the friendly name. Discovery, re-finding a moved
server and which resource is played: `dlna/src/doc.md`.

- Keys are `dlna::Res::id` (media path + `#size`), covers `dlna::Item::cover_id`; the crate
  doc says why the size is part of the key.
- Bitrate: measured from `size` and `duration` when both are known; else `res@bitrate` as
  bytes per second (the spec), and a value that would exceed 20 Mbit/s is taken as bits per
  second.
- Extension: from the media path when it ends in a known audio extension, else from the
  MIME type (some servers' resource URLs carry none).
- Nothing to log in to, no stars; there is a Sync button, since a listing can change any time.

## Torrents

`uri = btih:<hash>`; same settings rows, sync, statuses and playback path as a server.
Engine behaviour (patience, leases, probes) is in `torrent/src/doc.md`.

- `TorrentHost` holds the engine config, the one `torrent::Engine`, `set_upload`,
  `state_lock` and the index-retry backoff. `Services::torrents` owns the one host and every
  torrent's `Config` carries it with the info hash, so a test builds its own. `engine()`
  creates the engine on first call — only off the UI thread and only for a configured
  torrent; `running_engine()` and `set_upload` never create it.
- `ping` only checks that the `.torrent` stored by `resolve` is still there (and the index
  backoff below). A torrent without peers is not offline until a sync fails. Error mapping:
  `Timeout` and transport errors are `Unreachable` (retried); `NoPeers`, `Unknown`,
  `Invalid` are `Other` (final).
- `fetch_range` is `Engine::read` of one file with a 20 s quiet time (`READ_TIMEOUT`).
- Songs are indexed once per torrent and saved, since its content never changes. The plan
  takes the head of every audio file (plus the tail for formats that keep tags or length at
  the end), every `.cue` / `.lrc`, and one cover per album folder chosen with
  `music_indexer`'s ranking (album folder, its artwork folders, then the parent); booklet
  scans are never fetched. A few more rounds fetch what the tag reader needs beyond that —
  FLAC metadata blocks, a whole ID3v2 tag, an MP4's `moov` found by walking top-level atoms —
  never past `METADATA_LIMIT`. Only bytes known to have arrived are parsed, never the zeros of
  a sparse file.
- The scan is the local one: the planned files go into `<work>/<hash>.view` and
  `music_indexer` runs over it (tags, cue expansion, external covers), so a torrent track
  reads like the same file in a music folder. A file is hard-linked only when already at its
  full length; otherwise it is a sparse copy of the fetched ranges at the torrent's length
  — librqbit does not preallocate, and tag readers derive bitrate (FLAC) or duration (MP3
  without a Xing header) from the file length. Only planned files go in: the rest of the
  probe tree holds zeros or stray lookahead pieces and would be taken for a cover. The view
  and the probe tree are deleted right after.
- Keys are file indexes; cue tracks share their image's key and carry `start_offset_ms`.
  `<hash>.index.json` (rebuilt when `INDEX_VERSION` changes) and `<hash>.covers/<cover hash>`
  live next to the `.torrent`; `cover_art` reads the saved large cover and accepts only a
  hex key, since it becomes a file name. `forget` (source removal) deletes everything named
  after the hash.
- `syncs_alone`: each torrent syncs in its own blocking task (`LibraryService::sync_torrent`, one
  at a time per torrent), outside the servers' one-by-one queue, so a slow or dead swarm
  holds nobody back. A probe gives up after 90 s without a byte. A failed index makes `ping`
  fail as `Unreachable` with its reason for the next 10 minutes (`RETRY_INDEX_AFTER`), so the offline watcher
  does not re-download heads every minute — except when the torrent was removed meanwhile:
  that failure is not recorded, so adding it again syncs at once.
- `state_lock` serializes what writes or deletes a torrent's files in the state dir:
  `resolve`, `forget`, saving an index (which also checks the `.torrent` still exists, so a
  sync finishing after a removal leaves no orphans).

## Decisions

Owner's calls; ask before changing.

- Playlists are read, never written back: an import is a one-time copy.
- Server lyrics are asked live and never stored in `library.db`, like the audio itself.
- No Sync button for torrents (`manual_sync` false): content never changes and the index is
  saved, so a manual sync would only re-apply the same listing. An unavailable torrent
  comes back through the offline watcher and the retry backoff; launch syncs as usual.

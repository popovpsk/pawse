# servers

Media-server protocols (and torrents) behind one interface. Everything above this module —
sync, playback, settings rows — talks to `ServerClient` and `ServerKind`, never
to `subsonic::` or `jellyfin::` directly.

## Files

- `mod.rs` — `ServerKind` (the closed list of protocols, with its stored name and
  title), `RemoteConfig` (a server's saved connection, one variant per kind),
  `RemoteServer` (a configured server: uri, name, config), `RemoteError`,
  the `ServerClient` trait, and the tag cleanups every adapter shares
  (`real_artist`, `real_album`, `real_track_number`, `joined_genres`).
- `subsonic.rs` — the Subsonic adapter: `subsonic::Song` → `RemoteSong`,
  `subsonic::Error` → `RemoteError`.
- `jellyfin.rs` — the Jellyfin adapter: `jellyfin::Item` → `RemoteSong`, error
  mapping, and `authenticate` (log in once, get a token).
- `dlna.rs` — the DLNA adapter: `dlna::Item` → `RemoteSong`, error mapping,
  and `describe` (an address typed by the user → `dlna::Device`).
- `torrent/mod.rs` — the torrent adapter over the `torrent` crate.
  `TorrentHost` owns what used to be process-wide statics: the engine config,
  the one `torrent::Engine` (created by the first `engine()` call — never on
  the UI thread, never without a torrent), `set_upload`, `state_lock` and the
  index retry backoff. `Services::torrents` holds the one host
  (`torrent_settings::torrent_host` builds it at startup) and every torrent's
  `Config` carries it next to the info hash, so a client needs no global and a
  test can build its own host. Also error mapping, `forget`, `peers`, and
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

- **`cover_art(key, max_size)`** returns a picture for the sync's thumbnails;
  servers that can scale get `max_size` (Subsonic `size`, Jellyfin
  `maxWidth`/`maxHeight`), DLNA and torrents ignore it. A sync calls it from 4
  threads at once, so a client must be safe to share (the `ureq` agents and the
  torrent's file reads are). Eight `Unreachable`/`Auth` answers in a row stop
  the sync's cover phase; any other error or an empty body is a key without a
  picture for this sync (see `library_views/doc.md`).
- **Cover view asks for the big picture.** A server cover's
  `cover_art.source_path` is `<kind>-cover://<source_id>/<key>`
  (`music_library::remote::cover_source` / `parse_cover_source`).
  `CoverModeView::load_full_cover` shows the 320 px thumbnail from the DB at
  once and, on the background executor, asks `RemoteMedia::cover` → the
  source's `SourceMedia::cover` → `cover_art(key, 2048)` for the full-size layer
  on top. 2048 px covers a full-screen cover on a Retina laptop and keeps a
  Jellyfin original of several thousand pixels from arriving whole. Nothing is
  cached: every new cover in the view is one request, as a local cover is one
  disk read; the DB dedupes covers by content, so an album is one request.
  Only one server cover request runs at a time: the call blocks an executor
  thread (up to the HTTP timeouts on a slow or unreachable server), so skipping
  through tracks must not start one per track. A cover asked for meanwhile is
  only remembered, and when the running request ends the view loads whatever
  cover is current then; the covers skipped past are never fetched. If the
  server does not answer, the usual file fallback runs (the cover inside a
  local track whose cover row came from a server), else the thumbnail stays and
  a warning is logged. A torrent stores only its thumbnails, so it hands back
  the same 320 px picture.
- **What differs between kinds is asked, not compared.** `ServerKind` answers
  `manual_sync`, `imports_favorites`, `imports_playlists`, `reports_plays` / `sends_favorites` (what
  `server_scrobble` may send back: plays to Subsonic, likes to Subsonic and
  Jellyfin), `syncs_alone` (its own sync thread),
  `titled_by_name` and `has_peers`; `RemoteError::NotFound` is a song the
  server says is gone (shown and fetched like `Other`, but a report back to the
  server drops it quietly); `RemoteError::NotSynced` / `Syncing` never come from a
  client: `LibraryService`'s favorites and playlist imports return them when the
  server has no enabled source row or (playlists) is mid-sync, and
  `library_sources::describe_error` shows them as localized lines (the other
  `match`es never see them and turn them into a plain retry/fatal message);
  `ServerClient` has `scrobble`,
  `now_playing`, `set_favorite` and `playlists` (an error unless the kind says it reports
  or imports that), `lyrics` (`Ok(None)` by default; see below), `forget` (removal
  cleanup), `peers` and `moved` (a config to save because the server was
  found elsewhere — DLNA only; a sync sends it as `LibraryEvent::RemoteMoved`
  and `remote_settings::server_moved` stores it), all no-ops by default; `RemoteConfig::web_url` is the
  address the "Open in browser" button opens (none for a torrent). Code outside this module
  never tests `kind == Torrent`; the only per-kind `match`es left are
  exhaustive maps from a kind to a value (icon, element ids),
  which the compiler keeps complete.
- **Playlists are read, never written.** `playlists(scope)` returns every
  playlist the scope keeps as a `RemotePlaylist` (name + song keys in server
  order, repeats included); a playlist that disappears between the listing and
  its own request (`NotFound`) is skipped, any other error fails the whole call.
  `PlaylistScope::Mine` means "made by hand by this user": Subsonic keeps a
  playlist whose `owner` is the configured user (or missing) and that is not
  OpenSubsonic `readonly` — Navidrome sets `readonly` on smart playlists, other
  users' playlists and the ones it imports and keeps in sync from `.m3u` files,
  even when the admin owns those. Jellyfin has no owner field in its listings, so
  each playlist is asked `/Playlists/{id}/Users`, which only its owner may read
  (403 = not mine; a 404 = a server older than 10.9, where every visible
  playlist is the user's own). On 10.9+ library `.m3u` playlists are open-access
  and ownerless, so only `All` brings them. `All` takes everything the server lists
  for the user. What is done with the result is `remote_sync::import_playlists`'s
  business (see `library_views/doc.md`).
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
- **Lyrics come from the server, never cached.** `ServerClient::lyrics(key)`
  returns `lyrics::Lyrics` for Subsonic and Jellyfin; DLNA and torrents keep
  the default `None`. The lyrics panel asks every time a server track becomes
  current (like the audio itself, nothing is stored in `library.db`).
  `server_lyrics` is the shared finish: line breaks inside a line become
  spaces, timed lyrics keep only timed lines sorted by time (blank ones stay as
  gaps), plain lyrics drop blank lines, all-blank is `None`. Word ranges point
  into the text, so a text that this flattening changes (or a range outside it)
  loses its words — adapters flatten with `one_line` *before* placing words.
  Subsonic: of the `structuredLyrics` entries only `kind` main (or no kind) is
  used — synced ones first, then plain, the first that has any words;
  `offset` is applied (positive = earlier, per the spec); translations and
  pronunciations are ignored. `cueLine`s are grouped by `index` (= position in
  `line`). When a line has `cueLine`s of a `bg`-role agent and of another
  agent, every one of them carries a `value` and the non-`bg` values are not
  blank, those values become the line's text and the `bg` ones its
  `background`. Otherwise the server's `line` value is kept whole — never
  glued together from word cues, whose spacing is not reliable (a v1 reply,
  without `cueLine`s, always does this; backing vocals then stay inline).
  Words: each `cue` (time minus `offset`) is placed in the text it belongs to
  with `lyrics::locate_words` — the front cues in the line's text, the `bg` ones
  in the backing text; an unsplit line places all its cues, front then `bg`, in
  the whole `line` value. Unsynced entries get no words. An unsupported
  method or a missing song is `Ok(None)`; only unreachable/auth are errors.
  Jellyfin: `Start` ticks → ms; `Cues` become words: each cue's slice of
  `Text` (UTF-16 positions) is placed in the flattened text with
  `locate_words`, so blank cues are skipped and line breaks don't matter; a
  cue outside the text drops the line's words. No backing vocals (Jellyfin
  drops `[bg:]` lines), and no offset (its LRC parser does not report one).
- **`genre` is one raw string.** An adapter whose source lists several genres
  (Jellyfin, DLNA, a torrent's tags, OpenSubsonic's `genres[]`) joins them with
  `; ` (`joined_genres`) and leaves the splitting, dedup and junk filtering to
  `normalize_genres` at projection, so a server track lands on the same genres
  as the file would. A Subsonic server without `genres[]` keeps sending its
  single `genre` string, which is used as it comes. A torrent's saved index
  keeps the genre it was built with; only a new index gets the joined list.
- **Jellyfin's extension** comes from the file path, else from the container
  list, preferring a known audio extension (`mov,mp4,m4a,…` → `m4a`).
- **Placeholders** (`[Unknown Artist]`, `[Unknown Album]`) become empty, and a
  track number above 999 is dropped — Navidrome takes one from a leading number
  in an untagged file name.

## DLNA

A DLNA/UPnP media server is `ServerKind::Dlna` with `uri` = its UDN
(lowercased), titled by its friendly name. The `dlna` crate's `doc.md` has the
protocol side (discovery, re-finding a moved server, listing, which resource
is played). What the adapter adds:

- **Nothing to log into, no stars.** `imports_favorites` is false. There is a
  Sync button, like the other servers: a DLNA listing can change any time.
- **Keys** are `dlna::Res::id` (media path + `#size`) and covers
  `dlna::Item::cover_id` — see the `dlna` crate for why the size is part of
  the key.
- **Units.** The bitrate is measured from `size` and `duration` when both are
  known; otherwise `res@bitrate` is taken as bytes per second (the spec), and a
  value that would be over 20 Mbit/s is taken as bits per second instead.
  MiniDLNA sends bits per second, but always sends size and duration too.
- **Extension.** From the media path when it ends in a known audio extension
  (MiniDLNA, Gerbera), else from the MIME type (Serviio's
  `/resource/…/ORIGINAL` has none).
- **Year** is the leading four digits of `dc:date`. The first performer is the
  artist, the others are aliases; the first `AlbumArtist` is the album artist.
- **Settings** (`dlna_settings.rs`): "Search the network" runs `dlna::discover`
  in the background and lists what is not added yet; an address field adds a
  server that discovery cannot reach (VPN, another subnet, multicast filtered).
  Both add through a ping, like the other servers.

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
- **The scan is the local one.** Just the planned files go into
  `<work>/<hash>.view`, and `music_indexer` runs over it — tags, cue
  expansion, external covers — so a torrent track reads exactly like the same
  file in a music folder. A file is hard-linked only when it is already at its
  full length; otherwise (and where hard links fail) it is a sparse copy of just
  the fetched ranges at the torrent's length. librqbit does not preallocate:
  a file is only as long as the furthest piece written, and tag readers derive
  the bitrate (FLAC) or even the duration (MP3 without a Xing header) from the
  file length — a 335 MB image read at its 3 MB probe length came out at
  8 kbps. Only planned files go in, because every other file of the probe tree
  holds zeros or stray lookahead pieces and would otherwise be taken for a
  cover. The view and the probe tree are deleted right after.
- **Keys** are file indexes in the torrent; cue tracks share their image's key
  and carry `start_offset_ms`. The index (`<hash>.index.json`, versioned by
  `INDEX_VERSION`; 2 re-reads indexes built from short probe files) and the chosen covers (`<hash>.covers/<cover hash>`) live next
  to the `.torrent`; removing the source calls `Engine::forget`, which deletes
  everything named after the hash there.
- **Covers** go through the normal `fetch_covers`: `cover_art(key)` reads the
  saved large thumbnail.
- **Each torrent syncs on its own thread** (`LibraryService::sync_torrent`,
  one at a time per torrent), outside the servers' one-by-one queue: a slow or
  dead swarm must not hold Subsonic/Jellyfin or the other torrents back. A probe
  gives up after 90 s without a byte. Any failed index makes `ping` fail for
  the next 10 minutes with the failure's reason, so the offline watcher does
  not re-download heads every minute (except when the torrent was removed
  meanwhile: that failure is not recorded, so adding it again syncs at once). The sync's "running" marks are drop
  guards (`Claim`), so a panic in an index does not leave a torrent syncing.
- **Peers** (`connected/known`) show next to a torrent while it is loaded —
  indexing or playing — polled every 2 s by `LibrarySources` through
  `ServerClient::peers`, only while a source with `has_peers` is configured
  (no timer runs otherwise).
- **`state_lock`** serialises what writes or deletes a torrent's files in the
  state dir: `resolve` from the settings, `forget` on removal, saving an index
  (which also checks the `.torrent` still exists, so a sync finishing after a
  removal leaves no orphans).
- **Idle torrents** are unloaded 15 minutes after their last read
  (`torrent_settings::IDLE_UNLOAD`). A track downloads into the media cache
  much faster than it plays, so the clock usually starts early in a track;
  15 minutes keeps a torrent loaded across long tracks, and prefetching the
  next track 60 s before the end reloads it if not.
- **Stars** do not exist; `imports_favorites` is false, so the torrent has no
  import button and no link to the Scrobbling tab.
- **No Sync button** (`manual_sync` is false). A torrent's content never changes and its index is saved,
  so a manual sync would only re-apply the same listing. An unavailable torrent
  comes back through the offline watcher (every 60 s and on window activation),
  a failed index is retried after its 10-minute backoff; launch syncs as usual.
- **Off until the user agrees.** Until `torrents_enabled` is set, the Torrents
  group in Settings → Library is just an "Enable torrents" switch. The switch
  opens a dialog warning that downloading and sharing copyrighted music over
  torrents is illegal in some countries; "Yes, enable" saves the flag, Cancel
  leaves it off. The flag is a one-time acknowledgement, not a feature switch,
  so once it is on the group has no way to turn it off again. A settings file
  that already lists torrent sources (from before the flag existed) loads as
  enabled (`migrate_torrents`). With the flag off nothing else changes: no
  sources means no engine is ever created.

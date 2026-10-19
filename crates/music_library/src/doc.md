# music_library

The SQLite-backed library and the only crate that touches the database: the catalog the
screens browse, the user data hanging off it (playlists, likes, fetched lyrics,
listening history, scrobble deliveries) and the identity layer that keeps the two apart.
`LibraryRepository` is the API for everything; scans go through `ScanWrite`
(`ScanSession`).

Not here: driving scans, probing folders, the scan fingerprint and server sync
(`library_service.rs`, `remote_sync.rs` in `pawse/src/doc.md`); reading files
(`music_indexer/src/doc.md`); server protocols (`pawse/src/servers/doc.md`); the
scrobble engine (`scrobble/src/doc.md`); running the sound analysis
(`pawse/src/similar_tracks/doc.md`).

## Files

- `lib.rs` — re-exports, `NO_METADATA_ALBUM_ID` (-1) / `NO_METADATA_ARTIST_ID` (-2) (a
  virtual album / artist for tracks without one, understood by the per-id queries) and
  every test. Migration tests replay `MIGRATIONS[..=8]` into a v8 database.
- `repository.rs` — the two traits. `sqlite.rs` — `SqliteLibrary`, `ScanSession`, the
  migration runner, the SQL a scan shares; `compute_sort_name` is public so the app's
  in-memory sorts agree with `artists.sort_name`; `clean_album_title` (trim, blank is
  no album) is the one ingest rule for album titles, applied by the app to files and by
  `project_remote_tracks` to server rows. `thumbnail.rs` — a decoder panic comes
  back as an error: one broken picture costs that cover only.
- `remote.rs` — the server-track locator `pawse-source://<source_id>/<key>.<suffix>`,
  `location`, and a server cover's `source_path` `<kind>-cover://<source_id>/<key>`.
  Code holding a `Track` asks it (`location`, `remote`, `local_file`, `own_file` = a
  local file that is this track's alone, not a cue piece), never parsing `path`.
- `adoption.rs` — `match_tracks`, `normalize_tag` (shared with the scrobble like-import
  and `similar_tracks`). `album_artists.rs` — `derive_album_artists`, `is_credit_of`.
- `genres.rs` — `normalize_genres`, the one rule from raw genre values to catalog
  genres: split on `,` `;` `/` (not `&`: `Drum & Bass`, `R&B` stay whole), drop junk and
  bare numbers, dedup case-insensitively. The indexer and `project_remote_tracks` both
  use it; changing it needs an `INDEXER_FORMAT_VERSION` bump
  (`music_indexer/src/doc.md`).

## Rows

- **Catalog** (`tracks`, `albums`, `artists`, `genres`, link tables, disk lyrics) is
  re-derivable: every scan clears and refills it and touches nothing else. `cover_art`
  is kept: content-addressed by SHA-256, the first `source_path` for a hash wins.
- **Identity and user data** (`media_items`, `media_bindings`, `sources`, `adoptions`,
  `remote_tracks`, `playlist_tracks`, network lyrics, `plays`, `loves`, `*_deliveries`)
  is not derivable: no scan deletes it. **Derived caches** (`track_embeddings`,
  `track_sound_genres`, `artist_info`) are costly to re-derive, so a scan leaves them
  alone.
- `maintain_storage` (after every real scan) runs on `conn`, so a UI write waits for it
  rather than failing on the busy timeout mid-vacuum. It `VACUUM`s only when more than
  a quarter of the pages are free: a refill reuses the pages its clear freed, and a
  vacuum rewrites the whole file. It `ANALYZE`s only when `user_version` or the track
  count (by more than a quarter) moved, or a day passed, since
  `scan_meta.analyzed_version` / `analyzed_tracks` / `analyzed_at`: a clear and refill
  of the same library leaves the statistics true, while plays, deliveries and
  playlists grow with no scan to notice.
- `tracks.id` borrows `media_items.id`, so a rescan puts every row back under its id.
  User tables reference `media_items`, never `tracks`.

## Connections and locking

- Three connections behind mutexes (`conn` for the UI, `scrobble_conn`,
  `embedding_conn`); `ScanSession` and each `apply_remote_listing` open their own. A
  writer waiting for a scan batch's write lock must not hold the mutex the UI reads
  through (hence `embedding_coverage` reads on `conn`). WAL with a 5 s busy timeout: a
  UI write mid-scan waits instead of failing.
- A writer that reads before it writes uses `BEGIN IMMEDIATE`: a deferred transaction
  that reads, sees another commit, then writes fails with `SQLITE_BUSY_SNAPSHOT`, which
  the busy timeout does not retry (a like was lost this way). Any new read-then-write
  path must do the same.

## Identity: sources, bindings, items

- `sources` — one row per root, `(kind, uri)` unique: `local` or the app's server kind.
  Source `1` is a disabled placeholder that migration 9 hangs every pre-existing binding
  on (a migration cannot see `settings.json`); `sources()` hides it.
- `media_items` carry a display snapshot, so an item without a `tracks` row still
  renders; `updated_at` moves only when the snapshot really changes.
- `reconcile_local_sources` disables every local source, then upserts one enabled row
  per folder. A removed folder stays a disabled source with its bindings, so re-adding
  it restores likes and playlist entries under their old ids.

## Matching across sources

- An item has at most one live binding per source: two files in one source are two items
  (a copy beside its original), the same recording in different sources is one item.
  Which binding plays and names the catalog row is priority, not identity: local folders
  first, then servers (no kind outranks another), by source id.
- Matching uses only database content, so offline, disabled or gone sources still take
  part: `file_size` with the cue offset (same bytes, even untagged) and the
  `media_items` snapshot. Not relative paths: a server's path is its own invention.
- `match_tracks` tiers, strongest first, each over all arrivals before the next: `file`
  (shared size and offset), `tags` (artist + title + album), `title` (artist + title,
  duration required, only candidates with no live binding, so an offline item never
  merges with a different recording that shares a title).
- Tag tiers need a non-empty artist (untagged `01.flac`s match by size only) and accept
  the arrival's aliases: Navidrome splits "A feat. B" where a file keeps one string.
  Pairs are assigned best-first, so arrival order never decides.
- A match only inserts a binding and an `adoptions` row; candidates are all items, live,
  dead or offline. `revive_lost_items` runs the other way after retirement: items with
  user data and no playable binding absorb a playable spare without any (rows move to
  the lost id, foreign keys deferred): a copy made before the original went away.

## Scans

`ScanWrite`: `clear()`, `add_cover` / `add_track` in any order (a track whose cover hash
has no `add_cover` yet waits, or is inserted cover-less at `finish`), `flush()`,
`finish()`.

- Batches open (`BEGIN IMMEDIATE`) only to write; the app calls `flush` whenever it has
  caught up with the indexer, so the lock is not held while it parses.
- `finish` is one transaction: absorbing an item moves rows under deferred foreign keys,
  and a server listing slipping in while held arrivals settle would mint a second item
  for a song whose local copy is about to be written. It runs only after the indexer
  reported `Complete`; a dropped session retires nothing and records no fingerprint.
- The session writes no album cover, only each track's own album-artist tag and a
  provisional album year (the first track's); the app settles the rest after every
  write (`settle_derived_rows`:
  `regroup_albums`, covers, years, album artists, item snapshots before
  `delete_orphaned_albums_and_artists`, which keeps any cover a snapshot references). An
  album's cover is its lowest `(disc, track, path)` track with one.
- A file whose `(path, offset)` has a binding in any local source (placeholder and
  disabled included) keeps its item and is re-pointed to the longest enabled root
  containing it, the placeholder only when none does: that heals the placeholder.
- Other files are held until `finish` when the database has items or there is more than
  one folder, then settled source by source in id order; a held file that fails to write
  is logged and skipped, since failing `finish` would stop retire and sweep for good.
- A second binding of an item already written this scan (the same file in another
  folder) writes nothing unless its source has priority, which rewrites the row from
  that file: the row never depends on which file the parallel indexer delivered first.
- A second binding in a source the item was already placed in (`placed`: an adopted item
  whose old file came back from the trash) gets a fresh item; `seen` drops the same
  `(path, offset)` delivered twice (nested folders, two cue sheets for one image).
- Retire sets `present = 0` on bindings of enabled, available local sources the scan did
  not see; disabled and offline sources are left alone. Placeholder bindings under no
  enabled root or an available one go too; the app skips its scan fast path while
  `has_unplaced_media()` is true.
- `SWEEP_UNREFERENCED_ITEMS` runs last: items with no `tracks` row, no present binding
  on an enabled source and nothing user-side pointing at them go, which bounds growth.
- Offline folders: the app passes `LocalFolder::available` through
  `reconcile_local_sources` (`has_media_under(root)` tells an unmounted mount point from
  an empty folder). An unavailable folder keeps its bindings and items, which stay match
  candidates: offline is not deleted.
- An unavailable folder's tracks leave the catalog and show as unavailable in playlists
  and likes; with every folder offline the catalog ends up empty.

## Server sources

A server is a `sources` row and its songs are bindings with `source_key` = the song id.
Nothing here depends on the kind beyond `'local'` versus not.

- `reconcile_remote_sources(kind, …)` enables the configured sources of that kind and
  disables the rest of that kind only; if the enabled set changed (the return value),
  the caller must drop the scan fingerprint and rescan (the fast path fingerprints only
  local folders).
- `apply_remote_listing` records a full listing in one `BEGIN IMMEDIATE` transaction on
  its own connection; covers arrive with it, so a concurrent scan's orphan-cover cleanup
  cannot delete them first. Its upsert writes only when something differs, which is how
  `RemoteSyncReport::changed` says a rescan is needed.
- Songs no longer listed get `present = 0`; new songs are born against every item with
  no binding the listing accounts for, so a renamed song (new id) finds its old item. An
  empty listing while the server had present songs is refused (an unmounted library).
- `cover_key` stays out of that change check and is written by a separate uncounted
  `UPDATE`: it never reaches the catalog, but `remote_cover_hashes` (key to stored
  image) is how the app recognises an image it has, and Navidrome's cover ids carry the
  album's timestamp, so a stale key would re-download the image every sync.
- A NULL key marks a song whose picture is not fetched yet. The first `source_path`
  stored for an image may carry an older key; Navidrome ignores the `_<timestamp>`
  suffix, so it still works.
- `project_remote_tracks`, at the end of every scan, inserts a `tracks` row from
  `remote_tracks` for each present binding of an enabled, available server whose item
  got none from a local file: local wins, and no network is needed.
- The listing's genre is cached raw and normalized at projection. Changing that rule for
  cached rows takes a migration like 11 (it drops the scan fingerprint only if some
  cached genre would change; its junk list is a copy on purpose); bumping
  `INDEXER_FORMAT_VERSION` would reindex every library.
- Cue images: a server that does not read cue sheets (Navidrome) lists a whole-disc
  image as one song that never matches, so the projection skips a server binding that
  has no other binding and whose `file_size` equals a playable cue piece's in another
  source (it shows as one long track while that folder is offline). A server that splits
  cues lists tracks with the image's size and no offset, so they join local cue tracks
  by tags.
- A source that reads cue sheets itself (a torrent) lists each piece with
  `start_offset_ms = Some(…)`: one key with several offsets is several items, and a
  piece file-matches the local cue track by `(size, offset)`.
- `playback_locators(item)` lists where an item can play now, local first; the catalog
  row names only one, so playback falls back when its file vanished.

## Stable album and artist ids

A refilled catalog would get ids from 1 again in whatever order the parallel indexer
delivered files, so a screen holding an old id would land on another album.

- `ScanSession::clear` snapshots the ids first (`StableIds`): an album gets its old id
  back by `identity` (see "Album identity"), an artist by name.
- Rows from before migration 13 have no identity. On the first rescan after it, an album
  whose identity is not in the snapshot takes the old id of the track that created it,
  if no other album took it yet (`legacy_albums`): the seven albums a compilation used to
  split into become one that keeps one of their ids. Which one depends on delivery
  order, once.
- A new one gets an id above everything ever handed out (`allocate_catalog_id`; the
  high-water marks live in `scan_meta`). Every writer allocates through it, the tag
  editor's `upsert_album` / `get_or_insert_artist` included, so a tag edit between two
  scan batches can neither reuse a dead id nor take one the scan is about to hand back.
- Genres are not covered: their ids are reissued on every scan, so a screen holding a
  genre holds its `key` (the lowercased name) and every genre query takes the key.
- Artists are keyed by the lowercased name (`artist_key`), shown under the spelling met
  first. Lowercasing is Rust's (Unicode), not SQLite's ASCII-only `NOCASE`, so
  `get_or_insert_artist` compares in Rust too.

## Album identity

Albums are grouped close to Navidrome's default album id (`musicbrainz_albumid |
album, releasedate`) and MusicBee: the track's own year is not part of it. A compilation
whose tracks carry each song's year (`DATE` 1972, 1977, …) used to split into one album
per year (issue #13).

- `albums.identity` (migration 13): `mb␟<id>␟<exact title>` when the track has a
  MusicBrainz album id (trimmed, lowercased; years ignored), else `tag␟<exact
  title>␟<release year>`, where the release year is the `RELEASEDATE` / `TDRL` tag only,
  empty without one. The title is exact but trimmed at ingest (`clean_album_title`):
  a stray space must not split an album. `albums.release_year` keeps the release year
  for the display year, and the album editor's year edits it (`album_release_year`).
  The title stays in the `mb` key so a rename lands the same way in a tag edit and a rescan:
  neither path stores the album title per track.
- The album artist is not part of the key: a track without the tag stays with siblings
  that have one (`album_artists.rs`). It is evidence for a split instead.
- `regroup_releases` looks at every `tag` family (the plain row plus rows split off it)
  that has two tracks with different years on one `(disc, track)`, or two different
  first album-artist tags. Different first album-artist tags split it by that tag (an
  untagged track is a group of its own); a group with such a collision then splits by
  the track year, which is the old `(title, year)` rule for two releases under one name.
  Same-year collisions are copies (FLAC beside MP3) and split nothing, so copies of a
  per-song-year compilation stay one album. One group left means the family merges back
  into the plain row.
- The largest group keeps the plain identity, ties go to the smallest key; the others
  get `<plain>␝a<artist>␝y<year>` (an empty part means that split did not apply).
  `mb` albums never split: two copies of one release are one album.
- It runs at the end of the scan's `finish`, where split rows get their old ids back
  from `StableIds`, and as `regroup_albums` in `settle_derived_rows` (new ids), so a tag
  edit and a rescan agree. Until `finish`, a family sits in its plain row.
- `upsert_album` (the tag editor's re-index) finds or creates the plain row; the regroup
  then moves the track to the row a scan would give it.
- `resolve_album_years`: an album's year is its `release_year`, else the newest track
  year, since a release is no older than its newest recording.
- Known gaps: the plain row (and its id) goes to the larger release, so it
  moves to the other one when their sizes cross. A two-disc compilation without disc
  tags collides on every position and still splits by year, unless its discs sit in
  `CD n` / `Disc n` folders, which the app numbers at ingest. Server tracks carry no
  MusicBrainz id, so server-only songs of an album whose local files have one form a
  `tag` album of their own beside it.

(`␟` is U+001F, `␝` U+001D.)

## Artists, albums and genres

- `resolve_album_artists` rebuilds `album_artists` and `albums.artist_known` with
  `derive_album_artists` over each album's tracks in `(disc, track, path)` order: the
  first album-artist tag wins; with none, identical credits on every track are kept
  whole, else the one first credit that every track's first credit equals or continues
  with a join marker; anything else is `artist_known = 0` with the first track's artists
  as a placeholder.
- Join markers: `/ & , ; + ( [`, a spaced dash, or a whole word such as feat, ft, with.
  A bare space is no marker ("Queen" must not swallow "Queen Latifah"), nor is an
  unspaced hyphen ("Jay-Z").
- `set_album_artists` sets the same flag, and the result does not depend on insertion
  order, so a scan and a point update agree. `known_album_artists_map` lists only
  `artist_known` albums (otherwise the row is a placeholder). `listed_artist_for_credit`
  applies the marker rule to send "Samurai feat. Refused" to the listed "Samurai"; a
  compilation guest keeps their own page.
- `artists`, `artist_album_covers` and `artist_search_haystacks` build CTEs: `m`, who is
  listed (track-artist grouping: `track_artists`; album-artist grouping: per-track
  album-artist tags, else `album_artists` of `artist_known` albums, else track artists),
  and `u`, `m` plus every track credit. Only artists in `m` are listed, get avatars or
  are searchable; counts, an artist's page and search words use `u`.
- The listed set is `a.id IN (SELECT artist_id FROM m)`, built once: a correlated
  `EXISTS` re-ran the union per artist and made the first load quadratic
  (`artist_summary` runs once and keeps it).

## Playlists and likes

- Likes have one source of truth: a hidden playlist (id in
  `scan_meta.liked_playlist_id`, created at open if missing). `Track.liked` comes from
  the `liked_track_ids` view; there is no `liked` column. `playlists()` and
  `playlists_containing_track` leave it out and `rename_playlist` refuses it.
- `add_tracks_to_playlist` appends in one transaction, in order, skipping tracks already
  present and ids with no `media_items` row (a stale queue after a rescan), so one stale
  entry cannot fail the batch; it returns the number inserted. Positions stay dense: the
  UI addresses entries by index.
- `tracks_for_playlist` (and `liked_tracks`) returns every entry joined through
  `media_items`: one without a `tracks` row comes back with `available = false` and its
  fields from the snapshot. Positions must line up with what the UI shows, or
  `move_track_in_playlist` moves the wrong row.
- `track_artists_map` and `track_albums_map` fall back to the snapshot for ids with no
  catalog row. They and `known_album_artists_map` bind ids as one JSON parameter:
  callers pass the whole library and SQLite caps parameters.

## Lyrics

- One row per source, `PRIMARY KEY (track_id, source)`: a `.lrc` sidecar, the embedded
  tag and an LRCLIB result live side by side, a scan never overwrites fetched words and
  the user can switch. A `not_found` row is the "looked up, nothing" marker.
- `clear()` removes the disk rows (`lrc`, `embedded`) so a deleted `.lrc` disappears;
  fetched rows and markers go only through `delete_lyrics_source` (the user turns LRCLIB
  off), and an item kept alive only by them is swept by the next scan.
- Which row shows is decided outside SQL (`lyrics_source::choices`, `pick`): default
  order `server > lrc > embedded > lrclib` (`lrclib` first if the user prefers it), a
  row whose text equals a higher-ranked one is dropped (an exported `.lrc` hides the
  LRCLIB copy it came from). `server` is never a row: the app fetches a server track's
  own lyrics live.

## Artist info cache

- `artist_info` is the app's artist index: one row per artist **name** with `facts`
  (JSON the app writes, opaque here), `photo`, `photo_pending` and `fetched_at`; `facts`
  NULL means "looked up, not found". Nothing deletes a row (not `clear()`, not the
  orphan cleanup), hence the name key with no foreign key.
- `artists_without_info()` lists artists without a row, and not-found ones with a
  credited track (same three paths as `artist_titles`) whose `media_items.updated_at` is
  at or after the row's `fetched_at`. Found rows are final. The query is set-based, not
  a correlated `EXISTS` per artist: `album_artists` has no index on `artist_id`.
- The app saves `fetched_at` = `ArtistTitles::read_at`, when `artist_titles(id)` read
  the titles, so a track written while the lookup waited still counts as newer (`>=`:
  both are whole seconds). Items dated in the future are ignored until their time, or
  each lookup would save a `fetched_at` still below them.

## Listening history and the scrobble queue

- Tables (migration 8, rebuilt onto `media_items` by 9): `plays` (`track_id` →
  `media_items` `ON DELETE SET NULL`; unique `(started_at, artist, title)`), `loves`,
  `play_deliveries` / `love_deliveries` (`state` 0 pending / 1 sent / 2 dropped),
  `target` = `scrobble::TargetId::key()` verbatim. They live in `library.db` so queries
  can join the catalog; all writes go through `scrobble_conn`.
- `record_play` upserts on `(started_at, artist, title)`: `played_secs` and `qualified`
  only grow, and delivery rows (`INSERT OR IGNORE`) are written only for a qualified
  play, so an early commit at a pause and the final one share a row and a
  later-qualifying play gets its deliveries then. A play with no target configured is
  still history.
- `record_love` always inserts. No scrobble path deletes `plays` / `loves` rows.
  `settle_plays` / `settle_loves` touch only `state = 0` rows (Deferred only bumps
  attempts and the error), so a finished row is never reopened.
- `trim_pending_deliveries(cap)` counts items (plays plus loves), not delivery rows,
  over all targets, configured or not (rows would divide the cap by the number of
  services), and drops the oldest plays' pending deliveries with `queue overflow`, then
  loves'.
- Both `track_id` columns need an index (`idx_plays_track`, `idx_loves_track`; a test
  guards it): with foreign keys on, SQLite looks for children of every deleted parent,
  so without one each swept item costs a full scan of the history inside the scan
  transaction. The `media_items_guard_user_data` trigger aborts deleting an item that
  `plays` / `loves` reference, so `SET NULL` is only a backstop.

### Reading listening history

`pawse::tools::ai_prompt` and `similar_tracks` read `plays` on `conn`. `play_tallies`
counts every row, qualified or not; `track_listings` covers catalog tracks only, so
plays of tracks that left the catalog show only in the tallies;
`same_artist_track_ids(ids)` returns every track credited, in any position, to the first
artist of one of the given tracks.

## Sound vectors (`track_embeddings`)

One row per `(item_id, version)`: the `audio_embedding` vector (`DIM` f32
little-endian), the style scores (`styles`, 400 f32) and the `EMBEDDING_VERSION` that
made them, both `NOT NULL`. Written by `pawse::similar_tracks`; this crate knows neither
the model nor the dimensions. Nothing reads `styles` yet: it keeps the whole model
output so the genre rows can be derived again without the GPU.

- Keyed by `media_items.id`, not `tracks`: the item survives rescans, and one recording
  held by several sources is analysed once. Not in `media_items_guard_user_data`, and
  the sweep ignores it: `ON DELETE CASCADE` drops the vectors of swept items.
- `embedding_candidates(version)` lists catalog tracks without a row for `version`:
  liked first, then by last play, then library order. `save_embeddings` is one
  `BEGIN IMMEDIATE` transaction replacing the item's row and genre rows; an item a scan
  swept meanwhile is skipped, so it cannot fail the batch.
- `scan_embeddings` streams the vectors of items in the catalog only: those of items
  that left it stay for when they return but would only take places in a fixed-size
  top-N that nothing can play. Its callback runs under the connection lock and must not
  call back into the embedding methods.
- `prune_embeddings(keep_version)` deletes every other version and, if it deleted any,
  the genre rows of items left without a vector; the app runs it when its analysis
  thread starts.

### Genres by sound (`track_sound_genres`)

`(item_id, genre, score)`, `WITHOUT ROWID`, index `(genre, score)`: the genres the app
derived from a track's style scores, saved in the same transaction as the vector.
`genre` is a number the code owns (`audio_embedding::styles::genre_id`); names live in
code and queries get them from the caller (`SoundGenres::names`).

- No version, so no foreign key to `track_embeddings`: the rows belong to the one
  version kept, which holds once `prune_embeddings` has run. `item_id → media_items`
  cascades.
- Kept apart from `genres` / `track_genres`, which are the tags: a scan and the tag
  editor rewrite and sweep those, and the tag editor is prefilled from `album_genres`,
  so genres by sound there would be swept or written into the user's files.
- Display thresholds are query parameters (`SoundGenres`; `None` keeps the tag-only
  SQL), so changing them needs no re-analysis. Tags and genres by sound meet by `key`.
  Why the app shows them as it does: `pawse/src/similar_tracks/doc.md`, "Genres by
  sound".
- `genres(sound)` is one row per `key` over the union of tag links and genre rows with
  `score >= min_score` on catalog tracks (`s.item_id IN (SELECT +id FROM tracks)`: the
  `+` makes SQLite scan a `tracks` index instead of one rowid lookup per genre row). A
  genre no tag links to is listed at `min_tracks` tracks or more, a tag genre however
  few; one order, by key, since nothing in the row says where it came from.

## Migrations

There are real users: a schema change is a new versioned step in `MIGRATIONS` (bump
`user_version`), never a wipe. Re-deriving track data from files is
`INDEXER_FORMAT_VERSION`'s job, not a migration's.

- The probe in `SqliteLibrary::open` that deletes a database lacking old-schema
  artifacts runs only while `user_version == 0`; above that the migrations own the
  schema, and the history, unlike tracks, cannot be rebuilt by rescanning.
- `run_migrations` backs the database up before crossing `IDENTITY_MIGRATION` (9) from a
  version above 0, once: `VACUUM INTO <db>.bak-v<old>.partial`, then an atomic rename to
  `<db>.bak-v<old>`. An existing `.bak-v<old>` is kept; a leftover `.partial` (crash
  mid-backup) is never trusted and is rewritten. A fresh database gets no backup.
- Pending steps run in one transaction with `foreign_keys` **off**: migration 9 rebuilds
  tables (`CREATE x_new`, copy, `DROP x`, `RENAME`), and with foreign keys on
  `DROP TABLE tracks` would cascade into every child that references `tracks` by name,
  including the freshly copied tables. The pragma cannot change inside a transaction, so
  it is set around it.
- `pragma_foreign_key_check` rows are counted before and after; it rolls back only if
  the migration **added** violations, so pre-existing junk cannot fail every launch.
- Migration 9 keeps ids one-to-one (`media_items.id := tracks.id`), so user rows are
  copied as-is and checked by counts and sums; child tables that reference a rebuilt
  parent only by name are not rebuilt. `seed_liked_playlist_from_column` runs first and
  builds the hidden liked playlist for databases that predate it.
- A migration that rebuilds a table named by `media_items_guard_user_data`
  (`playlist_tracks`, `lyrics`, `plays`, `loves`) must drop and recreate the trigger
  around it, as 10 does: `ALTER TABLE … RENAME` refuses while a trigger names the
  dropped table.

## Invariants

- **Nothing user-side is lost to a scan.** `clear()` never touches user tables, and the
  `media_items_guard_user_data` trigger makes it structural: deleting an item referenced
  by `playlist_tracks`, network lyrics, `plays` or `loves` aborts.
  `SWEEP_UNREFERENCED_ITEMS` mirrors the trigger's conditions exactly; if they ever
  diverge, the sweep aborts the scan's last transaction instead of deleting user data.
- **Ids are stable across rescans and never reused.** `media_items.id` is
  `AUTOINCREMENT`: without it SQLite hands out `max(rowid) + 1`, so sweeping the newest
  item would give its id to the next new track, and anything holding the old id (the web
  remote before its refetch, a persisted queue) would show a different song.

## Decisions

Owner's calls; ask before changing.

- The catalog is cleared and refilled by every scan, not diffed; ids and user data
  survive through the identity layer.
- User tables keep the column name `track_id` though it holds `media_items.id`: renaming
  would touch every query for no gain.
- Two items that are both alive and missed each other are never merged automatically;
  that is for a manual screen.
- Artist names match case-insensitively, album titles exactly: tags differ in case far
  more often than two real artists do.
- Album identity follows Navidrome's default with the track year out of the key; the
  album artist and position collisions split, they do not key (2026-10-17).
- Not-found artists are retried only when the item snapshot's `updated_at` moves;
  credits the snapshot does not hold (a second track artist, an album-artist tag added
  to existing tracks) trigger nothing: catching them would need per-row state and a
  migration.
- No table for failed analyses and no persisted state beyond the rows above.
- No compilation or Various Artists flag and no feat.-splitting of artist names; album
  artists come from the tag, else the shared-primary inference in `album_artists.rs`,
  whose marker list is closed: ask before extending name-based rules.



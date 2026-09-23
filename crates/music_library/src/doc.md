# music_library

The SQLite-backed library: the catalog the screens browse, the user data that
hangs off it (playlists, likes, fetched lyrics, listening history, scrobble
deliveries), and the identity layer that keeps the two apart. Only this crate
touches the database; `pawse::library_service` drives scans through
[`ScanWrite`] and everything else goes through [`LibraryRepository`].

## Files

- `lib.rs` — crate root, re-exports, and the bulk of the tests (GUI-less,
  against a temp database). The migration tests build a real schema-v8 database
  by replaying `MIGRATIONS[..=8]` and seeding it, then open it through
  `SqliteLibrary::open_at`.
- `repository.rs` — the `LibraryRepository` and `ScanWrite` traits.
- `sqlite.rs` — the only implementation: `SqliteLibrary` (main + scrobble
  connections), `ScanSession` (the batched scan writer on its own connection),
  the migration runner, and the SQL shared between them (`CLEAR_CATALOG`,
  `RETIRE_UNSEEN_LOCAL_BINDINGS`, `SWEEP_UNREFERENCED_ITEMS`,
  `REFRESH_ITEM_SNAPSHOTS`).
- `migrations.rs` — `MIGRATIONS`, the versioned schema steps.
- `models.rs` — row and transfer types (`Track`, `ScanTrack`, `LocalFolder`, …).
- `adoption.rs` — the pure matching that re-attaches moved files to their old
  items (`match_arrivals`), plus `normalize_tag`, the one tag normalization
  shared with the scrobble like-import.
- `album_artists.rs` — deriving an album's credited artists from its tracks.
- `thumbnail.rs` — cover thumbnail generation.
- `error.rs` — `LibraryError`.

## Two kinds of rows

**Catalog** — `tracks`, `albums`, `artists`, `genres`, `track_artists`,
`track_album_artists`, `track_genres`, `album_artists`, `cover_art`, and
disk-derived lyrics (`lrc` / `embedded`). Everything here is re-derivable from
the files. A scan is still `clear()` + refill for this part, and that is fine:
`clear()` touches nothing else.

**Identity and user data** — `media_items`, `media_bindings`, `sources`,
`playlist_tracks`, `lyrics` from the network, `plays`, `loves`, and their
`*_deliveries`. None of it is derivable, so no scan deletes it.

The link between them is the id. `media_items.id` is the durable identity of a
track; `tracks.id` borrows it (`tracks.id REFERENCES media_items(id)`), so a
rescan that deletes and refills `tracks` puts every row back under the id it
had. Every user table references `media_items`, never `tracks`. The column is
still called `track_id` in those tables — the values are identical and
renaming would have been churn across every query for no behavioural gain.

## Identity: sources, bindings, items

- `sources` — one row per library root. For now only `kind = 'local'`, one per
  configured music folder (`uri` = the folder path). Source `1` is a disabled
  placeholder that migration 9 hangs every pre-existing binding on, because the
  migration cannot see `settings.json`.
- `media_bindings` — "this item is available at this place": `(source_id,
  source_key, start_offset_ms)` is unique, `source_key` is the absolute file
  path. `present` and `last_seen_scan` record whether the last scan saw it.
- `media_items` — the identity, plus a display snapshot (`title`, `artist`,
  `album`, `duration_ms`, `cover_art_id`) used to render an item that has no
  `tracks` row any more.

`reconcile_local_sources(folders)` makes `sources` match the configured folders:
it disables every local source, then upserts one enabled row per folder with its
`available` flag. A folder removed from settings therefore becomes a *disabled*
source whose bindings stay put — re-adding the same folder re-enables it and
every binding matches again by path, so likes and playlist entries come back
under their old ids.

`ScanSession::resolve_item` looks a scanned file up by `(path, start_offset_ms)`
across **all** local sources (placeholder and disabled ones included), re-points
the binding to the longest matching enabled root, and only mints a new item when
nothing matches. That is what makes the placeholder self-healing: the first real
scan after migration moves every binding onto the right folder without a
separate step.

## Offline folders

`library_service` probes every configured folder before a scan (`ScanScope`): a
folder that cannot be listed, or is empty while bindings under it are still
present (an unmounted mount point), is *unavailable*. The scan goes ahead over
the rest. The unavailable folder's source gets `available = 0`, its bindings are
not retired, and its items are neither swept nor offered for adoption — offline
is not the same as deleted. Its tracks drop out of the catalog (the scan did not
re-add them), so they disappear from the library screens and show as unavailable
in playlists and likes until the folder is back; then the next scan finds every
binding by path and they return under the same ids.

With every folder offline the scan still runs and the catalog ends up empty:
offline means hidden, whether one folder is gone or all of them. Likes and
playlists keep their entries, shown as unavailable.

The fast-path key (`scan_meta.folders`) marks offline folders with a `?` prefix,
so going offline and coming back each force one real scan. With every folder
online the key is the same plain sorted list it always was.

## Moved and renamed files: adoption

A scanned file whose `(path, start_offset_ms)` has no binding is an *arrival*.
Before minting a new item for it, the session looks for an *orphan* to re-attach
it to: an item that has no binding seen by this scan and no present binding on an
enabled source this scan did not cover (an offline folder, later a server).
Orphans include items without user data too, so an ordinary move keeps its id
(the persisted queue and the web remote hold ids).

Orphans are only known once the scan has seen everything, so while the database
already has items, arrivals are held in memory and settled in `finish`. On a
fresh database there is nothing to adopt and arrivals are inserted directly —
that keeps the first scan of a big library from buffering all of it.

`adoption::match_arrivals` runs three tiers, strongest first, each over every
unmatched arrival before the next tier starts:

1. `path` — same path relative to its folder root and same cue offset, duration
   within 2 s when both are known. Covers a library moved to a new mount point,
   including untagged files.
2. `tags` — normalized first artist + title + album, duration within 5 s when
   both are known.
3. `title` — normalized first artist + title, duration within 5 s and required.

Tag tiers need a non-empty artist: an untagged file's title is its file name,
and matching those across folders would pair unrelated `01.flac`s. Orphan tags
come from the `media_items` snapshot, which `refresh_item_snapshots` keeps
current for every live item — so no match keys are stored, none go stale after a
tag edit, and none are lost when a source's bindings go. Among candidates the
closest duration wins, then the lowest item id; an adopted orphan leaves the
pool. Ambiguity picks the best candidate instead of giving up: a wrongly
restored like is visible and one click to undo, a missed one is silent.

Adoption only ever inserts a binding (plus an `adoptions` row with the tier);
it never writes a user row and never merges two live items — a copy next to its
original is an arrival while the original is seen, so it gets its own item.

An adopted item keeps its old, retired binding, so the old file can come back
(restored from the trash, a folder re-added) while the new one is still there.
Both then resolve to the same item, and `tracks.id` can hold only one of them.
The session tracks which items it has already written (`written`); a second file
resolving to a written item has its old binding dropped and gets a fresh item.
One of the two keeps the id and the user data, the other is an ordinary new
track — neither disappears from the library.

A held arrival that fails to write is logged and skipped, as `add_track` errors
always were; it must not fail `finish`, or retire, sweep and the fingerprint would
never run again for as long as that file is there.

### Reviving from files already in the library

Arrival matching only sees new files. A copy made *before* the original went
away was an arrival while the original was still seen, so it got its own item;
once the original is deleted its like is dead and the copy is no longer new. So
whenever a scan had at least one arrival, `revive_orphans_from_library` runs a
second pass: the orphans that still carry user data are matched, with the same
tiers, against every track in the catalog that carries none
(`UNCLAIMED_TRACKS`). A match *absorbs* the live item into the orphan: its
`tracks` row, catalog links, disk lyrics and bindings move to the orphan's id
(foreign keys deferred for the move), and the now-empty item is deleted. Nothing
user-side is merged — the absorbed item had none — and the queue finds the file
again by path. It costs one query per side and an in-memory match, and a scan
without new files never runs it.

The scan writer opens its batches with `BEGIN IMMEDIATE`. The adoption passes
start a batch with reads; in WAL mode a deferred transaction that reads, sees the
UI commit a like, and then writes fails with `SQLITE_BUSY_SNAPSHOT`, which the
busy timeout does not retry. Taking the write lock up front makes the UI wait
instead.

## What a scan does to identity

`ScanSession::finish` (only after the indexer reported `Complete` — see below):

0. Settles held arrivals (adoption, above).
1. `RETIRE_UNSEEN_LOCAL_BINDINGS` — bindings of enabled, available local
   sources that this scan did not see get `present = 0`. Disabled and offline
   sources are left alone.
2. `SWEEP_UNREFERENCED_ITEMS` — deletes items that have no `tracks` row, no
   present binding on an enabled source, and nothing user-side pointing at them.
   This bounds growth: removing a 50k-track folder leaves only the items somebody
   liked, listened to, filed in a playlist or fetched lyrics for.

`library_service::run_scan` drops the session without calling `finish` when the
indexer stopped before `Complete` (a panicked worker closes the channel). A
partial enumeration must never retire bindings or record a fingerprint.

## Invariants and what enforces them

- **Nothing user-side is lost to a scan.** `clear()` never touches user tables,
  and the `media_items_guard_user_data` trigger makes it structural: deleting an
  item referenced by `playlist_tracks`, network lyrics, `plays` or `loves`
  aborts. `SWEEP_UNREFERENCED_ITEMS` mirrors the trigger's conditions exactly;
  if the two ever diverge the sweep aborts the scan's last transaction instead of
  deleting user data.
- **Ids are stable across rescans and never reused.** `tracks.id` is borrowed,
  not minted, and `media_items.id` is `AUTOINCREMENT`: once the sweep deletes an
  item its number is retired. Without that SQLite hands out `max(rowid) + 1`, so
  sweeping the newest item would give its id to the next new track, and anything
  still holding the old id (the web remote before its refetch, a persisted queue)
  would show a different song.
- **Likes have one source of truth.** The hidden playlist (its id in
  `scan_meta.liked_playlist_id`). `Track.liked` is computed through the
  `liked_track_ids` view in `TRACK_COLUMNS`; there is no `liked` column.
- **Lyrics precedence is unchanged:** a disk lyric found by a scan overwrites a
  fetched one (`ON CONFLICT DO UPDATE` in `insert_track`); `clear()` removes
  disk lyrics so a deleted `.lrc` disappears; fetched lyrics survive.

## Unavailable tracks

`track_artists_map` falls back to the item snapshot's artist for ids that have
no `track_artists` rows (i.e. unavailable ones), so a greyed playlist row still
shows who it is. It binds every id through one JSON parameter (`json_each`)
rather than one placeholder per id: callers pass the whole library, and SQLite
caps bound parameters at 32766.

`tracks_for_playlist` (and so `liked_tracks`) returns every entry, joined
through `media_items`: an entry without a `tracks` row comes back with
`available = false`, its title/duration/cover from the item snapshot and its
path from a binding. Returning every entry is deliberate — positions in
`playlist_tracks` must line up with what the UI shows, or
`move_track_in_playlist` would move the wrong row. Catalog reads (`all_tracks`,
albums, artists) only ever see `tracks`, so unavailable items never appear there.

`refresh_item_snapshots` copies the current catalog values into the snapshot. It
runs in `settle_derived_rows` **before** the orphan sweep: the sweep keeps any
cover a snapshot still references, so refreshing afterwards would keep a replaced
cover alive forever.

## Migrations

There are users: schema changes are versioned steps in `MIGRATIONS`, never a
wipe. `run_migrations`:

- backs the database up before crossing `IDENTITY_MIGRATION` (9), once:
  `VACUUM INTO <db>.bak-v<old>.partial`, then an atomic rename to
  `<db>.bak-v<old>`. An existing `.bak-v<old>` is kept; a leftover `.partial`
  (crash mid-backup) is never trusted and gets rewritten;
- turns `foreign_keys` **off** for the migration transaction. This is required,
  not a shortcut: migration 9 rebuilds tables (`CREATE x_new`, copy, `DROP x`,
  `RENAME`), and with foreign keys on, `DROP TABLE tracks` would cascade into
  every child that references `tracks` by name — including the freshly copied
  new tables. The pragma cannot change inside a transaction, so it is set before
  `BEGIN` and restored after;
- counts `pragma_foreign_key_check` rows before and after the steps and rolls
  back only if the migration **added** violations. Pre-existing junk in someone's
  catalog (a dangling `track_artists` row) must not turn every launch into a
  failed open.

Migration 9 keeps ids one-to-one (`media_items.id := tracks.id`), so user rows
are copied as-is and can be verified by counts and sums. Child tables that
reference a rebuilt parent only by name (`track_artists`, `play_deliveries`, …)
are not rebuilt: after the rename they resolve to the new parent. Before the SQL
runs, `seed_liked_playlist_from_column` covers databases old enough to have no
hidden liked playlist, since the `liked` column they depend on is dropped.

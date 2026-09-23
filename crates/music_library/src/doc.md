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
- `models.rs` — row and transfer types (`Track`, `ScanTrack`, `NewPlay`, …).
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

`reconcile_local_sources(roots)` makes `sources` match the configured folders:
it disables every local source, then upserts one enabled row per root. A folder
removed from settings therefore becomes a *disabled* source whose bindings stay
put — re-adding the same folder re-enables it and every binding matches again by
path, so likes and playlist entries come back under their old ids.

`ScanSession::resolve_item` looks a scanned file up by `(path, start_offset_ms)`
across **all** local sources (placeholder and disabled ones included), re-points
the binding to the longest matching enabled root, and only mints a new item when
nothing matches. That is what makes the placeholder self-healing: the first real
scan after migration moves every binding onto the right folder without a
separate step.

## What a scan does to identity

`ScanSession::finish` (only after the indexer reported `Complete` — see below):

1. `RETIRE_UNSEEN_LOCAL_BINDINGS` — bindings of enabled local sources that this
   scan did not see get `present = 0`. Disabled sources are left alone.
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

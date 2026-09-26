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
- `remote.rs` — the locator format for server tracks (`pawse-source://…`) and
  `location`, which tells a file path from a server track from a broken locator.
  Code holding a `Track` asks the track instead of parsing its path:
  `Track::location`, `remote`, `is_remote`, `local_file`, and `own_file` (a
  local file that is this track's alone — not a cue piece — i.e. the one whose
  tags and sidecar lyrics belong to it). Parsing a bare locator string is left to
  the layers that only have strings (`pawse::remote_media`, playback's
  `playback_locators` walk).
- `adoption.rs` — the pure matching that decides which files are the same track
  (`match_tracks`), plus `normalize_tag`, the one tag normalization shared with
  the scrobble like-import.
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
separate step. That scan must actually happen: `has_unplaced_media()` reports
present bindings still on the placeholder, and `library_service` skips the
fast path while it is true. So `finish` retires (`present = 0`) every placeholder
binding that is under no enabled root, or under an available one (which this scan
walked completely and did not find it in). Only bindings under an offline folder
stay unplaced. A liked file deleted before the upgrade would otherwise keep the
fast path off forever. Retiring loses nothing: the placeholder is disabled, so
`present` there affects neither playback nor matching, and a file that comes back
is still found by its key and moved onto its folder.

## Offline folders

`library_service` probes every configured folder before a scan (`ScanScope`): a
folder that cannot be listed, or is empty while bindings under it are still
present (an unmounted mount point), is *unavailable*. The scan goes ahead over
the rest. The unavailable folder's source gets `available = 0`, its bindings are
not retired and its items are not swept — offline is not the same as deleted.
They stay candidates for matching, though: the same file showing up somewhere
else (the folder renamed on disk and added again, a backup copy) joins them. Its tracks drop out of the catalog (the scan did not
re-add them), so they disappear from the library screens and show as unavailable
in playlists and likes until the folder is back; then the next scan finds every
binding by path and they return under the same ids.

With every folder offline the scan still runs and the catalog ends up empty:
offline means hidden, whether one folder is gone or all of them. Likes and
playlists keep their entries, shown as unavailable.

The fast-path key (`scan_meta.folders`) marks offline folders with a `?` prefix,
so going offline and coming back each force one real scan. With every folder
online the key is the same plain sorted list it always was.

## Identity across sources

**The rule.** An item has at most one live binding per source. Two files in one
source are two items (a copy next to its original stays a separate track); the
same recording in different sources — two folders, a folder and a server, two
servers — is one item with one binding in each. Which binding plays and names
the catalog row is priority, not identity: local folders first, then every
server (Subsonic, Jellyfin — no kind outranks another), each by source id
(`project_remote_tracks`, `playback_locators`, `ScanSession::place`).

**What a match needs.** Only what is in the database, so a source that is
offline, disabled or gone still takes part:

- `media_bindings.file_size` (from the indexer's stat, from the server's `size`)
  with the cue offset — the same bytes, even untagged;
- the `media_items` snapshot (first artist, title, album, duration), which
  `refresh_item_snapshots` keeps current while an item is seen and which
  freezes when it is not.

Relative paths are not used: a server's path is its own invention, and anything
a path found between folders the size finds too.

`adoption::match_tracks` runs three tiers, strongest first, each over every
unmatched track before the next starts:

1. `file` — a shared `(size, cue offset)`, duration within 2 s when both are
   known.
2. `tags` — normalized first artist (or an alias) + title + album, duration
   within 5 s when both are known. The same size wins among several, then the
   closest duration, then the lowest id.
3. `title` — normalized artist + title, duration within 5 s and required, and
   only for a candidate with no present binding at all (truly dead). An offline
   item is never merged with another recording that merely shares a title.

A candidate that already has a live binding in the arriving track's source is
skipped. Within a tier every passing pair is ranked first (same file, then
closest duration, then lowest id) and assigned best-first, so the order tracks
arrive in never decides who gets a candidate; a claimed candidate leaves the
pool. Tag tiers need a non-empty
artist, so untagged `01.flac`s only ever match by size.

**When matching runs — two events.**

- *Birth*: a file or song the source has no binding for. Local arrivals are held
  until `finish` (the candidates are only known once the scan has seen
  everything; on an empty database with one folder there is nothing to match and
  they are inserted directly) and settled source by source in id order, so a
  second new folder in the same scan sees the items the first one just created.
  Server arrivals are matched in `apply_remote_listing`. Candidates are all
  items, live, dead or offline. A match only inserts a binding and an
  `adoptions` row.
- *Death*: after local bindings are retired, `revive_lost_items` takes the items
  that carry user data and have no playable binding left (file deleted, folder
  offline or removed, server gone) and matches them against the playable items
  that carry none. A match *absorbs* the spare item: its `tracks` row, catalog
  links, disk lyrics and bindings move to the lost item's id (foreign keys
  deferred for the move) and the empty item is deleted. Nothing user-side is
  merged — the absorbed item had none. This covers a copy made before the
  original went away, and a copy that only existed as a separate item because
  its tags differed.

Everything is loaded once per pass (`load_identities`: items, bindings with
their source state, and the set with user data) and matched in memory, so a
pass costs two queries whatever the library size.

Two items that are both alive and both missed each other (older data, a tag
edit that made them equal later) are never merged automatically; that is for a
manual screen.

**One row per item.** A second binding of an item already written this scan
(the same file in another folder) does not write another row — unless its source
has priority, and then the row is rewritten from that file entirely (tags, year,
cover, `is_cue`, disk lyrics), so the row never depends on which file the
parallel indexer delivered first. A second binding in a source the item was
already placed in this scan (`placed`: an adopted item whose old file came back
from the trash) gets its old binding dropped and a fresh item, so neither file
disappears from the library. The same `(path, start_offset_ms)` delivered twice in
one scan is not such a case: nested folders in the settings walk the file twice,
and two cue sheets for one image yield the same tracks twice. `seen` drops the
repeat before anything else, or it would split off a fresh item every scan and
leave the catalog row on an item without a binding.

A held arrival that fails to write is logged and skipped, as `add_track` errors
always were; it must not fail `finish`, or retire, sweep and the fingerprint would
never run again for as long as that file is there.

The scan writer opens its batches with `BEGIN IMMEDIATE`. The matching passes
start a batch with reads; in WAL mode a deferred transaction that reads, sees the
UI commit a like, and then writes fails with `SQLITE_BUSY_SNAPSHOT`, which the
busy timeout does not retry. Taking the write lock up front makes the UI wait
instead. A batch opens only when there is something to write (a held arrival
writes nothing) and closes after 256 writes or 250 ms, and `library_service`
calls `flush` whenever it has caught up with the indexer (its channel is empty).
So the lock is not held while the indexer parses the next files, and other
writers — a like, a server listing — get in between batches. `finish` is one transaction: absorbing an item moves rows
under deferred foreign keys, which only works inside one, and a server listing
that got in while held arrivals are being settled would not see them and mint a
second item for a song whose local copy is about to be written. The price is
that `finish` holds the lock for as long as writing every held arrival takes —
about 65 ms for 2000 new files, seconds for a first scan of tens of thousands,
during which a like waits on the busy timeout.

The UI-side writers that read before they write (`set_liked`, `like_many`,
`add_track_to_playlist`, `remove_track_from_playlist`, `move_track_in_playlist`)
open their transaction with `BEGIN IMMEDIATE` for the same reason: a deferred
transaction that already read is refused `SQLITE_BUSY` at once while a scan batch
or a server listing holds the lock, without the busy timeout ever waiting, and the
like was silently lost.

## Server sources (Subsonic, Jellyfin, torrents)

A server is a `sources` row with `kind = 'subsonic'`, `'jellyfin'` or `'torrent'` and
`uri = user@url` (a torrent: `btih:<info hash>`) (`reconcile_remote_sources(kind, …)` enables the configured ones
of that kind and disables the rest of that kind only). Nothing in this crate
depends on the kind beyond `'local'` versus not; the protocol lives in `pawse`.
Its songs are bindings like files, with `source_key` = the server's song id.

`apply_remote_listing` records a full listing in one `BEGIN IMMEDIATE`
transaction on its own connection (a deferred one that reads first fails with
`SQLITE_BUSY_SNAPSHOT` whenever a scan batch or a like commits in between):
covers arrive with the listing and are inserted in the same transaction, so the
orphan-cover sweep of a concurrent scan cannot delete them before `remote_tracks`
names them; known songs refresh their `remote_tracks` row (the upsert only writes
when something differs, which is how `RemoteSyncReport::changed` knows whether a
rescan is needed); songs no longer listed get `present = 0`; new songs are born
against every item that has no binding this listing accounts for — so a song
the server renamed (new id) finds its old item. An empty listing while the server still had present songs is refused, not
applied: a server whose library is unmounted must not retire everything.

Server songs carry artist aliases (the joined display name and the other
credited artists), because Navidrome splits "A feat. B" into two artists that a
local file keeps as one string. A server copy of a file you already have lands
as a second binding on the same item — usually by size, since `download` serves
the original bytes — and one item still means one catalog row.

`remote_tracks` is the server listing cached per binding (tags, duration, suffix,
the cover's content hash). The catalog is still cleared and refilled by every
scan; `ScanSession::project_remote_tracks` runs at the end of each one and
inserts a `tracks` row for every present server binding on an enabled, available
server whose item did not get a row from a local file. So local always wins,
a server going away hides only what it alone provided, and projecting needs no
network. Server rows use a locator path, `pawse-source://<source_id>/<key>.<suffix>`
(`remote.rs`), for every kind of source: the source id says which one, so the
locator carries no protocol. The suffix is there because the decoder picks a
backend by extension. Locators from before the rename (`subsonic://…`) still
parse; migration 12 rewrites them in `tracks`, and `pawse` rewrites the saved
queue on load (`remote::canonical`), because the queue is matched to the catalog
by `(path, start_offset_ms)` and an unmatched entry is dropped from it.

Servers that do not read cue sheets (Navidrome) list a whole-disc image as one
song, while the local scan splits the same file into its cue tracks. Such a song
never matches anything (no single track lasts a whole disc), so the projection
skips a server binding whose item has no binding elsewhere and whose `file_size`
equals a cue track's (`start_offset_ms > 0`) that is playable in another source
(enabled, available, present). While that folder is offline, removed, or the cue
image is gone, the server image shows up as one long track instead. A server that does
split cues lists every track with the image's size and no offset, so file keys
say whether they name a whole file or a cue piece (`WHOLE_FILE` vs the offset;
a local binding is a piece when its file has any track at an offset). Media
servers list whole files, so their songs never file-match a local cue track and
join it by tags instead.

A source that reads cue sheets itself (a torrent, indexed by `music_indexer`)
lists every cue track as its own `RemoteSong` with `start_offset_ms = Some(…)`
(`Some(0)` for the first track) and the image's key and size. The binding stores
the offset, so a listing is matched to its bindings by `(key, offset)`, and one
key with several offsets is several items, exactly like a local cue image.
`None` means a whole file. A cue piece file-matches the local cue track of the
same image by `(size, offset)`. The projection writes the binding's offset into
`tracks.start_offset_ms` and marks it `is_cue` when the key has any track at an
offset (`REMOTE_BINDING_IS_CUE`); cue pieces are never hidden by the
whole-image rule above. No schema change: `media_bindings.start_offset_ms`
already existed and was always 0 for servers.

`playback_locators(item)` lists every place an item can play from right now —
present bindings on enabled, available sources, local ones first, server ones as
locators. The catalog row only names one of them and is refreshed by the next
scan, so when a local file has vanished since, playback tries the next entry
instead of failing.

The local adoption pool (`ORPHANED_ITEMS`) looks only at **local** bindings: an
item held only by a server is adoptable by a local file with the same path or
tags (it is `live`, so not by title alone), which is the other direction of the
same dedupe. Items held by an offline local folder are still excluded, and the
revive pass only considers dead items.

Covers from a server are stored like any other cover; the orphan-cover sweep
keeps a cover while any `remote_tracks.cover_hash` names it, so an offline
server does not lose its artwork.

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

## Stable album and artist ids

The catalog is cleared and refilled by every scan, and after a `DELETE` SQLite
would hand out ids from 1 again in whatever order the parallel indexer delivered
files — every scan reshuffled them, and a screen still holding an old id (an
open artist page, the web remote) linked to a different album. So
`ScanSession::clear` snapshots the ids first (`StableIds`): an album gets its
old id back by `(title, year)` — the same key `resolve_album` merges on — and
an artist by name. A new one gets an id above everything ever handed out
(`allocate_catalog_id`); the high-water marks live in `scan_meta`
(`album_id_high`, `artist_id_high`), so a removed album's id is not given to
the next new one. The snapshot raises the marks to its own maximum before the
clear, and every writer allocates through the same function — the scan and the
tag editor's `upsert_album` / `get_or_insert_artist` — so a tag edit that lands
between two scan batches can neither reuse a dead id nor take one the scan is
about to hand back. Genres are not covered:
nothing outside the database holds their ids.

Artists are keyed by the lowercased name (`artist_key`): `Cage the Elephant`
and `Cage The Elephant`, or `MUSE` and `Muse`, are one artist, shown under the
name met first. Tags differ in case across sources and rips far more often than
two real artists do. Lowercasing is Rust's (Unicode), not SQLite's `NOCASE`
(ASCII only), so the tag editor's `get_or_insert_artist` compares in Rust too.
Albums still merge on the exact title.

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

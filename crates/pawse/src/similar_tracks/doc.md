# similar_tracks

Tracks that sound alike, from `audio_embedding` vectors: background analysis of the
library, a nearest-neighbour search, a re-ranking step that turns neighbours into a
list worth listening to, and two queue actions built on them — "radio from this
track" and "mix other artists into the queue" — in a menu in the queue panel's
header. There is no progress display for the analysis.

The switch is in Settings → General, "Sound-based
recommendations" (`similar_tracks_enabled`, off by default; `tr().similar_tracks`).
Its description talks about the result (similar-sounding tracks from the library), says
the analysis is local and that the first pass can take a while; it does not mention the
background thread or the one-time model download (16 MB). Turning
it off stops the thread within one track and cancels a model download in progress.

## Files

- `mod.rs` — `SimilarTracks` (a cheap `Clone` handle on the shared state), the `State`
  global holding the running instance, `setup`, `set_enabled` (the switch), `current`
  (the running handle, if any), `is_running` (the same without the clone, for render)
  and `shutdown`.
- `worker.rs` — the analysis thread: model, candidates, decoding, saving.
- `neighbors.rs` — `Neighbors`: the cached library mean, a track's vector, the mean of
  several tracks' vectors, and `search`, which answers any number of
  `audio_embedding::similarity::Query`s in one streaming pass.
- `rerank.rs` — `Rerank`, the pure re-ranking step (no database, no GPUI), `Candidate`,
  `Taken` (what the list already holds), `Weights`, and `Familiarity` (the menu's
  "familiar / any / new" choice, persisted as `similar_familiarity`).
- `pool.rs` — `Pool`: turns search results into `Candidate`s (track rows, first
  artist, play stats) and the re-ranked ids back into `Track`s.
- `radio.rs` — `SimilarTracks::radio`: the queries and re-ranking for a radio.
- `mix.rs` — `SimilarTracks::mix`: the same for mixing other artists into a queue, and
  `GAP` (how many tracks go after each one).
- `actions.rs` — `start_radio` / `mix_queue`: snapshot the queue, build the list on
  the background executor, apply it to `PlaybackQueue` if the queue has not moved on
  meanwhile, or show why nothing happened.
- `menu.rs` — `queue_menu`, the popover in the queue panel's header.

## Lifecycle

- `setup` (after `Services`, next to the other bridges) installs the `State` global and
  the library-event subscription, and starts the analysis if the setting is on.
  `set_enabled(true)` (the switch) starts it the same way: a new thread with its own
  `Shared` and the first pass queued. `set_enabled(false)` and `shutdown` (from
  `on_app_quit`) set that instance's stop flag and drop its request channel; the
  thread finishes the track in hand (or aborts the model download between two reads)
  and exits. Vectors already saved stay, so turning it back on only analyses what is
  missing. A quick off/on can leave the old thread finishing one track next to the new
  one; both only add rows. Two threads never download at once: `ensure` lets one
  download at a time, so the new thread waits for the old one to cancel and then
  downloads (or finds the file) itself.
- The model lives in `dirs::data_dir()/pawse/models/` — not the cache dir, which users
  and the OS clear.
- The first pass downloads the model if needed (`audio_embedding::model_file::ensure`,
  blocking, on this thread), loads it, and drops vectors of other versions
  (`prune_embeddings`). A failed download or load is logged and retried every 5
  minutes (`LOAD_RETRY`, the thread waits on its request channel with that timeout
  while it has no model) or sooner on the next library event, so starting offline only
  postpones the analysis. `ensure` trusts a file of
  the right size without hashing it; when such a file does not load,
  `discard_if_corrupt` hashes it and deletes it if the SHA is wrong, so the next pass
  downloads it again (a genuine file that tract rejects is kept, not re-downloaded on
  every pass).
- `ScanComplete`, `CatalogChanged` and `RemoteSyncFinished` queue another pass. The
  queue is a `bounded(1)` channel: passes never overlap, and any number of events
  during a pass collapse into one follow-up pass, which picks up whatever the
  catalog gained meanwhile. A scan sends `ScanComplete` and `CatalogChanged` back
  to back, so it usually costs two passes; one with nothing to do is the candidates
  query plus a file check per server track not in the cache, and logs nothing.
- The stop flag is checked between tracks.

## The analysis thread

- Long CPU work owns its thread, like the indexer's pool, but **one** thread: the owner
  does not want the machine to spin up for this. It runs at normal priority, by the
  owner's decision: lowering it needs OS calls (`unsafe`), and parking those in a
  crate only because `unsafe` is allowed there is not worth it.
- A pass: `embedding_candidates(EMBEDDING_VERSION)` — liked first, then recently
  played, then the rest by album — and for each track:
  - a local file (`Track::local_file`) is opened with `audio_decoder::Decoder`;
  - a server track is analysed only when its file is already in the media cache, via
    `RemoteMedia::peek_cached`, never `cached`: that one touches the file for the LRU,
    and walking the library would reorder what gets evicted. A server track not in the
    cache is skipped and counted (fetching ranges from the server is a later phase);
  - a cue track is decoded over `start_offset_ms` .. `+ duration_ms` (`TrackRange`), a
    plain file whole.
  - `prepare` then `embed`, one track at a time (batching is a later phase: `prepare`
    in a small pool with lanes per source kind, `embed` over whatever is ready).
- Results are saved every 16 tracks (`save_embeddings`, which also skips items a scan
  swept meanwhile, on `music_library`'s own embedding connection, so waiting for a
  scan's write lock never blocks the UI's reads), and every save invalidates the
  cached mean. Every pass invalidates it too when it starts: passes follow catalog
  changes, and a scan that swept items took their vectors out of the mean.
- A track that fails (decode error, under one second, a non-finite vector, a panic
  in decoding or inference — caught per track, so one broken file does not end the
  session's analysis) is logged with
  `warn!` and remembered in memory for this session only: no table of failures, the
  next launch tries again. A model error or a database error stops the pass with
  `error!`. There are no user-facing notices in v1; progress goes to the log every 100
  tracks and at the end of a pass.

## Search

`Neighbors::search(repo, queries)` streams every vector of the current version
through one `TopN` per query in `SCAN_CHUNK` (1024) rows (`scan_embeddings`); nothing
but a ~5 MB buffer and the heaps is in memory, whatever the library size. A query
knows only a vector, `n`, nearest or farthest, and ids to skip; deciding which ids
and why is the caller's job. The centering mean (5 KB) is cached and computed the
same streaming way on the first query after an invalidation. It is computed under its
mutex, so `invalidate` (the worker, after a save and at the start of a pass) waits for
a computation in flight and then clears it; a mean never outlives the save that made
it stale, and deletions are caught up by the next pass. A track without a vector (or
with one of the wrong length) has no `vector`. `mean_of` is one more streaming pass
that sums the vectors of the given ids, so its cost does not grow with a long queue;
`None` means none of them had one. Only catalog tracks are scanned
(`scan_embeddings`), so vectors of tracks on an offline source never take places in
a top-N.

## Re-ranking

Nearest neighbours alone are the seed's own artist: in the PoC (EffNet multi, 2184
tracks, 49 artists) the median top 10 held 8 tracks of the same artist, and 74 seeds
had no other artist at all in their top 40. So the candidates are re-ranked, greedily,
one pick at a time. A candidate's value is

`score + familiarity bonus − recent penalty + noise − artist × (tracks of its artist
already taken) − repeat × (same artist as the previous pick)`

- `Taken` starts with the seed (radio) or empty (mix) and grows with every pick, so the
  artist penalty spreads an artist through the list instead of capping it; there is no
  hard per-artist limit. Album is not a signal: the artist term already covers it.
- The same first artist + normalised title is taken once (single and album version);
  skits (under `MIN_DURATION_MS`, 60 s), unavailable tracks and tracks scoring below
  `MIN_SCORE` (0: less alike than a random pair, whose median was −0.05 in the PoC)
  are dropped, so a small library gives a shorter list rather than one padded with
  the opposite sound; tracks without an artist are neither penalised nor
  deduplicated.
- History (`play_stats`): played within `RECENT_SECS` (a day) costs `recent`;
  `Familiarity::Familiar` adds `familiarity` to tracks played or liked before, `New`
  to the others, `Any` neither.
- Noise is Gumbel, drawn once per candidate from an `StdRng` seeded by the caller:
  the same seed gives the same list (tests), the app passes a random one, so pressing
  again gives another list. Gumbel noise plus a greedy max is sampling from a softmax
  of the values, with `noise` as the temperature.
- `Weights::default()` (artist 0.1, repeat 0.1, recent 0.2, familiarity 0.15, noise
  0.04) came from a simulation over the PoC's real neighbour lists: on 12 picks out of
  40 neighbours the seed's artist fell from 8.0 to 3.9 tracks, distinct artists rose
  from 3.0 to 6.0, the mean similarity only from 0.78 to 0.73, and two runs shared
  about half their tracks. The gap between the best neighbour and the best one of
  another artist is ~0.08 in the median, which is the scale the penalties work at.

## Radio

`SimilarTracks::radio(seed, familiarity, rng_seed)` (blocking; background executor)
returns `None` when the seed is not in the catalog or has no vector, otherwise up to
`LENGTH` (30) tracks, the seed not included. One pass answers two queries: the
`ANY_ARTIST_POOL` (50) nearest overall, and the `OTHER_ARTISTS_POOL` (250) nearest
without any track of the seed's first artist (`same_artist_track_ids`). Without the second one a big
discography fills the whole pool and the re-ranking has no one else to pick.

`start_radio` keeps the playing track's queue entry (read again when the radio is
ready, so a refresh of that entry meanwhile is not undone) and replaces the queue with
it and the radio (`set_tracks_and_play_at`, index 0, so playback is not touched and
shuffle, if on, shuffles the radio). It asks nothing when the queue was custom: the
user picked the action from the queue panel, looking at the queue. The radio queue is
not custom. The result is dropped if another track started meanwhile.

## Mix

`SimilarTracks::mix(basis, queue, count, …)` searches from the mean vector of `basis`
(the queue's own tracks), skipping every track in `queue` and every track of the
basis tracks' first artists, so what comes in is other artists only; `None` when no
basis track has a vector. The pool is `count × POOL_PER_TRACK` (at least `MIN_POOL`),
`count` is capped here at `MAX_MIXED` (400).

`mix_queue` takes `PlaybackQueue::mix_plan()`: the basis, what stays in the queue, the
number of anchors (the current track and every own track after it) and a snapshot of
the ids. It draws `GAP` (1–3) per anchor — the owner wants the queue to become mostly
new music, not the same queue with a few extra tracks — asks `mix` for the sum, and
`mix_in` puts each anchor's block right after it, in the re-ranked order. History
before the current track is not touched, and the queue is not shuffled. If the picks
run out, the last anchors get nothing.

- `PlaybackQueue` remembers the mixed-in ids for the session (`mixed`, not persisted;
  cleared by a new queue or a restore). Mixing again first drops the mixed tracks
  after the current one, so it re-rolls instead of mixing into a mixed queue; mixed
  tracks already heard stay. They are not part of the basis.
- With shuffle on, every block also goes after its anchor in `original_order`, so
  turning shuffle off keeps each mixed track after the one it was mixed in for.
- The custom flag is left as it was: a mixed album is, like a radio, a generated list
  the next click on a track replaces without asking, while a hand-built queue stays
  protected. The source becomes `Unknown`: the queue is no longer the playlist it came
  from, and a playlist-backed queue is rebuilt from the playlist on every edit
  (`sync_queue_with_playlist`), which would silently drop the mix.
- `mix_in` refuses (and nothing changes) when the queue's ids or its current index
  differ from the plan's `MixSnapshot`: a track that ended meanwhile would shift every
  block by one anchor and could put the now-current mixed track right after itself.

## The menu

`queue_menu` is a popover on the queue panel header, left of "save to playlist",
shown only while the queue has tracks and the analysis is running. It is built like
the library view menu (`library_views::view_menu`'s `menu_surface`, `MenuColors`,
`section_label`, `separator`). Most used nearest to the trigger: radio (disabled with
no current track), mix, then the "Tracks: familiar / any / new" switch. Picking an
action closes the popover. When the list is ready, the queue view refreshes and
scrolls to the current track (`OnApplied`, built once in `QueueView::new`) before
`queue_mutated`, so the `QueueChanged` handler does not jump to the bottom the way it
does for "add to queue". A seed or queue without vectors, an empty result and a
database error each show a notification (`similar_strings()`).

## Checking by hand

Turn the switch on (or set `similar_tracks_enabled` in `settings.json`) in an isolated `HOME` (the real library and scrobbling stay out of it)
with a small folder of music, and watch `logs/pawse.log` for `similar tracks:` lines.
The quality check from the PoC is the share of "same artist in the top 10, other
albums" over the vectors in `track_embeddings` (`.plans/smart-playlists-poc/scripts/
analyze.py`), about 0.48 on `../music-test`; much lower means the audio preparation
drifted. Analysing all of `music-test` (2184 tracks) on one background thread takes
roughly 20–40 minutes.

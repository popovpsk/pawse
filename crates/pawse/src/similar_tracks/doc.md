# similar_tracks

Tracks that sound alike, from `audio_embedding` vectors: background analysis of the
library, nearest neighbours of a track, and a "radio from this track" queue. Nothing
in the UI uses the results yet and the radio is not connected to the player; there is
no progress display. Delivery to users is a separate task.

The one user-facing piece is the switch in Settings → General, "Sound-based
recommendations" (`similar_tracks_enabled`, off by default; `tr().similar_tracks`).
Its description talks about the result (similar-sounding tracks from the library), says
the analysis is local and that the first pass can take a while; it does not mention the
background thread or the one-time model download (16 MB). Turning
it off stops the thread within one track and cancels a model download in progress.

## Files

- `mod.rs` — `SimilarTracks` (a cheap `Clone` handle on the shared state), the `State`
  global holding the running instance, `setup`, `set_enabled` (the switch), `current`
  (the running handle, if any) and `shutdown`.
- `worker.rs` — the analysis thread: model, candidates, decoding, saving.
- `neighbors.rs` — `Neighbors`: the cached library mean and the streaming nearest-
  neighbour query.
- `radio.rs` — `SimilarTracks::radio` and `pick`, the pure filter that turns
  neighbours into a queue.

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

## Neighbours

`Neighbors::nearest(repo, seed, n)` reads the seed's vector, then streams every vector
of the current version through `audio_embedding::similarity::TopN` in `SCAN_CHUNK`
rows (`scan_embeddings`); nothing but a ~5 MB buffer is in memory. The centering mean
(5 KB) is cached and computed the same streaming way on the first query after an
invalidation. It is computed under its mutex, so `invalidate` (the worker, after a
save and at the start of a pass) waits for a computation in flight and then clears
it; a mean never outlives the save that made it stale, and deletions are caught up
by the next pass. A seed without a vector has no neighbours.

## Radio

`SimilarTracks::radio(seed)` is blocking: take the handle with `similar_tracks::current(cx)`
(`None` while the switch is off) and call it on the background executor. It takes the `POOL`
(200) nearest tracks and filters them with `pick`, a pure function:

- the track is still in the catalog and available;
- it lasts at least `MIN_DURATION_MS` (60 s): skits, intros and short interludes all
  sound alike and would cluster together; an unknown duration passes;
- at most `MAX_PER_ARTIST` (2) per first artist (`track_artists_map`, compared with
  `normalize_tag`), the seed counting as one — otherwise the same artist and album
  fill the list;
- the same first artist + title is played once (the single and the album version);
- tracks without an artist are neither capped nor deduplicated: there is nothing to
  tell them apart by.

The result is `[seed] + LENGTH` (30) tracks, best first. The constants are a starting
point; the final rules are decided with the delivery of the feature.

## Checking by hand

Turn the switch on (or set `similar_tracks_enabled` in `settings.json`) in an isolated `HOME` (the real library and scrobbling stay out of it)
with a small folder of music, and watch `logs/pawse.log` for `similar tracks:` lines.
The quality check from the PoC is the share of "same artist in the top 10, other
albums" over the vectors in `track_embeddings` (`.plans/smart-playlists-poc/scripts/
analyze.py`), about 0.48 on `../music-test`; much lower means the audio preparation
drifted. Analysing all of `music-test` (2184 tracks) on one background thread takes
roughly 20–40 minutes.

# similar_tracks

Tracks that sound alike, from `audio_embedding` vectors: background analysis of the
library, a nearest-neighbour search, a re-ranking step that turns neighbours into a
list worth listening to, two queue actions built on them — "radio from this
track" and "mix other artists into the queue" — in a menu in the queue panel's
header, and a queue that runs out continuing as a radio from its last track. The
analysis' progress is in a dialog behind a button in Settings.

The switch is in Settings → General, "Sound-based
recommendations" (`similar_tracks_enabled`, off by default; `tr().similar_tracks`).
Its description talks about the result (similar-sounding tracks from the library), says
the analysis is local and that the first pass can take a while; it does not mention the
background thread or the one-time model download (16 MB). The analysis runs only on a
graphics card (`audio_embedding_gpu`; no CPU fallback, owner's decision): without a
hardware adapter the switch is disabled and shown off, with "No graphics card found"
left of it, the queue menu never shows, and the rest of the player works as usual. The
setting itself is kept, so the same `settings.json` on a machine with a GPU turns the
analysis on. Turning
it off stops the analysis within one track, model run or server download (polled every
250 ms) and cancels a model download in progress; a server check (`ping`) after a
failed download is not cancellable and runs to its own timeout.

## Files

- `mod.rs` — `SimilarTracks` (a cheap `Clone` handle on the shared state), the `State`
  global holding the running instance, `setup`, `set_enabled` (the switch), `current`
  (the running handle, if any), `is_running` (the same without the clone, for render)
  and `shutdown`.
- `worker.rs` — the analysis: the pass thread (model, candidates, saving) and the
  prepare / model pipeline it runs for each pass.
- `fetch.rs` — the server half of a pass: `ServerFile`, the `Board` of per-source
  queues (order, per-source and total limits, a source given up), and `Fetcher`, the
  download threads that fill `ready` from it.
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
- `autoplay.rs` — the queue's continuation: `continue_ended_queue` (called by
  `services::advance_on_track_end`) and `set_queue_refresh`.
- `indexing.rs` — `open` (`open_indexing`), the progress dialog behind the
  "Indexing" button in Settings → General.

## Lifecycle

- `setup` (after `Services`, next to the other bridges) installs the `State` global and
  the library-event subscription, and starts the analysis if the setting is on.
  `start` first needs a graphics adapter: the first time it runs (`State::gpu` is
  `Unchecked`) it checks on the background executor (`audio_embedding_gpu::adapter_name`,
  no weights, tens of ms) and returns; when the answer comes it is logged, the analysis
  starts if the setting is still on and there is an adapter, and the windows refresh so
  Settings shows the switch's real state. The check never runs while the feature is
  off: it loads the platform's GPU drivers (Vulkan, DX12), which most users of a
  disabled feature should not pay for, and a broken driver must not take the app down
  for them. So a machine without a GPU learns it the first time the switch is turned
  on, which then shows disabled with the note.
  `set_enabled(true)` (the switch) starts it the same way: a new thread with its own
  `Shared` and the first pass queued. `set_enabled(false)` and `shutdown` (from
  `on_app_quit`) set that instance's stop flag and drop its request channel; each of
  the pass's threads finishes the track or model run in hand (or the model download
  is aborted between two reads) and the thread exits. Vectors already saved stay, so
  turning it back on only analyses what is missing. A quick off/on can leave the old
  pass finishing its runs in hand (two at most, on the GPU) next to
  the new instance loading its model; both only add rows. Two threads never download at once: `ensure` lets one
  download at a time, so the new thread waits for the old one to cancel and then
  downloads (or finds the file) itself.
- The model lives in `dirs::data_dir()/pawse/models/` — not the cache dir, which users
  and the OS clear.
- A pass lists the candidates first; with nothing to analyse it ends there, without
  touching the GPU or the network. Otherwise it checks the adapter again (the worker
  cannot see GPUI's state), downloads the weights if needed
  (`audio_embedding::model_file::ensure`, blocking, on this thread), opens the GPU
  (`audio_embedding_gpu::open`: kernels, weights, the probe; ~0.1–0.4 s) and drops
  vectors of other versions (`prune_embeddings`), analyses, and **drops the `Device` at
  the end of the pass**: its buffers are ~220 MB of GPU memory, and passes are rare
  (library changes), so the app does not hold them in between. `Model` is `Unloaded`
  (closed; the next pass with work opens it), `Missing` (the last attempt failed: the
  thread then wakes every `LOAD_RETRY` to try again, until a pass has nothing to do),
  `Faulted` (the GPU failed mid-pass; the thread wakes after `GPU_RETRY`, see below) or
  `Unavailable(phase)`. A failed download or a weights file that does not parse is
  logged and retried every 5 minutes (`LOAD_RETRY`, the thread waits on its request
  channel with that timeout while the model is missing) or sooner on the next library
  event, so starting offline only postpones the analysis. `ensure` trusts a file of the
  right size without hashing it; when such a file does not parse, `discard_if_corrupt`
  hashes it and deletes it if the SHA is wrong, so the next pass downloads it again (a
  genuine file is kept, not re-downloaded on every pass). No hardware adapter
  (`Phase::NoGpu`) or no adapter that passes the probe (`Phase::GpuFailed`) is final for
  the session: every later pass only sets the phase. The log says which adapter the
  model runs on and how well the probe agreed.
- `CatalogChanged` and `RemoteSyncFinished` queue another pass. The queue is a
  `bounded(1)` channel: passes never overlap, and any number of events during a pass
  collapse into one follow-up pass, which picks up whatever the catalog gained
  meanwhile. Not `ScanComplete`: every window activation and folder-watcher event
  rescans, and a rescan that finds nothing new sends `ScanComplete` alone
  (`library_service`'s unchanged fast path) — as a trigger it restarted the server
  downloads every time the user switched to the window (seen in the first full
  re-analysis: 103 server tracks over four passes); a rescan that changed something
  also sends `CatalogChanged`. A pass with nothing to do is the candidates query plus
  a cache check per server track, and logs nothing. Server tracks that wait for their
  server are tried again at the next pass, which comes with the next catalog change
  or sync; a server coming back does not start one by itself.
- The stop flag is checked between tracks and between model runs.

## The analysis threads

- Long CPU work owns its threads, like the indexer's pool. The `similar-tracks` thread
  loads the model, lists the candidates, saves and logs; each pass runs a pipeline of
  scoped threads under it (`std::thread::scope`, joined before the pass returns):
  `prepare_threads()` `similar-prepare-N` threads — half the cores, at most
  `MAX_PREPARE_THREADS` (8): not all of them, so the machine does not spin up
  entirely, a plain rule chosen by the owner over a load-driven governor — one
  `similar-gpu` thread that feeds the graphics card, and, when there are server tracks
  to download, a `similar-fetch` thread that waits for the prepare threads and then
  runs the download threads (`similar-fetch-N`). All at normal priority, by the owner's
  decision: changing it needs OS calls (`unsafe`), and parking those in a crate only
  because `unsafe` is allowed there is not worth it.
- The GPU does resampling, mel and the model (~19 ms a run of two tracks on an M3 Max,
  ~108 tracks/s), so the prepare threads — decoding — are the limit, as they already
  were with Metal-tract (which ran ~38 tracks/s on three model threads). With tract
  the earlier history was: three CPU model threads at ~550 MB of tract tensors each
  (~1.5 GB peak, ~9 tracks/s), then Metal with three feeding threads.
- Measured on `music-test` (2184 tracks, M3 Max, isolated `HOME`, release): 58 s
  against 86 s with Metal-tract, 7 prepare threads at ~400–700% CPU, the GPU busy
  ~25% of the time. Peak RSS ~1.32 GB (tract: ~1.36 GB). After the pass the footprint
  stays ~940 MB (tract: ~750 MB) although the GPU buffers are gone: ~450 MB of it is
  libmalloc's cache of freed large blocks (the PCM windows and the 16 kHz signals of the
  full pass), which it gives back on the system's memory-pressure signal, not a leak
  (`vmmap`: "MALLOC_LARGE (empty)"; live allocations ~390 MB). The slow spots are DSD
  (~9 s per DSF track: DSD→PCM plus the full pass) and APE images with a CUE sheet
  (~1.7 s per track).
- On macOS an idle app in the background (no playback, window hidden) gets demoted by
  the system (App Nap): its threads drop to the background band, the efficiency
  cores, ~3.5× slower — 2.8 s a track on one thread was measured that way. Nothing
  opts out of it; playing music keeps the app at normal speed.
- A pass: `embedding_candidates(EMBEDDING_VERSION)` — liked first, then recently
  played, then the rest by album — split by `source_of` into files and server files.
  A local file (`Track::local_file`) and a server track whose file is already in the
  media cache go into a queue (`jobs`) for the prepare threads. The cache is asked
  with `RemoteMedia::peek_cached`, never `cached`: that one touches the file for the
  LRU, and walking the library would reorder what gets evicted. For each such track a
  prepare thread:
  - opens the file with `audio_decoder::Decoder`;
  - decodes a cue track over `start_offset_ms` .. `+ duration_ms` (`TrackRange`), a
    plain file whole.
  - runs `audio_embedding::prepare` (decode, patch choice, the PCM windows; lossy and
    short tracks also resampled to 16 kHz) and puts the `Prepared` into `ready`, a
    channel bounded at `READY` (4 tracks, two runs), so prepared tracks (~4–12 MB each,
    more for hi-res) never pile up; a failure goes straight to the collector.
- Every other server track is downloaded, by the owner's decision, whole: the same
  vectors as a local copy and no new audio code, at the cost of the full file
  (~27 MB for four minutes of 16/44.1 FLAC). Reading only the 32 windows over the
  network (`audio_embedding` already decodes only them from a local lossless file)
  would save about 3× for lossless, with a sparse range reader — a later step; MP3,
  AAC and Opus files are small enough to stay whole. The server tracks are grouped by
  their remote file (`ServerFile`: a cue image is downloaded once for all its tracks)
  and keep the candidates' order.
- **Local first, then the network.** The `similar-fetch` thread joins the prepare
  threads before it starts: local files go through at full speed (prepare is
  CPU-bound), and the network phase does not take CPU from them. Then every source
  downloads at once, `PER_SOURCE` (2) files at a time each, `MAX_DOWNLOADS` (8) in
  all (`Board::workers`: no more threads than files to give them). Each download
  thread takes the next file from the first source in order that has one and is
  under its limit (`Board::take`); sources are in the candidates' order with torrents
  last (`RemoteMedia::is_torrent`), so when the total limit binds, servers come
  before torrents (fetching a torrent's track from peers may be slow or never
  finish). The limits are for the network, not the CPU: each thread prepares the file
  it downloaded, but only `prepare_threads()` of them prepare at a time (`Gate`;
  `prepare_slots`), so the network phase takes no more CPU than the local one — on a
  four-core machine one decode, not eight, next to playback. The bounded `ready`
  channel holds them back when the GPU is the bottleneck. Up to
  `MAX_DOWNLOADS` downloaded files can be on disk at once.
- A download thread fetches with `RemoteMedia::download_to` (`SourceMedia::fetch_whole`,
  the same `media_stream` download playback uses) into `scratch_dir` —
  `<cache dir>/pawse/analysis/<pid>-<instance>/<first track id>.<suffix>`, outside the
  media cache, so
  a first pass over a big server does not evict what the user cached for listening —
  then runs `prepare` for each of its tracks, then the file is deleted (`Scratch`, a
  guard made before the download, so every way out removes it). A file already in the
  cache is read from there; one that playback is downloading into the cache right now
  (`RemoteMedia::is_downloading`) is joined through `resolve` instead of being
  fetched twice, and lands in the cache. The downloads are not paused while a server
  track plays.
- Downloads can take hours on a big server, so the download threads give way between
  files when a new pass is requested (the worker's inbox is not empty): the pass ends,
  and the next one lists the candidates again — new local tracks first, the server
  files not done yet after them. A pass empties the inbox right before it reads the
  candidates, since those already include whatever the requests were about; without
  that, the second of two back-to-back events (a sync with changes sends
  `CatalogChanged` and `RemoteSyncFinished`) would make the pass skip its downloads.
- A download that fails is followed by a `ping` of its source. A source that answers
  had a problem with this file (gone, refused): its tracks fail like a broken file
  (`EmbedError::Network`, remembered for the session; a file the server serves but
  that does not decode is downloaded again on the next launch, like a broken local
  file is read again). A source that does not answer, is no longer configured, or is
  a torrent is unreachable: the rest of its files are not tried this pass, their
  tracks are counted in `Progress::unreachable` (the dialog's note; a scratch folder
  that cannot be created counts every server track there too) and wait for the next
  pass, not marked failed (`Board::give_up` drops the source's queue; a file of it
  already in another thread finishes on its own). Torrents are never pinged: their `ping` only looks at local
  state, so a torrent without seeders would answer and each of its files would wait
  out its read timeouts in turn.
- Stopping aborts a download in progress (`wait_while` polls the stop flag); its
  `.partial` is deleted once `media_stream`'s current request returns, and a file
  that completed right then can stay behind. Each running instance has its own folder
  (`<pid>-<n>`, `INSTANCES`), so a quick off/on cannot have the old instance clean up
  the new one's downloads: the worker removes its own folder when it exits, and when
  it starts every folder of another process id (`clear_leftovers`: a crash, an earlier
  run). A panic while handling one file is caught for that file (`catch_unwind`;
  `prepare` has its own), so its source keeps its download slot.
- A torrent loaded for a download keeps the torrent session running, so it uploads
  (per its upload setting) while the analysis downloads, even with nothing playing.
- The `similar-gpu` thread takes the next prepared track and `gather`s more that are
  ready, up to `Device::patches_per_run()` (64) patches — two full tracks — waiting for
  them until one `FILL_WAIT` (300 ms) deadline for the whole batch, since a half-empty
  run costs as much as a full one; a track that does not fit is carried into the next
  batch. It submits the batch (the PCM is copied into GPU staging memory, the
  `Prepared`s are dropped) and keeps up to `IN_FLIGHT` (2) submitted runs: while the
  GPU computes one, the thread gathers and uploads the next. It waits for the oldest
  run when more are in flight or nothing is ready, and sends its results to the
  collector (the `similar-tracks` thread). A panic in `submit` re-runs the batch's
  tracks one at a time, so only the track that panics fails (as a decode error,
  remembered for the session) and the others get their vectors; a panic in `wait` is a
  model error for the batch (`catch_unwind`).
- On stop the collector keeps saving what still arrives until every thread has left
  (each finishes the track or run in hand), so finished vectors are not thrown away.
- Results are saved every 16 tracks (`save_embeddings`, which also skips items a scan
  swept meanwhile, on `music_library`'s own embedding connection, so waiting for a
  scan's write lock never blocks the UI's reads), and every save invalidates the
  cached mean. Every pass invalidates it too when it starts: passes follow catalog
  changes, and a scan that swept items took their vectors out of the mean.
- A track that fails (decode error, under one second, an unsupported sample rate, a
  non-finite vector whose re-check of the GPU passed, a panic in decoding — caught per
  track in `prepare`; one broken file does not end the session's analysis) is logged
  with
  `warn!` and remembered in memory for this session only: no table of failures, the
  next launch tries again. A model error or a database error stops the pass: the
  collector raises `halt`, and every thread leaves at its next check (a prepare thread
  blocked on a full `ready` gets an error once the GPU thread has dropped its
  receiver). A database error leaves the phase at `Stopped` (resumes at the next
  library event). A model error is a GPU fault, a failed wait, a probe that stops
  agreeing (`audio_embedding_gpu` re-runs it every 64 runs) or a panic in the GPU
  thread; that `Device` then refuses every later run, and `after_analysis` decides
  (owner's rule: retry only a card that can pass the probe):
  - the pass saved at least one vector → `Faulted` + `Phase::GpuRetry` ("trying again
    in 30 seconds"): after `GPU_RETRY` (or sooner on a library event) the next pass
    opens a new `Device`, whose load-time probe decides — a transient fault is gone, a
    card that fails it makes the analysis `GpuFailed` for the session;
  - nothing saved → `GpuFailed` for the session at once. Without this a card that
    passes the probe but faults on real work (say a driver reset on one heavy batch,
    which stays first in the queue) would be reopened every 30 s forever; with it every
    retry follows progress, so the retries end with the library.
  Vectors saved before a failed periodic check stay (owner: not now). There are no
  pop-up notices in v1: the phase shows in the indexing dialog, and progress goes to
  the log every 100 tracks and at the end of a pass.

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

`SimilarTracks::radio(seed, skip, familiarity, rng_seed)` (blocking; background executor)
returns `None` when the seed is not in the catalog or has no vector, otherwise up to
`LENGTH` (100) tracks, the seed not included. One pass answers two queries: the
`ANY_ARTIST_POOL` (100) nearest overall, and the `OTHER_ARTISTS_POOL` (400) nearest
without any track of the seed's first artist (`same_artist_track_ids`). Without the second one a big
discography fills the whole pool and the re-ranking has no one else to pick. `skip`
takes ids out of both queries: empty for the menu's radio, the whole queue for the
continuation. The length was 30 at first; the weights were tuned on 12 picks, so
the far end of a 100-track list leans more on the penalties than on the sound.

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
  differ from the plan's `QueueSnapshot`: a track that ended meanwhile would shift every
  block by one anchor and could put the now-current mixed track right after itself.

## Queue continuation

While the analysis is running, a queue that runs out goes on as a radio from its
last track, whatever was playing before (a radio mode with its own switch may come
later). The queue changes only when it has really ended: until then "add to queue",
repeat, shuffle and a playlist-backed source behave as without the feature. An
automatic action does not notify; what goes wrong is logged.

- `advance_on_track_end` finds no next track and, unless the sleep timer's "stop at
  the end of the track" is armed, calls `continue_ended_queue` with the last track.
  The radio is built then, on the background executor, and its first track starts
  with the usual fade-in (`play_track`). Not gapless, by the owner's decision: the
  next track is another artist's, so a pause of one build is not heard — two or
  three streaming passes over the vectors (~20 ms each on 10k tracks; more while
  the analysis saves and the mean has to be recomputed) plus the rows of the pool.
  Building it ahead, when the last track starts, was tried and dropped: appending
  then changed the queue under "add to queue", repeat and the playlist source for
  the whole last track, and holding the list aside only to save that pause was not
  worth the code.
- The seed is the last track only; one without a vector continues nothing. The
  whole queue is the radio's `skip`, so an album that ended does not come back;
  familiarity is the menu's setting.
- `PlaybackQueue::continue_with(snapshot, tracks)` appends only if the queue still
  matches the `QueueSnapshot` taken at the end (ids, no current track), and returns
  where the continuation starts; a click on another track meanwhile drops it. A
  result that arrives after the switch was turned off is dropped too.
- Like a mix, the continuation keeps the custom flag as it was and makes the source
  `Unknown` (a playlist-backed queue would be rebuilt from the playlist and drop
  it); with shuffle on it also goes at the end of `original_order`. Unavailable
  tracks are dropped.
- The queue view is refreshed through its `OnApplied` (scrolled to the current
  track; registered with `set_queue_refresh` in `QueueView::new`, kept in `State`)
  before `queue_mutated`, so `QueueChanged` does not jump to the bottom.

## Indexing dialog

Settings → General shows an "Indexing" button left of the switch while it is on. It
opens a dialog (`window.open_dialog`, like the torrents confirmation) that starts
with a short note (`about`): every track is analysed to learn how it sounds, server
tracks are downloaded for it, and the app loads the processor and the graphics card
more and takes more memory while it runs (decoding on the CPU, everything else on the
GPU). Below it comes
`SimilarTracks::status()`: `embedding_coverage` (catalog tracks with a vector of the
current version, out of all catalog tracks) plus what the worker publishes in
`Shared::progress` — the phase (`Idle`, `LoadingModel`, `Analysing`, `NoModel`,
`Stopped`, `NoGpu`, `GpuFailed`, `GpuRetry`; an `AtomicU8` with explicit discriminants and a
round-trip test), the
current candidates that failed this session (counted against the catalog, so a
folder removed since does not inflate it) and the server tracks the current or last
pass could not get because their server did not answer (`unreachable`). The button shows only while the analysis is running (`is_running`), so it
never opens onto "off" next to a switch that is on. A failed read shows a message
instead of loading forever. The count line avoids plural agreement ("Tracks analysed:
N of M") so no language needs plural forms. An unreachable server does not start a
pass when it comes back by itself, so the note says its tracks are tried again at the
next library update. The
coverage moves in steps of `SAVE_BATCH` during a pass. Nothing updates by itself: the
snapshot is read on the background executor when the dialog opens and on "Refresh",
and its texts are built then (`Shown`), not per frame.

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

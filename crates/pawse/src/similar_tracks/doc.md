# similar_tracks

Tracks that sound alike, built on `audio_embedding` vectors: a background analysis of the library
(vector, style scores and genres by sound, saved per track), a streaming nearest-neighbour search,
a re-ranking step, and on top of them "radio from this track" and "mix other artists into the
queue" (menu in the queue panel header), a queue that runs out continuing as a radio, genres by
sound in the library views, and a progress dialog in Settings.

Not here: decoding, windows and the model (`audio_embedding/src/doc.md`,
`audio_embedding_gpu/src/doc.md`), the tables and their queries (`music_library/src/doc.md`, "Sound
vectors"), cache, whole downloads and range reads (`pawse/src/remote_media/doc.md`,
`media_stream/src/doc.md`), the queue itself (`playback_queue.rs`, `pawse/src/doc.md`), how the
library views draw genres (`library_views/doc.md`).

## Files

- `mod.rs` — `SimilarTracks` (cheap `Clone` handle), the `State` global, `setup` / `set_enabled` /
  `shutdown`, `Phase` / `Status`.
- `worker.rs` — the `similar-tracks` thread: model, a pass, the prepare / GPU pipeline, the
  collector that saves. `fetch.rs` — the server half of a pass (`Board`, `Fetcher`, `Gate`).
- `neighbors.rs` — streaming search, cached mean. `rerank.rs` — pure re-ranking (no database, no
  GPUI). `pool.rs` — search results to `Candidate`s and back to `Track`s.
- `radio.rs`, `mix.rs`, `actions.rs` (`start_radio` / `mix_queue`), `menu.rs`, `autoplay.rs`,
  `indexing.rs` (the dialog), `genres.rs` (display side of genres by sound).

## Switches

Settings → General, group "Recommendations" (`similar_group` in `settings_view.rs`).

- `similar_tracks_enabled` (off by default) turns on the analysis and everything below. Its
  description speaks of the result and says the analysis is local and the first pass can take a
  while; no threads or model download.
- `similar_autoplay` (on by default, also for a `settings.json` that predates it) is the queue
  continuation; `sound_genres_shown` (off by default) is the display of genres by sound only. Both
  are greyed out and shown off while the main switch is off or there is no GPU, their own values
  kept.
- The analysis needs a graphics card. Without a hardware adapter the main switch is disabled and
  shown off and the queue menu never shows; the stored setting is kept, so the same `settings.json`
  on a machine with a GPU turns the analysis on.

## Lifecycle

- `start` needs a GPU verdict first (`Gpu::Unchecked` → `adapter_name`). The check never runs while
  the feature is off: it loads the platform's GPU drivers, which users of a disabled feature should
  not pay for, and a broken driver must not take the app down for them. So a machine without a GPU
  learns it when the switch is first turned on; `check_gpu` then touches `SettingsStore` so the
  views' settings observers drop what they loaded.
- Each start is a new thread with its own `Shared`; stopping sets its stop flag and drops its request
  channel. It ends the pass at the next track, model run, download poll or ranged request and
  cancels a model download; the `ping` after a failed download is not cancellable. A quick off/on can
  leave the old pass finishing next to the new instance; both only add rows, and
  `model_file::ensure` allows one download at a time.
- The model lives in `dirs::data_dir()/pawse/models/`, not the cache dir, which users and the OS
  clear.
- Before any pass the thread drops vectors of other `EMBEDDING_VERSION`s (`prune_embeddings`, genres
  by sound included). It needs no model, so a new version that cannot get its weights or pass the GPU
  check still does not leave the old version's genres on screen — new labels may number them
  differently.
- A pass with nothing to analyse ends after listing candidates (no GPU, no network). Otherwise it
  **drops the `Device` at the end**: its GPU buffers are large and passes are rare.
- `CatalogChanged` and `RemoteSyncFinished` queue a pass; the queue is `bounded(1)`, so passes never
  overlap and events during a pass collapse into one follow-up. Not `ScanComplete`: every window
  activation and watcher event rescans, and an unchanged rescan sends it alone — as a trigger it
  restarted the server downloads each time the user switched to the window. A server coming back
  does not start a pass either; its waiting tracks are tried at the next one.
- `Model::Missing` (download or weights failed) is retried every `LOAD_RETRY` or at the next library
  event; `Unavailable(NoGpu | GpuFailed)` is final for the session. A weights file that does not
  parse, or a probe that fails on every adapter, first goes through `discard_if_corrupt`: a damaged
  file becomes `Missing` and is downloaded again, not `GpuFailed`.

## The pipeline

- A local file or a server track already in the media cache goes to the prepare threads; the cache is
  asked with `peek_cached`, never `cached`, which touches the file for the LRU, and walking the
  library would reorder what gets evicted. Other server tracks become `ServerFile`s, grouped by
  remote file (a cue image is one file, many tracks).
- A panic in `submit` re-runs the batch's tracks one at a time, so only the guilty track fails; a
  panic in `wait` is a model error for the batch.
- A result — vector, 400 style scores and the genre rows — is one `TrackAnalysis`, so a track with a
  vector always has its genres.
- `Shared::saved` (`bounded(1)`, a busy UI coalesces) becomes `LibraryEvent::SoundGenresChanged`,
  sent every `ANNOUNCE_EVERY` saved tracks and at the end of a pass that saved any — not on every
  save: the album list re-reads its genre map on the event, and a first pass saves every half second.
- `stop` is the user or the app; `halt` is the pass ending itself (model or database error). After
  either, the collector keeps saving what still arrives until every thread has left.

## Server tracks

Local files go first: `similar-fetch` joins the prepare threads before it starts, so the network
phase takes no CPU from CPU-bound local work. A server file is read one of two ways (`Mode`).

- **Ranges** — FLAC, WAV and AIFF on a source that serves byte ranges: the decoder runs on a
  `RangeReader`, so only what `prepare` reads crosses the network; a FLAC's 32 windows are placed by
  bytes, one request each (`audio_embedding/src/doc.md`, "A whole FLAC"). Not Symphonia's seek over
  the network: servers pay per request, not per byte. The vector must equal the one from a local
  copy; a test compares the patches bit for bit.
- **Whole** — downloaded, then decoded from the file: files under `SHORT_TRACK_MS` in total (the
  windows would cover the whole track; unknown durations do not count as short); torrents (pieces are
  1–4 MB, scattered reads would fetch more than the file); lossy formats and M4A (decoded whole
  anyway; an M4A may be ALAC, but telling it from AAC needs the file); APE and DSD; and every file of
  a source that answered the first range request without a range (`RangeError::Unsupported`: the file
  returns to its queue and `Board::no_ranges` switches the source to whole).
- Limits are for the network, not the CPU: `PER_SOURCE_RANGED` per ranged source, at most
  `PER_SOURCE` of them whole; torrents and sources without ranges `PER_SOURCE`; `MAX_DOWNLOADS` in
  all. Torrents come last, so when the total binds servers go first. A torrent loaded for a download
  keeps the torrent session running, so it uploads per its setting even with nothing playing.
- A file downloaded whole is decoded only when a `Gate` slot (`prepare_threads()` of them) is free, so
  the network phase takes no more CPU than the local one. A ranged read takes no slot: it mostly
  waits on the network, and a slot held through that would cut the reads to the core count.
- Whole downloads go into `<cache dir>/pawse/analysis/<pid>-<instance>/`, outside the media cache so
  a first pass over a big server evicts nothing cached for listening; a `Scratch` guard deletes the
  file on every exit. A folder per instance keeps a quick off/on from cleaning up the new one's
  downloads; a starting worker removes folders left by other process ids.
- Downloads can take hours, so threads give way between files when a new pass is requested
  (`superseded`: the inbox is not empty). A pass empties the inbox right before it reads the
  candidates; otherwise the second of two back-to-back events (a sync with changes sends
  `CatalogChanged` and `RemoteSyncFinished`) would make it skip its downloads.
- A failed download or ranged read is followed by a `ping` of its source (`network_failure`); a ranged
  read's network error is told from a decode error by the reader's `ReadFailure`, checked even when
  `prepare` succeeded, so a vector from a fallback that ran after a network error is never saved. A
  source that answers had a problem with this file: its tracks fail (`EmbedError::Network`, session
  only). A source that does not answer, is no longer configured, or is a torrent is unreachable: its
  queue is dropped, its tracks count in `Progress::unreachable` and wait for the next pass, not
  marked failed. Torrents are never pinged: their `ping` only looks at local state, so a torrent
  without seeders would answer and each file would wait out its timeouts.

## Failures

- A track that fails (decode error, shorter than a second, a non-finite vector whose GPU re-check
  passed) is remembered in memory for the session, no table; one broken file does not end the pass.
- A model or database error halts the pass. A database error leaves the phase `Stopped`; the next
  library event resumes. A model error (GPU fault, failed wait, a probe that stops agreeing) makes
  that `Device` refuse every later run, and `after_analysis` decides:
  - at least one vector saved in the pass → `Faulted` / `GpuRetry`: after `GPU_RETRY` (or a library
    event) the next pass opens a new `Device`, whose load-time probe decides;
  - nothing saved → `GpuFailed` for the session at once. Otherwise a card that passes the probe but
    faults on real work would be reopened every 30 s forever; with this rule every retry follows
    progress, so retries end with the library.

## Search and re-ranking

`Neighbors::search` streams every vector of the current version through one `TopN` per query, so
memory does not grow with the library. A query knows only a vector, a count, nearest or farthest, and
ids to skip; choosing the ids is the caller's job. Scores are centred cosines over catalog tracks
only, so an offline source takes no places in a top-N.

- The centring mean is cached and computed under its mutex, so `invalidate` (every save, the start of
  a pass, a prune) waits for a computation in flight: a mean never outlives the save that made it
  stale. `mean_of` is one streaming pass, so its cost does not grow with a long queue.
- Nearest neighbours alone are mostly the seed's own artist, so candidates are re-ranked greedily, one
  pick at a time: `score + familiarity bonus − recent penalty + noise − artist × (tracks of its artist
  already taken) − repeat × (same artist as the previous pick)`. `Taken` grows with every pick, so the
  penalty spreads an artist through the list; no hard per-artist limit. Album is not a signal.
- Dropped: unavailable tracks, tracks under `MIN_DURATION_MS` (unknown length passes), tracks below
  `MIN_SCORE` (less alike than a random pair; a small library gives a shorter list, not one padded
  with the opposite sound) and a second copy of the same first artist + normalised title. Tracks
  without an artist are neither penalised nor deduplicated.
- Noise is Gumbel from an `StdRng` seeded by the caller: the same seed gives the same list (tests),
  the app passes a random one so pressing again gives another list. `Weights::default()` was tuned on
  about a dozen picks by simulation over real neighbour lists, so the far end of a 100-track radio
  leans more on the penalties than on the sound.

## Radio, mix, continuation

- `radio(seeds, skip, …)`: one seed is searched from its own vector, several from the mean of theirs;
  seeds without a vector are left out. One pass answers two queries: the nearest overall and the
  nearest without any track of the seeds' first artists — without the second, a big discography fills
  the pool and the re-ranking has no one else to pick. Each seed artist counts once in `Taken` however
  many seeds it has (five tracks of one album must not push it five penalties down). `skip` is empty
  for the menu's radio and the whole queue for the continuation.
- `start_radio` replaces the queue with the playing track's entry (read again when the radio is
  ready) and the radio, without asking even for a custom queue — the user picked it looking at the
  queue. Playback is not touched; shuffle shuffles the radio.
- `mix` searches from the mean of the queue's own tracks, skipping what stays in the queue and every
  track of their first artists, so only other artists come in. `mix_queue` draws `GAP` per anchor (the
  current track and every own track after it) and `mix_in` puts each block right after its anchor.
  Mixing again re-rolls: it first drops the mixed tracks after the current one; mixed tracks already
  heard stay and are never part of the basis.
- Continuation: `services::advance_on_track_end` finds no next track and, unless the sleep timer's
  "stop at the end of the track" is armed, calls `continue_ended_queue`. It needs the analysis running
  and `similar_autoplay` on (read then). Until the queue has really ended, "add to queue", repeat,
  shuffle and playlist-backed sources behave as without the feature. An automatic action does not
  notify; what goes wrong is logged.
- Its seeds are the last `SEEDS` tracks this queue really played (`QueueSnapshot::played_tail`),
  searched from their mean: albums often end on an outlier (outro, acoustic or hidden track). A tail
  without the track that ended falls back to that track alone.
- Mix and continuation change the queue only through `mix_in` / `continue_with`, which compare a
  `QueueSnapshot` (ids and current index) and change nothing on a mismatch: a track that ended
  meanwhile would shift every block by one anchor and could put the now-current mixed track right
  after itself. Both leave the custom flag as it was (a generated list is replaced by the next click
  without asking, a hand-built queue stays protected) and set the source to `Unknown` (a
  playlist-backed queue is rebuilt from the playlist on every edit and would silently drop the
  additions).
- The queue view refreshes through `OnApplied` (registered by `QueueView::new` via
  `set_queue_refresh`; scrolls to the current track) before `queue_mutated`, so `QueueChanged` does
  not jump to the bottom the way it does for "add to queue".

## Genres by sound

Both networks run in every analysis run, so the analysis needs no extra pass, download or switch;
the user switches only the display. `genres::shown` needs `sound_genres_shown`, the main switch and a
GPU, so the views agree with the Settings switch even when an earlier session left genre rows in the
database. Off, the views run their tag-only queries.

- The thresholds differ on purpose: an album gets the genres whose mean score over its analysed
  tracks reaches `ALBUM_MIN` (at most `PER_ALBUM`, tag genres left out); a track belongs to a genre
  page from the higher `TRACK_MIN` — pages hold tracks that clearly sound like the genre, so they can
  hold part of an album. A genre only heard is listed with at least `MIN_TRACKS` tracks.
- On the Genres tab tags and genres by sound are one list by name, all alike: a genre both tagged and
  heard is one row, and its page holds the tracks of both. Not dimmed there: a row gathers tracks
  from the whole library, so a heard genre whose name matched one tag looked tagged while one without
  a match looked apart — the mark said "no tag happens to share the name", not "from the analysis".
  An album's genres are about that one album, so there dimming (`dimmed`, `tooltip`) stays honest.
- Never in the files or the tag editor: the rows live in their own table and the album export's genre
  stays the first tag genre.

## Dialog and menu

- The "Indexing" button shows only while the analysis is running, so the dialog never opens onto
  "off". Failed candidates are counted against the catalog, so a removed folder does not inflate
  them. The count line avoids plural agreement ("Tracks analysed: N of M"): no language needs plural
  forms. The note on an unreachable server says its tracks are tried again at the next library
  update, since its return starts nothing.
- `queue_menu` shows only while the queue has tracks and the analysis is running; radio, the most
  used, is nearest the trigger and disabled with no current track.
- `compact` (the mini player's queue) drops the hints, the separator and the "Tracks" label,
  leaving three rows that fit the mini player's default strip. gpui fits a popup into the window by
  moving it, never by shrinking it, and a scrolling menu squeezes its rows, so the menu is made
  shorter instead.

## Checking by hand

Turn the switch on in an isolated `HOME` with a small folder of music and watch `logs/pawse.log` for
`similar tracks:` lines. The owner's quality gauge is `.plans/smart-playlists-poc/scripts/analyze.py`
over `track_embeddings` (~0.48 on `../music-test`); much lower means the audio preparation drifted.
On macOS an idle background app is slowed by App Nap, so speeds from a shell-launched build mislead.

## Decisions

Owner's calls; ask before changing.

- GPU only, no CPU fallback.
- Prepare threads are half the cores, at most `MAX_PREPARE_THREADS`: a plain rule, not a load-driven
  governor. All analysis threads run at normal priority — raising it needs OS calls (`unsafe`), and
  parking those in a crate only because `unsafe` is allowed there is not worth it.
- Retry the GPU only for a card that can pass the probe. Vectors saved before a failed periodic check
  are kept. No pop-up notices in v1: the phase shows in the dialog, progress goes to the log.
- `SHORT_TRACK_MS` and `PER_SOURCE_RANGED` are the owner's choices (ranged requests are small and a
  few tracks at once hide each other's latency).
- `GAP` draws 1–3 per anchor: the queue should become mostly new music, not the same queue with a few
  extra tracks.
- Continuation is not gapless: the next track is another artist's, so the pause of one build is not
  heard. Building it ahead was tried and dropped — appending then changed the queue under "add to
  queue", repeat and the playlist source for the whole last track, and holding the list aside only to
  save that pause was not worth the code.
- No "Refresh" button in the dialog.
- Genre names are compared by `to_lowercase` only (tag `Trip-Hop` and heard `Trip Hop` are two
  rows); the Genres tab does not dim heard genres.

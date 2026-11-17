# normalization

Volume normalization on the app side: the `Normalization` handle in `Services`, the
in-memory tables of known track gains (our measurements and servers' own), the job queue
that measures tracks on GPUI's background executor, the 100 ms estimate when a track is clicked, and the hooks that decide what to measure
when. One switch in Settings → General (`volume_normalization`, off by default).

Not here: the meter, quick and full measurement, the gain rule (`loudness/src/doc.md`),
applying the gain (`audio_engine/src/doc.md`, "Normalization"), reading tags
(`audio_decoder/src/doc.md`), the table (`music_library/src/doc.md`, "Loudness"), where a
server gain comes from (`jellyfin/src/doc.md`, `music_library/src/doc.md`, "Server
sources"), how a click reaches the opener (`pawse/src/doc.md`, "Opening a track").

## Files

- `mod.rs` — `Normalization`, `Spawner`, `takes_file_tags`, the queue and its drain
  (`Inner::push` / `drain`), `ServerGains`, `VERSION`.
  What to measure is a `TrackRequest` (`playback_opener.rs`): item, locator, cue span.
- `playback.rs` — `Tracker` (the engine-events hooks), `set_enabled` (the setting),
  `setup` (reloads server gains on library events).
- `tests.rs` — click estimate, tags win, full replaces quick across a restart, nothing runs
  while off, the cue tag rule, a server gain per copy (not measured, follows a sync, an
  older reload never wins). Real fixtures and a temp database; no GPUI.

## Spec

- Values live in a `HashMap` keyed by item id, loaded in `Normalization::new` (after
  pruning other `VERSION`s) while `Services` starts, before the launch restore and the
  first click can ask; after that the opener reads them without touching SQLite. A full value is never replaced by a quick one, in the
  map or in the table. Bump `VERSION` whenever the meter or the estimate would give other
  numbers.
- Server gains (`ServerGains`) are keyed by item and source, since one item can have a
  local copy and copies on several servers: `known(item, locator)` gives the server's gain
  only when the locator that plays is that server's (`remote::parse`), else the measured
  value. They are loaded with the measurements and again on every `CatalogChanged` and
  successful `RemoteSyncFinished` (a sync can change a gain without changing the catalog; a
  scan can move a binding to another item). Reloads run through the `Spawner` beside the
  queue and may finish out of order, so each carries a generation and an older one never
  replaces a newer table.
- A server gain has no peak, so the gain rule (`loudness/src/doc.md`) lets it only turn a
  track down. A track whose catalog locator has one is not measured (`leaves_to_others`,
  like a tagged file): the opener would never use the result.
- What is measured comes only from bytes already on this computer: a local file that
  exists, or a server or torrent track already complete in the media cache
  (`peek_cached`, so the cache order is not touched). Nothing is downloaded to measure.
- A file whose own tags carry a track gain is not measured, unless it is a cue segment
  (`takes_file_tags`: not a cue track and no start offset): the engine prefers the tags, so
  the work would be thrown away. Such items are remembered for the session (`tagged`, not
  stored), so a tagged library opens each file for this only once: later clicks take the
  direct path (`wants_estimate`) and no measurement is queued for them.
- Jobs run one at a time: the first job queued starts a single drain task through the
  `Spawner` (in the app, GPUI's background executor at `Priority::Low`; tests pass a plain
  thread, so no GPUI), and it ends once the queue is empty. No thread of our own sits idle.
  Urgent jobs (a quick estimate of the next track, saving a click estimate) go to the
  front, full measurements to the back; a job already queued for the same item and kind is
  not queued again, and a job whose value appeared meanwhile does nothing. A long full
  measurement (an APE image, a long DSD file) holds one pool thread while it runs, which
  on Linux is one of a fixed pool. Each job runs under `catch_unwind`: a decoder that
  panics on a broken file must not unwind through the executor's dispatch (GCD on macOS). Turning the switch off drops queued measurements and stops a
  running full one at its next batch; saves still run. `shutdown` stops new jobs and waits
  up to 1 s for the drain to finish the saves still queued.
- Click (`estimate_now`, called by the opener on its `track-opener` thread): a quick
  estimate with a 100 ms budget, saved through the queue so a busy database never delays
  the start. `wants_click_estimate` says no for APE and DSD by extension (they decode too
  slowly to finish in time), so those clicks keep the direct path.
- `Tracker::loaded` (every `EngineEvent::Loaded`): a full measurement of the current track,
  a quick one of the next, and a full one of the previous track, which catches a streamed
  server track whose download finished while it played. `wants_next` / `prepare_next`:
  once per track, 5 s before its end, a quick estimate of whatever is next by then, since
  the queue may have changed (edits, the similar-tracks continuation) and a remote
  prefetch may have finished.
- `set_enabled(false)` clears the output's bit-perfect issue at once; turning it on starts
  measuring the current and next track.

## Decisions

Owner's calls; ask before changing.

- No library-wide pass: measurement follows listening, two kinds (quick to start fast,
  full to end up exact), so remote sources are never read just to be measured.
- A clicked track with no known gain waits up to 100 ms for a quick estimate; past that it
  plays as is. A gapless start never waits.
- Not applied on DLNA and Chromecast renderers (they get the original file), as with the
  equalizer.
- A server's own gain (Jellyfin's `NormalizationGain`) ranks with file tags, above our
  measurement: tags, then the server, then a measurement, then none.

# media_stream

Progressive download of one remote file into a sparse local file, with readers that start before
it ends. It is the streaming path and the cache entry at once: a file read to the end is renamed
to its destination and is a cache hit from then on. `RangeReader` is the other mode, a
`Read + Seek` that fetches only the blocks it is asked for and writes no file. A source implements
only `RangeFetch` (`fetch(start, end)`: `end` exclusive, `None` = to the end; `Fetched` says where
the body starts, the total if known, and whether the server honoured the range); the crate knows
nothing about servers or audio.

Not here: cache layout, limit and trimming, the one `RangeFetch` implementation
(`pawse/src/remote_media/doc.md`); the HTTP range requests (`server_http/src/doc.md`); the sound
analysis that uses `RangeReader` (`pawse/src/similar_tracks/doc.md`).

## Files

- `lib.rs` — `RangeFetch`, `Downloads` (registry), `Download` (handle), `StreamReader`
  (`Read + Seek`), `AbortHandle`, the worker (one `media-stream` thread per download).
- `ranges.rs` — `RangeReader`, `RangeError`, `ReadFailure`, `StopCheck`.
- `tests.rs` — an in-memory `RangeFetch` that throttles, cuts connections, answers `Retry` /
  `Fatal`, ignores ranges or hides the length.

## Download

- One download per destination: `Downloads::start` joins a running download that still has
  interest, or a finished one whose file exists, so the prefetch of the next track and its
  playback share bytes.
- Every `Download` (clones too) and `StreamReader` is interest. When the last is dropped before
  the file is complete, the worker stops and deletes its `.partial`. Completion is checked before
  interest: a complete file is kept even if every reader left first.
- The worker fetches the first missing byte at or after the reader's position (every read and
  seek reports it), then the holes before it. A reader waiting for a byte that lies behind the
  byte being received, or 1 MB or more ahead, makes the worker drop the connection and restart
  there — this makes a seek in a large FLAC fast.
- Requests are at most 4 MB, so cancellation, seeks and stalls are noticed quickly and the
  source's per-request timeout bounds a hung connection.
- A server that ignores Range (`ranged == false`) is read once, from byte 0 to the end; a retry
  restarts at byte 0 and only new bytes count as progress, so a connection that keeps dropping at
  the same byte fails. An unknown length ends where the body ends; a seek from the end waits for it.
- `Retry` backs off 0.25 s doubling to 4 s, six times in a row without progress. Until the first
  byte arrives only two retries (under a second): a server that refuses the very first request is
  down and the user is waiting on a click. `Fatal` fails at once.
- A failed download has already deleted its `.partial`, so `Download::reader` returns the outcome
  first; opening the file would give a bare "No such file" instead of the server's reason.
  `AbortHandle::failure` gives the reason to whoever holds the handle, since a decoder probing
  the format may hide the read error behind its own.
- Partial names carry the process id and a counter, so a cancelled download still deleting its
  file never races a new one for the same destination.
- `on_complete` runs on the worker thread, only after the rename to `dest`; if the rename fails
  the `.partial` stays and `on_complete` is not called.

## Readers

- `give_up_waiting_when` takes a check that a waiting read polls; when it says a newer request is
  queued, the read fails instead of waiting. Bytes already present are still read, so the check
  never breaks a read that would not wait.
- Readers never return `Interrupted`: `read_exact` and Symphonia would retry it forever.

## RangeReader

Reads on the calling thread: no file, no worker, no `Downloads` entry.

- Blocks of 32 KB; a miss right after the previous request doubles it, up to 4 MB. Blocks start
  small because Symphonia's FLAC seek is a binary search of many small reads; a sequential decode
  then costs a few large requests. The last 8 MB stay in memory, oldest inserted out first.
- Opening needs a ranged answer from byte 0 with the total length, else `RangeError::Unsupported`
  (the caller downloads the file whole). Later answers must stay ranged, start where asked and
  keep the length.
- Retries follow the download's rules (two when opening); an answer that brought a block is
  followed at once by a request for the rest.
- A failure is final: every later read fails with the same message without asking the server.
  Symphonia and `audio_embedding` answer a read error by trying another way, and each of those
  reads would otherwise wait out six more retries. The message also goes to a `ReadFailure` the
  caller took before handing the reader to the decoder, which may report the I/O error as its own.
- A `StopCheck` is polled before every request, between blocks and during back-off; a stopped
  read is `RangeError::Stopped`, not final, and records no failure.
- `prefetch(start, len)` fetches a span in one request (at most half the cache):
  `audio_decoder`'s `seek_near` says how much a window needs (`MediaStream::will_need`), so a
  window is one request instead of a block and its growth.
- After the last byte it asked for, the reader reads on until the body ends (at most 4 KB): `ureq`
  returns a connection to its pool only after the body was read to its end.

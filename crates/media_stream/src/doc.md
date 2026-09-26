# media_stream

Progressive download of one remote file into a sparse local file, with readers
that can start before the download ends. It is the streaming path and the cache
at the same time: a file read to the end is renamed into place and is a cache
hit from then on. It knows nothing about servers or audio — `RangeFetch` is the
only thing a source implements (`pawse::remote_media::http::ServerFetch`, for
every server that serves byte ranges).

## Files

- `lib.rs` — `Downloads` (registry), `Download` (handle), `StreamReader`
  (`Read + Seek`), `AbortHandle`, the worker.
- `tests.rs` — an in-memory `RangeFetch` with throttling, dropped connections,
  retries and a server that ignores ranges.

## Behaviour worth knowing

- **One download per file.** `Downloads::start` joins a running (or finished)
  download of the same destination instead of starting a second one, so the
  prefetch of the next track and its playback share bytes.
- **Interest.** Every `Download` and `StreamReader` counts. When the last one is
  dropped before the file is complete, the worker stops and deletes its
  `.partial`. A file that is already complete is kept even if every reader left
  first — completion is checked before interest.
- **Where to fetch next.** The first missing byte at or after the reader's
  position; once everything after it is present, the holes before it. A reader
  that jumps far outside what is downloaded (more than 1 MB past the byte being
  received) makes the worker drop the connection and restart there — this is
  what makes a seek in a large FLAC fast.
- **Requests are 4 MB ranges.** Each request is short, so cancellation, seeks
  and stalls are noticed quickly, and the source's per-request timeout bounds a
  hung connection. Bytes received before a failure are kept; only the rest is
  retried.
- **Servers that ignore Range** (`Fetched::ranged == false`) are read once from
  the start to the end.
- **Unknown length** (no `Content-Length`/`Content-Range` total) ends where the
  body ends.
- **Retries.** `FetchError::Retry` backs off 0.25 s … 4 s, six times in a row
  without progress, then the download fails and every reader gets the error.
  Any progress resets the count. Until the first byte arrives only two retries
  are made (under a second): a server that refuses the very first request is
  down, and the user is waiting on a click. `FetchError::Fatal` fails at once.
  `AbortHandle::failure` gives the reason to whoever holds the handle, since a
  decoder probing the format may hide the read error behind its own.
- **Partial names** carry the process id and a counter
  (`<dest>.<pid>-<n>.partial`), so a cancelled download that is still deleting
  its file never races a new one for the same destination.
- **A reader can be told to stop waiting.** `give_up_waiting_when` takes a
  check that a waiting read polls; when it says a newer request is queued, the
  read fails instead of waiting for missing bytes. Bytes that are already there
  are still read, so the check never breaks a read that would not wait.
- **Readers never return `Interrupted`.** An aborted reader returns a plain
  error; `Interrupted` would make `read_exact` and Symphonia retry forever.

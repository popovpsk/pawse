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
- `ranges.rs` — `RangeReader`: reading parts of a file by ranges, without a
  download (below).
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

## Reading parts of a file (`RangeReader`)

The sound analysis (`pawse::similar_tracks::fetch`) needs only ~32 two-second
windows of a server track plus whatever Symphonia reads to find them, so
`RangeReader` is a `Read + Seek` that fetches only the bytes it is asked for: no
file on disk, no worker thread, no `Downloads` entry — a missing block is
requested on the reading thread.

- **Blocks of 32 KB.** A read of a missing block asks for that one block; a read
  of the block right after the last request doubles the request (64 KB, 128 KB, …
  up to 4 MB), so a sequential decode (a lossy track, decoded whole) costs a few
  large requests and a seek costs one small one (a short lossless track that
  `audio_embedding` decodes whole). Symphonia's FLAC seek is a binary
  search over bytes with 6–11 small reads, which is why the blocks start small:
  with 32 KB the analysis of a 3–5 minute CD track reads ~45% of the file in
  ~280 requests, with 256 KB blocks ~75%, with 1 MB all of it (measured on 40
  music-test FLACs, `windows` path).
- **Cache.** The last 8 MB of blocks stay in memory, oldest out first; a pass
  over one track re-reads almost nothing (the same 40 files: no difference
  between 8 MB and unlimited).
- **Opening** asks for the first block and needs a `206` from byte 0 with the
  total length: a server that ignores ranges or hides the length is
  `RangeError::Unsupported`, and the caller downloads the file whole instead.
  Later answers must stay ranged, start where asked and keep the length
  ("the file changed on the server" otherwise).
- **Retries** follow the download's rules: `FetchError::Retry` backs off 0.25 s …
  4 s, six times without progress (two when opening), `Fatal` fails at once;
  bytes up to the last whole block of a broken answer are kept, and an answer that
  brought at least one block is followed at once by a request for the rest.
- **A failure is final.** Once a request has failed, every later read fails with
  the same message without asking the server again: Symphonia and
  `audio_embedding` answer a read error by trying another way (the windows path
  falls back to a full decode of the same stream), and each of those reads would
  otherwise wait out six more retries.
- **Stopping.** A `StopCheck` is polled before every request, between blocks and
  during back-off; a stopped read is `RangeError::Stopped`.
- **The reason survives.** A failed read records its message in a `ReadFailure`
  the caller took before handing the reader to the decoder, which may report the
  I/O error as its own decode error.
- **Keep-alive.** After the last byte it asked for, the reader reads the body once
  more (and gets 0): ureq returns a connection to its pool only after a body was
  read to its end. Without it every request opened a new connection — 1057
  connections for 1056 requests, with it 5 for 6361.

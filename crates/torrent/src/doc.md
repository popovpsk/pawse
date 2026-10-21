# torrent

A BitTorrent source over `librqbit` behind a small blocking interface: a torrent's file list, byte
ranges of one file, and throwaway sparse copies of some ranges for reading tags. Nothing outside
the crate sees `librqbit`, `tokio` or pieces. It knows nothing about audio, the library or the
media cache.

Not here: turning a torrent into library tracks (index plan, `<hash>.view`, statuses, error
mapping, the read timeout) — `pawse/src/servers/doc.md`; the cache a finished file lands in —
`pawse/src/remote_media/doc.md`.

## Files

- `lib.rs` — public types (`Config`, `Upload`, `Network`, `Input`, `Meta`, `FileEntry`, `Want`,
  `Swarm`, `Error`) and `is_info_hash`.
- `engine.rs` — `Engine`: the lazily started `librqbit` session on the shared `net_runtime`, the
  loaded torrents (slots with lease counts), the janitor (10 s tick).
- `body.rs` — `Lease` (keeps a torrent loaded), `Probe`, `Body` (`Read` over a byte range).
- `testing.rs` — `Seeder`: a real `librqbit` session seeding a folder on loopback, and an
  `Engine` whose only peer it is. Public under `test-support` so `pawse` tests can index a real
  torrent.
- `tests.rs` — the engine against that seeder.

## Interface

- Every call blocks the caller on `net_runtime` (`Handle::block_on`), so none may run in async code
  or on its worker threads; a thread of its blocking pool is fine.
- When the last `Engine`, `Probe` and `Body` are dropped, the session is cancelled: the runtime is
  shared and would otherwise keep its peers, listeners and DHT running.
- `resolve` lists a `.torrent` file or magnet and stores `<info_hash>.torrent` in `state_dir`;
  a torrent is known once resolved, and `meta` reads it back without the network. `forget`
  unloads it by force and deletes its work data and every `state_dir` entry starting with the hash.
- `Meta::files` omits padding files; `FileEntry::index` is the index in the full list (what `Want`
  and `read` take), so indices can skip.
- `meta`, `probe` and `read` answer `Unknown` to anything but 40 lowercase hex digits, and `forget`
  ignores it, so a hash never becomes a path outside `state_dir` / `work_dir` (`is_stored` is a
  bare existence check).

## Patience

- A piece is readable only once complete and verified; with 16 MiB pieces that takes minutes on a
  slow swarm while bytes keep arriving. So `read` (opening, first 64 KB) and `Body::read` give up
  only after their quiet time — `read`'s `timeout`, 30 s for `Body::read` — passes without the
  torrent receiving a byte from any peer (`patient`, watching `fetched_bytes`), or after 5 minutes
  in any case.
- With no peer connected at that moment the error is `NoPeers`, not `Timeout`. `pawse` retries
  `Timeout` but treats `NoPeers` as final, so a click on a track while offline fails after one
  quiet time.
- A probe's `stall` is a limit, not a deadline: `Timeout` only after that long without a new byte
  from its own streams.

## Streams

- librqbit's lookahead is 32 MB per stream, not a number of pieces: 1 MiB pieces keep ~32 pieces
  (and peers) busy, 16 MiB pieces only 2. So `read` opens up to 3 helper streams parked 32 MB,
  64 MB, … past its start (`helper_count`: enough for ~8 pieces in flight, none up to 4 MiB
  pieces). They only raise those pieces' priority and live as long as the `Body`. They open after
  the first byte arrived: librqbit interleaves streams in random order, and on a thin swarm a
  helper's piece taken first would delay the start by a whole piece.
- Every open file stream holds one permit of librqbit's blocking-I/O semaphore for its whole life,
  and writing a received block needs a permit from the same semaphore. With as many open streams
  as permits (default 8) no block can be written and every stream waits forever. So the session
  gets `BLOCKING_PERMITS` (64), a probe opens at most `PROBE_STREAMS` (4) at a time, probe and
  helper streams together stay under an engine-wide `SHARED_STREAMS` (24) (helpers are skipped
  when it is used up), and `read` bounds opening its stream by its timeout.
- An idle peer sleeps up to 5 s and opening a stream or seeking does not wake it (librqbit
  notifies only when pieces are released), so a read on an idle torrent can wait a few seconds for
  its first byte.

## Probes

- A probe fetches the asked ranges into `<work>/<hash>/<path>` and deletes the tree when dropped,
  unless a read still holds the torrent; then the tree stays until that torrent is unloaded
  (idle time or work limit).
- Each stream looks up to 32 MB ahead, to the end of its file, so a probe can pull far more than
  asked. Only a file no probe asked for is guaranteed to stay empty — the probe test checks that
  one.
- Files are sparse: zeros where nothing arrived, and a file is only as long as the furthest piece
  written (no preallocation). Anything that needs the real length (a tag reader computing a
  bitrate) must use `FileEntry::len`.
- Once a second a probe checks that `<hash>.torrent` still exists and fails with `Unknown` when
  `forget` removed it, so removing a source ends its running sync at once. The file, not the
  loaded slot, is the signal: `is_stored` is already false by then, and a `forget` landing while
  the probe is still being acquired is caught too.

## Leases and unloading

- `probe` and `read` hold a lease. A torrent with no lease is unloaded (session entry and work
  data deleted) after `idle_unload`, or at once when the last lease released was a probe's.
- `work_limit_bytes` (`u64::MAX` = none) is checked when a read's last lease is released —
  back-to-back reads (a cache fill, playback with prefetch) leave a torrent idle only for moments
  the janitor's tick would miss — and by the janitor, which unloads idle torrents oldest first.
  Sizes are allocated blocks (Unix), skipping the indexer's `<hash>.view` hard links.
- `unload` deletes a work root only when it unloaded a torrent (or `forget` forces it), so a late
  unload never wipes a folder a new `acquire` is filling.
- A hash being unloaded sits in `unloading` until its directory is gone; `acquire` waits for that
  instead of adding the torrent again into a folder about to be deleted. A newly added torrent
  whose start fails is deleted again, so no session entry outlives a failed `acquire`.
- The session starts on first use and stops when nothing is loaded, nothing is in flight
  (`resolve`, `meta`, `acquire` count as busy) and nothing was used for `idle_unload`, so nothing
  seeds while nothing plays or downloads.

## Storage and network

- `state_dir` keeps `<info_hash>.torrent` and `dht.json`. `work_dir` holds piece data only and is
  wiped when an `Engine` is created; nobody else may use it.
- Nothing is selected: torrents are added with `only_files = []`, so the session never downloads a
  file on its own. Every byte comes from a stream (librqbit prioritises a stream's pieces whether
  or not the file is selected), which is how `probe` gets the head of a file without the rest.
- librqbit opens every file of a loaded torrent read/write and keeps it open for the torrent's
  life: one descriptor per file. The host must raise the soft open-file limit (`pawse`'s `main`
  does); an app started from Finder has a soft limit of 256, which a torrent of a few hundred
  files exhausts, breaking everything else that opens files.
- `Public` runs DHT, trackers, local service discovery, and listens on TCP and uTP — many peers
  are reachable only over uTP. `Local` is loopback-only with fixed peers, for tests.
- `Upload::WhileActive` uploads without a limit while a torrent is loaded, `Limited` caps it
  (KiB/s, applied live), `Off` starts sessions with uploading disabled. A running session cannot
  change that: turning uploading off caps it at 16 KiB/s (lower would stop librqbit's upload
  scheduler, whose chunks are 16 KiB) and the session is restarted as soon as it is idle.
- `swarm` reports connected and known peers of a loaded torrent; librqbit does not say which are
  seeds, so seeds cannot be counted.
- No protocol encryption (MSE/PE): peers set to require it drop the connection during the
  handshake, the rest of the swarm still serves.

## Audit exception

`RUSTSEC-2026-0293` (ringbuf < 0.5.2: double free in `Consumer::skip` / `Consumer::clear` when an
element's `Drop` panics) is ignored in `.cargo/audit.toml`. ringbuf 0.4 comes only through
`librqbit-utp` 0.7, which pins `^0.4.7`; 0.4 has no fixed release. Its only ring buffer is
`SharedRb<Heap<u8>>`, and `u8` has no `Drop`, so the bug cannot trigger. Drop the exception once
`librqbit-utp` moves to ringbuf 0.5.

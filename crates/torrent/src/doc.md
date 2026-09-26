# torrent

A BitTorrent source behind a small blocking interface. Nothing outside this
crate sees `librqbit`, `tokio` or pieces: callers ask for a torrent's file list,
for byte ranges of a file, or for a throwaway sparse copy of some ranges to read
tags from. It knows nothing about audio, the library or the media cache.

## Files

- `lib.rs` — the public types: `Config`, `Upload`, `Network`, `Input`, `Meta`,
  `FileEntry`, `Want`, `Swarm`, `Error`, `is_info_hash`.
- `engine.rs` — `Engine`: its own tokio runtime, the lazily started `librqbit`
  session, the loaded torrents (slots with lease counts), the janitor that
  unloads idle torrents and stops an idle session.
- `body.rs` — `Lease` (keeps a torrent loaded), `Probe` (a sparse tree that is
  deleted when dropped, unless a read still holds the torrent), `Body` (`Read`
  over a byte range, blocking on pieces).
- `testing.rs` — `Seeder`: a real `librqbit` session seeding a folder on
  loopback (no DHT, trackers or LSD), and an `Engine` wired to it. Public under
  the `test-support` feature so `pawse` tests can index a real torrent.
- `tests.rs` — the engine against that seeder.

## Behaviour worth knowing

- **Blocking API, own runtime.** Every call blocks the caller on the engine's
  runtime, so it must not be called from inside a tokio runtime.
- **Patience follows the swarm, not the clock.** A piece is readable only once
  it is complete and verified, and with 16 MiB pieces that can take minutes on
  a slow swarm while bytes keep arriving. So `read` (opening, first byte) and
  `Body::read` give up only after their quiet time — `read`'s timeout, 30 s for
  `Body::read` — passes without the torrent receiving any byte from any peer
  (`patient`, watching librqbit's `fetched_bytes`), and after 5 minutes in any
  case. If no peer is connected at that moment the error is `NoPeers`, not
  `Timeout`: `pawse` treats it as final instead of retrying, so a click on a
  track while offline fails in one quiet time rather than after six retries.
- **Helper streams.** librqbit's lookahead is 32 MB, not a number of pieces:
  with 1 MiB pieces that keeps ~32 pieces (and as many peers) busy, with 16 MiB
  pieces only 2. `read` therefore opens up to 3 extra streams parked 32 MB,
  64 MB, … past its start (`helper_count`: enough for ~8 pieces in flight),
  which only raise those pieces' priority and live as long as the `Body`.
  They are opened only after the first byte arrived: librqbit interleaves
  streams in random order, and on a thin swarm a helper's piece taken first
  would delay the start of the track by a whole piece. A probe's
  duration is a stall limit, not a deadline: it fails with `Timeout` only after
  that long without a single new byte, so a slow swarm finishes and a dead one
  gives up quickly.
- **Stream count is limited.** librqbit 9 gives every open file stream one
  permit of its blocking-I/O semaphore for the stream's whole life, and writing
  a received block to disk needs a permit from the same semaphore. With as many
  open streams as permits (librqbit's default is 8) no block can be written and
  every stream waits forever. So the session gets `BLOCKING_PERMITS` (64), a
  probe opens at most `PROBE_STREAMS` (4) streams at a time, probe and helper
  streams together stay under an engine-wide `SHARED_STREAMS` (24) — helpers
  are skipped when it is exhausted — and `read` bounds opening its stream by
  its timeout too.
- **Idle peers wake up late.** A peer with nothing to fetch sleeps up to 5 s;
  opening a stream or seeking does not wake it (librqbit only notifies when
  pieces are released). A read that starts while the torrent is idle — a track
  starting, a seek — can therefore wait a few seconds before its first byte.
- **Lookahead.** Each stream asks for up to 32 MB ahead of its position
  (hardcoded in librqbit), so a fast swarm delivers more than a probe asked
  for: probing 11 FLAC heads of a real album fetched about 160 MB before the
  probe was dropped. It is deleted with the probe.
- **`swarm`** reports the peers of a loaded torrent: connected now, and known
  (seen). librqbit keeps whether a peer has the whole torrent to itself, so
  seeds cannot be counted; an unloaded torrent reports nothing.
- **State vs work.** `state_dir` keeps `<info_hash>.torrent` (and `dht.json`):
  a torrent is known once `resolve` stored it, and `meta` reads it back without
  the network. `work_dir` holds piece data only, `<work>/<info_hash>/<path>`;
  it is wiped when an `Engine` is created and owned by nobody else.
- **Nothing is selected.** Torrents are added with `only_files = []`, so the
  session never downloads a file on its own. Every byte comes from a stream
  (`librqbit` gives a stream's pieces priority whether or not the file is
  selected), which is how `probe` gets the head of a file without the rest.
  Whole pieces are fetched (piece length is the torrent's, 256 KiB–16 MiB), and
  a stream looks up to 32 MB ahead, so a probe of many files costs some extra
  pieces in flight — it is dropped as soon as the wanted bytes are there.
- **Leases.** `probe` and `read` hold a lease on the torrent; a torrent with no
  lease is unloaded (session entry and its work data deleted) after
  `idle_unload`, or at once when the last lease was a probe's. The work dir
  limit (`work_limit_bytes`) is checked whenever a torrent's last lease is
  released — back-to-back reads (a cache fill, playback with prefetch) leave a
  torrent idle only for moments, which the janitor's 10 s tick would miss — and
  by the janitor, which unloads idle torrents oldest first. Sizes are measured
  on blocking threads and skip the indexer's `<hash>.view` hard links.
  `unload` deletes a work root only when it unloaded a torrent (or `forget`
  forces it), so a late unload never wipes a folder a new `acquire` is filling.
- **Unloading is exclusive.** A hash being unloaded is in `unloading` until its
  directory is gone; `acquire` waits for that instead of adding the torrent
  again into a folder that is about to be deleted. A torrent whose start fails
  is deleted again, so no session entry outlives a failed `acquire`.
- **The session** starts on first use and stops when nothing is loaded, nothing
  is in flight (`resolve`, `meta`, `acquire` count as busy) and nothing was
  used for `idle_unload` — so no seeding happens while nothing plays.
- **Upload.** `WhileActive` uploads without a limit while a torrent is loaded,
  `Limited` caps it (KiB/s, applied live). `Off` starts sessions with
  uploading disabled. Turning it on or off while a session runs cannot change
  that session: it is capped at 16 KiB/s (lower would stop librqbit's upload
  scheduler, whose chunks are 16 KiB) and restarted as soon as it is idle.
- **Sparse files, not full-length ones.** A probe's tree has zeros where nothing
  arrived, but a file is only as long as the furthest piece written to it —
  librqbit does not preallocate. Anything that needs the real length (tag
  readers computing a bitrate) must use `FileEntry::len`, not the file on disk.
  librqbit marks its files sparse on every platform, NTFS included.
- **Info hashes** are 40 lowercase hex digits; anything else is `Unknown`, so a
  hash never becomes a path outside `state_dir`/`work_dir`.
- **Network.** `Public` runs DHT (its routing table in `state_dir/dht.json`),
  trackers, local service discovery, and listens on TCP and uTP — many peers
  are reachable only over uTP. `Local` is loopback-only with fixed peers, for
  tests.
- **No protocol encryption.** `librqbit` does not speak MSE/PE, so peers set
  to require encryption drop the connection during the handshake; the rest of
  the swarm still serves. A thin or slow swarm makes the first index slow and a
  read time out (`Error::Timeout` → the source shows offline and is retried).
- **TLS** (HTTPS trackers) is `rustls` on `aws-lc-rs`, which compiles C code:
  it builds natively everywhere, but `cargo check --target
  x86_64-pc-windows-msvc` from macOS fails for lack of MSVC headers.

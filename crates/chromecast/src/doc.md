# chromecast

A blocking Google Cast (CASTV2) sender for the Default Media Receiver: connect, launch or
join the receiver app, `LOAD` a URL, play / pause / seek / stop, receiver volume, status
events. It knows nothing about tracks or files; call it off the UI thread.

Not here: what to load, the media server, polling and end-of-track logic, mDNS browsing
(`crates/cast/src/doc.md`, `chromecast_driver`).

## Files

- `lib.rs` — `Client` (requests), `Device` (from an mDNS record), the status types and
  their parsing, `Event`, the `LOAD` payload.
- `connection.rs` — TLS and the I/O thread.
- `proto.rs` — the `CastMessage` protobuf, encoded and decoded by hand. The layout is
  pinned by a byte-level test against the reference encoding.
- `testing.rs` — `FakeChromecast` (feature `test-support`): a TLS receiver on loopback
  that answers the receiver and media namespaces and fetches the media URL it is given, so
  tests see what a device would download. `cannot_reach_media(player, idle_reason)` skips
  that fetch and sits in the given player state instead (BUFFERING, or IDLE with a reason).
- `tests.rs` — the client against the fake.
- `../testdata/fake.{crt,key}` — the fake's self-signed P-256 certificate. It must be
  X.509 v3 (webpki rejects the v1 certificates LibreSSL makes by default) and the key
  PKCS#8.

## Connection

- **TLS without verification.** Cast devices present self-signed certificates;
  `AnyCertificate` accepts any certificate but still checks the handshake signatures with
  ring's algorithms. The server name is the IP.
- **One thread per connection.** The socket has a 20 ms read timeout; the thread writes
  queued messages, sends a heartbeat PING every 5 s, answers the device's PINGs, reads
  frames and routes them: a reply goes to the caller waiting on its `requestId`, and every
  status is also an `Event`. 20 s without any message from the device closes the
  connection.
- **Framing.** A 4-byte big-endian length, then the protobuf. Only the fields CASTV2 uses
  are written (protocol version 0, source, destination, namespace, payload type STRING,
  payload); unknown fields and binary payloads are skipped when reading. Frames over
  64 KiB close the connection.
- Dropping the `Client` sends `CLOSE` and joins the thread; it does not stop the receiver
  app (`stop_app` does). Any other end of the connection is `Event::Disconnected(reason)`;
  a `CLOSE` from the device on the connection namespace is `Event::TransportClosed`.

## Requests

- Requests time out after 10 s, `LOAD` and `LAUNCH` after 30 s (the device fetches the
  media before answering).
- `LOAD_FAILED`, `LOAD_CANCELLED`, `INVALID_REQUEST`, `LAUNCH_ERROR` and
  `INVALID_PLAYER_STATE` replies are `Error::Rejected` for the waiting caller and
  `Event::LoadFailed` on the event channel.
- **Launch joins.** `launch` first asks for the receiver status and joins the Default Media
  Receiver if it already runs (ours from before, or another sender's) instead of
  restarting it. Joining is a `CONNECT` to the app's transport id; media commands go there.

## Discovery records

`_googlecast._tcp` TXT: `id` (required), `fn` (name), `md` (model), `ca` (capability bits;
devices without the audio-out bit, value 4, are skipped). IPv4 addresses win over IPv6.

## Decisions

Owner's calls; ask before changing.

- Hand-written protobuf, not `prost-build`: CASTV2 has exactly one message, an envelope of
  seven fields whose payload is a JSON string, and a generator would need `protoc` on every
  CI platform and in the Flatpak build.

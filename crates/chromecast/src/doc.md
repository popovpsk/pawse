# chromecast

A blocking Google Cast (CASTV2) sender for the Default Media Receiver. It
knows nothing about tracks or files; `cast::chromecast_driver` drives it.

## Files

- `lib.rs` — `Client` (requests), `Device` (from an mDNS record), the status
  types and their parsing, `Event`.
- `connection.rs` — TLS and the I/O thread.
- `proto.rs` — the `CastMessage` protobuf, encoded and decoded by hand. CASTV2
  has exactly one protobuf message, an envelope of seven fields whose payload
  is a JSON string; a generator (`prost-build`) would need `protoc` on every
  CI platform and in the Flatpak build for that. The layout is pinned by a
  byte-level test against the reference encoding.
- `testing.rs` — `FakeChromecast` (feature `test-support`): a TLS receiver on
  loopback that answers the receiver and media namespaces and fetches the
  media URL it is given, so tests see what a device would download.
- `tests.rs` — the client against the fake.
- `../testdata/fake.{crt,key}` — the fake's self-signed P-256 certificate
  (X.509 v3: webpki rejects the v1 certificates LibreSSL makes by default;
  the key must be PKCS#8).

## Behaviour worth knowing

- **TLS without verification.** Cast devices present self-signed
  certificates; `AnyCertificate` accepts any certificate but still checks the
  handshake signatures with ring's algorithms. The server name is the IP.
- **One thread per connection.** The socket has a 20 ms read timeout; the
  thread writes queued messages, sends a heartbeat PING every 5 s, answers
  the device's PINGs, reads frames and routes them: a reply goes to the
  caller waiting on its `requestId`, and every status is also an `Event`.
  20 s without any message from the device closes the connection.
- **Framing.** A 4-byte big-endian length, then the protobuf. Only the fields
  CASTV2 uses are written (protocol version 0, source, destination,
  namespace, payload type STRING, payload); unknown fields and binary
  payloads are skipped when reading. Frames over 64 KiB close the connection.
- **Requests** time out after 10 s, LOAD and LAUNCH after 30 s (the device
  fetches the media before answering). `LOAD_FAILED`, `LOAD_CANCELLED`,
  `INVALID_REQUEST`, `LAUNCH_ERROR` and `INVALID_PLAYER_STATE` replies are
  `Error::Rejected`.
- **Launch joins.** `launch` first asks for the receiver status and joins the
  Default Media Receiver if it already runs (ours from before, or another
  sender's) instead of restarting it.
- **Discovery records.** `_googlecast._tcp` TXT: `id` (required), `fn` (name),
  `md` (model), `ca` (capability bits; devices without the audio-out bit,
  value 4, are skipped). IPv4 addresses win over IPv6. Android TV and Google TV
  devices with Chromecast built-in advertise the same service.

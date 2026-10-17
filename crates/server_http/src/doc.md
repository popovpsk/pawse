# server_http

The HTTP pieces the media-server clients (`subsonic`, `jellyfin`, `dlna`) share: the
`ureq` agent, status classification, byte-range requests and replies, a TLS fallback and
serde helpers for sloppy server JSON. Blocking; knows no server protocol, and nothing of
`media_stream` or the library.

Not here: each client's protocol and error type (`subsonic/src/doc.md`,
`jellyfin/src/doc.md`, `dlna/src/doc.md`); the range reader built on these requests
(`media_stream/src/doc.md`).

## Files

- `lib.rs` — `agent()` and its timeouts, `classify` / `Status`, `range_header`,
  `with_range`, `range_body` / `RangeBody`, `parse_content_range`, `read_capped`,
  `is_json`, `redact`.
- `transport.rs` — `Transport`: the pair of agents (bundled roots, OS trust store) a client
  calls through, and the retry between them.
- `lenient.rs` — serde helpers: an odd value becomes `None` / empty / default instead of
  failing the record. `number` takes ints, floats (rounded) and numeric strings; a value
  that does not fit the target type is `None`. `id` is the one strict helper: a missing or
  empty id is an error, so the caller can skip that record.
- `tests.rs`.

## Rules

- `classify` is the one place that decides what a status means: 401/403 `Auth`, 5xx
  `Transient`, other non-2xx `Failed`. Clients map these onto their own errors; the UI's
  split between "wrong password" and "server is down" depends on it.
- Agents are built with `http_status_as_error(false)`: a non-2xx reply is a `Response`, not
  an `Err`, and the client must classify it.
- `range_header(start, end)` takes an exclusive `end` and writes the inclusive HTTP form.
- `with_range` sets a 15 s response timeout, and for a bounded range a 30 s body timeout, so
  a stalled connection becomes a retry quickly. An open-ended range keeps the agent's body
  timeout.
- `range_body`: a `206` needs a parseable `Content-Range` (else an error); any other reply
  means the server ignored the range and sends the whole file (`ranged: false`, `offset` 0,
  total from `Content-Length`). The caller classifies the status first.
- Agents keep up to 8 idle connections per host (`IDLE_PER_HOST`; ureq's default is 3): the
  sound analysis reads 4 server files at once in small ranges, and with 3 every fourth
  request opened a new connection. The reader's half of keep-alive (a body must be read to
  its end to return the connection to the pool) is in `media_stream/src/doc.md`.
- `redact` strips query strings from error texts: Subsonic carries credentials in the URL
  and transport errors echo it. `subsonic` and `jellyfin` pass their transport errors
  through it.
- `read_capped` truncates silently at the cap; the caller decides what a short body means.

## Certificates

- `Transport::call` runs the request on the bundled-roots agent and, only on a rustls
  `UnknownIssuer` (a private CA, a TLS-inspecting antivirus or proxy), repeats it once on an
  agent that uses the OS verifier (`RootCerts::PlatformVerifier`). The OS verifier is the
  fallback, not the default: on Linux it has no roots of its own and fails when no CA bundle
  is installed.
- The repeat is safe because the handshake fails before any request bytes are sent: it
  cannot duplicate a scrobble or a star. The closure given to `call` rebuilds the request on
  each attempt.
- After a fallback success the `Transport` stays on the system agent (logged once).
- If the fallback fails verification too, the error carries both reasons,
  `<first> (system store: <second>)`, and a warning is logged. Any other fallback error is
  returned as is; a first error other than `UnknownIssuer` is not retried.
- Verification is never turned off.
- `subsonic` and `jellyfin` call through a `Transport`; `dlna` uses plain `agent()` and
  speaks HTTP to the LAN.

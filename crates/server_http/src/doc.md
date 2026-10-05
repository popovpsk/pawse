# server_http

The HTTP pieces every media-server client needs, so `subsonic`, `jellyfin` (and
a future DLNA client) do not each carry a copy. Blocking `ureq`, no knowledge of
any server protocol and no dependency on `media_stream` or the library.

## Files

- `lib.rs` — the shared agent, status classification, byte-range requests and
  replies (`range_header`, `with_range`, `range_body`, `RangeBody`),
  `parse_content_range`, `redact`, `read_capped`, `is_json`.
- `transport.rs` — `Transport`: the pair of agents (bundled roots, system trust
  store) a client calls through, and the retry between them.
- `lenient.rs` — serde helpers that turn odd JSON (numbers as strings or floats,
  a list that is not a list, an object that is not an object) into `None`/empty
  instead of failing the whole record. `id` is the one strict helper: a record
  without an id is an error, so the caller can skip it.
- `tests.rs`.

## Behaviour worth knowing

- **`classify`** is the one place that decides what an HTTP status means:
  401/403 is `Auth`, 5xx is `Transient`, other non-2xx is `Failed`. Clients map
  these onto their own error types; the split between "wrong password" and
  "server is down" is what the UI shows.
- **Ranges.** `with_range` gives a range request a 15 s response timeout and,
  for a bounded range, a 30 s body timeout, so a stalled connection becomes a
  retry quickly. `range_body` reads a `206` with its `Content-Range`; any other
  success means the server ignored the range and sends the whole file
  (`ranged: false`, total from `Content-Length`).
- **Certificates.** `agent()` trusts the bundled Mozilla roots. A user's own
  server is often signed by a CA only the OS knows (a private CA, a TLS-inspecting
  antivirus or proxy, a LAN DNS override), which fails as `UnknownIssuer`.
  `Transport::call` runs the request on the bundled-roots agent first and, only
  on `UnknownIssuer`, repeats it once on an agent that uses the OS verifier
  (`RootCerts::PlatformVerifier`). The first attempt is therefore unchanged for
  everyone it already worked for; on Linux the OS verifier has no bundled roots
  of its own and fails when no CA bundle is installed, which is why it is the
  fallback and not the default. The handshake fails before any request bytes are
  sent, so the repeat cannot duplicate a scrobble or a star. After a fallback
  success the `Transport` stays on the system agent (and logs that once). If
  the fallback fails verification too, the error text carries both reasons,
  `<first> (system store: <second>)`, and a warning is logged: a screenshot of
  that text shows the system store was consulted, and why it refused (an
  `UnknownIssuer` there, or "no CA certificates" on a Linux without a CA bundle).
  Any other fallback error is returned as is. Verification is never turned
  off. The closure given to `call` rebuilds the request on each attempt.
  `dlna` keeps plain `agent()`: it speaks HTTP to the LAN.
- **`redact`** strips query strings from error texts: some protocols (Subsonic)
  carry credentials in the URL, and transport errors echo it.

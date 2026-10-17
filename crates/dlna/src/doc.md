# dlna

A blocking client for UPnP/DLNA media servers (SSDP discovery, ContentDirectory listing, media
bytes) and for media renderers (AVTransport / RenderingControl / ConnectionManager control,
discovery, announcements). It knows nothing about the library database. Blocking `ureq` and
plain UDP sockets.

Not here: `Item` → `RemoteSong`, bitrate and extension rules, the Sync button
(`pawse/src/servers/doc.md`, "DLNA"); what a cast session does with a `Renderer`
(`cast/src/doc.md`); status classification, ranges and `read_capped` (`server_http/src/doc.md`).

## Files

- `lib.rs` — `Client`, `Config`, `Device`, `Error`, `describe` (an address typed by the user →
  `Device`), `discover` (SSDP → servers), the listing, ranges and covers.
- `ssdp.rs` — M-SEARCH over every IPv4 interface (and to given hosts), reply parsing,
  `NotifyListener` (announcements on port 1900).
- `device.rs` — the description: the device carrying a ContentDirectory (for a renderer:
  AVTransport, with RenderingControl and ConnectionManager beside it), UDN, name, control URLs.
- `renderer.rs` — `Renderer`, `discover_renderers`, `describe_renderer`, `track_didl` (the
  DIDL-Lite sent with `SetAVTransportURI`).
- `soap.rs` — the SOAP envelope, out-arguments, UPnP faults.
- `didl.rs` — DIDL-Lite → `Item` / `Res`, `Item::pick`, `parse_duration`.
- `address.rs` — keys relative to the server's address, and back to URLs.
- `xml.rs` — `roxmltree` helpers matching elements by local name.
- `tests.rs` — a `TcpListener` stub server (description, SOAP, media); `captured` checks
  responses saved from real servers in `testdata/<server>/`, one `#[case]` per server.

## A server is its UDN

- `Config` keeps the UDN and the last known description URL. `ping` always re-reads the
  description; when it does not answer, or another UDN does, an SSDP search for that UDN (3 s)
  finds where the server moved (new DHCP address, new port after a restart).
- `Client::location` is where it answers now. The app saves it after a sync
  (`ServerClient::moved`), so the next start does not begin at a dead address and a server whose
  multicast is later filtered still starts from where it was last seen.
- Any transport failure or 5xx drops the resolved endpoint; the next request resolves again.
- A UDN search that found nothing is not repeated for 30 s by ordinary requests (`media_stream`
  retries would each wait 3 s); `ping` (sync, the offline watcher) always searches.

## Keys

- A song's key (`Res::id`) is the chosen `res` URL without scheme and host (`/MediaItems/12.flac`;
  `:10243/WMPNSSv4/…` for another port of the same host; the whole URL for another host) plus
  `#<size>`. It survives an address change.
- The size is in the key because MiniDLNA reuses its numbers: after a rescan
  `/MediaItems/455.flac` names another song, and a plain path key would refresh the old track's
  tags in place, moving its likes, plays and playlist entries onto a different song. With the size
  a reused address is a new key; the new song is adopted by size (`music_library`'s adoption).
- The `#…` part is never requested (`address::path` strips it). Covers (`Item::cover_id`) carry
  their track's size for the same reason: `/AlbumArt/1-455.jpg` is numbered like the media.

## Listing

- `Search(upnp:class derivedfrom "object.item.audioItem")` from the root when
  `GetSearchCapabilities` lists `upnp:class` (or `*`). Anything else (no search, a UPnP error on
  any page, an empty result) falls back to a breadth-first `Browse` of the whole tree, each
  container once.
- Items are kept once per chosen media key: the same file shows up under folders, artists, albums
  and genres.
- Paging follows `NumberReturned` (the entry count when omitted) until an empty page or
  `TotalMatches`. A page that brings no new entry means the server ignores `StartingIndex`: an
  error.
- A transport failure or an unparseable page fails the whole listing: a partial one would retire
  every song past the failure.
- Which `res`: only `http-get`; never raw PCM (`audio/L16`, … — the decoder cannot read it without
  a container) and never `image/`, `video/` or `text/` resources. The first non-transcode
  (`DLNA.ORG_CI=1`) wins, else the first transcode; an item with none is left out.
- Servers use `dlna:` / `sec:` / `pv:` prefixes without declaring them, which `roxmltree`
  rejects: a DIDL that does not parse is parsed again with the usual namespaces declared on its
  root.
- CUE images are not split: MiniDLNA neither reads `.cue` sheets nor serves them, so a FLAC + CUE
  album is one hour-long item titled after the file, without artist or album.

## Media bytes

- Ranges ask the media URL with `Range` and `getcontentFeatures.dlna.org: 1` (some servers refuse
  media requests without it); a `200` means the range was ignored.
- A `416` on a bounded range is asked again open-ended (`bytes=start-`) and the body is cut to the
  length first asked for: MiniDLNA answers `416` to a range whose end lies past the file instead
  of clamping, and `media_stream` asks for up to 4 MB, so every smaller file would fail.
- Covers are capped at 32 MB, SOAP and description bodies at 64 MB.
- A SOAP fault is read before the HTTP status.

## Discovery and announcements

- `discover` sends M-SEARCH for ContentDirectory:1 and MediaServer:1 on every non-loopback IPv4
  interface (`IP_MULTICAST_IF` per socket, so a VPN or a second network card does not swallow
  them) and describes each replying location with a 3 s timeout. Devices are de-duplicated by
  UDN; those without a ContentDirectory are dropped.
- `search` can also send the requests unicast to `<host>:1900` of given hosts (renderer discovery
  uses it). They keep the multicast `HOST` and `MX`, which Platinum-based devices require.
- `NotifyListener::open` fails only when no interface joined the group. `next` returns
  `ssdp:alive` / `ssdp:update` (they must carry a `LOCATION`) and `ssdp:byebye`, with the UDN from
  `USN`, the NT and the sender's address.

## Renderers

- `discover_renderers` searches `AVTransport:1` and `MediaRenderer:1` (also unicast to the hosts it
  is given) and keeps devices whose description has an AVTransport service. A renderer is
  addressed by its description URL only.
- SOAP calls to a renderer time out after 5 s (descriptions after 3 s); the server agent's 60 s
  would freeze a session on a TV that went to sleep.
- `Seek` uses `REL_TIME` with whole seconds (`H:MM:SS`), the form every renderer accepts.
  `GetPositionInfo` times may carry fractions or be `NOT_IMPLEMENTED` (unknown).
- `sink_protocols` is `GetProtocolInfo`'s `Sink` list. Volume and mute are RenderingControl's
  Master channel; `GetMute` answers `0` / `1` or `false` / `true`.

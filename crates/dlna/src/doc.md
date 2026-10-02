# dlna

A blocking client for UPnP/DLNA media servers (MiniDLNA/ReadyMedia, NAS and
router media servers, Plex and Jellyfin in DLNA mode, Serviio, Gerbera, the
Windows media-streaming service). It knows nothing about the library database;
`pawse::servers::dlna` turns its `Item`s into `music_library::RemoteSong`s.
Blocking `ureq` and plain UDP sockets on the caller's thread — no async runtime.

## Files

- `lib.rs` — `Client`, `Config`, `Device`, `Error`, `describe` (an address typed
  by the user → `Device`), `discover` (SSDP → `Device`s), the listing (search,
  browse, paging, de-duplication), ranges and covers.
- `ssdp.rs` — M-SEARCH over every IPv4 interface and reply parsing.
- `device.rs` — the device description: the device that carries a
  ContentDirectory service, its UDN, name, model and control URL.
- `soap.rs` — the SOAP envelope, out-arguments and UPnP faults.
- `didl.rs` — DIDL-Lite → `Item`/`Res`, `Item::pick`, `parse_duration`.
- `address.rs` — keys relative to the server's address, and back to URLs.
- `xml.rs` — `roxmltree` helpers matching elements by local name.
- `tests.rs` — a `TcpListener` stub server: description, SOAP and media;
  `captured` checks responses saved from real servers (`testdata/<server>/`,
  one `#[case]` per server).

## Behaviour worth knowing

- **A server is its UDN, not its address.** `Config` keeps the UDN and the last
  known description URL. `ping` always re-reads the description; when it does
  not answer, or answers with another UDN, an SSDP search for that UDN (3 s)
  finds where the server moved (a NAS or a router that got a new address from
  DHCP, a server that picks a new port on restart). `Client::location` is where
  it answers now; the app saves it after a sync (`ServerClient::moved`), so the
  next start does not begin at a dead address — and a server whose multicast is
  later filtered still starts from where it was last seen.
  Any transport failure or 5xx drops the resolved endpoint, so the next request
  resolves again.
- **Keys are media paths plus the file size, not object ids.** A song's key
  (`Res::id`) is the URL of the chosen `res` without scheme and host —
  `/MediaItems/12.flac`, or `:10243/WMPNSSv4/…` when the media is served on
  another port of the same host (the Windows service), or the whole URL when it
  is on another host — followed by `#<size>`. So a key survives an address
  change. The size is there because MiniDLNA *reuses* its numbers: after files
  are added or removed and it rescans, `/MediaItems/455.flac` names another
  song, and a plain path key would make the library refresh the old track's
  tags in place — its likes, plays and playlist entries silently moving to a
  different song. With the size a reused address is a new key: the old song is
  gone, the new one is adopted by size (see `music_library`'s adoption). The
  `#…` part is never requested (`address::path` strips it). Covers
  (`Item::cover_id`) carry their track's size for the same reason: MiniDLNA's
  `/AlbumArt/1-455.jpg` is numbered like the media.
- **Listing.** `Search(upnp:class derivedfrom "object.item.audioItem")` from
  the root when `GetSearchCapabilities` lists `upnp:class` (or `*`). Anything
  else — no search, a UPnP error on any page, an empty result — falls back to a
  breadth-first `Browse` of the whole tree, each container once. Items are kept
  once per chosen media key: the same file shows up under folders, artists,
  albums and genres. Paging follows `NumberReturned` (the entry count when a
  server omits it) until an empty page or `TotalMatches`. A page that brings no
  new entry means the server ignores `StartingIndex` and is an error. A
  transport failure on any page fails the whole listing: a partial listing
  would retire every song past the failure.
- **Genres.** An item may carry several `upnp:genre` elements; `Item::genres`
  keeps them all, and the app's adapter joins them into the one genre string it
  hands the library.
- **Which `res`.** Only `http-get` resources; raw PCM (`audio/L16`, …) never,
  the decoder cannot read it without a container. The first one that is not a
  transcode (`DLNA.ORG_CI=1`) wins; without one, the first transcode.
- **Namespaces.** Servers use `dlna:`/`sec:`/`pv:` prefixes without declaring
  them; `roxmltree` rejects that, so a DIDL that does not parse is parsed again
  with the usual namespaces declared on its root.
- **Discovery** sends two M-SEARCHes for ContentDirectory:1 and MediaServer:1 on
  every non-loopback IPv4 interface (`IP_MULTICAST_IF` per socket, so a VPN or a
  second network card does not swallow them) and describes each replying
  location with a 3 s timeout. Devices without a ContentDirectory are dropped.
- **Addresses typed by hand** may be a full description URL or just
  `host:port`; the latter tries `/rootDesc.xml`, `/description.xml` and
  `/DeviceDescription.xml`.
- **Ranges** ask the media URL with `Range` and `getcontentFeatures.dlna.org: 1`
  (some servers refuse media requests without it); a `200` means the server
  ignored the range. MiniDLNA answers `416` to a range whose end lies past the
  file instead of clamping it — and `media_stream` asks for 4 MB at a time, so
  every file under 4 MB would fail — so a `416` is asked again open-ended
  (`bytes=start-`) and the body is cut to the length first asked for. Covers
  are capped at 32 MB, SOAP bodies at 64 MB.
- **Re-finding backs off.** A search for the UDN that finds nothing is not
  repeated for 30 s by requests (`media_stream` retries would otherwise wait
  3 s each); `ping` — sync and the offline watcher — always searches.
- **MiniDLNA quirks seen live** (1.3.3 from Homebrew): `Search` fails with
  708 (its own SQL uses a double-quoted string the bundled SQLite rejects), so
  the listing browses; `bitrate` is bits per second, not bytes; requests
  through `localhost` get `400` (it only answers on the LAN address); `.opus`
  is not indexed; a folder shared as audio lists videos as `audio/mp4`; Ogg
  files are served as `/MediaItems/N.dat` (the extension comes from the MIME
  type). `testdata/minidlna/` holds real responses. MiniDLNA 1.3.3 on Debian
  (`testdata/minidlna-pi/`) searches fine.
- **CUE images come through whole.** MiniDLNA neither reads `.cue` sheets nor
  serves them, so an album ripped as one FLAC + CUE is listed as one hour-long
  item titled after the file, without artist or album. Nothing on the DLNA side
  can split it; only a cuesheet embedded in the FLAC could be read from its
  head, and the images seen so far keep theirs outside.

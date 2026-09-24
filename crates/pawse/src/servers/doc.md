# servers

Media-server protocols behind one interface. Everything above this module —
sync, playback, settings rows — talks to `ServerClient` and `ServerKind`, never
to `subsonic::` or `jellyfin::` directly.

## Files

- `mod.rs` — `ServerKind` (the closed list of protocols, with its stored name and
  title), `RemoteConfig` (a server's saved connection, one variant per kind),
  `RemoteServer` (a configured server: uri, name, config), `RemoteError`,
  the `ServerClient` trait, and the tag cleanups every adapter shares
  (`real_artist`, `real_album`, `real_track_number`).
- `subsonic.rs` — the Subsonic adapter: `subsonic::Song` → `RemoteSong`,
  `subsonic::Error` → `RemoteError`.
- `jellyfin.rs` — the Jellyfin adapter: `jellyfin::Item` → `RemoteSong`, error
  mapping, and `authenticate` (log in once, get a token).

## Adding a protocol

1. A client crate (blocking, using `server_http`).
2. A `ServerKind` variant. Every `match` on the kind is exhaustive — settings
   removal, button ids, `RemoteConfig::kind` — so the compiler lists what is left.
3. A `RemoteConfig` variant and an adapter implementing `ServerClient`.
4. If bytes do not come as HTTP ranges, a `remote_media::SourceMedia`
   implementation instead of `HttpMedia`.

## Behaviour worth knowing

- **Servers are keyed `kind:uri`** (`source_key`), in `remote_sync::source_ids`,
  the sync queue, `LibraryEvent`s and the settings rows, so two kinds at the
  same address never share state.
- **`RemoteSong` is the library's model, not a protocol's.** Adapters convert
  units on the way in: duration to ms, bitrate to kbit/s (`bitrate_kbps`;
  Jellyfin sends bit/s), size in bytes, `suffix` = the file's extension (the
  decoder picks a backend by it). `artist` is the first credited artist;
  `artist_aliases` are other names the same recording's artist is credited
  under, used only for matching (Subsonic: the joined display name and the
  other credited artists; Jellyfin: the other entries of its split `Artists`).
- **Jellyfin's extension** comes from the file path, else from the container
  list, preferring a known audio extension (`mov,mp4,m4a,…` → `m4a`).
- **Placeholders** (`[Unknown Artist]`, `[Unknown Album]`) become empty, and a
  track number above 999 is dropped — Navidrome takes one from a leading number
  in an untagged file name.

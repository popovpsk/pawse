# playlist_file

Reads `.m3u`, `.m3u8` and `.pls` playlist files into a name and a list of paths, and finds a
library file for a path that does not match exactly (`PathIndex`). Pure and GPUI-free: no
database, no network; `read` is the only file access.

Not here: turning paths into library items and copying into playlists (`pawse/src/doc.md`,
"Playlist import" and "Folder playlists"); finding playlist files in music folders
(`music_indexer/src/doc.md`); the exact path lookup (`music_library/src/doc.md`, "Playlists
and likes").

## Files

- `parse.rs` — `EXTENSIONS`, `is_playlist`, `Playlist`, `read`, `parse`: decoding, the M3U
  and PLS line rules, entry to path.
- `path_index.rs` — `PathIndex`: the fallback match of a path against the library's paths.
- `lib.rs` — re-exports. `tests.rs` — the tests.

## Parsing

- Text: a byte order mark decides the encoding (UTF-8, UTF-16); else valid UTF-8 is UTF-8;
  else a `.m3u8` is lossy UTF-8 and anything else is guessed by `chardetng`. Winamp and AIMP
  write `.m3u` in the system code page, and a Windows-1252 fallback (the CUE rule) would
  turn Cyrillic paths into names that match nothing.
- Lines split on `\n` and on a lone `\r` (classic Mac exports).
- PLS is recognised by its first non-blank line `[playlist]`, not the extension. Entries are
  `FileN=` in `N` order (players write them out of order); titles and lengths are ignored.
- M3U: `#` lines are directives; only `#PLAYLIST:` is read (the name). `#EXTINF` is ignored:
  matching by artist and title would be name parsing, which the owner declined.
- The name is `#PLAYLIST:`, else the file stem.
- An entry with a `scheme://` other than `file` is a stream and is dropped. A bare colon is
  not a scheme (`Re:Zero.flac`). `file:` URIs are percent-decoded; `localhost` and an empty
  host are local, another host is a UNC path, `/C:/` loses its leading slash, and a
  drive in the host's place (`file://C:/…`, written by some players) is the path.
- Separators become the platform's. A relative entry joins the playlist's folder; `.` and
  `..` are resolved lexically (no file system access, a missing file still yields a path).
  On macOS and Linux a `C:\…` entry keeps its drive (as `C:/…`): it matches nothing
  exactly but keeps its components for `PathIndex`.

## Path matching (`PathIndex`)

The caller tries the exact path first; `PathIndex` is the fallback.

- Folded: NFC plus lowercase, so a playlist written on Windows finds a macOS file with
  decomposed letters (`й`) or different case. Only a unique folded match counts.
- Path tail: the library path sharing the most trailing components with the entry, at least
  two (folder and file name), and only if no other path shares as many. This is what finds
  a playlist from another computer or drive. A bare file name never matches: `01.flac`
  exists in every album.
- No extension swapping, no tag lookups.

## Decisions

- Formats: M3U, M3U8 and PLS only. No XSPF, WPL or foobar `.fpl`, and no export.
- Unmatched entries are skipped, never added to the library.
- Fallback matching is the folded path and the unique path tail, nothing based on names.

# lyrics

Lyrics for the player: an LRC parser plus the public lyric types, and an isolated
blocking web client for the LRCLIB lyrics database. GPUI-free so the library crates
can depend on it; networking is synchronous and must run off the UI thread.

## Responsibilities

- Parse LRC text into structured lyrics (`parse_lrc`), distinguishing time-synced
  lyrics from plain ones (`Lyrics::synced` is derived from the presence of time tags).
- Fetch lyrics from LRCLIB (`fetch`) given track metadata, with a get → get-without-
  album → search fallback chain, normalizing empty payloads to `None`.

## Files

- `lib.rs` — module wiring and the public re-exports (`Lyrics`, `LyricLine`,
  `parse_lrc`, `LyricsQuery`, `RemoteLyrics`, `fetch`).
- `parser.rs` — `Lyrics` / `LyricLine` types and the LRC parser:
  - `parse_lrc`: splits each line into leading `[..]` bracket tags and trailing text.
    Time tags (`[mm:ss.xx]` / `[mm:ss.xxx]` / `[mm:ss]`) become `LyricLine`s; multiple
    tags on one line duplicate the text per tag. Metadata tags (`ti`/`ar`/`al`/`by`/
    `offset`/`length`) and blank lines are dropped. Any time tag sets `synced=true`
    and the lines are sorted by `time_ms`. With no time tags, every non-empty line is
    a plain `LyricLine { time_ms: None, .. }` and `synced=false`. `offset` is parsed
    away but intentionally not applied in v1. Out-of-range timestamps (overflowing
    `u32` ms) are dropped, never panicking.
- `web.rs` — the LRCLIB client:
  - `LyricsQuery` / `RemoteLyrics` types.
  - `fetch`: **blocking** ureq calls (~10s timeouts). Tries `GET /api/get` with album,
    then without album, then `GET /api/search` (structured `track_name`/`artist_name`/
    `album_name` params), picking the closest-duration hit whose artist+title match and
    that has non-empty lyrics. HTTP 404 falls through to the next step; other network/
    HTTP errors return `Err`. Never panics.
  - `parse_response` / `parse_search_response` / `into_remote`: body parsing split out
    from the network so it is unit-tested from JSON fixtures with no live network.
    Empty / whitespace-only `syncedLyrics` / `plainLyrics` map to `None`.

## Non-obvious behavior

- `fetch` is synchronous and blocks the calling thread on I/O. Callers must invoke it
  from a background thread (e.g. GPUI's background executor), never the render thread.
- Synced lyrics keep empty-text lines (timed gaps); plain lyrics drop empty lines.

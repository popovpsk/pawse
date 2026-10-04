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
  `parse_lrc`, `Word`, `Backing`, `locate_words`, `LyricsQuery`, `RemoteLyrics`,
  `fetch`).
- `parser.rs` — `Lyrics` / `LyricLine` types and the LRC parser:
  - `parse_lrc`: splits each line into leading `[..]` bracket tags and trailing text.
    Time tags (`[mm:ss.xx]` / `[mm:ss.xxx]` / `[mm:ss]`) become `LyricLine`s; multiple
    tags on one line duplicate the text per tag. Metadata tags (`ti`/`ar`/`al`/`by`/
    `offset`/`length`) and blank lines are dropped. Any time tag sets `synced=true`
    and the lines are sorted by `time_ms`. With no time tags, every non-empty line is
    a plain `LyricLine { time_ms: None, .. }` and `synced=false`. `offset` is parsed
    away but intentionally not applied in v1. Out-of-range timestamps (overflowing
    `u32` ms) are dropped, never panicking.
  - **Enhanced LRC.** Inline `<mm:ss.xx>` markers in a line's text become
    `LyricLine::words` and are removed from the text. A `<…>` that is not a time
    stays as text ("<3"). A marker followed by nothing or only spaces ends the
    word before it (`end_ms`); otherwise a word's end is open. Spaces on both
    sides of a marker (`<t> When <t> the`) collapse to one. Words are kept
    only on a line with exactly one `[time]` tag — a line repeated under several
    tags would share absolute word times — and plain lines just lose the
    markers.
  - **Backing vocals.** `LyricLine::background` (`Backing`: text + words) is
    sung over the line and shown under it. In LRC it comes from Navidrome's
    `[bg: <mm:ss.xx>word…]` convention: such a line (with no time tag) attaches
    to the line before it — to every copy of a line repeated under several time
    tags, then without words — several are joined with a space, and one before
    any line is dropped. On a blank timed line it becomes the line's own text
    (an instrumental gap that only has backing vocals). Server lyrics fill it
    from OpenSubsonic agents.
- `words.rs` — `Word` (`start_ms`, optional `end_ms`, `range` = byte range in
  the line's text, always on char boundaries), `Backing`, the Enhanced LRC
  splitter, and `locate_words`: finds each cue's trimmed text in the line, in
  order, from where the previous one ended (blank cues, e.g. a timed space, are
  skipped). One cue that cannot be found drops every word of the line, so a
  line is filled either word by word with correct positions or not by words at
  all.
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

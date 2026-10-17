# lyrics

The lyric types, an LRC parser, and a blocking client for the LRCLIB database. GPUI-free;
only `pawse` depends on it. `fetch` blocks on network I/O and must run off the UI thread;
everything else is pure.

Not here: where lyrics come from and which source is shown — disk `.lrc` / embedded text
(`music_indexer/src/doc.md`), per-source rows and the pick order (`music_library/src/doc.md`),
server lyrics (`pawse/src/servers/doc.md`); turning `Lyrics` into rows and the karaoke fill
(`pawse/src/doc.md`, "Lyrics karaoke fill").

## Files

- `parser.rs` — `Lyrics`, `LyricLine`, `parse_lrc`, `parse_time_tag`.
- `words.rs` — `Word`, `Backing`, the Enhanced LRC splitter, `locate_words`.
- `web.rs` — `LyricsQuery`, `RemoteLyrics`, `fetch` and the body parsing.
- `lib.rs` — re-exports.

## LRC parsing (`parse_lrc`)

Never fails or panics: unusable input gives fewer lines.

- Input splits on `\n` and `\r`. Tags count only at the start of a line.
- Time tags: `[mm:ss]`, `[mm:ss.xx]` (hundredths), `[mm:ss.xxx]` (ms). A longer fraction is
  cut to hundredths; a one-digit fraction, or a timestamp overflowing `u32` ms, is not a
  time tag.
- Any other leading tag is dropped: `ti` / `ar` / `al` / `by` / `length`, section labels like
  `[Chorus]`, and `offset`, which is parsed away but not applied.
- Several time tags on a line give one `LyricLine` per tag with the same text.
- Any time tag makes `Lyrics::synced` true; the lines are then sorted by time and untimed
  lines are dropped. Without time tags every non-blank line is a plain line (`time_ms: None`).
- Synced lyrics keep timed lines with blank text (gaps); plain lyrics drop blank lines.

### Enhanced LRC

- Inline `<mm:ss.xx>` markers become `LyricLine::words` and leave the text. A `<…>` that is
  not a time stays text ("<3"). A marker followed by nothing or only spaces ends the word
  before it (`end_ms`); otherwise a word's end is open. Spaces on both sides of a marker
  collapse to one.
- Words are kept only on a line with exactly one `[time]` tag (a line repeated under several
  tags would share absolute word times); plain lines just lose the markers.
- `Word::range` is a byte range into the line's text, always on char boundaries.

### Backing vocals

- `LyricLine::background` (`Backing`: text + words) is sung over the line and shown under it.
  In LRC it is Navidrome's `[bg: <mm:ss.xx>word…]` convention: a `bg` line with no time tag
  attaches to the line before it — to every copy of a line repeated under several time tags,
  then without words. Several are joined with a space; one before any line is dropped.
- On a blank timed line the backing becomes the line's own text (an instrumental gap with
  only backing vocals).
- Subsonic server lyrics fill `background` too, from OpenSubsonic background-role agents.

### `locate_words`

For callers that have word cues but no markers (server lyrics): finds each cue's trimmed text
in the line, in order, from where the previous one ended; blank cues (a timed space) are
skipped. One cue that cannot be found drops every word of the line, so a line is filled
either word by word with correct positions or not by words at all.

## LRCLIB client (`fetch`)

- Chain: `GET /api/get` with the album (sent only if the query has one), then without it
  (only if the query has an album), then `GET /api/search`. `duration` goes on the get
  requests when known. A 404 moves to the next step.
- A get answer without usable lyrics ends the chain with `Ok(None)`, no search: the
  `instrumental` flag (even when text is present), both fields blank, or an unparsable body.
- Search hits are skipped when instrumental, when artist or title does not match
  (case-insensitive; equal or one contains the other; empty never matches), when both
  durations are known and differ by more than 5 s, or without non-blank lyrics. The closest
  duration wins; with an unknown duration a text match is accepted.
- Each request gets up to 5 attempts, 250 ms apart, on transport errors and HTTP 429 / 5xx;
  a 429 `Retry-After` (seconds) replaces the pause, capped at 10 s. Any other failing status
  is `Err`.
- `Ok(None)` means LRCLIB has nothing usable; `Err` means it could not be asked.
- Body parsing (`parse_get`, `select_search_match`, `into_remote`) is split from the network
  and tested from JSON; no test touches the live service.

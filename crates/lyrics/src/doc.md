# lyrics

The lyric types, an LRC parser, and a blocking lookup of lyrics on the web (BiniLyrics,
LRCLIB, lrcmux) that returns the best match as Enhanced LRC. GPUI-free; only `pawse` depends
on it. `fetch` blocks on network I/O and must run off the UI thread; everything else is pure.

Not here: where lyrics come from and which source is shown — disk `.lrc` / embedded text
(`music_indexer/src/doc.md`), per-source rows and the pick order (`music_library/src/doc.md`),
server lyrics (`pawse/src/servers/doc.md`); turning `Lyrics` into rows and the karaoke fill
(`pawse/src/doc.md`, "Lyrics karaoke fill").

## Files

- `parser.rs` — `Lyrics`, `LyricLine`, `parse_lrc`, `parse_time_tag`.
- `words.rs` — `Word`, `Backing`, the Enhanced LRC splitter, `locate_words`.
- `web/mod.rs` — `LyricsQuery`, `WebLyrics`, `fetch`: the provider chain.
- `web/pick.rs` — matching a candidate to the track and ranking candidates.
- `web/binilyrics.rs`, `web/lrclib.rs`, `web/lrcmux.rs` — one provider each: requests and
  body parsing into candidates.
- `web/ttml.rs` — Apple-style TTML to sheet lines. `web/elrc.rs` — sheet lines to
  Enhanced LRC.
- `web/http.rs` — `Session`: the agent, the lookup's deadline, retries, user agent.
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

## Web lookup (`fetch`)

- Providers are asked one after another — BiniLyrics, LRCLIB, lrcmux — and the chain stops
  as soon as the best candidate so far is final (complete, word-timed, uncensored, duration
  within 3 s). Only a later synced sheet that runs on could still beat a first sheet that
  ends early; that is accepted, since Apple's sheets are rarely cut. lrcmux is last because
  its public instance is rate limited per IP.
- The whole lookup has 30 s (`BUDGET`, `http::Session`): every request gets the time left as
  its end-to-end timeout, no retry starts or sleeps past it, and a provider not reached in
  time counts as not asked (so the result is `Err` unless something was found).
- Every provider's answer becomes Enhanced LRC before ranking, so the app stores and parses
  one format: `[mm:ss.xx]` lines, `<mm:ss.xx>` syllable markers, `[bg: …]` backing lines.
  A word's end marker is written only before a gap over 20 ms and at the line end (the
  splitter treats a marker followed by blank text as the end of the word before it). Times
  are hundredths, which every LRC reader takes.
- `Ok(Some)` when any candidate survives, even if another provider failed; `Ok(None)` when
  every provider answered and none had usable lyrics; `Err` when nothing was found and at
  least one provider could not be asked, so the app does not store a "not found" marker.
  A 2xx body that does not parse (a challenge page, a changed API) is a failure, not
  "nothing": it must not become a permanent marker.
- Bodies are parsed apart from the network and tested from JSON / XML; no test touches a
  live service.

### Picking (`pick.rs`)

- A candidate is dropped when its title or artist does not match the query
  (case-insensitive; equal or one contains the other; empty never matches), when both
  durations are known and differ by more than 10 s, or when it has no non-blank line.
- The rest are ordered by, in turn: complete before incomplete, still better than nothing;
  word-timed, then line-timed, then plain; uncensored (a token, its edge punctuation
  trimmed, with `*` and a letter that is not `*emphasis*`; `* * *` separators are not
  masks) — only within the same timing; duration drift, where up to 3 s counts as none;
  provider order last.
- Incomplete: a synced sheet whose last sung line starts more than 5 s after the track ends
  (it belongs to another recording), or before 60% of it while another synced candidate
  reaches past 60%. An early end every synced sheet shares is a long outro, not a cut, so it
  costs nothing — and a plain text, which has no times to compare, never makes it one.
  Completeness is relative, so it is judged over the candidates found so far, which is
  also what the early stop sees.
- Unknown durations skip the duration checks: no evidence is not a mismatch.

### Providers

- BiniLyrics: `GET lyrics-api.binimum.org/?track&artist[&album][&duration]` (it redirects
  to lrc.red) lists Apple Music sheets with `timing_type`, duration and an https
  `lyricsUrl`; the fitting hit with the best timing, then the closest duration, is the only
  sheet fetched. The album filters the server's results (a "Deluxe" or compilation title
  finds nothing), so a search with an album and no fitting hit is repeated without it.
  404 is "none", for the search and for the sheet. Two attempts per request.
- TTML: the root must be `<tt>`; every `<p>` under its `<body>` is a line (`<head>` —
  songwriters, translations — is never read); timed `<span>`s are syllables, whitespace or
  `<br/>` between them splits words; text outside a timed span joins the syllable before
  it (keeping a space it starts with), or the next one when it comes first;
  `ttm:role="x-bg"` spans become the line's backing — several in one line join with a
  space — and untimed backing text a `[bg: …]` line without markers; `x-translation` / `x-roman` spans are skipped; agents are ignored (the
  app has no duet sides). A `<p>` without timed spans keeps its text as one timed or plain
  line. Clock values: `h:m:s.f`, `m:s.f`, `s.f`, `Ns`, `Nms`; one that overflows is no
  time.
- LRCLIB: `GET /api/get` with the album (sent only if the query has a non-empty one), then
  without it (only if it had one), then `GET /api/search`. `duration` goes on the get
  requests when known; a 404 moves to the next step. A get answer is LRCLIB's own exact
  match, so it is named as asked; one without usable lyrics (instrumental, both fields
  blank) ends the provider with no candidates. Synced and plain text of a record are two
  candidates, so a broken or truncated synced sheet still leaves the plain one to rank.
  Five attempts per request.
- lrcmux: `GET api.lrcmux.dev/get?artist&title[&album][&duration]` returns one result with
  `meta.level` (`word` / `line` / `none`); word timings become syllables (a word text's
  trailing space splits words), `instrumental` gives no candidate. One attempt: a 429 there
  lasts up to an hour.
- Requests (`http::Session`): 10 s each to connect, to the response and to the body, within
  the lookup's budget. Retries are 250 ms apart on transport errors and HTTP 429 / 5xx; a
  429 `Retry-After` (seconds) replaces the pause, capped at 10 s. Any other failing status
  is `Err`.

## Decisions

Owner's calls; ask before changing.

- Apple Music lyrics come through BiniLyrics, a third-party mirror that needs no
  subscription; the official API (developer token + a subscriber's `media-user-token`) was
  not taken.
- Ranking follows what sonora, nd-lyrics, lrcmux and lyrimuse agree on: timing first, a
  static provider preference only as the last tie-break.
- Not taken from them: version words in titles ("Live", "Remix") — duration catches other
  recordings; cross-source text consensus — it needs every provider asked, no early stop;
  re-segmenting community word timings by another source's lines; filtering shouted lines.
- Which provider gave the lyrics is logged, not stored or shown.

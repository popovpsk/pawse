# lyrics (web remote)

The remote's lyrics: the LRC parser, the rows and karaoke timing, and the feed that fetches
the current track's text from `/api/lyrics` and asks the app to search the web. The server
sends raw text only, so every format is parsed here. Not here: which source is picked and
when a search is allowed (`crates/pawse/src/doc.md`, "Web remote reader and lyrics
access"), the panel's markup (`../LyricsPanel.svelte`).

## Files

- `parse.ts` — `parseLrc`: plain, LRC, Enhanced LRC words, `[bg: …]` backing lines.
- `rows.ts` — `buildRows` (♪ interludes), `activeRow`, line and word fill timing.
- `feed.svelte.ts` — `LyricsFeed`: fetch, status, the one `find_lyrics` per track.
- `*.test.ts` — `npm test` (`node --test`; Node strips the types, so it needs Node
  22.18+). Excluded from `svelte-check`, which has no Node typings.

## Parity

- `parse.ts` is a port of `crates/lyrics/src/parser.rs` + `words.rs` and `rows.ts` of the
  pure half of `crates/pawse/src/lyrics_fill.rs`; the desktop and the remote must show the
  same lines with the same timing, so a rule changes in both. The tests port the Rust
  cases. Server lyrics reach the browser through `lyrics::to_lrc`, whose output
  `parse.test.ts` also reads.
- Word positions are UTF-16 indices (Rust's are byte ranges); `[offset:]` is not applied,
  as on the desktop.
- Whitespace is Rust's `char::is_whitespace`, not JS `trim`'s (they differ in U+FEFF and
  U+0085), so `parse.ts` trims with its own class. A leading byte order mark is dropped
  by both explicitly; one inside the text stays, as in Rust.

## Panel

- One feed per page, shared by the desktop side panel and the phone sheet; it loads only
  while one of them is shown (`wanted`), after a socket's first state (`connection`), and
  again on a new track, a `lyrics_rev` or a new socket. A new socket also forgets which
  track was asked about: the app may have restarted and dropped the search. Waiting for
  the first state, not the socket's open, keeps the reload off the track from before
  the drop. A reply for an older request is dropped; a reply with the same text keeps the
  parsed object, so the panel does not rebuild its rows or jump back to the active line.
- `find_lyrics` goes once per track while it stays searchable: a failed lookup writes no
  row, so asking on every searchable reply would search again on every bump. A reply
  that is not searchable (found, not found, switch off) clears the ask, so a track whose
  web rows were forgotten is asked about again.
- "Searching…" shows from the `find_lyrics` while the reply says `searching`; every
  `lyrics_rev` reloads, and the app bumps it at the end of every search for the current
  track, found or not. A lost command, or a search the app ended without a bump (a panel
  task dropped on a track change), would leave it there, so after `SEARCH_WAIT_MS` (the
  lookup's 30 s budget plus margin) the feed reloads once more.
- The fill is a gradient clipped to the text on an inline span. `box-decoration-break:
  slice` lays a wrapped line's rows end to end, so a line fill sweeps row after row as on
  the desktop. A word piece runs from the previous word's end to its own end, so the gap
  before a word fills with it; once a later word starts, every piece before it is full
  (overlapping singers never pull the fill back); trailing untimed text fills when all
  words are over.
- Only the active row and the lingering one read the position, so a frame touches at
  most two rows. Fill and lingering use the position plus `ACTIVE_TOLERANCE_MS`, as the
  desktop's `display_ms` does, so a row starts filling the moment it lights.
- Autoscroll centers the active row; wheel or touch stops it, the locate button or a
  click on a line (which seeks) resumes it. The button sits in the host's header row (the
  side panel's Lyrics title, the sheet's tabs), so the panel only reports `canResync`
  (synced, a row active, not following, as the desktop's `show_sync`) and exposes
  `resync()`.

## Decisions

- No automatic return to the active row after a scroll: whoever scrolled to read ahead
  is not pulled back; only the locate button or a click on a line returns.
- On a wide screen lyrics replace the side panel's tabs rather than being a tab. The
  lyrics and panel buttons in the header each switch the panel to their content, and
  close it when that content is already shown.

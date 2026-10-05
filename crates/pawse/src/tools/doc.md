# tools

The Tools screen: one-off utilities that sit next to the player rather than in
Settings. `MainView` shows it as an overlay like Settings (`show_tools`, the
wrench button left of the gear, back button closes it). The button can be
hidden in Settings → Appearance (`SettingsStore::tools_enabled`, default off — also for a `settings.json` written before the key existed).
Each tool is one `SettingPage` on the shared `ui_components::Settings` widget,
so a new tool (e.g. playlist / likes importers from other players) is a new
page in `build_pages`.

## Files

- `mod.rs` — `ToolsView` (the screen entity). Builds the pages once and caches
  them; rebuilds on `LangChanged`, because page and item labels are baked in at
  build time, and when the AI prompt mode changes, because the answer-import
  group exists only for playlist modes, and when the Covers page gains or loses
  its results / skipped items. Owns the long-lived state entities of every tool
  (for the AI prompt: `AiPromptState` plus the `AiPromptInputs` text fields; for
  covers: `CoversState`).
- `ai_prompt/` — the AI prompt generator:
  - `mod.rs` — `AiPromptState` (mode, period, count, detail, busy flag, the
    finished prompt, its preview and size line, the answer-import status), the
    page and its fields, and the `generate` / `copy` / `import_answer` actions.
  - `options.rs` — `Mode` (with `counts()` and `builds_playlist()`), `Period`, `RELEASE_PERIODS`,
    `Detail`, `ALBUM_COUNTS` / `PLAYLIST_COUNTS`, `PromptOptions`,
    `language_name` (UI language code → English name for the prompt).
  - `taste.rs` — `TasteSnapshot` and `gather`: every read the prompt needs, done
    in one go on a background thread through `LibraryRepository`.
  - `builder.rs` — `build_prompt`, a pure function from snapshot + options to
    text. Prompt tests live here.
  - `template.rs` — every sentence of the prompt. Nothing else holds prompt text.
  - `answer.rs` — the way back: `TrackIndex` over the library's `TrackListing`s
    and `parse_answer`, a pure function from the pasted model answer to track
    ids in order + the lines it could not match. Parser tests live here.
- `covers/` — cover search for albums without art (own `doc.md`).
- `timer.rs` — the Timer page: status with +5 min / turn off, presets and
  "end of track", a minutes field with Start for any other length, the
  fade-out row (shared with Settings), and an "Automatic sleep timer" row
  (its hours or "Off") whose Settings button jumps to Settings → General,
  scrolled to the timer group (`OpenSleepTimerSettings`). The timer itself is `crate::sleep_timer`
  (own `doc.md`); the page only reads it. The field and slider states come
  from `sleep_timer::controls`, passed in by `MainView`. It is page `TIMER_PAGE`, which
  `show_page` opens when the title-bar badge is clicked (`page_request` makes
  the `Settings` widget switch tabs even if the screen was already open).

## AI prompt

- **Counts** depend on the mode: New music and New releases offer 5 / 10 / 20
  (a longer album list stops being useful), playlist modes add 50. Switching to a mode that
  lacks the current count drops it to the largest one that fits.
- **The answer-import group** ("paste the answer → playlist") is not on the page
  in New music and New releases — that answer is albums to go find, not tracks
  from the library.
- **New releases** is the one mode that needs the model to browse. It carries
  the same data as New music (taste, "Albums in my library", the full history)
  and a different task: releases of the chosen window by the artists the user
  listens to most, then by close ones. The window is its own selector,
  "Release period" (Week / Month / 6 months, `RELEASE_PERIODS`, state
  `release_window`), shown on the page only in this mode and independent of
  "Current taste": the taste period may be All time while the window is a week.
  The task states the window as dates
  (`{from}`–`{to}`, today included) because a model without a clock cannot
  resolve "the last 6 months"; it tells the model to search instead of
  recalling (its training data ends before the window), to check every release
  and its date against a source but keep links and citations out of the answer,
  never to invent a release, to return fewer than asked rather than pad, and,
  if it has no web access, to say so in its first line and stop — otherwise a
  model without search answers from memory with plausible fake releases. Reissues, remasters
  and deluxe editions of old albums are allowed but go after the genuinely new
  releases and are typed `reissue`: some users want them, but unmarked they
  crowd out the new releases within a window of a few months.
  The mode description in the UI tells the user to pick a chat with web search
  on. The answer is `Artist — Title (type, release date)` plus the explanation,
  most relevant first, reissues last. The task's window dates are ISO because
  they only inform the model, but the answer's date has no fixed format: the
  prompt asks for it "the way it is usually written in the language of your
  answer", so the model picks the local convention itself (no per-locale
  date formatting in code).

Pawse has no recommender of its own. Instead it writes a prompt describing the
library and the listening history; the user pastes it into any LLM chat.
Nothing is sent anywhere: the only output is the clipboard.

- **The prompt is English on purpose**, independent of the UI language: models
  follow English instructions best and the text stays one stable template.
  Only the answer language follows the UI (`ANSWER_LANGUAGE`, filled from
  `ui_resources::i18n::active()` at generation time). The prompt's own texts live
  in `template.rs`, not in the i18n tables; the screen's UI labels live in their
  own table, `ui_resources/src/i18n/tools.rs`.
- **Nothing is trimmed** except the Forgotten candidate list
  (`FORGOTTEN_CANDIDATES`) and the top-N lists. Current models take very large
  contexts, and the exclusion lists are only useful when complete: "Albums in my
  library" (everything the user owns or knows) and "Everything I have played in
  this player" (the whole `plays` history, each track even if played once,
  including server tracks and tracks no longer in the library). The full
  history goes into New music only: From my library is limited to the library
  anyway and gets its taste from "My taste right now". The character count
  under the buttons gives a sense of the size.
- Changing any option clears the finished prompt and bumps
  `AiPromptState::generation`; a build still running for the old options is
  dropped when it finishes instead of showing up under the new ones.
- **Period** only shapes "My taste right now" (top artists / tracks). The
  exclusion lists are always all-time. In Forgotten mode the period is also the
  gap: a track is a candidate if it was never played or last played before the
  cutoff; All time means never played. It does not set the New releases window
  (that is `release_window`). Today's date is in the New releases task as
  `today is {to}`, once before and once after the data.
- **Forgotten ranking**: artist affinity = all-time plays of the artist (by
  name, from `plays`) + `LIKED_AFFINITY` per liked track. Ranking happens here,
  the model only picks and orders.
- **Likes are a taste signal, never a pick.** Likes are often imported from
  elsewhere (Last.fm, servers), so a liked track is one the user knows by heart
  even with no plays here. Forgotten drops liked tracks from the candidates in
  code; New music and From my library can't (the lists must stay complete), so
  the task and the liked-section heading forbid recommending them. Without this
  the model happily echoes the liked list back.
- **The history is partial, and the prompt says so.** `plays` starts when the
  user started using Pawse; everything heard in other players before is
  missing. `DATA_NOTES` tells the model the history is from one player and
  incomplete, and "never played" is phrased as "never played here". The prompt
  never names Pawse. A "finish these partly heard albums" block was tried and
  removed: "partly heard" from this history almost always meant "heard
  elsewhere", so it recommended albums the user already knew. Partly heard
  albums still stay out of New music as an exclusion, which a partial history
  only makes less strict, never wrong.
- **Answer format.** New music: each item starts on its own line with
  `Artist — Album (year)`. Playlist modes (From my library, Forgotten): the
  playlist comes first as a single code block, one `Artist — Title` per line
  and nothing else; per-track reasons go after the block as `Artist — Title:
  reason`. The block is what the answer import reads, and models keep a code
  block clean far more reliably than a "no numbering" rule; reasons stay out
  of it because code blocks don't wrap. **Detail** (low / medium / high) only
  picks how much explanation follows: low is one short sentence per
  recommendation and none per playlist track, medium 2–3 sentences / one per
  track, high a paragraph / 2–3 per track. One `ANSWER_*` constant per mode ×
  detail in `template.rs`.

## Answer → playlist

The second group on the page takes the model's answer pasted back, finds the
tracks in the library and saves them as a playlist in the model's order
(`create_playlist` + `add_tracks_to_playlist`). The name is the typed one,
else the first line of the wishes (cut to `PLAYLIST_NAME_MAX_CHARS`), else a
localized default — playlists can't be renamed yet, so the name matters.

`parse_answer` works by recognition, not grammar: every line is looked up in
the library, and lines that match nothing (intros, prose, headings) are simply
skipped. That makes it tolerant of whatever the model wraps around the list.

- **Scope.** If the answer has a fenced code block (```` ``` ```` or `~~~`;
  a one-line ```` ```…``` ```` counts as its content), only lines inside fences
  are read (the reasons after the block would only repeat the same tracks). No
  block, or a block with no library track in it (an empty block before the
  list) — all lines are read.
- **Line cleanup.** Non-breaking/thin spaces become spaces; leading `#`,
  bullets (`-` `*` `•` `+` dashes), numbering (`1.` `1)`, also without a
  space, but not `2.0`), backticks and `*` at the line edges are dropped.
  Inner `*` stay for the first lookup (titles like `F**k You`); a second
  lookup without any `*` catches markdown bold/italics. Lines without a
  letter or digit are skipped.
- **Splitting.** Every `—` / `–` / ` - ` / `: ` is a possible cut. For each cut
  as the artist/title boundary, every later cut (and the line end) is tried as
  the title end, longest title first. This survives titles with dashes
  (`Glory Box - 2011 Remaster`), trailing reasons and artist names with
  dashes. The reversed order (`Title — Artist`) is tried too. `Title by
  Artist` is deliberately not: in free text it matched prose ("Stand by Me" →
  a track `Stand` by `Me`).
- **Keys.** Exact: `normalize_tag` after folding typographic quotes and
  apostrophes and stripping surrounding double quotes. Loose fallback (only if
  no exact match anywhere in the line): alphanumerics only, bracketed parts of
  the title dropped (`(Remastered)`, `[Live]`). Both the track artist and the
  album artist are indexed, since "My library" lists tracks under the album
  artist; an empty artist is indexed as `UNKNOWN_ARTIST`, the same text the
  prompt shows for it. On a name clash the first listing wins.
- **No fuzzy/edit-distance matching** by design: a wrong track silently in the
  playlist is worse than a visible miss.
- **Misses.** In block mode every unmatched block line is reported. In
  free-text mode only lines that look like a track (a dash separator, ≤
  `MISSING_LINE_MAX_CHARS`, not ending in `:`) are, so prose isn't listed as
  "not found". Duplicates keep their first position; a repeated miss is
  reported once.
- An answer with no library tracks creates no playlist.
- The task is stated twice, before and after the data: with a long context in
  between, the model otherwise loses it.
- No file paths, server names or ids ever go into the prompt — only artist,
  album, title, year, genre, like flags and play counts.
- `recent_plays` and the top lists count every `plays` row, qualified or not:
  an unqualified row still means at least `HISTORY_MIN_SECS` of listening.

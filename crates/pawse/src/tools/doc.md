# tools

The Tools screen: utilities that sit next to the player instead of in Settings. Each tool is one `SettingPage` on the shared `ui_components::Settings` widget, so a new tool is a new page in `ToolsView::build_pages`. Pages: AI prompt, Covers, Timer.

Not here: the overlay switching (`MainView::open_tools`, `pawse/src/doc.md`); the sleep timer (`sleep_timer/doc.md`); the Covers page (`covers/doc.md`); the screen's strings (`ui_resources/src/doc.md`, `i18n/tools.rs`); the listening-history queries (`music_library/src/doc.md`, "Reading listening history").

## Files

- `mod.rs` — `ToolsView`: builds and caches the pages, owns the state of every tool, `show_page`, `show_ai_playlist`, the `OpenAiPlaylist` action.
- `timer.rs` — the Timer page. It only reads `SleepTimer`; field and slider states come from `sleep_timer::controls` (passed in by `MainView`), `duration_field` / `fade_item` from `sleep_timer::settings`. Its automatic-timer row has a Settings button dispatching `OpenSleepTimerSettings`.
- `ai_prompt/mod.rs` — `AiPromptState`, the page, the `generate` / `copy` / `import_answer` actions.
- `ai_prompt/options.rs` — `Mode`, `Period`, `Detail`, counts, `PromptOptions`, `language_name`.
- `ai_prompt/taste.rs` — `TasteSnapshot` and `gather`: every library read the prompt needs, in one background pass.
- `ai_prompt/builder.rs` — `build_prompt`, pure: snapshot + options + now to text. Prompt tests live here.
- `ai_prompt/template.rs` — every sentence of the prompt. Nothing else holds prompt text.
- `ai_prompt/answer.rs` — the way back: `TrackIndex` and `parse_answer`, pure. Parser tests live here.
- `covers/` — the Covers page, own `doc.md`.

## Screen

- The header Tools button opens page 0 and exists only with `tools_enabled` (Settings → Appearance, default off, also for a `settings.json` from before the key). The `OpenAiPlaylist` action (AI button on the Playlists screen) and the footer sleep-timer badge (`TIMER_PAGE`) open the screen with the button hidden: `open_tools` never reads `tools_enabled`.
- The page index is the page's identity: `AI_PROMPT_PAGE` 0, Covers 1, `TIMER_PAGE` 2. Add pages after these or fix the constants. The Timer page exists only if the `SleepTimer` global does. `show_page` bumps `page_request` so the widget switches tabs even when the screen is already open.
- Pages are built once. Labels are baked in at build time, so they are rebuilt on `LangChanged`, on an AI prompt mode change (the answer-import group exists only in playlist modes, the release-window item only in New releases) and when Covers gains or loses its results / skipped items. A new conditional item needs a trigger like these.
- Tool state lives in `ToolsView`, so a running cover search or a built prompt survives closing the screen.
- A scrolling box inside a page (the prompt preview, the Covers skipped list) stops the wheel event while it can still scroll (`contain_scroll`): gpui scroll containers don't stop propagation, so the page would scroll with it. A box that doesn't overflow, or is at its end, lets the page scroll.
- `show_ai_playlist` first switches to From my library when the current mode builds no playlist (New music and New releases have no answer import).

## AI prompt

- Four modes: New music, New releases, From my library, Forgotten. The last two build a playlist and offer one larger count (a longer album list stops being useful).
- Changing any option clears the finished prompt and bumps `AiPromptState::generation`; a build still running for the old options is dropped when it finishes.
- **Period** only shapes "My taste right now"; the exclusion lists are always all-time. In Forgotten it is also the gap: a track is a candidate if never played or last played before the cutoff (All time: never played). The New releases window is a separate selector (`release_window`), independent of the period.
- **English on purpose**, whatever the UI language: models follow English instructions best and the text stays one stable template. Only the answer language follows the UI (`ANSWER_LANGUAGE`). Prompt text lives in `template.rs`, not in the i18n tables.
- **The task is stated twice**, before and after the data: with a long context in between, the model otherwise loses it.
- **Nothing is trimmed** except the top-N lists and `FORGOTTEN_CANDIDATES`: the exclusion lists only work complete. New music and New releases carry "Albums in my library" and "Everything I have played in this player" (the whole `plays` history, even one play, including server tracks and tracks no longer in the library). From my library and Forgotten don't: they are bounded by the library and take their taste from "My taste right now".
- Only artist, album, title, year, genre, like flags and play counts go in; never paths, server names or ids. Play counts count every `plays` row, qualified or not (an unqualified row is still at least `scrobble_bridge::HISTORY_MIN_SECS` of listening).
- **The history is partial, and the prompt says so.** `plays` starts when the user started using Pawse, so "never played" reads "never played here" and `DATA_NOTES` calls the history incomplete. The prompt never names Pawse.
- **Likes are a taste signal, never a pick.** Likes are often imported, so a liked track is one the user knows by heart with no plays here. Forgotten drops liked tracks in code; the other modes can't (their lists must stay complete), so the task and the liked-section heading forbid recommending them. Without it models echo the liked list back.
- **Forgotten ranking** is done in code, the model only picks and orders: artist affinity = all-time plays of the artist (by name) + `LIKED_AFFINITY` per liked track. A track's last play is also looked up in `plays` by artist + title, so a play of another copy or of a server track counts.
- **New releases** is the one mode that needs the model to browse. The window is given as ISO dates, because a model without a clock cannot resolve "the last 6 months". The task tells a model that cannot search to say so in its first line and stop; otherwise it answers from memory with plausible fake releases. Reissues are allowed but go after the genuinely new releases and are typed `reissue`; unmarked they crowd out the new ones within a window of a few months. The answer's date format is left to the model ("the way it is usually written in the language of your answer"): no per-locale date formatting in code. The mode description in the UI tells the user to pick a chat with web search on.
- **Playlist answers come as one code block**, `Artist — Title` per line and nothing else; per-track reasons go after it. The answer import reads the block; models keep a code block clean far more reliably than a "no numbering" rule, and reasons stay out of it because code blocks don't wrap.

## Answer → playlist

The pasted answer becomes a playlist in the model's order. The name is the typed one, else the first line of the wishes (cut to `PLAYLIST_NAME_MAX_CHARS`), else a localized default. An answer with no library tracks creates no playlist.

`parse_answer` works by recognition, not grammar: every line is looked up in the library and lines that match nothing (intros, prose, headings) are skipped, so it tolerates whatever the model wraps around the list.

- **Scope.** If the answer has a fenced block (```` ``` ```` or `~~~`; a one-line ```` ```…``` ```` counts as its content), only lines inside fences are read: the reasons after the block repeat the same tracks. No block, or a block with no library track in it: all lines.
- **Line cleanup.** Heading marks, bullets, numbering (`1.` `1)`, also without a space, but not `2.0`), backticks and edge `*` are dropped. Inner `*` stay for the first lookup (`F**k You`); a second lookup without any `*` catches markdown bold and italics.
- **Splitting.** Every `—` / `–` / ` - ` / `: ` is a possible cut (a bare `-` is not, so `Jay-Z` survives). For each cut as the artist/title boundary, every later cut and the line end is tried as the title end, longest first: this survives titles with dashes (`Glory Box - 2011 Remaster`), trailing reasons and dashed artist names. `Title — Artist` is tried too.
- **Keys.** Exact: `normalize_tag` after folding typographic quotes and stripping surrounding double quotes. Loose fallback, only if nothing in the line matched exactly: alphanumerics only, bracketed parts of the title dropped. Both the track artist and the album artist are indexed, since "My library" lists tracks under the album artist; an empty artist is indexed as `UNKNOWN_ARTIST`, the text the prompt shows. On a name clash the first listing wins.
- **Misses.** In block mode every unmatched block line is reported. In free text only lines that look like a track (a dash separator, at most `MISSING_LINE_MAX_CHARS`, not ending in `:`) are, so prose isn't listed as "not found". Duplicates keep their first position; a repeated miss is reported once.

## Decisions

- A prompt generator, not an in-app recommender: nothing is sent anywhere, the only output is the clipboard.
- No fuzzy / edit-distance matching in the answer import: a wrong track silently in the playlist is worse than a visible miss.
- No `Title by Artist` form there: in free text it matched prose ("Stand by Me" became a track `Stand` by `Me`).
- No "finish these partly heard albums" block: "partly heard" in this history almost always meant "heard elsewhere", so it recommended albums the user already knew. Partly heard albums stay out of New music as an exclusion, which a partial history only makes less strict, never wrong.

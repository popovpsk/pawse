pub const INTRO: &str = "Below is an export of my music library and the listening history from my music player. Use it to understand what I own, what I know and what I like.";

pub const DATA_NOTES: &str = "How to read the data:
- `×N` after a track is how many times I played it; no mark means once.
- `[Album]` groups tracks by album; `{genre}` is the genre from the file tags.
- Names are copied from my file tags as-is, so spelling and language may vary.
- The listening history comes from this one player only and is incomplete: I have listened to music elsewhere before and still do. A track or album with no plays here may well be one I know.";

pub const TASK_NEW_MUSIC: &str = "Task: recommend {count} artists or albums that are new to me.
- Do NOT recommend anything from the section \"Albums in my library\" — I own those or already know them.
- Do NOT recommend anything from the section \"Everything I have played in this player\" — I have heard it already, even if only once.
- If I have heard only part of an album, it is not new and must not be recommended.
- Do NOT recommend tracks from \"Tracks I marked as liked\" — I know them by heart; they only show my taste.
- Base the choice on \"My taste right now\" first and on the whole library second. Prefer lesser-known picks over the obvious hits of the genre.";

pub const TASK_NEW_RELEASES: &str = "Task: find up to {count} music releases that came out between {from} and {to} ({window}; today is {to}) and that I would likely enjoy.
- This needs current information. Search the web and rely on what you find, not on memory: your training data does not cover this period. If you cannot search the web, say so in your first line and stop — do not answer from memory.
- Start with new albums and EPs by the artists I listen to most (\"Everything I have played in this player\" lists them most played first; \"My taste right now\" shows my recent focus). Then add releases by artists close to my taste that I do not have yet.
- Do NOT recommend anything from the section \"Albums in my library\" or \"Everything I have played in this player\" — I already have it or have heard it.
- Genuinely new releases come first. Reissues, remasters and anniversary or deluxe editions of old albums may follow after them, only if they are worth my time (new songs, a notable remaster); mark each one with the type `reissue`. Leave out compilations of old material.
- Check every release against at least one source — the artist's or label's site, Bandcamp, a streaming page, MusicBrainz or Discogs, Wikipedia, or a music publication — and make sure its release date is inside the period. Never invent a release; if you cannot confirm something, leave it out. Use the sources only to check the facts: do not put links or citations in the answer. If you find fewer than {count}, list fewer and say so.";

pub const TASK_FROM_LIBRARY: &str = "Task: build a playlist of {count} tracks using ONLY tracks from the section \"My library\". Do not invent tracks and do not use anything outside that section. Do NOT include tracks from \"Tracks I marked as liked\" — I know them by heart and want something else; use them, like the rest of \"My taste right now\", only to understand what I like. Take my wishes into account if there are any. Order the tracks so the playlist flows well.";

pub const TASK_FORGOTTEN: &str = "Task: pick {count} tracks from the section \"Forgotten candidates\" — music I own but have not played for a while ({gap}). Choose the ones I am most likely to enjoy right now given \"My taste right now\", and order them so the playlist flows well. Use only tracks from that section.";

pub const GAP_NEVER: &str = "never played here";

pub const ANSWER_NEW_MUSIC_LOW: &str = "Answer format: one recommendation per line as `Artist — Album (year) — why it fits`, where \"why\" is one short sentence that refers to artists I actually listen to. No numbering, no other headings.";

pub const ANSWER_NEW_MUSIC_MEDIUM: &str = "Answer format: each recommendation starts on its own line as `Artist — Album (year)`, followed by 2–3 sentences: what it sounds like and which artists or tracks from my history it connects to, by name. No numbering, no other headings.";

pub const ANSWER_NEW_MUSIC_HIGH: &str = "Answer format: each recommendation starts on its own line as `Artist — Album (year)`, followed by a short paragraph of 4–6 sentences: the sound and mood, its place in the artist's work and scene, concrete links to artists, albums or tracks from my history, and which track to start with. No numbering, no other headings.";

pub const ANSWER_NEW_RELEASES_LOW: &str = "Answer format: one release per line as `Artist — Title (type, release date) — why it fits`, where type is album, EP or reissue, the release date is written the way it is usually written in the language of your answer, and \"why\" is one short sentence that refers to artists I actually listen to. Most relevant first, reissues last. No numbering, no other headings.";

pub const ANSWER_NEW_RELEASES_MEDIUM: &str = "Answer format: each release starts on its own line as `Artist — Title (type, release date)`, where type is album, EP or reissue and the release date is written the way it is usually written in the language of your answer, followed by 2–3 sentences: what it sounds like and which artists or tracks from my history it connects to, by name. Most relevant first, reissues last. No numbering, no other headings.";

pub const ANSWER_NEW_RELEASES_HIGH: &str = "Answer format: each release starts on its own line as `Artist — Title (type, release date)`, where type is album, EP or reissue and the release date is written the way it is usually written in the language of your answer, followed by a short paragraph of 4–6 sentences: the sound and mood, how it differs from the artist's earlier work, concrete links to artists, albums or tracks from my history, and which track to start with. Most relevant first, reissues last. No numbering, no other headings.";

pub const ANSWER_PLAYLIST_LOW: &str = "Answer format: first the playlist in a single code block (```), one track per line exactly as `Artist — Title`, spelled exactly as in the list above. Nothing else inside the block: no numbering, no comments, no blank lines. After the block, at most three sentences about the idea of the playlist.";

pub const ANSWER_PLAYLIST_MEDIUM: &str = "Answer format: first the playlist in a single code block (```), one track per line exactly as `Artist — Title`, spelled exactly as in the list above. Nothing else inside the block: no numbering, no comments, no blank lines. After the block, one line per track as `Artist — Title: reason`, the reason one short sentence on why the track is here, then two or three sentences about the idea of the playlist.";

pub const ANSWER_PLAYLIST_HIGH: &str = "Answer format: first the playlist in a single code block (```), one track per line exactly as `Artist — Title`, spelled exactly as in the list above. Nothing else inside the block: no numbering, no comments, no blank lines. After the block, one paragraph per track starting with `Artist — Title:` — 2–3 sentences on why it fits my taste and how it connects to the tracks around it — then a paragraph about the arc of the playlist: how it starts, builds and ends.";

pub const ANSWER_LANGUAGE: &str = "Write every explanation in {language}. Keep artist, album and track names in their original spelling — do not translate them.";

pub const WISHES: &str = "My wishes for this request (follow them):";

pub const SECTION_TASTE: &str = "My taste right now";
pub const SECTION_LIBRARY_ALBUMS: &str =
    "Albums in my library (I own or already know them — do not recommend)";
pub const SECTION_LIBRARY_TRACKS: &str = "My library";
pub const SECTION_HISTORY: &str = "Everything I have played in this player (all time, even once)";
pub const SECTION_FORGOTTEN: &str = "Forgotten candidates";

pub const TOP_ARTISTS: &str = "Most played artists ({period}):";
pub const TOP_TRACKS: &str = "Most played tracks ({period}):";
pub const RECENT: &str = "Recently played, newest first:";
pub const LIKED: &str =
    "Tracks I marked as liked (I know them well — they show my taste, never recommend them):";
pub const GENRES_BY_PLAYS: &str = "Genres weighted by my plays:";
pub const GENRES_BY_LIBRARY: &str = "Genres by share of my library:";
pub const NO_PLAYS_IN_PERIOD: &str = "No plays in this period.";
pub const NO_HISTORY: &str = "No listening history yet — judge my taste by the library and likes.";

pub const UNKNOWN_ARTIST: &str = "Unknown artist";
pub const UNKNOWN_ALBUM: &str = "No album";
pub const NEVER_PLAYED: &str = "never played here";
pub const MONTHS_AGO: &str = "last played {n} months ago";

# artist_info

Identifies a library artist on MusicBrainz and returns structured facts about
them plus a photo: kind, life-span years, origin (begin area + area), top
genres, band members / bands they belong to, and their Deezer picture. Blocking
`ureq` on the caller's thread (run it off the UI thread), no GPUI, knows
nothing about the library. HTTP plumbing (`agent()` with the MusicBrainz-
compliant User-Agent, `get_text`, size-capped `get_bytes`, `Error`) and title
`normalize` are reused from `cover_search`. Used by `pawse::artist_card` (the
artist indexer behind the artist page's info block). Wikipedia was tried first
and dropped at the user's request: everything shown now comes from MusicBrainz
data, the photo from Deezer.

## Files

- `lib.rs` — re-exports.
- `lookup.rs` — `Lookup::find(query)`: the whole chain and the match rule
  (`names_match`, `LocalTitles`). Returns `Found` (`ArtistFacts`, photo bytes,
  `photo_pending`). `Lookup` is `Clone` and holds nothing but the HTTP agent —
  the rate limit lives in `cover_search` — so callers clone it per task.
- `musicbrainz.rs` — artist search (`artist:"…" OR alias:"…"`), release-group
  browse (titles plus the total, 100 per page), artist lookup with
  `inc=url-rels+genres+artist-rels` → `Details { facts: ArtistFacts, deezer }`.
  `ArtistFacts` is serde (`#[serde(default)]` on every field) because the app
  stores it as JSON and never refetches; a field added later reads as its
  default from old rows. Parsers are pure and tested.
- `deezer.rs` — `api.deezer.com/artist/{id}` → `picture_big` (500 px).

## The chain

1. MusicBrainz artist search by the library name, up to 10 results. Candidates
   are the ones whose name or an alias equals the library name after
   `cover_search::matching::normalize` (so "KoЯn" finds Korn through its alias),
   never MusicBrainz's "Various Artists" entity, at most 4.
2. Each candidate is **confirmed** against the library: its release-group titles
   must contain one of the artist's album titles, or at least two "weak" hits
   (`WEAK_HITS_NEEDED`): track titles, or a self-titled album — an album named
   like the artist is what a namesake is most likely to share too, so on its own
   it is not proof, and neither is one shared "Intro". Release groups
   are read page by page until confirmed, up to 3 pages (300 groups) — the
   browse order is MusicBrainz's, so a prolific artist's albums need not be on
   the first page. The first confirmed candidate wins; none confirmed =
   `Ok(None)`. There is no fallback to "the only candidate": with no MBID tags
   in the files this check is the only thing between a namesake and the page.
   Measured on a real 108-artist library: 44 confirmed — every album artist but
   Various Artists and two fictional bands. The rest are combined credits
   ("Gorillaz & Lou Reed", "Two Feathers/…") and soundtrack performers with a
   single track in the library (Deep Purple, Neil Diamond), which one track
   title cannot confirm.
3. Artist lookup: kind (`Person` / `Group` incl. orchestra and choir / `Other`),
   begin/end year, area and begin area (English names, as MusicBrainz stores
   them), genres by vote count (top 5 kept), "member of band" relations split by
   direction — `members` (backward: who is in this group) and `member_of`
   (forward: groups this artist is in), each name once, `current` if any of its
   periods is open — and the Deezer id from the URL relations, kept in the facts
   so a caller can fetch the photo again later without MusicBrainz.
4. Deezer picture → bytes (`Lookup::photo(deezer_id)`, also public for that
   retry). Deezer serves an empty-hash placeholder (`/images/artist//`) for
   artists without a photo; that counts as no photo (`Ok(None)`).

Step 4 is optional: its failure is logged, leaves the photo empty and sets
`Found::photo_pending`. Only a MusicBrainz failure is an `Err`.

## Non-obvious behavior

- **Rate limit.** MusicBrainz allows one request per second per client; every
  request takes a turn from `cover_search::musicbrainz_turn()`, a 1.1 s gap shared
  by the whole process, so the Covers tool and the artist index together still
  stay under the limit. MusicBrainz
  still answers 503 under load even at that pace (seen on 3 of ~150 requests),
  so a 503 is retried twice after 2 s and 4 s. A found artist costs 3–4
  MusicBrainz requests; the 108-artist library above took ~4.7 min.
- The Deezer id comes from MusicBrainz's own link, not from a Deezer search, so
  the photo never needs a second matching step.

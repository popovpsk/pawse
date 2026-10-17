# artist_info

Identifies a library artist on MusicBrainz and returns `ArtistFacts` plus a photo from
Deezer. Blocking `ureq` on the caller's thread (run it off the UI thread), GPUI-free, knows
nothing about the library: the caller passes the artist name and the album and track titles
it holds. HTTP helpers, `matching::normalize` and the MusicBrainz rate limit come from
`cover_search`.

Not here: the `artist_info` table, when to look up or retry, the card and photo stores
(`pawse/src/doc.md`, "Artist cards and photos"); the rate limit (`cover_search/src/doc.md`).

## Files

- `lib.rs` — re-exports; `Candidate` and `TitlePage`, the two types both services return.
- `lookup.rs` — `Lookup` (`find`, `photo`; `Clone`, holds only the HTTP agent), `Query`,
  `Found`, the match rule, the 503 retry.
- `musicbrainz.rs` — artist search, release-group browse, artist lookup. Parsers are pure and
  tested.
- `deezer.rs` — artist page (fans, `picture_big`), artist search, album list, `best_picture`.

## The chain (`Lookup::find`)

No album and no track titles = `Ok(None)` without a request.

1. MusicBrainz artist search by the library name (name or alias). Candidates: name or an
   alias equals the library name after `normalize` ("KoЯn" finds Korn by alias); never the
   "Various Artists" entity; at most 4.
2. Each candidate is confirmed (below); the first confirmed wins, none = `Ok(None)`. No
   fallback to a lone unconfirmed candidate: files carry no MBID, so this check is all that
   separates an artist from a namesake.
3. Artist lookup → `ArtistFacts`. Group covers orchestra and choir; genres are the top 5 by
   vote count; "member of band" relations split by direction — `members` (backward: who is in
   this group), `member_of` (forward: the groups this artist is in) — each name once,
   `current` if any period is open; every Deezer artist id from the URL relations goes into
   `deezer_ids`, so the photo can be fetched again without MusicBrainz.
4. `Lookup::photo(query, facts)` (public for the retry). Only when `deezer_ids` is empty:
   Deezer artist search by the library name, the same confirmation against the artist's
   Deezer album list. Search results carry no aliases, so a library name that is only an
   alias finds no page. The first confirmed page becomes `deezer_ids`; none = no photo.
5. Every page in `deezer_ids` is read; the picture of the one with the most fans among those
   that have one is downloaded. MusicBrainz often links several pages (the main one plus
   duplicates and junk uploads) in no useful order, so the first link is not taken. Deezer's
   empty-hash placeholder (`/images/artist//`) counts as no picture. One failing page fails
   the step.

MusicBrainz's own Deezer links need no matching, so they come first. Deezer has namesakes
too, hence the confirmation on its search. Its album list follows the regional catalogue: an
artist unavailable in the user's country cannot be confirmed and stays without a photo.

## Confirmation

- Library titles and remote ones (release groups / Deezer albums) are compared after
  `normalize`; blank library titles are ignored.
- Confirmed by one library album title that is not the artist's own name, or by two distinct
  weak hits: library track titles, or an album named like the artist. A self-titled album
  alone is no proof (a namesake is most likely to share it), nor is one shared "Intro".
- Titles are read page by page until confirmed, at most 3 pages: the browse order is the
  service's, so a prolific artist's albums need not be on the first page.
- Known misses: combined credits ("A & B"), artists the library holds by a single track.

## Failures and stored facts

- Only a MusicBrainz failure is an `Err` from `find`. A photo failure is logged, leaves the
  photo empty and sets `Found::photo_pending`.
- `photo` adds the Deezer pages it finds to the `facts` it is given even if the download then
  fails, so stored facts let the retry only read the page.
- Deezer answers errors with HTTP 200 and `{"error":…}`. Code 800 ("no data") reads as no
  picture / an empty list; any other (the quota, 4) is `Err(Parse)`, so it is retried and
  never stored as "no photo".
- The app stores `ArtistFacts` as JSON and never refetches it. Every field is
  `#[serde(default)]`, so a field added later reads as its default from old rows; existing
  field and `Kind` names must stay.

## Rate limit

- Every MusicBrainz request takes a turn from `cover_search::musicbrainz_turn()` (1.1 s gap,
  shared by the whole process, so the Covers tool and this crate together stay under the
  limit). Deezer is not throttled.
- MusicBrainz still answers 503 under load at that pace: retried twice, after 2 s and 4 s.

## Decisions

Owner's calls; ask before changing.

- MusicBrainz facts plus the Deezer photo only; Wikipedia was tried and dropped at the
  owner's request.

# cover_search

Finds album cover art on the web by artist + album title. Blocking `ureq` on the
caller's thread (run it off the UI thread), no GPUI, knows nothing about the
library or files. Used by `pawse::tools::covers` (the Covers tool) and by
`discord` (Rich Presence art URL).

## Files

- `lib.rs` — re-exports and `image_extension` (JPEG / PNG sniff by magic bytes;
  anything else is not a cover we save).
- `http.rs` — `agent()` (timeouts, `http_status_as_error(false)`, the
  `Pawse/<version> ( repo url )` User-Agent MusicBrainz requires), `Error`
  (`Transport` / `Status` / `Parse`), `get_text` and size-capped `get_bytes`.
- `candidate.rs` — `Candidate` (source, the artist/album the service reports,
  and where its art lives) and `art_url(size)`: iTunes URLs are resized by
  rewriting the `100x100bb` token, Cover Art Archive URLs round up to the
  nearest size CAA serves (250 / 500 / 1200).
- `itunes.rs` — iTunes Search (`entity=album`). `parse` is pure; results
  without artwork are dropped.
- `musicbrainz.rs` — release-group search (Lucene phrase query, quotes and
  backslashes escaped); art comes from the Cover Art Archive by release-group
  MBID, so one search finds the art of any release in the group.
- `matching.rs` — `normalize` and `is_exact`. Pure, rstest-covered.
- `finder.rs` — `Finder`: the per-album search order and the rate limits.

## Search order

1. iTunes, up to 10 results. The first result whose artist **and** album match
   after `normalize` and whose thumbnail downloads is an exact hit.
2. Otherwise MusicBrainz, same rule (a release group without CAA art answers
   404 on the thumbnail and is passed over).
3. Otherwise the **top** result of iTunes, then of MusicBrainz, as an uncertain
   hit (`Found::exact == false`). Only the top one: further down the list the
   results are unrelated albums, not near-misses.
4. Nothing: `Ok(None)` — but if any request along the way failed (a search, or
   a thumbnail that timed out or answered 5xx), `Err` with that failure, so a
   network problem is not reported as "nothing found". A failed thumbnail never
   stops the search: the next candidate / source is still tried.

A thumbnail (100 px) comes with every hit so the tool can show a preview
without holding full-size images; `download` fetches 1200 px when the user
applies.

## Non-obvious behavior

- **Rate limits live in `Finder`.** iTunes: one search per 3 s (its unofficial
  limit is ~20/min and it answers 403 for a while once exceeded). MusicBrainz:
  one per 1.1 s (its hard limit is 1/s per client). The wait is a
  `thread::sleep` before the request, so a `Finder` must be used from one
  background task at a time. Image downloads (mzstatic, archive.org) are not
  throttled.
- **Normalization** lowercases, drops iTunes' ` - Single` / ` - EP` suffixes, a
  leading `the`, and every non-alphanumeric character; `&` counts as `and`.
  Bracketed parts are dropped **only** when they name an edition
  (`EDITION_WORDS`: deluxe, remaster, anniversary, …): `(Vol. 1)` vs `(Vol. 2)`,
  `(Disc 1)`, `(Black Album)` or `(Taylor's Version)` are different albums with
  different covers and must not come out as exact. No fuzzy / edit-distance
  matching: anything else is an uncertain hit for the user to judge. A title
  that normalizes to nothing (`!!!`) keeps its lowercased text as key.
- **Joint credits.** The library searches with the album's first album artist,
  while services credit collaborations as one string (`JAY-Z & Kanye West`).
  `is_exact` splits the candidate's credit on `ARTIST_SEPARATORS` (`&`, `,`,
  `feat.`, ` x `, …) and accepts the whole credit or any one part; the album
  title must still match exactly.
- **CAA redirects to archive.org.** Where archive.org is unreachable, MusicBrainz
  hits fail with a connect timeout and surface as a search error, never as a
  wrong fallback.

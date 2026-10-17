# cover_search

Finds album cover art on the web by artist + album title: iTunes Search, then MusicBrainz + the Cover Art Archive (CAA). Blocking `ureq` on the caller's thread (run it off the UI thread), no GPUI, knows nothing about the library or files. Used by the Covers tool (`Finder`), `discord` (`itunes::search` only) and `artist_info` (HTTP helpers, `musicbrainz_turn`, `matching::normalize`).

Not here: which albums are searched and where the image is saved (`crates/pawse/src/tools/covers/doc.md`).

## Files

- `lib.rs` — re-exports; `image_extension` (JPEG / PNG by magic bytes; anything else is not a cover we save).
- `http.rs` — `agent()` (carries the `Pawse/<version> ( repo url )` User-Agent MusicBrainz requires), `Error`, `get_text`, size-capped `get_bytes`, `musicbrainz_turn`. Public so `artist_info` shares them instead of keeping a copy.
- `candidate.rs` — `Candidate` and `art_url(size)`: iTunes URLs are resized by rewriting the `100x100bb` token, CAA URLs round up to a size CAA serves (250 / 500 / 1200).
- `itunes.rs`, `musicbrainz.rs` — the two searches; `parse` is pure. MusicBrainz searches release groups and takes art from the CAA by release-group MBID, so one search finds the art of any release in the group.
- `matching.rs` — `normalize`, `is_exact`. `finder.rs` — `Finder`: search order and the iTunes rate limit.

## Search order

1. iTunes (10 results). Exact hit: artist **and** album match after `normalize` and the thumbnail downloads; at most 3 matching results are tried.
2. MusicBrainz, same rule (a release group without CAA art answers 404 and is passed over).
3. The **top** result of iTunes, then of MusicBrainz, as an uncertain hit (`Found::exact == false`).
4. Nothing: `Ok(None)`. But if any request failed on the way (a search, an unparsable body, a thumbnail that timed out or answered 5xx), `Err` with that failure, so a network problem is not reported as "nothing found". A failed thumbnail never stops the search; a 404 or a non-image body is a miss, not a failure.

Every hit carries a thumbnail for the preview; `download` fetches the 1200 px image on apply.

## Non-obvious behavior

- **Rate limits.** iTunes: one search per 3 s, clocked inside `Finder`, so a `Finder` belongs to one background task at a time (it answers 403 for a while once its ~20/min is exceeded). MusicBrainz: one request per 1.1 s for the whole process (its limit is 1/s per client) through `musicbrainz_turn()`, which every MusicBrainz caller (`artist_info` too) takes a turn from. Image downloads are not throttled.
- **Normalization** ignores case, punctuation, a leading `the` and iTunes' ` - Single` / ` - EP`; bracketed parts are dropped **only** when they name an edition (`EDITION_WORDS`). `(Vol. 1)` vs `(Vol. 2)`, `(Disc 1)`, `(Black Album)` or `(Taylor's Version)` are different albums with different covers and must not come out as exact. A title that normalizes to nothing (`!!!`) keeps its lowercased text as key.
- **Joint credits.** The library searches with the album's first album artist, services credit collaborations as one string (`JAY-Z & Kanye West`). `is_exact` splits the candidate's credit on `ARTIST_SEPARATORS` and accepts the whole credit or any one part; the album title must still match exactly.
- **CAA redirects to archive.org.** Where it is unreachable, MusicBrainz hits fail with a connect timeout and count as failures (step 4), not as misses.

## Decisions

- No fuzzy / edit-distance matching: anything not exact is an uncertain hit for the user to judge.
- Only the top result of each service is offered as uncertain: further down the list are unrelated albums, not near-misses.

use std::cmp::Reverse;
use std::collections::HashMap;
use std::fmt::Write as _;

use music_library::TrackListing;

use super::options::{Detail, Mode, PromptOptions};
use super::taste::TasteSnapshot;
use super::template as t;

const TOP_ARTISTS: usize = 40;
const TOP_TRACKS: usize = 40;
const TOP_GENRES: usize = 15;
const FORGOTTEN_CANDIDATES: usize = 1500;
const LIKED_AFFINITY: u64 = 3;
const MONTH_SECS: u64 = 30 * 86_400;
pub fn build_prompt(snapshot: &TasteSnapshot, options: &PromptOptions, now: u64) -> String {
    let task = task(options, now);
    let mut out = String::new();
    out.push_str(t::INTRO);
    out.push_str("\n\n");
    out.push_str(&task);
    out.push_str("\n\n");
    out.push_str(t::DATA_NOTES);
    out.push('\n');
    let wishes = options.wishes.trim();
    if !wishes.is_empty() {
        out.push('\n');
        out.push_str(t::WISHES);
        out.push('\n');
        out.push_str(wishes);
        out.push('\n');
    }

    heading(&mut out, t::SECTION_TASTE);
    taste(&mut out, snapshot, options);

    match options.mode {
        Mode::NewMusic | Mode::NewReleases => {
            heading(&mut out, t::SECTION_LIBRARY_ALBUMS);
            library_albums(&mut out, snapshot);
            heading(&mut out, t::SECTION_HISTORY);
            history(&mut out, snapshot);
        }
        Mode::FromLibrary => {
            heading(&mut out, t::SECTION_LIBRARY_TRACKS);
            library_tracks(&mut out, snapshot);
        }
        Mode::Forgotten => {
            heading(&mut out, t::SECTION_FORGOTTEN);
            forgotten(&mut out, snapshot, options, now);
        }
    }

    out.push('\n');
    out.push_str(&task);
    out.push_str("\n\n");
    out.push_str(match (options.mode, options.detail) {
        (Mode::NewMusic, Detail::Low) => t::ANSWER_NEW_MUSIC_LOW,
        (Mode::NewMusic, Detail::Medium) => t::ANSWER_NEW_MUSIC_MEDIUM,
        (Mode::NewMusic, Detail::High) => t::ANSWER_NEW_MUSIC_HIGH,
        (Mode::NewReleases, Detail::Low) => t::ANSWER_NEW_RELEASES_LOW,
        (Mode::NewReleases, Detail::Medium) => t::ANSWER_NEW_RELEASES_MEDIUM,
        (Mode::NewReleases, Detail::High) => t::ANSWER_NEW_RELEASES_HIGH,
        (Mode::FromLibrary | Mode::Forgotten, Detail::Low) => t::ANSWER_PLAYLIST_LOW,
        (Mode::FromLibrary | Mode::Forgotten, Detail::Medium) => t::ANSWER_PLAYLIST_MEDIUM,
        (Mode::FromLibrary | Mode::Forgotten, Detail::High) => t::ANSWER_PLAYLIST_HIGH,
    });
    out.push('\n');
    out.push_str(&t::ANSWER_LANGUAGE.replace("{language}", options.answer_language));
    out.push('\n');
    out
}

fn task(options: &PromptOptions, now: u64) -> String {
    let template = match options.mode {
        Mode::NewMusic => t::TASK_NEW_MUSIC,
        Mode::NewReleases => t::TASK_NEW_RELEASES,
        Mode::FromLibrary => t::TASK_FROM_LIBRARY,
        Mode::Forgotten => t::TASK_FORGOTTEN,
    };
    let gap = match options.period.days() {
        Some(_) => format!("not played in {}", options.period.describe()),
        None => t::GAP_NEVER.to_string(),
    };
    template
        .replace("{count}", &options.count.to_string())
        .replace("{gap}", &gap)
        .replace(
            "{from}",
            &date(options.release_window.cutoff(now).unwrap_or(now)),
        )
        .replace("{to}", &date(now))
        .replace("{window}", options.release_window.describe())
}

fn date(secs: u64) -> String {
    i64::try_from(secs)
        .ok()
        .and_then(|secs| chrono::DateTime::from_timestamp(secs, 0))
        .map_or_else(String::new, |at| at.format("%Y-%m-%d").to_string())
}

fn heading(out: &mut String, title: &str) {
    let _ = write!(out, "\n## {title}\n");
}

fn artist_name(name: &str) -> &str {
    if name.trim().is_empty() {
        t::UNKNOWN_ARTIST
    } else {
        name
    }
}

fn plays_mark(plays: u32) -> String {
    if plays > 1 {
        format!(" ×{plays}")
    } else {
        String::new()
    }
}

fn taste(out: &mut String, snapshot: &TasteSnapshot, options: &PromptOptions) {
    if !snapshot.has_history() {
        out.push_str(t::NO_HISTORY);
        out.push('\n');
    } else {
        let period = options.period.describe();
        out.push_str(&t::TOP_ARTISTS.replace("{period}", period));
        out.push('\n');
        if snapshot.period_tallies.is_empty() {
            out.push_str(t::NO_PLAYS_IN_PERIOD);
            out.push('\n');
        } else {
            let mut artists: Vec<(String, u32)> = Vec::new();
            let mut index: HashMap<String, usize> = HashMap::new();
            for tally in &snapshot.period_tallies {
                let key = tally.artist.to_lowercase();
                let ix = *index.entry(key).or_insert_with(|| {
                    artists.push((artist_name(&tally.artist).to_string(), 0));
                    artists.len() - 1
                });
                artists[ix].1 += tally.plays;
            }
            artists.sort_by_key(|(_, plays)| Reverse(*plays));
            let line: Vec<String> = artists
                .iter()
                .take(TOP_ARTISTS)
                .map(|(name, plays)| format!("{name}{}", plays_mark(*plays)))
                .collect();
            out.push_str(&line.join("; "));
            out.push('\n');

            out.push_str(&t::TOP_TRACKS.replace("{period}", period));
            out.push('\n');
            let mut tracks: Vec<_> = snapshot.period_tallies.iter().collect();
            tracks.sort_by_key(|tally| Reverse(tally.plays));
            for tally in tracks.into_iter().take(TOP_TRACKS) {
                let _ = writeln!(
                    out,
                    "{} — {}{}",
                    artist_name(&tally.artist),
                    tally.title,
                    plays_mark(tally.plays)
                );
            }
        }

        if !snapshot.recent.is_empty() {
            out.push_str(t::RECENT);
            out.push('\n');
            for play in &snapshot.recent {
                let _ = writeln!(out, "{} — {}", artist_name(&play.artist), play.title);
            }
        }
    }

    let liked: Vec<String> = snapshot
        .listings
        .iter()
        .filter(|l| l.liked)
        .map(|l| format!("{} — {}", artist_name(&l.artist), l.title))
        .collect();
    if !liked.is_empty() {
        out.push_str(t::LIKED);
        out.push('\n');
        out.push_str(&liked.join("; "));
        out.push('\n');
    }

    genres(out, snapshot);
}

fn genres(out: &mut String, snapshot: &TasteSnapshot) {
    let album_genres = snapshot.album_genres();
    let tally = |weight: &dyn Fn(&TrackListing) -> u64| {
        let mut totals: HashMap<&str, u64> = HashMap::new();
        for listing in &snapshot.listings {
            let w = weight(listing);
            if w == 0 {
                continue;
            }
            let Some(genres) = listing.album_id.and_then(|id| album_genres.get(&id)) else {
                continue;
            };
            if let Some(genre) = genres.first() {
                *totals.entry(genre.as_str()).or_default() += w;
            }
        }
        totals
    };
    let by_plays = tally(&|l| u64::from(l.plays));
    let (title, totals) = if by_plays.is_empty() {
        (t::GENRES_BY_LIBRARY, tally(&|_| 1))
    } else {
        (t::GENRES_BY_PLAYS, by_plays)
    };
    let sum: u64 = totals.values().sum();
    if sum == 0 {
        return;
    }
    let mut sorted: Vec<(&str, u64)> = totals.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let line: Vec<String> = sorted
        .iter()
        .take(TOP_GENRES)
        .map(|(genre, n)| format!("{genre} {}%", (n * 100 / sum).max(1)))
        .collect();
    out.push_str(title);
    out.push('\n');
    out.push_str(&line.join(", "));
    out.push('\n');
}

fn library_albums(out: &mut String, snapshot: &TasteSnapshot) {
    let mut artists: Vec<(&str, Vec<String>)> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for album in &snapshot.albums {
        let name = artist_name(&album.artist);
        let ix = *index.entry(name.to_lowercase()).or_insert_with(|| {
            artists.push((name, Vec::new()));
            artists.len() - 1
        });
        let mut entry = album.title.clone();
        if let Some(year) = album.year {
            let _ = write!(entry, " ({year})");
        }
        if let Some(genre) = album.genres.first() {
            let _ = write!(entry, " {{{genre}}}");
        }
        artists[ix].1.push(entry);
    }
    for (artist, albums) in artists {
        let _ = writeln!(out, "{artist}: {}", albums.join("; "));
    }
}

fn library_tracks(out: &mut String, snapshot: &TasteSnapshot) {
    let album_genres = snapshot.album_genres();
    let group_artist =
        |l: &TrackListing| artist_name(l.album_artist.as_deref().unwrap_or(&l.artist)).to_string();
    let mut listings: Vec<&TrackListing> = snapshot.listings.iter().collect();
    listings.sort_by_cached_key(|l| {
        (
            group_artist(l).to_lowercase(),
            l.album.as_deref().unwrap_or_default().to_lowercase(),
            l.album_id,
            l.track_id,
        )
    });
    let mut current: Option<(String, Option<i64>)> = None;
    let mut line = String::new();
    for listing in listings {
        let artist = group_artist(listing);
        let key = (artist.to_lowercase(), listing.album_id);
        if current.as_ref() != Some(&key) {
            if !line.is_empty() {
                out.push_str(&line);
                out.push('\n');
                line.clear();
            }
            let album = listing.album.as_deref().unwrap_or(t::UNKNOWN_ALBUM);
            let _ = write!(line, "{artist} — [{album}");
            if let Some(year) = listing.year {
                let _ = write!(line, " ({year})");
            }
            if let Some(genre) = listing
                .album_id
                .and_then(|id| album_genres.get(&id))
                .and_then(|genres| genres.first())
            {
                let _ = write!(line, " {{{genre}}}");
            }
            line.push_str("]: ");
            current = Some(key);
        } else {
            line.push_str("; ");
        }
        line.push_str(&listing.title);
        let own = artist_name(&listing.artist);
        if !own.eq_ignore_ascii_case(&artist) {
            let _ = write!(line, " (by {own})");
        }
    }
    if !line.is_empty() {
        out.push_str(&line);
        out.push('\n');
    }
}

fn history(out: &mut String, snapshot: &TasteSnapshot) {
    if snapshot.history.is_empty() {
        out.push_str(t::NO_HISTORY);
        out.push('\n');
        return;
    }
    struct ArtistPlays<'a> {
        name: &'a str,
        total: u32,
        albums: Vec<(Option<&'a str>, Vec<String>)>,
    }
    let mut artists: Vec<ArtistPlays<'_>> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for tally in &snapshot.history {
        let name = artist_name(&tally.artist);
        let ix = *index.entry(name.to_lowercase()).or_insert_with(|| {
            artists.push(ArtistPlays {
                name,
                total: 0,
                albums: Vec::new(),
            });
            artists.len() - 1
        });
        let artist = &mut artists[ix];
        artist.total += tally.plays;
        let album = tally.album.as_deref();
        let slot = match artist
            .albums
            .iter()
            .position(|(a, _)| a.map(str::to_lowercase) == album.map(str::to_lowercase))
        {
            Some(slot) => slot,
            None => {
                artist.albums.push((album, Vec::new()));
                artist.albums.len() - 1
            }
        };
        artist.albums[slot]
            .1
            .push(format!("{}{}", tally.title, plays_mark(tally.plays)));
    }
    artists.sort_by_key(|a| Reverse(a.total));
    for artist in artists {
        let parts: Vec<String> = artist
            .albums
            .iter()
            .map(|(album, tracks)| match album {
                Some(album) => format!("[{album}] {}", tracks.join(", ")),
                None => tracks.join(", "),
            })
            .collect();
        let _ = writeln!(out, "{}: {}", artist.name, parts.join("; "));
    }
}

fn forgotten(out: &mut String, snapshot: &TasteSnapshot, options: &PromptOptions, now: u64) {
    let cutoff = options.period.cutoff(now);
    let mut affinity: HashMap<String, u64> = HashMap::new();
    for tally in &snapshot.history {
        *affinity.entry(tally.artist.to_lowercase()).or_default() += u64::from(tally.plays);
    }
    for listing in snapshot.listings.iter().filter(|l| l.liked) {
        *affinity.entry(listing.artist.to_lowercase()).or_default() += LIKED_AFFINITY;
    }
    let mut heard: HashMap<(String, String), u64> = HashMap::new();
    for tally in &snapshot.history {
        let last = heard
            .entry((tally.artist.to_lowercase(), tally.title.to_lowercase()))
            .or_default();
        *last = (*last).max(tally.last_played);
    }
    let last_played = |l: &TrackListing| {
        let by_name = heard
            .get(&(l.artist.to_lowercase(), l.title.to_lowercase()))
            .copied();
        l.last_played.max(by_name)
    };
    let is_forgotten = |last: Option<u64>| match (cutoff, last) {
        (_, None) => true,
        (Some(cutoff), Some(last)) => last < cutoff,
        (None, Some(_)) => false,
    };
    let mut candidates: Vec<(u64, &TrackListing, Option<u64>)> = snapshot
        .listings
        .iter()
        .filter(|l| !l.liked)
        .map(|l| (l, last_played(l)))
        .filter(|(_, last)| is_forgotten(*last))
        .map(|(l, last)| {
            let score = affinity.get(&l.artist.to_lowercase()).copied().unwrap_or(0);
            (score, l, last)
        })
        .collect();
    candidates.sort_by_key(|(score, l, _)| (Reverse(*score), l.track_id));
    candidates.truncate(FORGOTTEN_CANDIDATES);
    if candidates.is_empty() {
        out.push_str("(none)\n");
        return;
    }
    for (_, listing, last_played) in candidates {
        let _ = write!(out, "{} — {}", artist_name(&listing.artist), listing.title);
        if let Some(album) = &listing.album {
            let _ = write!(out, " [{album}]");
        }
        let note = match last_played {
            Some(last) => {
                let months = (now.saturating_sub(last) / MONTH_SECS).max(1);
                t::MONTHS_AGO.replace("{n}", &months.to_string())
            }
            None => t::NEVER_PLAYED.to_string(),
        };
        let _ = writeln!(out, " ({note})");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ai_prompt::options::Period;
    use crate::tools::ai_prompt::taste::AlbumEntry;
    use music_library::{PlayTally, RecentPlay};

    const NOW: u64 = 1_800_000_000;
    const DAY: u64 = 86_400;

    fn listing(id: i64, artist: &str, title: &str, album: (i64, &str)) -> TrackListing {
        TrackListing {
            track_id: id,
            title: title.into(),
            artist: artist.into(),
            album_id: Some(album.0),
            album: Some(album.1.into()),
            album_artist: Some(artist.into()),
            year: Some(1997),
            liked: false,
            plays: 0,
            last_played: None,
        }
    }

    fn tally(artist: &str, title: &str, album: Option<&str>, plays: u32) -> PlayTally {
        PlayTally {
            artist: artist.into(),
            title: title.into(),
            album: album.map(Into::into),
            plays,
            qualified_plays: plays,
            last_played: NOW - DAY,
        }
    }

    fn snapshot() -> TasteSnapshot {
        let mut airbag = listing(1, "Radiohead", "Airbag", (10, "OK Computer"));
        airbag.plays = 7;
        airbag.last_played = Some(NOW - DAY);
        let mut lucky = listing(2, "Radiohead", "Lucky", (10, "OK Computer"));
        lucky.liked = true;
        let exit_music = listing(4, "Radiohead", "Exit Music", (10, "OK Computer"));
        let mut teardrop = listing(3, "Massive Attack", "Teardrop", (11, "Mezzanine"));
        teardrop.plays = 1;
        teardrop.last_played = Some(NOW - 400 * DAY);
        let history = vec![
            tally("Radiohead", "Airbag", Some("OK Computer"), 7),
            tally("Burial", "Archangel", Some("Untrue"), 1),
            PlayTally {
                last_played: NOW - 400 * DAY,
                ..tally("Massive Attack", "Teardrop", Some("Mezzanine"), 1)
            },
        ];
        TasteSnapshot {
            period_tallies: history[..2].to_vec(),
            history,
            recent: vec![RecentPlay {
                artist: "Burial".into(),
                title: "Archangel".into(),
                album: Some("Untrue".into()),
                started_at: NOW - DAY,
            }],
            albums: vec![
                AlbumEntry {
                    id: 10,
                    artist: "Radiohead".into(),
                    title: "OK Computer".into(),
                    year: Some(1997),
                    genres: vec!["Alternative".into()],
                },
                AlbumEntry {
                    id: 11,
                    artist: "Massive Attack".into(),
                    title: "Mezzanine".into(),
                    year: Some(1998),
                    genres: vec!["Trip-Hop".into()],
                },
            ],
            listings: vec![airbag, lucky, teardrop, exit_music],
        }
    }

    fn options(mode: Mode, period: Period) -> PromptOptions {
        PromptOptions {
            mode,
            period,
            release_window: Period::HalfYear,
            count: 10,
            detail: Detail::Low,
            wishes: String::new(),
            answer_language: "Russian",
        }
    }

    #[test]
    fn new_music_lists_every_album_and_every_heard_track() {
        let prompt = build_prompt(&snapshot(), &options(Mode::NewMusic, Period::HalfYear), NOW);
        assert!(prompt.contains("Radiohead: OK Computer (1997) {Alternative}"));
        assert!(prompt.contains("Massive Attack: Mezzanine (1998) {Trip-Hop}"));
        assert!(prompt.contains("Burial: [Untrue] Archangel"));
        assert!(prompt.contains("Massive Attack: [Mezzanine] Teardrop"));
        assert!(prompt.contains("[OK Computer] Airbag ×7"));
        assert!(prompt.contains("recommend 10 artists or albums"));
    }

    #[test]
    fn new_releases_states_the_window_and_demands_sources() {
        let prompt = build_prompt(&snapshot(), &options(Mode::NewReleases, Period::Week), NOW);
        assert!(prompt.contains(
            "find up to 10 music releases that came out between 2026-07-17 and 2027-01-15 (the last 6 months; today is 2027-01-15)"
        ));
        assert!(prompt.contains("Search the web"));
        assert!(prompt.contains("Never invent a release;"));
        assert!(prompt.contains("do not put links or citations in the answer"));
        assert!(prompt.contains("mark each one with the type `reissue`"));
        assert!(prompt.contains(t::ANSWER_NEW_RELEASES_LOW));
        let mut opts = options(Mode::NewReleases, Period::AllTime);
        opts.release_window = Period::Week;
        let week = build_prompt(&snapshot(), &opts, NOW);
        assert!(week.contains("between 2027-01-08 and 2027-01-15 (the last 7 days;"));
    }

    #[test]
    fn new_releases_answer_has_no_links_and_a_localized_date() {
        for detail in Detail::ALL {
            let mut opts = options(Mode::NewReleases, Period::HalfYear);
            opts.detail = detail;
            let prompt = build_prompt(&snapshot(), &opts, NOW);
            let answer = prompt.split("Answer format:").nth(1).unwrap();
            assert!(!answer.contains("URL"));
            assert!(!answer.contains("Source"));
            assert!(!answer.contains("YYYY"));
            assert!(
                answer.contains("the way it is usually written in the language of your answer")
            );
        }
    }

    #[test]
    fn new_releases_share_the_exclusion_lists_with_new_music() {
        let prompt = build_prompt(
            &snapshot(),
            &options(Mode::NewReleases, Period::HalfYear),
            NOW,
        );
        assert!(prompt.contains(&format!("## {}", t::SECTION_LIBRARY_ALBUMS)));
        assert!(prompt.contains(&format!("## {}", t::SECTION_HISTORY)));
        assert!(prompt.contains("Radiohead: OK Computer (1997) {Alternative}"));
        assert!(prompt.contains("[OK Computer] Airbag ×7"));
    }

    #[test]
    fn only_new_releases_mention_dates_and_the_web() {
        for mode in [Mode::NewMusic, Mode::FromLibrary, Mode::Forgotten] {
            let prompt = build_prompt(&snapshot(), &options(mode, Period::HalfYear), NOW);
            assert!(!prompt.contains("Search the web"));
            assert!(!prompt.contains("2027-01-15"));
        }
    }

    #[test]
    fn answer_language_follows_the_option() {
        let prompt = build_prompt(&snapshot(), &options(Mode::NewMusic, Period::HalfYear), NOW);
        assert!(prompt.contains("Write every explanation in Russian."));
    }

    #[test]
    fn from_library_lists_tracks_grouped_by_album() {
        let prompt = build_prompt(&snapshot(), &options(Mode::FromLibrary, Period::Month), NOW);
        assert!(
            prompt.contains(
                "Radiohead — [OK Computer (1997) {Alternative}]: Airbag; Lucky; Exit Music"
            )
        );
        assert!(prompt.contains("Massive Attack — [Mezzanine (1997) {Trip-Hop}]: Teardrop"));
    }

    #[test]
    fn forgotten_skips_recent_plays_and_ranks_by_affinity() {
        let prompt = build_prompt(
            &snapshot(),
            &options(Mode::Forgotten, Period::HalfYear),
            NOW,
        );
        let section = prompt.split("## Forgotten candidates").nth(1).unwrap();
        assert!(!section.contains("Airbag ["));
        let exit_music = section.find("Radiohead — Exit Music").unwrap();
        let teardrop = section.find("Massive Attack — Teardrop").unwrap();
        assert!(exit_music < teardrop);
        assert!(section.contains("Exit Music [OK Computer] (never played here)"));
        assert!(section.contains("last played 13 months ago"));
    }

    #[test]
    fn forgotten_never_offers_liked_tracks() {
        for period in Period::ALL {
            let prompt = build_prompt(&snapshot(), &options(Mode::Forgotten, period), NOW);
            let section = prompt.split("## Forgotten candidates").nth(1).unwrap();
            assert!(!section.contains("Radiohead — Lucky"));
        }
    }

    #[test]
    fn full_history_only_for_new_music() {
        let heading = format!("## {}", t::SECTION_HISTORY);
        let new = build_prompt(&snapshot(), &options(Mode::NewMusic, Period::HalfYear), NOW);
        assert!(new.contains(&heading));
        let lib = build_prompt(
            &snapshot(),
            &options(Mode::FromLibrary, Period::HalfYear),
            NOW,
        );
        assert!(!lib.contains(&heading));
    }

    #[test]
    fn forgotten_all_time_means_never_played() {
        let prompt = build_prompt(&snapshot(), &options(Mode::Forgotten, Period::AllTime), NOW);
        let section = prompt.split("## Forgotten candidates").nth(1).unwrap();
        assert!(section.contains("Radiohead — Exit Music"));
        assert!(!section.contains("Teardrop ["));
    }

    #[test]
    fn forgotten_counts_plays_matched_only_by_name() {
        let mut snap = snapshot();
        let mut stray = tally("Radiohead", "exit music", None, 1);
        stray.last_played = NOW - 2 * DAY;
        snap.history.push(stray);
        let prompt = build_prompt(&snap, &options(Mode::Forgotten, Period::AllTime), NOW);
        let section = prompt.split("## Forgotten candidates").nth(1).unwrap();
        assert!(!section.contains("Radiohead — Exit Music"));
    }

    #[test]
    fn empty_history_falls_back_to_library() {
        let mut snap = snapshot();
        snap.history.clear();
        snap.period_tallies.clear();
        snap.recent.clear();
        let prompt = build_prompt(&snap, &options(Mode::NewMusic, Period::HalfYear), NOW);
        assert!(prompt.contains(t::NO_HISTORY));
        assert!(prompt.contains(t::GENRES_BY_PLAYS));
    }

    #[test]
    fn detail_picks_the_answer_format() {
        let mut opts = options(Mode::NewMusic, Period::HalfYear);
        let low = build_prompt(&snapshot(), &opts, NOW);
        assert!(low.contains(t::ANSWER_NEW_MUSIC_LOW));
        opts.detail = Detail::High;
        let high = build_prompt(&snapshot(), &opts, NOW);
        assert!(high.contains(t::ANSWER_NEW_MUSIC_HIGH));
        assert!(!high.contains(t::ANSWER_NEW_MUSIC_LOW));
        opts.mode = Mode::Forgotten;
        opts.detail = Detail::Medium;
        let medium = build_prompt(&snapshot(), &opts, NOW);
        assert!(medium.contains(t::ANSWER_PLAYLIST_MEDIUM));
        opts.mode = Mode::NewReleases;
        opts.detail = Detail::High;
        let releases = build_prompt(&snapshot(), &opts, NOW);
        assert!(releases.contains(t::ANSWER_NEW_RELEASES_HIGH));
    }

    #[test]
    fn wishes_are_included_trimmed() {
        let mut opts = options(Mode::NewMusic, Period::HalfYear);
        opts.wishes = "  something calm for the evening \n".into();
        let prompt = build_prompt(&snapshot(), &opts, NOW);
        assert!(prompt.contains("\nsomething calm for the evening\n"));
    }

    #[test]
    fn no_file_paths_leak() {
        let prompt = build_prompt(
            &snapshot(),
            &options(Mode::FromLibrary, Period::AllTime),
            NOW,
        );
        assert!(!prompt.contains('/'));
    }
}

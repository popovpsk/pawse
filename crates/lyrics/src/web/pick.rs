use super::{Candidate, LyricsQuery};
use crate::parser::{Lyrics, parse_lrc};

const DURATION_TOLERANCE_SECS: f64 = 10.0;
const DURATION_CLOSE_SECS: f64 = 3.0;
const TRUNCATED_SHARE: f64 = 0.6;
const OVERSHOOT_SECS: f64 = 5.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Timing {
    Words,
    Lines,
    Plain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ending {
    Fits,
    Early,
    Late,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Rank {
    incomplete: bool,
    timing: Timing,
    censored: bool,
    drift: u64,
    provider: usize,
}

impl Rank {
    fn is_final(&self) -> bool {
        !self.incomplete && self.timing == Timing::Words && !self.censored && self.drift == 0
    }
}

#[derive(Debug)]
pub(super) struct Scored {
    ending: Ending,
    timing: Timing,
    censored: bool,
    drift: u64,
    provider: usize,
    candidate: Candidate,
}

impl Scored {
    pub(super) fn new(q: &LyricsQuery, candidate: Candidate, provider: usize) -> Option<Self> {
        if !fits(
            q,
            &candidate.title,
            &candidate.artist,
            candidate.duration_secs,
        ) {
            return None;
        }
        let lyrics = parse_lrc(&candidate.text);
        if lyrics.lines.iter().all(|line| line.text.trim().is_empty()) {
            return None;
        }
        Some(Self {
            ending: ending(&lyrics, q.duration_secs),
            timing: timing(&lyrics),
            censored: censored(&lyrics),
            drift: drift(q, candidate.duration_secs),
            provider,
            candidate,
        })
    }

    fn rank(&self, found: &[Scored]) -> Rank {
        let incomplete = match self.ending {
            Ending::Fits => false,
            Ending::Late => true,
            Ending::Early => found
                .iter()
                .any(|other| other.ending == Ending::Fits && other.timing != Timing::Plain),
        };
        Rank {
            incomplete,
            timing: self.timing,
            censored: self.censored,
            drift: self.drift,
            provider: self.provider,
        }
    }
}

fn ranked(found: &[Scored]) -> Option<(usize, Rank)> {
    found
        .iter()
        .enumerate()
        .map(|(ix, scored)| (ix, scored.rank(found)))
        .min_by_key(|(_, rank)| *rank)
}

pub(super) fn settled(found: &[Scored]) -> bool {
    ranked(found).is_some_and(|(_, rank)| rank.is_final())
}

pub(super) fn into_best(mut found: Vec<Scored>) -> Option<Candidate> {
    let (ix, _) = ranked(&found)?;
    Some(found.swap_remove(ix).candidate)
}

pub(super) fn fits(q: &LyricsQuery, title: &str, artist: &str, duration: Option<f64>) -> bool {
    if !text_matches(title, &q.title) || !text_matches(artist, &q.artist) {
        return false;
    }
    match (duration, q.duration_secs) {
        (Some(found), Some(wanted)) => (found - wanted as f64).abs() <= DURATION_TOLERANCE_SECS,
        _ => true,
    }
}

pub(super) fn drift(q: &LyricsQuery, duration: Option<f64>) -> u64 {
    let (Some(found), Some(wanted)) = (duration, q.duration_secs) else {
        return 0;
    };
    let delta = (found - wanted as f64).abs();
    if delta <= DURATION_CLOSE_SECS {
        0
    } else {
        delta.round() as u64
    }
}

fn text_matches(candidate: &str, wanted: &str) -> bool {
    let candidate = candidate.trim().to_lowercase();
    let wanted = wanted.trim().to_lowercase();
    if candidate.is_empty() || wanted.is_empty() {
        return false;
    }
    candidate == wanted || candidate.contains(&wanted) || wanted.contains(&candidate)
}

fn timing(lyrics: &Lyrics) -> Timing {
    if !lyrics.synced {
        Timing::Plain
    } else if lyrics.lines.iter().any(|line| !line.words.is_empty()) {
        Timing::Words
    } else {
        Timing::Lines
    }
}

fn ending(lyrics: &Lyrics, duration_secs: Option<u64>) -> Ending {
    let Some(duration) = duration_secs
        .filter(|secs| *secs > 0)
        .map(|secs| secs as f64)
    else {
        return Ending::Fits;
    };
    if !lyrics.synced {
        return Ending::Fits;
    }
    let Some(last) = lyrics
        .lines
        .iter()
        .filter(|line| !line.text.trim().is_empty())
        .filter_map(|line| line.time_ms)
        .max()
    else {
        return Ending::Fits;
    };
    let last = last as f64 / 1000.0;
    if last > duration + OVERSHOOT_SECS {
        Ending::Late
    } else if last < duration * TRUNCATED_SHARE {
        Ending::Early
    } else {
        Ending::Fits
    }
}

fn censored(lyrics: &Lyrics) -> bool {
    lyrics.lines.iter().any(|line| {
        masked(&line.text)
            || line
                .background
                .as_ref()
                .is_some_and(|backing| masked(&backing.text))
    })
}

fn masked(text: &str) -> bool {
    text.split_whitespace().any(|token| {
        let token = token.trim_matches(|c: char| !c.is_alphanumeric() && c != '*');
        if !token.contains('*') || !token.chars().any(char::is_alphabetic) {
            return false;
        }
        let inner = token.trim_matches('*');
        let emphasis = token.starts_with('*')
            && token.ends_with('*')
            && !inner.contains('*')
            && inner.chars().any(char::is_alphabetic);
        !emphasis
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(duration_secs: Option<u64>) -> LyricsQuery {
        LyricsQuery {
            artist: "Band".to_string(),
            title: "Song".to_string(),
            album: None,
            duration_secs,
        }
    }

    fn candidate(text: &str, duration_secs: Option<f64>) -> Candidate {
        Candidate {
            provider: "test",
            title: "Song".to_string(),
            artist: "Band".to_string(),
            duration_secs,
            text: text.to_string(),
        }
    }

    fn scored(text: &str, duration_secs: Option<f64>, provider: usize) -> Scored {
        Scored::new(&query(Some(100)), candidate(text, duration_secs), provider).unwrap()
    }

    const WORDS: &str = "[00:10.00]<00:10.00>la <00:11.00>la\n[01:30.00]<01:30.00>end";
    const LINES: &str = "[00:10.00]la la\n[01:30.00]end";
    const PLAIN: &str = "la la\nend";
    const SHORT_WORDS: &str = "[00:10.00]<00:10.00>la <00:11.00>la\n[00:40.00]<00:40.00>end";

    fn winner(found: Vec<Scored>) -> String {
        into_best(found).unwrap().text
    }

    fn settled_alone(scored: Scored) -> bool {
        settled(&[scored])
    }

    #[test]
    fn timing_outranks_provider_and_duration() {
        let found = vec![
            scored(PLAIN, Some(100.0), 0),
            scored(LINES, Some(100.0), 0),
            scored(WORDS, Some(108.0), 2),
        ];
        assert_eq!(winner(found), WORDS);
    }

    #[test]
    fn a_sheet_ending_early_loses_to_a_synced_one_that_goes_on() {
        let found = vec![
            scored(SHORT_WORDS, Some(100.0), 0),
            scored(LINES, Some(100.0), 2),
        ];
        assert_eq!(winner(found), LINES);
    }

    #[test]
    fn an_early_end_every_synced_sheet_shares_is_a_long_outro() {
        let short_lines = "[00:10.00]la la\n[00:40.00]end";
        let found = vec![
            scored(PLAIN, Some(100.0), 1),
            scored(short_lines, Some(100.0), 1),
            scored(SHORT_WORDS, Some(100.0), 0),
        ];
        assert_eq!(winner(found), SHORT_WORDS);
    }

    #[test]
    fn lyrics_running_past_the_track_are_incomplete() {
        let late = "[00:10.00]la\n[01:50.00]too late";
        let found = vec![scored(late, Some(100.0), 0), scored(PLAIN, Some(100.0), 2)];
        assert_eq!(winner(found), PLAIN);
    }

    #[test]
    fn an_incomplete_sheet_still_beats_nothing() {
        assert_eq!(
            winner(vec![scored(SHORT_WORDS, Some(100.0), 0)]),
            SHORT_WORDS
        );
    }

    #[test]
    fn censored_loses_only_within_the_same_timing() {
        let censored_lines = "[00:10.00]la f**k\n[01:30.00]end";
        let found = vec![
            scored(censored_lines, Some(100.0), 0),
            scored(LINES, Some(100.0), 2),
        ];
        assert_eq!(winner(found), LINES);

        let censored_words = "[00:10.00]<00:10.00>la <00:11.00>sh*t\n[01:30.00]<01:30.00>end";
        let found = vec![
            scored(censored_words, Some(100.0), 2),
            scored(LINES, Some(100.0), 0),
        ];
        assert_eq!(winner(found), censored_words);
    }

    #[test]
    fn close_durations_tie_and_the_provider_order_settles_it() {
        let found = vec![
            scored("[00:10.00]second\n[01:30.00]x", Some(102.0), 1),
            scored("[00:10.00]first\n[01:30.00]x", Some(97.0), 0),
        ];
        assert!(winner(found).contains("first"));

        let found = vec![
            scored("[00:10.00]far\n[01:30.00]x", Some(108.0), 0),
            scored("[00:10.00]near\n[01:30.00]x", Some(100.0), 1),
        ];
        assert!(winner(found).contains("near"));
    }

    #[test]
    fn mismatched_candidates_are_dropped() {
        let q = query(Some(100));
        let mut other_artist = candidate(LINES, Some(100.0));
        other_artist.artist = "Someone Else".to_string();
        assert!(Scored::new(&q, other_artist, 0).is_none());

        assert!(Scored::new(&q, candidate(LINES, Some(111.0)), 0).is_none());
        assert!(Scored::new(&q, candidate(LINES, Some(110.0)), 0).is_some());
        assert!(Scored::new(&q, candidate(LINES, None), 0).is_some());
        assert!(Scored::new(&q, candidate(" \n ", Some(100.0)), 0).is_none());
    }

    #[test]
    fn only_complete_clean_close_word_lyrics_are_final() {
        assert!(settled_alone(scored(WORDS, Some(100.0), 2)));
        assert!(settled_alone(scored(SHORT_WORDS, Some(100.0), 0)));
        assert!(!settled_alone(scored(LINES, Some(100.0), 0)));
        assert!(!settled_alone(scored(WORDS, Some(107.0), 0)));
        assert!(!settled(&[
            scored(SHORT_WORDS, Some(100.0), 0),
            scored(LINES, Some(100.0), 1)
        ]));
    }

    #[test]
    fn unknown_track_duration_skips_the_duration_checks() {
        let q = query(None);
        let short = Scored::new(&q, candidate(SHORT_WORDS, Some(500.0)), 0).unwrap();
        let lines = Scored::new(&q, candidate(LINES, Some(500.0)), 1).unwrap();
        assert!(settled(&[short, lines]));
    }

    #[test]
    fn masked_words_are_censored_but_emphasis_is_not() {
        for text in ["f**k", "sh*t", "mother****in", "a*s here", "f**k!"] {
            assert!(masked(text), "{text}");
        }
        for text in [
            "*sigh*",
            "plain words",
            "",
            "**bold**",
            "* * *",
            "***",
            "(*laughs*)",
            "*sigh*,",
        ] {
            assert!(!masked(text), "{text}");
        }
    }

    #[test]
    fn text_matches_handles_substrings_and_case() {
        assert!(text_matches("Zephyrnauts", "zephyrnauts"));
        assert!(text_matches("Zephyrnauts feat. Marn", "Zephyrnauts"));
        assert!(text_matches("Vaeloth", "vaeloth (remastered)"));
        assert!(!text_matches("Quill Vesper", "Zephyrnauts"));
        assert!(!text_matches("", "x"));
    }
}

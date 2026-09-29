use rstest::rstest;

use super::*;

fn with_presets(ids: &[&str]) -> Rewriter {
    let config = RewriteConfig {
        presets: ids.iter().map(|s| s.to_string()).collect(),
        rules: Vec::new(),
    };
    let (rewriter, errors) = Rewriter::compile(&config);
    assert!(errors.is_empty());
    rewriter
}

fn run(id: &str, field: Field, text: &str) -> String {
    with_presets(&[id])
        .rewrite(field, text)
        .unwrap_or_else(|| text.to_string())
}

fn scrobble(artist: &str, title: &str, album: Option<&str>) -> Scrobble {
    Scrobble {
        artist: artist.to_string(),
        title: title.to_string(),
        album: album.map(str::to_string),
        album_artist: None,
        track_number: Some(3),
        duration_secs: Some(200),
        timestamp: 1_700_000_000,
    }
}

#[rstest]
#[case::parenthesized("Mothership (Remastered)", "Mothership")]
#[case::year_inside("Let It Be (Remastered 2009)", "Let It Be")]
#[case::bracketed("How The West Was Won [Remastered]", "How The West Was Won")]
#[case::deluxe_remaster("Ride the Lightning (Deluxe Remaster)", "Ride the Lightning")]
#[case::dash_year_dash("Outside The Wall - 2011 - Remaster", "Outside The Wall")]
#[case::dash_year("China Grove - 2006 Remaster", "China Grove")]
#[case::digital("Learning To Fly - 2001 Digital Remaster", "Learning To Fly")]
#[case::remastered_version("Red Right Hand - 2011 Remastered Version", "Red Right Hand")]
#[case::dash_remastered("Here Comes The Sun - Remastered", "Here Comes The Sun")]
#[case::remastered_year("1979 - Remastered 2012", "1979")]
#[case::live_slash("Ticket To Ride - Live / Remastered", "Ticket To Ride - Live")]
#[case::double(
    "Wish You Were Here [Remastered] (Remastered Version)",
    "Wish You Were Here"
)]
#[case::untouched("Master of Puppets", "Master of Puppets")]
#[case::keeps_an_earlier_feat("Song (feat. X) (2011 Remaster)", "Song (feat. X)")]
#[case::keeps_an_earlier_live("Song (Live) [Remastered]", "Song (Live)")]
fn remastered(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(run("remastered", Field::Title, input), expected);
    assert_eq!(run("remastered", Field::Album, input), expected);
}

#[rstest]
#[case::explicit("HUMBLE. (Explicit)", "HUMBLE.")]
#[case::clean_brackets("Track [Clean]", "Track")]
#[case::explicit_version("6 Foot 7 Foot (Explicit Version)", "6 Foot 7 Foot")]
#[case::lowercase("Song (explicit)", "Song")]
fn explicit(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(run("explicit", Field::Title, input), expected);
}

#[rstest]
#[case::single("Blinding Lights - Single", "Blinding Lights")]
#[case::ep("Ghost Stories - EP", "Ghost Stories")]
#[case::inside_name("Single Ladies", "Single Ladies")]
fn single_ep(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(run("single_ep", Field::Album, input), expected);
}

#[test]
fn single_ep_leaves_titles_alone() {
    assert_eq!(
        run("single_ep", Field::Title, "Blinding Lights - Single"),
        "Blinding Lights - Single"
    );
}

#[rstest]
#[case::album_version("Love Will Come To You (Album Version)", "Love Will Come To You")]
#[case::rerecorded("I Melt With You (Rerecorded)", "I Melt With You")]
#[case::re_recorded("When I Need You [Re-Recorded]", "When I Need You")]
#[case::single_version("Your Cheatin' Heart (Single Version)", "Your Cheatin' Heart")]
#[case::edit("All Over Now (Edit)", "All Over Now")]
#[case::mono(
    "(I Can't Get No) Satisfaction - Mono Version",
    "(I Can't Get No) Satisfaction"
)]
#[case::stereo("Ruby Tuesday - Stereo Version", "Ruby Tuesday")]
#[case::original("6 Foot 7 Foot - Original", "6 Foot 7 Foot")]
#[case::original_single_version("Personal Jesus - Original Single Version", "Personal Jesus")]
#[case::original_version_year("YMCA - Original Version 1978", "YMCA")]
fn version(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(run("version", Field::Title, input), expected);
}

#[rstest]
#[case::deluxe_edition("Pure McCartney (Deluxe Edition)", "Pure McCartney")]
#[case::deluxe("Midnights (Deluxe)", "Midnights")]
#[case::super_deluxe("Abbey Road [Super Deluxe Edition]", "Abbey Road")]
#[case::expanded("Ace of Spades (Expanded Edition)", "Ace of Spades")]
#[case::expanded_remastered("On Parole (Expanded and Remastered)", "On Parole")]
#[case::dash_expanded("Sound of White Noise - Expanded Edition", "Sound of White Noise")]
#[case::bonus("No Remorse (Bonus Track Edition)", "No Remorse")]
#[case::anniversary(
    "Persistence of Time (30th Anniversary Remaster)",
    "Persistence of Time"
)]
#[case::reissue("Album Title Re-issue", "Album Title")]
#[case::reissue_parens("Album Title (2015 Reissue)", "Album Title")]
#[case::reissue_keeps_an_earlier_note("Album (Live) (2015 Reissue)", "Album (Live)")]
fn edition(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(run("edition", Field::Album, input), expected);
}

#[rstest]
#[case::remix("Track - X Remix", "Track (X Remix)")]
#[case::edit("Strobe - Club Edit", "Strobe (Club Edit)")]
#[case::bare_remix("Track - Remix", "Track (Remix)")]
#[case::instrumental("Track - Instrumental", "Track (Instrumental)")]
#[case::untouched("Track (X Remix)", "Track (X Remix)")]
fn remix_suffix(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(run("remix_suffix", Field::Title, input), expected);
}

#[rstest]
#[case::parens("Get Lucky (feat. Pharrell Williams)", "Get Lucky")]
#[case::brackets("Song [Feat. Someone]", "Song")]
#[case::ft("Song (ft. Someone)", "Song")]
#[case::bare("Song feat. Someone", "Song")]
#[case::featuring("Song (featuring Someone)", "Song")]
#[case::keeps_a_trailing_part("Song (feat. X) [Live]", "Song [Live]")]
#[case::word_inside("Defeated", "Defeated")]
fn feat(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(run("feat", Field::Title, input), expected);
}

#[rstest]
#[case::dash("Ticket To Ride - Live", "Ticket To Ride")]
#[case::dash_at("Ticket To Ride - Live At The Hollywood Bowl", "Ticket To Ride")]
#[case::parens("Track (Live)", "Track")]
#[case::parens_at("Track (Live at Wembley)", "Track")]
#[case::word_inside("Live Forever", "Live Forever")]
fn live(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(run("live", Field::Title, input), expected);
}

#[test]
fn every_preset_compiles_and_its_example_holds() {
    for preset in PRESETS {
        let rewriter = Rewriter::for_preset(preset);
        assert_eq!(
            rewriter.steps.len(),
            preset.rules.len(),
            "{} has a pattern that does not compile",
            preset.id
        );
        let field = preset.fields[0];
        let (before, after) = preset.example;
        assert_eq!(
            rewriter.rewrite(field, before).as_deref(),
            Some(after),
            "example of {}",
            preset.id
        );
    }
}

#[test]
fn preset_ids_are_unique() {
    let ids: BTreeSet<&str> = PRESETS.iter().map(|p| p.id).collect();
    assert_eq!(ids.len(), PRESETS.len());
}

#[test]
fn presets_run_in_table_order_regardless_of_config_order() {
    let rewriter = with_presets(&["remastered", "feat"]);
    assert_eq!(
        rewriter
            .rewrite(Field::Title, "Song (feat. X) - 2011 Remaster")
            .as_deref(),
        Some("Song")
    );
}

#[test]
fn an_empty_config_changes_nothing() {
    let (rewriter, errors) = Rewriter::compile(&RewriteConfig::default());
    assert!(errors.is_empty());
    assert!(rewriter.is_empty());
    let s = scrobble("A", "T (Remastered)", Some("B"));
    assert_eq!(rewriter.apply(&s), s);
}

#[test]
fn unknown_preset_ids_are_ignored() {
    let rewriter = with_presets(&["no_such_preset"]);
    assert!(rewriter.is_empty());
}

#[test]
fn a_literal_rule_escapes_its_pattern_and_replacement() {
    let rule = CustomRule {
        field: Field::Artist,
        pattern: "AC/DC (Official)".to_string(),
        replacement: "AC/DC $1".to_string(),
        ..CustomRule::default()
    };
    let rewriter = Rewriter::for_rule(&rule).unwrap();
    assert_eq!(
        rewriter
            .rewrite(Field::Artist, "AC/DC (Official)")
            .as_deref(),
        Some("AC/DC $1")
    );
    assert_eq!(rewriter.rewrite(Field::Artist, "AC/DC Official"), None);
    assert_eq!(rewriter.rewrite(Field::Title, "AC/DC (Official)"), None);
}

#[test]
fn a_regex_rule_expands_groups_and_replaces_every_match() {
    let rule = CustomRule {
        pattern: r"([a-z]+)_".to_string(),
        replacement: "${1} ".to_string(),
        regex: true,
        ..CustomRule::default()
    };
    let rewriter = Rewriter::for_rule(&rule).unwrap();
    assert_eq!(
        rewriter.rewrite(Field::Title, "one_two_three").as_deref(),
        Some("one two three")
    );
}

#[rstest]
#[case::number_then_letters("$1abc", "Xabc")]
#[case::two_groups("$2 $1", "Y X")]
#[case::braced("${1}!", "X!")]
#[case::literal_dollar("$$1", "$1")]
#[case::named("$first", "X")]
#[case::trailing_dollar("costs $", "costs $")]
fn plain_group_numbers_mean_what_people_expect(#[case] replacement: &str, #[case] expected: &str) {
    let rule = CustomRule {
        pattern: r"^(?P<first>X)(Y)$".to_string(),
        replacement: replacement.to_string(),
        regex: true,
        ..CustomRule::default()
    };
    let rewriter = Rewriter::for_rule(&rule).unwrap();
    assert_eq!(
        rewriter.rewrite(Field::Title, "XY").as_deref(),
        Some(expected)
    );
}

#[test]
fn ignore_case_applies_to_literal_rules() {
    let rule = CustomRule {
        field: Field::Artist,
        pattern: "the beatles".to_string(),
        replacement: "The Beatles".to_string(),
        ignore_case: true,
        ..CustomRule::default()
    };
    let rewriter = Rewriter::for_rule(&rule).unwrap();
    assert_eq!(
        rewriter.rewrite(Field::Artist, "THE BEATLES").as_deref(),
        Some("The Beatles")
    );
}

#[test]
fn a_broken_rule_is_reported_and_the_rest_still_run() {
    let config = RewriteConfig {
        presets: BTreeSet::new(),
        rules: vec![
            CustomRule {
                pattern: "(unclosed".to_string(),
                regex: true,
                ..CustomRule::default()
            },
            CustomRule {
                pattern: "x".to_string(),
                replacement: "y".to_string(),
                ..CustomRule::default()
            },
        ],
    };
    let (rewriter, errors) = Rewriter::compile(&config);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].index, 0);
    assert_eq!(rewriter.rewrite(Field::Title, "x").as_deref(), Some("y"));
}

#[test]
fn disabled_and_empty_rules_are_skipped() {
    let config = RewriteConfig {
        presets: BTreeSet::new(),
        rules: vec![
            CustomRule {
                enabled: false,
                pattern: "x".to_string(),
                replacement: "y".to_string(),
                ..CustomRule::default()
            },
            CustomRule::default(),
        ],
    };
    let (rewriter, errors) = Rewriter::compile(&config);
    assert!(errors.is_empty());
    assert!(rewriter.is_empty());
}

#[test]
fn custom_rules_see_the_output_of_presets() {
    let config = RewriteConfig {
        presets: ["remastered".to_string()].into(),
        rules: vec![CustomRule {
            pattern: "^Let It Be$".to_string(),
            replacement: "Let It Be!".to_string(),
            regex: true,
            ..CustomRule::default()
        }],
    };
    let (rewriter, _) = Rewriter::compile(&config);
    assert_eq!(
        rewriter
            .rewrite(Field::Title, "Let It Be (Remastered 2009)")
            .as_deref(),
        Some("Let It Be!")
    );
}

#[test]
fn artist_and_title_never_become_empty() {
    let config = RewriteConfig {
        presets: BTreeSet::new(),
        rules: vec![
            CustomRule {
                field: Field::Title,
                pattern: ".*".to_string(),
                regex: true,
                ..CustomRule::default()
            },
            CustomRule {
                field: Field::Artist,
                pattern: "Artist".to_string(),
                ..CustomRule::default()
            },
        ],
    };
    let (rewriter, _) = Rewriter::compile(&config);
    let s = scrobble("Artist", "Title", None);
    let out = rewriter.apply(&s);
    assert_eq!(out.artist, "Artist");
    assert_eq!(out.title, "Title");
}

#[test]
fn an_album_rewritten_to_nothing_is_dropped() {
    let config = RewriteConfig {
        presets: BTreeSet::new(),
        rules: vec![CustomRule {
            field: Field::Album,
            pattern: "Unknown Album".to_string(),
            ..CustomRule::default()
        }],
    };
    let (rewriter, _) = Rewriter::compile(&config);
    let out = rewriter.apply(&scrobble("A", "T", Some("Unknown Album")));
    assert_eq!(out.album, None);
}

#[test]
fn apply_keeps_the_fields_it_does_not_rewrite() {
    let rewriter = with_presets(&["remastered", "single_ep"]);
    let s = scrobble("A", "T - 2011 Remaster", Some("T - Single"));
    let out = rewriter.apply(&s);
    assert_eq!(out.title, "T");
    assert_eq!(out.album.as_deref(), Some("T"));
    assert_eq!(out.track_number, s.track_number);
    assert_eq!(out.duration_secs, s.duration_secs);
    assert_eq!(out.timestamp, s.timestamp);
}

#[test]
fn a_love_is_rewritten_like_the_scrobble_it_matches() {
    let rewriter = with_presets(&["remastered"]);
    let love = Love {
        track_id: Some(7),
        artist: "A".to_string(),
        title: "T (Remastered)".to_string(),
        loved: true,
        at: 5,
    };
    let out = rewriter.apply_love(&love);
    assert_eq!(out.title, "T");
    assert_eq!(out.track_id, Some(7));
    assert!(out.loved);
}

#[test]
fn preview_counts_tracks_and_collects_distinct_examples() {
    let rewriter = with_presets(&["remastered"]);
    let sample = |title: &str, album: Option<&str>| Sample {
        artist: "A".to_string(),
        title: title.to_string(),
        album: album.map(str::to_string),
        album_artist: None,
    };
    let samples = vec![
        sample("One (Remastered)", Some("Album (Remastered)")),
        sample("Two (Remastered)", Some("Album (Remastered)")),
        sample("Three", Some("Album")),
        sample("Four (Remastered)", None),
    ];
    let result = preview(&rewriter, &samples, 3);
    assert_eq!(result.tracks, 3);
    assert_eq!(
        result.examples,
        vec![
            ("One (Remastered)".to_string(), "One".to_string()),
            ("Album (Remastered)".to_string(), "Album".to_string()),
            ("Two (Remastered)".to_string(), "Two".to_string()),
        ]
    );
}

#[test]
fn preview_skips_what_delivery_would_not_change() {
    let config = RewriteConfig {
        presets: BTreeSet::new(),
        rules: vec![CustomRule {
            field: Field::Artist,
            pattern: "Unknown".to_string(),
            ..CustomRule::default()
        }],
    };
    let (rewriter, _) = Rewriter::compile(&config);
    let samples = vec![Sample {
        artist: "Unknown".to_string(),
        title: "T".to_string(),
        album: None,
        album_artist: None,
    }];
    assert_eq!(preview(&rewriter, &samples, 3), Preview::default());
    assert_eq!(
        rewriter.apply(&scrobble("Unknown", "T", None)).artist,
        "Unknown"
    );
}

#[test]
fn old_configs_without_rewrite_fields_deserialize() {
    let config: RewriteConfig = serde_json::from_str("{}").unwrap();
    assert_eq!(config, RewriteConfig::default());
    let rule: CustomRule = serde_json::from_str(r#"{"pattern":"x"}"#).unwrap();
    assert!(rule.enabled);
    assert_eq!(rule.field, Field::Title);
}

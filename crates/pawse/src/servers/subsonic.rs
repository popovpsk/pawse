use music_library::RemoteSong;

use super::{
    PlaylistScope, RemoteError, RemotePlaylist, ServerClient, joined_genres, one_line, real_album,
    real_artist, real_track_number, server_lyrics,
};

const MAIN_KIND: &str = "main";
const BACKGROUND_ROLE: &str = "bg";

pub struct Subsonic(subsonic::Client);

impl Subsonic {
    pub fn new(config: &subsonic::Config) -> Self {
        Self(subsonic::Client::new(config))
    }
}

fn error(error: subsonic::Error) -> RemoteError {
    match error {
        subsonic::Error::Auth => RemoteError::Auth,
        subsonic::Error::Transient(message) => RemoteError::Unreachable(message),
        subsonic::Error::NotFound(message) => RemoteError::NotFound(message),
        subsonic::Error::Server(message) => RemoteError::Other(message),
    }
}

impl ServerClient for Subsonic {
    fn ping(&self) -> Result<(), RemoteError> {
        self.0.ping().map_err(error)
    }

    fn songs(&self) -> Result<Vec<RemoteSong>, RemoteError> {
        Ok(self
            .0
            .songs()
            .map_err(error)?
            .into_iter()
            .map(song)
            .collect())
    }

    fn favorite_keys(&self) -> Result<Vec<String>, RemoteError> {
        Ok(self
            .0
            .starred_songs()
            .map_err(error)?
            .into_iter()
            .map(|song| song.id)
            .collect())
    }

    fn cover_art(&self, key: &str, max_size: u32) -> Result<Vec<u8>, RemoteError> {
        self.0.cover_art(key, max_size).map_err(error)
    }

    fn scrobble(&self, key: &str, played_at: u64) -> Result<(), RemoteError> {
        self.0
            .scrobble(key, played_at.saturating_mul(1000))
            .map_err(error)
    }

    fn now_playing(&self, key: &str) -> Result<(), RemoteError> {
        self.0.now_playing(key).map_err(error)
    }

    fn set_favorite(&self, key: &str, favorite: bool) -> Result<(), RemoteError> {
        self.0.set_starred(key, favorite).map_err(error)
    }

    fn playlists(&self, scope: PlaylistScope) -> Result<Vec<RemotePlaylist>, RemoteError> {
        let mut playlists = Vec::new();
        for playlist in self.0.playlists().map_err(error)? {
            if scope == PlaylistScope::Mine && !self.0.is_mine(&playlist) {
                continue;
            }
            match self.0.playlist_song_ids(&playlist.id) {
                Ok(keys) => playlists.push(RemotePlaylist {
                    name: playlist.name,
                    keys,
                }),
                Err(subsonic::Error::NotFound(message)) => {
                    log::info!("subsonic: playlist {} is gone: {message}", playlist.id);
                }
                Err(e) => return Err(error(e)),
            }
        }
        Ok(playlists)
    }

    fn lyrics(&self, key: &str) -> Result<Option<lyrics::Lyrics>, RemoteError> {
        match self.0.lyrics(key) {
            Ok(entries) => Ok(main_lyrics(entries)),
            Err(subsonic::Error::Server(message) | subsonic::Error::NotFound(message)) => {
                log::debug!("subsonic: no lyrics for {key}: {message}");
                Ok(None)
            }
            Err(e) => Err(error(e)),
        }
    }

    fn fetch_range(
        &self,
        key: &str,
        start: u64,
        end: Option<u64>,
    ) -> Result<server_http::RangeBody, RemoteError> {
        self.0.fetch_range(key, start, end).map_err(error)
    }
}

fn main_lyrics(entries: Vec<subsonic::StructuredLyrics>) -> Option<lyrics::Lyrics> {
    let (synced, plain): (Vec<_>, Vec<_>) = entries
        .into_iter()
        .filter(|entry| {
            entry
                .kind
                .as_deref()
                .is_none_or(|kind| kind.is_empty() || kind.eq_ignore_ascii_case(MAIN_KIND))
        })
        .partition(|entry| entry.synced);
    synced
        .into_iter()
        .chain(plain)
        .find_map(|entry| server_lyrics(lines(&entry)))
}

type Voices<'a> = (Vec<&'a subsonic::CueLine>, Vec<&'a subsonic::CueLine>);

fn lines(entry: &subsonic::StructuredLyrics) -> Vec<lyrics::LyricLine> {
    let background: Vec<&str> = entry
        .agents
        .iter()
        .filter(|agent| {
            agent
                .role
                .as_deref()
                .is_some_and(|role| role.eq_ignore_ascii_case(BACKGROUND_ROLE))
        })
        .map(|agent| agent.id.as_str())
        .collect();
    let mut voices: Vec<Voices> = vec![(Vec::new(), Vec::new()); entry.line.len()];
    for cue_line in &entry.cue_line {
        let Some((front, back)) = cue_line.index.and_then(|ix| voices.get_mut(ix)) else {
            continue;
        };
        if cue_line
            .agent_id
            .as_deref()
            .is_some_and(|id| background.contains(&id))
        {
            back.push(cue_line);
        } else {
            front.push(cue_line);
        }
    }
    let offset = entry.offset.unwrap_or(0);
    let at = |ms: i64| u32::try_from(ms.saturating_sub(offset).max(0)).ok();
    let words = |text: &str, cue_lines: &[&subsonic::CueLine]| {
        if !entry.synced {
            return Vec::new();
        }
        lyrics::locate_words(
            text,
            cue_lines
                .iter()
                .flat_map(|cue_line| &cue_line.cue)
                .filter_map(|cue| {
                    Some((at(cue.start?)?, cue.end.and_then(at), cue.value.as_str()))
                }),
        )
    };
    entry
        .line
        .iter()
        .zip(voices)
        .map(|(line, (front, back))| {
            let split = joined(&front)
                .zip(joined(&back))
                .filter(|(front, _)| !front.is_empty());
            let (text, sung, background) = match split {
                Some((front_text, back_text)) => {
                    let back_text = one_line(&back_text);
                    let backing = lyrics::Backing {
                        words: words(&back_text, &back),
                        text: back_text,
                    };
                    (one_line(&front_text), front, Some(backing))
                }
                None => {
                    let both = front.iter().chain(&back).copied().collect();
                    (one_line(&line.value), both, None)
                }
            };
            lyrics::LyricLine {
                time_ms: line.start.filter(|_| entry.synced).and_then(at),
                words: words(&text, &sung),
                text,
                background,
            }
        })
        .collect()
}

fn joined(cue_lines: &[&subsonic::CueLine]) -> Option<String> {
    if cue_lines.is_empty() {
        return None;
    }
    let values: Vec<&str> = cue_lines
        .iter()
        .map(|cue_line| cue_line.value.as_deref().map(str::trim))
        .collect::<Option<_>>()?;
    Some(
        values
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join(" "),
    )
}

fn first_name(names: &[subsonic::Named]) -> Option<String> {
    names
        .iter()
        .map(|named| named.name.trim())
        .find(|name| !name.is_empty())
        .map(str::to_string)
}

fn song(song: subsonic::Song) -> RemoteSong {
    let listed: Vec<String> = song.genres.iter().map(|named| named.name.clone()).collect();
    RemoteSong {
        key: song.id,
        title: song.title,
        artist: real_artist(first_name(&song.artists).or(song.artist.clone())),
        artist_aliases: song
            .artist
            .iter()
            .cloned()
            .chain(song.artists.iter().skip(1).map(|named| named.name.clone()))
            .filter_map(|name| real_artist(Some(name)))
            .collect(),
        album: real_album(song.album),
        album_artist: real_artist(first_name(&song.album_artists).or(song.album_artist)),
        track_number: real_track_number(song.track),
        disc_number: song.disc_number,
        year: song.year,
        genre: joined_genres(&listed).or(song.genre),
        duration_ms: song.duration.map(|secs| (secs * 1000) as i64),
        size: song.size.map(|size| size as i64),
        suffix: song.suffix,
        content_type: song.content_type,
        bitrate_kbps: song.bit_rate,
        cover_key: song.cover_art,
        cover_hash: None,
        start_offset_ms: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_credited_artist_is_used_instead_of_the_joined_display_name() {
        let converted = song(subsonic::Song {
            id: "1".into(),
            title: "Moonlight".into(),
            artist: Some("Daniel Lanois • Daryl Johnson".into()),
            artists: vec![
                subsonic::Named {
                    name: "Daniel Lanois".into(),
                },
                subsonic::Named {
                    name: "Daryl Johnson".into(),
                },
            ],
            ..Default::default()
        });
        assert_eq!(converted.artist.as_deref(), Some("Daniel Lanois"));
        assert_eq!(
            converted.artist_aliases,
            vec![
                "Daniel Lanois • Daryl Johnson".to_string(),
                "Daryl Johnson".to_string()
            ]
        );
    }

    #[test]
    fn server_placeholders_for_missing_tags_are_dropped() {
        let converted = song(subsonic::Song {
            id: "1".into(),
            title: "Whole Album Image".into(),
            artist: Some("[Unknown Artist]".into()),
            album_artist: Some(" [unknown artist] ".into()),
            album: Some("[Unknown Album]".into()),
            track: Some(1997),
            ..Default::default()
        });
        assert_eq!(converted.artist, None);
        assert_eq!(converted.album_artist, None);
        assert_eq!(converted.album, None);
        assert_eq!(converted.track_number, None);
    }

    #[test]
    fn units_are_converted_to_the_library_ones() {
        let converted = song(subsonic::Song {
            id: "1".into(),
            duration: Some(185),
            size: Some(4_000_000),
            bit_rate: Some(320),
            suffix: Some("flac".into()),
            ..Default::default()
        });
        assert_eq!(converted.duration_ms, Some(185_000));
        assert_eq!(converted.size, Some(4_000_000));
        assert_eq!(converted.bitrate_kbps, Some(320));
        assert_eq!(converted.suffix.as_deref(), Some("flac"));
    }

    #[test]
    fn the_genre_list_wins_over_the_single_genre_and_the_single_one_is_the_fallback() {
        let named = |names: &[&str]| -> Vec<subsonic::Named> {
            names
                .iter()
                .map(|name| subsonic::Named {
                    name: name.to_string(),
                })
                .collect()
        };
        let listed = song(subsonic::Song {
            id: "1".into(),
            genre: Some("Score".into()),
            genres: named(&["Score", " ", "Heavy Metal", "Industrial"]),
            ..Default::default()
        });
        assert_eq!(
            listed.genre.as_deref(),
            Some("Score; Heavy Metal; Industrial")
        );
        let single = song(subsonic::Song {
            id: "2".into(),
            genre: Some("Rock; Pop".into()),
            ..Default::default()
        });
        assert_eq!(single.genre.as_deref(), Some("Rock; Pop"));
        let blank_list = song(subsonic::Song {
            id: "3".into(),
            genre: Some("Folk".into()),
            genres: named(&[" "]),
            ..Default::default()
        });
        assert_eq!(blank_list.genre.as_deref(), Some("Folk"));
        let none = song(subsonic::Song {
            id: "4".into(),
            ..Default::default()
        });
        assert_eq!(none.genre, None);
    }

    #[test]
    fn errors_keep_the_auth_and_unreachable_split() {
        assert_eq!(error(subsonic::Error::Auth), RemoteError::Auth);
        assert_eq!(
            error(subsonic::Error::Transient("down".into())),
            RemoteError::Unreachable("down".into())
        );
        assert_eq!(
            error(subsonic::Error::Server("x".into())),
            RemoteError::Other("x".into())
        );
    }

    fn entry(
        kind: Option<&str>,
        synced: bool,
        lines: &[(Option<i64>, &str)],
    ) -> subsonic::StructuredLyrics {
        subsonic::StructuredLyrics {
            kind: kind.map(str::to_string),
            synced,
            line: lines
                .iter()
                .map(|(start, value)| subsonic::LyricsLine {
                    start: *start,
                    value: value.to_string(),
                })
                .collect(),
            ..Default::default()
        }
    }

    fn cue_line(index: usize, agent: &str, value: Option<&str>) -> subsonic::CueLine {
        subsonic::CueLine {
            index: Some(index),
            agent_id: Some(agent.into()),
            value: value.map(str::to_string),
            cue: Vec::new(),
        }
    }

    fn agent(id: &str, role: &str) -> subsonic::Agent {
        subsonic::Agent {
            id: id.into(),
            role: Some(role.into()),
        }
    }

    fn texts(lyrics: &lyrics::Lyrics) -> Vec<(Option<u32>, &str, Option<&str>)> {
        lyrics
            .lines
            .iter()
            .map(|line| {
                (
                    line.time_ms,
                    line.text.as_str(),
                    line.background
                        .as_ref()
                        .map(|backing| backing.text.as_str()),
                )
            })
            .collect()
    }

    #[test]
    fn backing_vocals_are_split_off_the_line_they_belong_to() {
        let mut main = entry(
            Some("main"),
            true,
            &[
                (Some(1_000), "Hello (echo)"),
                (Some(3_000), "(ooh)"),
                (Some(5_000), "Plain line"),
                (Some(7_000), "Out of range"),
            ],
        );
        main.agents = vec![agent("lead", "main"), agent("backing", "bg")];
        main.cue_line = vec![
            cue_line(0, "lead", Some("Hello")),
            cue_line(0, "backing", Some(" (echo) ")),
            cue_line(1, "backing", Some("(ooh)")),
            cue_line(2, "lead", Some("Plain line")),
            cue_line(9, "backing", Some("(lost)")),
        ];
        let lyrics = main_lyrics(vec![main]).unwrap();
        assert_eq!(
            texts(&lyrics),
            vec![
                (Some(1_000), "Hello", Some("(echo)")),
                (Some(3_000), "(ooh)", None),
                (Some(5_000), "Plain line", None),
                (Some(7_000), "Out of range", None),
            ]
        );
    }

    #[test]
    fn a_line_that_cannot_be_split_cleanly_stays_whole() {
        let mut main = entry(
            None,
            true,
            &[(Some(0), "Hello world (echo)"), (Some(2_000), "(ooh)")],
        );
        main.agents = vec![agent("lead", "main"), agent("backing", "bg")];
        main.cue_line = vec![
            cue_line(0, "lead", None),
            cue_line(0, "backing", Some("(echo)")),
            cue_line(1, "lead", Some("  ")),
            cue_line(1, "backing", Some("(ooh)")),
        ];
        assert_eq!(
            texts(&main_lyrics(vec![main]).unwrap()),
            vec![
                (Some(0), "Hello world (echo)", None),
                (Some(2_000), "(ooh)", None),
            ]
        );
    }

    #[test]
    fn a_line_with_several_singers_keeps_them_all_in_front() {
        let mut main = entry(None, true, &[(Some(0), "You and me (me)")]);
        main.agents = vec![
            agent("lead", "main"),
            agent("guest", "voice"),
            agent("bg", "bg"),
        ];
        main.cue_line = vec![
            cue_line(0, "lead", Some("You and")),
            cue_line(0, "guest", Some("me")),
            cue_line(0, "bg", Some("(me)")),
        ];
        assert_eq!(
            texts(&main_lyrics(vec![main]).unwrap()),
            vec![(Some(0), "You and me", Some("(me)"))]
        );
    }

    #[test]
    fn the_synced_main_track_wins_and_translations_are_left_out() {
        let lyrics = main_lyrics(vec![
            entry(Some("translation"), true, &[(Some(0), "Hallo")]),
            entry(Some("main"), false, &[(None, "plain words")]),
            entry(Some(""), true, &[(Some(2_000), "timed words")]),
        ])
        .unwrap();
        assert!(lyrics.synced);
        assert_eq!(texts(&lyrics), vec![(Some(2_000), "timed words", None)]);

        let plain = main_lyrics(vec![entry(None, false, &[(Some(5), "no times")])]).unwrap();
        assert!(!plain.synced);
        assert_eq!(texts(&plain), vec![(None, "no times", None)]);

        assert_eq!(
            main_lyrics(vec![entry(Some("pronunciation"), true, &[(Some(0), "x")])]),
            None
        );
        assert_eq!(main_lyrics(vec![entry(None, true, &[])]), None);
        assert_eq!(main_lyrics(Vec::new()), None);

        let behind_a_blank_one = main_lyrics(vec![
            entry(None, true, &[(Some(0), " "), (Some(1_000), "")]),
            entry(None, false, &[(None, "real words")]),
        ])
        .unwrap();
        assert_eq!(texts(&behind_a_blank_one), vec![(None, "real words", None)]);
    }

    #[test]
    fn the_offset_moves_lines_earlier_when_positive() {
        let mut sooner = entry(None, true, &[(Some(200), "a"), (Some(5_000), "b")]);
        sooner.offset = Some(500);
        assert_eq!(
            texts(&main_lyrics(vec![sooner]).unwrap()),
            vec![(Some(0), "a", None), (Some(4_500), "b", None)]
        );
        let mut later = entry(None, true, &[(Some(1_000), "a")]);
        later.offset = Some(-250);
        assert_eq!(
            texts(&main_lyrics(vec![later]).unwrap()),
            vec![(Some(1_250), "a", None)]
        );
    }

    fn cued(
        index: usize,
        agent: &str,
        value: Option<&str>,
        cues: &[(i64, &str)],
    ) -> subsonic::CueLine {
        subsonic::CueLine {
            cue: cues
                .iter()
                .map(|(start, value)| subsonic::Cue {
                    start: Some(*start),
                    end: None,
                    value: value.to_string(),
                })
                .collect(),
            ..cue_line(index, agent, value)
        }
    }

    fn spelled(text: &str, words: &[lyrics::Word]) -> Vec<(u32, String)> {
        words
            .iter()
            .map(|word| (word.start_ms, text[word.range.clone()].to_string()))
            .collect()
    }

    #[test]
    fn cues_become_words_of_the_text_and_of_the_backing_line() {
        let mut main = entry(
            None,
            true,
            &[
                (Some(1_000), "Hello there (hello)"),
                (Some(5_000), "Solo  line"),
            ],
        );
        main.offset = Some(100);
        main.agents = vec![agent("lead", "main"), agent("backing", "bg")];
        main.cue_line = vec![
            cued(
                0,
                "lead",
                Some("Hello there"),
                &[(1_000, "Hello"), (1_500, " "), (1_600, "there")],
            ),
            cued(0, "backing", Some("(hello)"), &[(2_000, "(hello)")]),
            cued(
                1,
                "lead",
                Some("Solo  line"),
                &[(5_000, "Solo "), (5_400, "line")],
            ),
        ];
        let lines = main_lyrics(vec![main]).unwrap().lines;
        assert_eq!(
            spelled(&lines[0].text, &lines[0].words),
            vec![(900, "Hello".into()), (1_500, "there".into())]
        );
        let backing = lines[0].background.as_ref().unwrap();
        assert_eq!(
            spelled(&backing.text, &backing.words),
            vec![(1_900, "(hello)".into())]
        );
        assert_eq!(
            spelled(&lines[1].text, &lines[1].words),
            vec![(4_900, "Solo".into()), (5_300, "line".into())]
        );
    }

    #[test]
    fn an_unsplit_line_takes_every_cue_in_text_order() {
        let mut main = entry(None, true, &[(Some(0), "Hi (hi)")]);
        main.agents = vec![agent("lead", "main"), agent("backing", "bg")];
        main.cue_line = vec![
            cued(0, "lead", None, &[(0, "Hi")]),
            cued(0, "backing", Some("(hi)"), &[(400, "(hi)")]),
        ];
        let lines = main_lyrics(vec![main]).unwrap().lines;
        assert_eq!(lines[0].background, None);
        assert_eq!(
            spelled(&lines[0].text, &lines[0].words),
            vec![(0, "Hi".into()), (400, "(hi)".into())]
        );

        let mut plain = entry(None, false, &[(None, "no timing")]);
        plain.cue_line = vec![cued(0, "lead", Some("no timing"), &[(0, "no")])];
        assert!(main_lyrics(vec![plain]).unwrap().lines[0].words.is_empty());
    }
}

use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CueSheet {
    pub title: Option<String>,
    pub performer: Option<String>,
    pub date: Option<String>,
    pub genre: Option<String>,
    pub files: Vec<CueFile>,
    pub tracks: Vec<CueTrack>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CueFile {
    pub name: String,
    pub file_type: CueFileType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CueFileType {
    Wave,
    Mp3,
    Aiff,
    Other(String),
}

impl CueFileType {
    pub fn parse_file_type(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "WAVE" => CueFileType::Wave,
            "MP3" => CueFileType::Mp3,
            "AIFF" => CueFileType::Aiff,
            other => CueFileType::Other(other.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CueTrack {
    pub number: u32,
    pub file: usize,
    pub title: String,
    pub performer: Option<String>,
    pub songwriter: Option<String>,
    pub isrc: Option<String>,
    pub indices: Vec<CueIndex>,
    pub pregap: Option<Duration>,
}

impl CueTrack {
    pub fn start(&self) -> Option<CueIndex> {
        self.indices.iter().find(|index| index.number == 1).copied()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CueIndex {
    pub number: u32,
    pub file: usize,
    pub position: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CueParseError {
    #[error("missing FILE directive")]
    MissingFile,
    #[error("no AUDIO tracks found")]
    NoAudioTracks,
    #[error("parse error at line {line}: {message}")]
    SyntaxError { line: usize, message: String },
}

#[derive(Default)]
struct SheetHeader {
    title: Option<String>,
    performer: Option<String>,
    date: Option<String>,
    genre: Option<String>,
}

impl SheetHeader {
    fn apply(&mut self, command: &str, parts: &[String], line: &str) {
        match command {
            "TITLE" if self.title.is_none() => {
                self.title = Some(extract_quoted(line, "TITLE"));
            }
            "PERFORMER" if self.performer.is_none() => {
                self.performer = Some(extract_quoted(line, "PERFORMER"));
            }
            "REM" if parts.len() >= 3 => {
                let value = strip_quotes(&parts[2..].join(" "));
                match parts[1].to_uppercase().as_str() {
                    "DATE" => self.date = Some(value),
                    "GENRE" => self.genre = Some(value),
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

struct OpenTrack {
    track: CueTrack,
    line: usize,
}

impl OpenTrack {
    fn start(parts: &[String], line: usize, file: usize) -> Result<Option<Self>, CueParseError> {
        if parts.len() < 3 || parts[2].to_uppercase() != "AUDIO" {
            return Ok(None);
        }
        let number: u32 = parts[1].parse().map_err(|_| CueParseError::SyntaxError {
            line,
            message: "invalid TRACK number".into(),
        })?;
        Ok(Some(Self {
            track: CueTrack {
                number,
                file,
                title: format!("Track {}", number),
                performer: None,
                songwriter: None,
                isrc: None,
                indices: Vec::new(),
                pregap: None,
            },
            line,
        }))
    }

    fn apply(
        &mut self,
        command: &str,
        parts: &[String],
        line: &CueLine,
        file: usize,
    ) -> Result<(), CueParseError> {
        let trimmed = line.content.trim();
        let track = &mut self.track;
        match command {
            "TITLE" => track.title = extract_quoted(trimmed, "TITLE"),
            "PERFORMER" => track.performer = Some(extract_quoted(trimmed, "PERFORMER")),
            "SONGWRITER" => track.songwriter = Some(extract_quoted(trimmed, "SONGWRITER")),
            "ISRC" if parts.len() >= 2 => {
                track.isrc = Some(parts[1].to_string());
            }
            "INDEX" if parts.len() >= 3 => {
                let number: u32 = parts[1].parse().map_err(|_| CueParseError::SyntaxError {
                    line: line.number,
                    message: "invalid INDEX number".into(),
                })?;
                let position =
                    parse_mmssff(&parts[2]).ok_or_else(|| CueParseError::SyntaxError {
                        line: line.number,
                        message: format!("invalid INDEX time: {}", parts[2]),
                    })?;
                track.indices.push(CueIndex {
                    number,
                    file,
                    position,
                });
            }
            "PREGAP"
                if parts.len() >= 2
                    && let Some(dur) = parse_mmssff(&parts[1]) =>
            {
                track.pregap = Some(dur);
            }
            _ => {}
        }
        Ok(())
    }

    fn finish(self) -> Result<CueTrack, CueParseError> {
        let mut track = self.track;
        let Some(start) = track.start() else {
            return Err(CueParseError::SyntaxError {
                line: self.line,
                message: format!("TRACK {} has no INDEX 01", track.number),
            });
        };
        track.file = start.file;
        Ok(track)
    }
}

struct CueLine {
    content: String,
    number: usize,
}

struct CueParser {
    lines: Vec<CueLine>,
}

impl CueParser {
    fn new(input: &str) -> Self {
        let input = input.strip_prefix('\u{feff}').unwrap_or(input);
        let lines: Vec<CueLine> = input
            .lines()
            .enumerate()
            .map(|(i, l)| CueLine {
                content: l.to_string(),
                number: i + 1,
            })
            .collect();
        Self { lines }
    }

    fn parse_sheet(&self) -> Result<CueSheet, CueParseError> {
        let mut header = SheetHeader::default();
        let mut files: Vec<CueFile> = Vec::new();
        let mut current_file: Option<usize> = None;
        let mut tracks = Vec::new();
        let mut in_tracks = false;
        let mut open: Option<OpenTrack> = None;

        for line in &self.lines {
            let trimmed = line.content.trim();
            let parts = split_command(trimmed);
            let Some(command) = parts.first().map(|c| c.to_uppercase()) else {
                continue;
            };

            match command.as_str() {
                "FILE" => {
                    let (name, file_type) = parse_file_directive(trimmed, line.number)?;
                    let index = match files.iter().position(|file| file.name == name) {
                        Some(index) => index,
                        None => {
                            files.push(CueFile {
                                name,
                                file_type: CueFileType::parse_file_type(&file_type),
                            });
                            files.len() - 1
                        }
                    };
                    current_file = Some(index);
                }
                "TRACK" => {
                    if let Some(track) = open.take() {
                        tracks.push(track.finish()?);
                    }
                    let Some(file) = current_file else {
                        return Err(CueParseError::MissingFile);
                    };
                    in_tracks = true;
                    open = OpenTrack::start(&parts, line.number, file)?;
                }
                _ if !in_tracks => header.apply(&command, &parts, trimmed),
                _ => {
                    if let (Some(track), Some(file)) = (open.as_mut(), current_file) {
                        track.apply(&command, &parts, line, file)?;
                    }
                }
            }
        }
        if let Some(track) = open {
            tracks.push(track.finish()?);
        }

        if files.is_empty() {
            return Err(CueParseError::MissingFile);
        }
        if tracks.is_empty() {
            return Err(CueParseError::NoAudioTracks);
        }

        Ok(CueSheet {
            title: header.title,
            performer: header.performer,
            date: header.date,
            genre: header.genre,
            files,
            tracks,
        })
    }
}

fn parse_file_directive(line: &str, number: usize) -> Result<(String, String), CueParseError> {
    let after = line.strip_prefix("FILE").unwrap_or(line);
    let after = after.trim();

    let (quoted_name, rest) = if let Some(stripped) = after.strip_prefix('"') {
        if let Some(end) = stripped.find('"') {
            let name = stripped[..end].to_string();
            let rest = stripped[end + 1..].trim();
            (name, rest)
        } else {
            (stripped.to_string(), "")
        }
    } else {
        let mut parts = after.splitn(2, char::is_whitespace);
        let name = parts.next().unwrap_or("").to_string();
        let rest = parts.next().unwrap_or("").trim();
        (name, rest)
    };

    let file_type = rest.split_whitespace().next().unwrap_or("").to_string();
    if file_type.is_empty() {
        return Err(CueParseError::SyntaxError {
            line: number,
            message: "FILE directive missing file type".into(),
        });
    }

    Ok((quoted_name, file_type))
}

fn extract_quoted(line: &str, command: &str) -> String {
    let after = line.strip_prefix(command).unwrap_or(line);
    let after = after.trim();
    if let Some(stripped) = after.strip_prefix('"') {
        if stripped.ends_with('"') && !stripped.is_empty() {
            stripped[..stripped.len() - 1].to_string()
        } else if let Some(end) = stripped.find('"') {
            stripped[..end].to_string()
        } else {
            stripped.to_string()
        }
    } else {
        after.to_string()
    }
}

fn split_command(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let mut result = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for ch in trimmed.chars() {
        if ch == '"' {
            in_quotes = !in_quotes;
            current.push(ch);
        } else if ch.is_whitespace() && !in_quotes {
            if !current.is_empty() {
                result.push(current.clone());
                current.clear();
            }
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        result.push(current);
    }
    result
}

fn parse_mmssff(s: &str) -> Option<Duration> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 3 {
        return None;
    }
    let minutes: u64 = parts[0].parse().ok()?;
    let seconds: u64 = parts[1].parse().ok()?;
    let frames: u64 = parts[2].parse().ok()?;
    if frames > 74 || seconds > 59 {
        return None;
    }
    Some(Duration::from_millis(
        minutes * 60_000 + seconds * 1000 + frames * 1000 / 75,
    ))
}

fn strip_quotes(s: &str) -> String {
    let s = s.trim();
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

pub fn parse(input: &str) -> Result<CueSheet, CueParseError> {
    CueParser::new(input).parse_sheet()
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    const SPLIT_GAPS_APPENDED: &str = r#"
PERFORMER "Soundgarden"
TITLE "Superunknown"
FILE "01 - Let Me Drown.flac" WAVE
  TRACK 01 AUDIO
    TITLE "Let Me Drown"
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    TITLE "My Wave"
    INDEX 00 03:50:65
FILE "02 - My Wave.flac" WAVE
    INDEX 01 00:00:00
FILE "03 - Fell On Black Days.flac" WAVE
  TRACK 03 AUDIO
    TITLE "Fell On Black Days"
    INDEX 01 00:00:00
"#;

    const SPLIT_GAPS_PREPENDED: &str = r#"
PERFORMER "Soundgarden"
TITLE "Superunknown"
FILE "01 - Let Me Drown.flac" WAVE
  TRACK 01 AUDIO
    TITLE "Let Me Drown"
    INDEX 01 00:00:00
FILE "02 - My Wave.flac" WAVE
  TRACK 02 AUDIO
    TITLE "My Wave"
    INDEX 00 00:00:00
    INDEX 01 00:02:10
FILE "03 - Fell On Black Days.flac" WAVE
  TRACK 03 AUDIO
    TITLE "Fell On Black Days"
    INDEX 01 00:00:00
"#;

    const SPLIT_GAPS_LEFT_OUT: &str = r#"
PERFORMER "Soundgarden"
TITLE "Superunknown"
FILE "01 - Let Me Drown.flac" WAVE
  TRACK 01 AUDIO
    TITLE "Let Me Drown"
    INDEX 01 00:00:00
FILE "02 - My Wave.flac" WAVE
  TRACK 02 AUDIO
    TITLE "My Wave"
    PREGAP 00:02:10
    INDEX 01 00:00:00
FILE "03 - Fell On Black Days.flac" WAVE
  TRACK 03 AUDIO
    TITLE "Fell On Black Days"
    INDEX 01 00:00:00
"#;

    fn file_names(sheet: &CueSheet) -> Vec<&str> {
        sheet.files.iter().map(|file| file.name.as_str()).collect()
    }

    fn track_files(sheet: &CueSheet) -> Vec<(u32, usize)> {
        sheet
            .tracks
            .iter()
            .map(|track| (track.number, track.file))
            .collect()
    }

    fn track_titles(sheet: &CueSheet) -> Vec<&str> {
        sheet
            .tracks
            .iter()
            .map(|track| track.title.as_str())
            .collect()
    }

    fn start_of(track: &CueTrack) -> Duration {
        track.start().expect("track has INDEX 01").position
    }

    fn mmssff(minutes: u64, seconds: u64, frames: u64) -> Duration {
        Duration::from_millis(minutes * 60_000 + seconds * 1000 + frames * 1000 / 75)
    }

    #[rstest]
    #[case::gaps_appended(SPLIT_GAPS_APPENDED)]
    #[case::gaps_prepended(SPLIT_GAPS_PREPENDED)]
    #[case::gaps_left_out(SPLIT_GAPS_LEFT_OUT)]
    fn test_split_rip_puts_each_track_in_its_own_file(#[case] input: &str) {
        let sheet = parse(input).expect("a cue with one FILE per track must parse");
        assert_eq!(
            file_names(&sheet),
            [
                "01 - Let Me Drown.flac",
                "02 - My Wave.flac",
                "03 - Fell On Black Days.flac"
            ]
        );
        assert_eq!(track_files(&sheet), [(1, 0), (2, 1), (3, 2)]);
        assert_eq!(
            track_titles(&sheet),
            ["Let Me Drown", "My Wave", "Fell On Black Days"]
        );
        assert_eq!(sheet.title.as_deref(), Some("Superunknown"));
        assert_eq!(sheet.performer.as_deref(), Some("Soundgarden"));
    }

    #[test]
    fn test_gap_in_previous_file_keeps_that_file() {
        let sheet = parse(SPLIT_GAPS_APPENDED).expect("should parse");
        assert_eq!(
            sheet.tracks[1].indices,
            [
                CueIndex {
                    number: 0,
                    file: 0,
                    position: mmssff(3, 50, 65),
                },
                CueIndex {
                    number: 1,
                    file: 1,
                    position: Duration::ZERO,
                },
            ],
            "INDEX 00 lies at the end of the previous file, INDEX 01 opens the track's own"
        );
        assert_eq!(
            sheet.tracks[0].indices.len(),
            1,
            "the next track's gap must not attach to the previous track"
        );
    }

    #[test]
    fn test_several_tracks_per_file_across_files() {
        let input = r#"
TITLE "Double"
PERFORMER "Artist"
FILE "CD1.flac" WAVE
  TRACK 01 AUDIO
    TITLE "A"
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    TITLE "B"
    INDEX 01 04:00:00
FILE "CD2.flac" WAVE
  TRACK 03 AUDIO
    TITLE "C"
    INDEX 01 00:00:00
  TRACK 04 AUDIO
    TITLE "D"
    INDEX 00 03:58:00
    INDEX 01 04:00:00
"#;
        let sheet = parse(input).expect("a cue over two images must parse");
        assert_eq!(file_names(&sheet), ["CD1.flac", "CD2.flac"]);
        assert_eq!(track_files(&sheet), [(1, 0), (2, 0), (3, 1), (4, 1)]);
        let starts: Vec<_> = sheet.tracks.iter().map(start_of).collect();
        assert_eq!(
            starts,
            [
                Duration::ZERO,
                mmssff(4, 0, 0),
                Duration::ZERO,
                mmssff(4, 0, 0)
            ],
            "each position is relative to its own file"
        );
        assert_eq!(sheet.title.as_deref(), Some("Double"));
    }

    #[test]
    fn test_repeated_file_line_names_the_same_file() {
        let input = r#"
FILE "album.flac" WAVE
  TRACK 01 AUDIO
    TITLE "One"
    INDEX 01 00:00:00
FILE "album.flac" WAVE
  TRACK 02 AUDIO
    TITLE "Two"
    INDEX 01 04:00:00
"#;
        let sheet = parse(input).expect("should parse");
        assert_eq!(file_names(&sheet), ["album.flac"]);
        assert_eq!(
            track_files(&sheet),
            [(1, 0), (2, 0)],
            "a FILE line repeated per track is still one image, not a split rip"
        );
        assert_eq!(start_of(&sheet.tracks[1]), mmssff(4, 0, 0));
    }

    #[test]
    fn test_trailing_data_track_is_skipped() {
        let input = r#"
TITLE "Enhanced"
FILE "album.flac" WAVE
  TRACK 01 AUDIO
    TITLE "One"
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    TITLE "Two"
    INDEX 01 03:00:00
  TRACK 03 MODE1/2352
    TITLE "DATA"
    INDEX 00 40:00:00
    INDEX 01 40:02:00
"#;
        let sheet = parse(input).expect("a data track must not fail the sheet");
        assert_eq!(track_titles(&sheet), ["One", "Two"]);
        assert_eq!(
            sheet.tracks[1].indices.len(),
            1,
            "the data track's indices must not attach to the last audio track"
        );
        assert_eq!(sheet.title.as_deref(), Some("Enhanced"));
    }

    #[test]
    fn test_leading_data_track_in_its_own_file_is_skipped() {
        let input = r#"
FILE "data.bin" BINARY
  TRACK 01 MODE1/2352
    TITLE "DATA"
    INDEX 01 00:00:00
FILE "album.flac" WAVE
  TRACK 02 AUDIO
    TITLE "One"
    INDEX 01 00:00:00
  TRACK 03 AUDIO
    TITLE "Two"
    INDEX 01 03:00:00
"#;
        let sheet = parse(input).expect("a leading data track must not fail the sheet");
        assert_eq!(file_names(&sheet), ["data.bin", "album.flac"]);
        assert_eq!(track_files(&sheet), [(2, 1), (3, 1)]);
        assert_eq!(track_titles(&sheet), ["One", "Two"]);
    }

    #[test]
    fn test_only_data_tracks_is_no_audio() {
        let input = r#"
FILE "data.bin" BINARY
  TRACK 01 MODE1/2352
    INDEX 01 00:00:00
"#;
        assert_eq!(parse(input), Err(CueParseError::NoAudioTracks));
    }

    #[test]
    fn test_track_title_never_becomes_sheet_title() {
        let input = r#"
FILE "album.flac" WAVE
  TRACK 01 AUDIO
    TITLE "Song"
    PERFORMER "Singer"
    REM DATE 1999
    INDEX 01 00:00:00
"#;
        let sheet = parse(input).expect("should parse");
        assert_eq!(sheet.title, None);
        assert_eq!(sheet.performer, None);
        assert_eq!(sheet.date, None);
        assert_eq!(sheet.tracks[0].performer.as_deref(), Some("Singer"));
    }

    #[rstest]
    #[case::no_index(
        "FILE \"album.flac\" WAVE\n  TRACK 01 AUDIO\n    INDEX 01 00:00:00\n  TRACK 02 AUDIO\n    TITLE \"No Index\"\n"
    )]
    #[case::only_gap(
        "FILE \"01.flac\" WAVE\n  TRACK 01 AUDIO\n    INDEX 01 00:00:00\n  TRACK 02 AUDIO\n    INDEX 00 03:50:65\n"
    )]
    fn test_track_without_index_01_reports_its_line(#[case] input: &str) {
        assert_eq!(
            parse(input),
            Err(CueParseError::SyntaxError {
                line: 4,
                message: "TRACK 2 has no INDEX 01".into(),
            })
        );
    }

    #[test]
    fn test_file_without_type_reports_its_line() {
        let input =
            "TITLE \"Album\"\nFILE \"album.flac\"\n  TRACK 01 AUDIO\n    INDEX 01 00:00:00\n";
        assert_eq!(
            parse(input),
            Err(CueParseError::SyntaxError {
                line: 2,
                message: "FILE directive missing file type".into(),
            })
        );
    }

    #[test]
    fn test_track_before_file_is_missing_file() {
        let input = "TRACK 01 AUDIO\n  INDEX 01 00:00:00\nFILE \"album.flac\" WAVE\n";
        assert_eq!(parse(input), Err(CueParseError::MissingFile));
    }

    fn fixture_path(name: &str) -> std::path::PathBuf {
        let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("..");
        path.push("..");
        path.push("fixtures");
        path.push(name);
        path
    }

    #[test]
    fn test_parse_am_fixture() {
        let path = fixture_path(
            "2013 - AM (WIGCD317J, HSE-10137)/Arctic Monkeys - AM [WIGCD317J, HSE-10137, Japan].cue",
        );
        if !path.exists() {
            eprintln!("AM fixture not found, skipping test");
            return;
        }
        let content = std::fs::read_to_string(&path).expect("should read cue file");
        let sheet = parse(&content).expect("should parse AM cue sheet");

        assert_eq!(sheet.tracks.len(), 13);
        assert_eq!(
            sheet.title.as_deref(),
            Some("AM [WIGCD317J, HSE-10137, Japan]")
        );
        assert_eq!(sheet.performer.as_deref(), Some("Arctic Monkeys"));
        assert_eq!(sheet.date.as_deref(), Some("2013"));
        assert_eq!(sheet.genre.as_deref(), Some("Alternative"));
        assert_eq!(sheet.files.len(), 1);
        assert_eq!(
            sheet.files[0].name,
            "Arctic Monkeys - AM [WIGCD317J, HSE-10137, Japan].flac"
        );
        assert!(sheet.tracks.iter().all(|track| track.file == 0));

        assert_eq!(sheet.tracks[0].title, "Do I Wanna Know?");
        assert_eq!(sheet.tracks[0].number, 1);
        assert_eq!(sheet.tracks[0].indices[0].position, Duration::ZERO);

        assert_eq!(sheet.tracks[1].title, "R U Mine?");
        let offset = sheet.tracks[1].indices[0].position;
        assert_eq!(
            offset.as_millis(),
            (4 * 60_000 + 33 * 1000 + 8 * 1000 / 75) as u128
        );

        assert_eq!(sheet.tracks[12].title, "2013 (Japan Bonus Track)");
        assert_eq!(sheet.tracks[12].number, 13);
        assert_eq!(sheet.tracks[12].indices.len(), 2);
        assert_eq!(sheet.tracks[12].indices[0].number, 0);
        assert_eq!(sheet.tracks[12].indices[1].number, 1);
    }

    #[test]
    fn test_parse_single_track() {
        let input = r#"
TITLE "Single"
PERFORMER "Artist"
FILE "track.flac" WAVE
  TRACK 01 AUDIO
    TITLE "Only Track"
    PERFORMER "Artist"
    INDEX 01 00:00:00
"#;
        let sheet = parse(input).expect("should parse single track cue");
        assert_eq!(sheet.tracks.len(), 1);
        assert_eq!(sheet.tracks[0].title, "Only Track");
        assert_eq!(sheet.tracks[0].number, 1);
        assert_eq!(sheet.tracks[0].indices[0].position, Duration::ZERO);
    }

    #[test]
    fn test_parse_rem_date_genre() {
        let input = r#"
TITLE "Album"
PERFORMER "Artist"
REM DATE 2020
REM GENRE Rock
FILE "album.flac" WAVE
  TRACK 01 AUDIO
    TITLE "Song"
    INDEX 01 00:00:00
"#;
        let sheet = parse(input).expect("should parse REM fields");
        assert_eq!(sheet.date.as_deref(), Some("2020"));
        assert_eq!(sheet.genre.as_deref(), Some("Rock"));
    }

    #[test]
    fn test_parse_missing_file() {
        let input = r#"
TITLE "No File"
PERFORMER "Artist"
  TRACK 01 AUDIO
    TITLE "Song"
    INDEX 01 00:00:00
"#;
        let result = parse(input);
        assert!(matches!(result, Err(CueParseError::MissingFile)));
    }

    #[test]
    fn test_parse_mmssff() {
        assert_eq!(parse_mmssff("00:00:00"), Some(Duration::ZERO));
        assert_eq!(parse_mmssff("01:30:00"), Some(Duration::from_secs(90)));
        assert_eq!(
            parse_mmssff("04:33:08"),
            Some(Duration::from_millis(
                4 * 60_000 + 33 * 1000 + 8 * 1000 / 75
            ))
        );
        assert_eq!(parse_mmssff("99:99:99"), None);
    }

    #[test]
    fn test_pregap_and_indices() {
        let input = r#"
TITLE "Test"
FILE "test.flac" WAVE
  TRACK 01 AUDIO
    TITLE "Track 1"
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    TITLE "Track 2"
    INDEX 00 02:00:00
    INDEX 01 02:05:00
"#;
        let sheet = parse(input).expect("should parse with pregap");
        assert_eq!(sheet.tracks[1].indices.len(), 2);
        assert_eq!(sheet.tracks[1].indices[0].number, 0);
        assert_eq!(sheet.tracks[1].indices[1].number, 1);
        let index_00 = sheet.tracks[1].indices[0].position;
        let index_01 = sheet.tracks[1].indices[1].position;
        let pregap_dur = index_01 - index_00;
        assert_eq!(pregap_dur, Duration::from_secs(5));
    }

    #[test]
    fn test_file_type_parsing() {
        let input = r#"
FILE "test.mp3" MP3
  TRACK 01 AUDIO
    TITLE "Song"
    INDEX 01 00:00:00
"#;
        let sheet = parse(input).expect("should parse MP3 file type");
        assert_eq!(sheet.files[0].file_type, CueFileType::Mp3);
    }

    #[test]
    fn test_track_performer() {
        let input = r#"
TITLE "Album"
PERFORMER "Album Artist"
FILE "album.flac" WAVE
  TRACK 01 AUDIO
    TITLE "Song"
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    TITLE "Song 2"
    PERFORMER "Featured Artist"
    INDEX 01 03:00:00
"#;
        let sheet = parse(input).expect("should parse performer");
        assert_eq!(sheet.tracks[0].performer, None);
        assert_eq!(
            sheet.tracks[1].performer.as_deref(),
            Some("Featured Artist")
        );
    }
}

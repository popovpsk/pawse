use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CueSheet {
    pub title: Option<String>,
    pub performer: Option<String>,
    pub date: Option<String>,
    pub genre: Option<String>,
    pub file: CueFile,
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
    pub title: String,
    pub performer: Option<String>,
    pub songwriter: Option<String>,
    pub isrc: Option<String>,
    pub indices: Vec<CueIndex>,
    pub pregap: Option<Duration>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CueIndex {
    pub number: u32,
    pub position: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CueParseError {
    #[error("missing FILE directive")]
    MissingFile,
    #[error("no AUDIO tracks found")]
    NoAudioTracks,
    #[error("duplicate FILE directive")]
    DuplicateFile,
    #[error("parse error at line {line}: {message}")]
    SyntaxError { line: usize, message: String },
}

struct CueLine {
    content: String,
    number: usize,
}

struct CueParser {
    lines: Vec<CueLine>,
    pos: usize,
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
        Self { lines, pos: 0 }
    }

    fn parse_sheet(&mut self) -> Result<CueSheet, CueParseError> {
        let mut title = None;
        let mut performer = None;
        let mut date = None;
        let mut genre = None;
        let mut file = None;
        let mut tracks = Vec::new();

        while self.pos < self.lines.len() {
            let line = &self.lines[self.pos];
            let trimmed = line.content.trim();
            let parts = split_command(trimmed);
            if parts.is_empty() {
                self.pos += 1;
                continue;
            }

            match parts[0].to_uppercase().as_str() {
                "TITLE" if title.is_none() => {
                    title = Some(extract_quoted(trimmed, "TITLE"));
                }
                "PERFORMER" if performer.is_none() => {
                    performer = Some(extract_quoted(trimmed, "PERFORMER"));
                }
                "REM" if parts.len() >= 3 => {
                    let value = parts[2..].join(" ");
                    let value = strip_quotes(&value);
                    match parts[1].to_uppercase().as_str() {
                        "DATE" => date = Some(value),
                        "GENRE" => genre = Some(value),
                        _ => {}
                    }
                }
                "FILE" => {
                    if file.is_some() {
                        return Err(CueParseError::DuplicateFile);
                    }
                    let (file_name, file_type_str) = parse_file_directive(trimmed)?;
                    file = Some(CueFile {
                        name: file_name,
                        file_type: CueFileType::parse_file_type(&file_type_str),
                    });
                }
                "TRACK" => {
                    if file.is_none() {
                        return Err(CueParseError::MissingFile);
                    }
                    let track = self.parse_track()?;
                    if track.indices.is_empty() {
                        return Err(CueParseError::SyntaxError {
                            line: 0,
                            message: format!("TRACK {} has no INDEX 01", track.number),
                        });
                    }
                    tracks.push(track);
                    continue;
                }
                _ => {}
            }
            self.pos += 1;
        }

        let file = file.ok_or(CueParseError::MissingFile)?;
        if tracks.is_empty() {
            return Err(CueParseError::NoAudioTracks);
        }

        Ok(CueSheet {
            title,
            performer,
            date,
            genre,
            file,
            tracks,
        })
    }

    fn parse_track(&mut self) -> Result<CueTrack, CueParseError> {
        let line = &self.lines[self.pos];
        let trimmed = line.content.trim();
        let parts = split_command(trimmed);

        if parts.len() < 3 || parts[2].to_uppercase() != "AUDIO" {
            self.pos += 1;
            self.skip_to_next_track();
            return Err(CueParseError::NoAudioTracks);
        }

        let number: u32 = parts[1].parse().map_err(|_| CueParseError::SyntaxError {
            line: line.number,
            message: "invalid TRACK number".into(),
        })?;

        let mut track = CueTrack {
            number,
            title: format!("Track {}", number),
            performer: None,
            songwriter: None,
            isrc: None,
            indices: Vec::new(),
            pregap: None,
        };

        self.pos += 1;

        while self.pos < self.lines.len() {
            let sub_line = &self.lines[self.pos];
            let trimmed = sub_line.content.trim();
            let cmd_parts = split_command(trimmed);
            if cmd_parts.is_empty() {
                self.pos += 1;
                continue;
            }

            match cmd_parts[0].to_uppercase().as_str() {
                "TITLE" => track.title = extract_quoted(trimmed, "TITLE"),
                "PERFORMER" => track.performer = Some(extract_quoted(trimmed, "PERFORMER")),
                "SONGWRITER" => track.songwriter = Some(extract_quoted(trimmed, "SONGWRITER")),
                "ISRC" if cmd_parts.len() >= 2 => {
                    track.isrc = Some(cmd_parts[1].to_string());
                }
                "INDEX" if cmd_parts.len() >= 3 => {
                    let index_num: u32 =
                        cmd_parts[1].parse().map_err(|_| CueParseError::SyntaxError {
                            line: sub_line.number,
                            message: "invalid INDEX number".into(),
                        })?;
                    let position = parse_mmssff(&cmd_parts[2]).ok_or_else(|| {
                        CueParseError::SyntaxError {
                            line: sub_line.number,
                            message: format!("invalid INDEX time: {}", cmd_parts[2]),
                        }
                    })?;
                    track.indices.push(CueIndex {
                        number: index_num,
                        position,
                    });
                }
                "PREGAP" if cmd_parts.len() >= 2 && let Some(dur) = parse_mmssff(&cmd_parts[1]) => {
                    track.pregap = Some(dur);
                }
                "TRACK" | "FILE" => break,
                _ => {}
            }
            self.pos += 1;
        }

        Ok(track)
    }

    fn skip_to_next_track(&mut self) {
        while self.pos < self.lines.len() {
            let trimmed = self.lines[self.pos].content.trim();
            let parts = split_command(trimmed);
            if !parts.is_empty() && parts[0].to_uppercase() == "TRACK" {
                return;
            }
            self.pos += 1;
        }
    }
}

fn parse_file_directive(line: &str) -> Result<(String, String), CueParseError> {
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

    let file_type = rest
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_string();
    if file_type.is_empty() {
        return Err(CueParseError::SyntaxError {
            line: 0,
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
    let mut parser = CueParser::new(input);
    parser.parse_sheet()
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(
            sheet.file.name,
            "Arctic Monkeys - AM [WIGCD317J, HSE-10137, Japan].flac"
        );

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
        assert_eq!(sheet.file.file_type, CueFileType::Mp3);
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
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::target::{ScrobbleTarget, SubmitError, TargetId};
use crate::{NowPlaying, Scrobble};

const HEADER: &str = "timeHuman,timeMs,artist,track,album,albumArtist,durationMs,mediaPlayerPackage,mediaPlayerName,mediaPlayerVersion,event";
const PLAYER: &str = "pawse";

pub fn is_pawse_log(path: &Path) -> bool {
    let Ok(file) = File::open(path) else {
        return false;
    };
    let mut head = Vec::with_capacity(HEADER.len() + 1);
    if file
        .take(HEADER.len() as u64 + 1)
        .read_to_end(&mut head)
        .is_err()
    {
        return false;
    }
    if head.is_empty() {
        return true;
    }
    if head.len() < HEADER.len() || &head[..HEADER.len()] != HEADER.as_bytes() {
        return false;
    }
    head.len() == HEADER.len() || head[HEADER.len()] == b'\n' || head[HEADER.len()] == b'\r'
}

pub struct CsvLog {
    path: PathBuf,
    version: String,
}

impl CsvLog {
    pub fn new(path: PathBuf, version: String) -> Self {
        Self { path, version }
    }

    fn append(&self, rows: &[String]) -> Result<(), SubmitError> {
        if rows.is_empty() {
            return Ok(());
        }
        if let Some(parent) = self.path.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            return Err(SubmitError::Transient(format!("create log directory: {e}")));
        }
        let needs_header = std::fs::metadata(&self.path)
            .map(|m| m.len() == 0)
            .unwrap_or(true);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| SubmitError::Transient(format!("open log: {e}")))?;
        let mut out = String::new();
        if needs_header {
            out.push_str(HEADER);
            out.push('\n');
        }
        for row in rows {
            out.push_str(row);
            out.push('\n');
        }
        file.write_all(out.as_bytes())
            .map_err(|e| SubmitError::Transient(format!("write log: {e}")))
    }

    fn row(&self, row: Row<'_>) -> String {
        let fields = [
            format_utc(row.timestamp),
            (row.timestamp * 1000).to_string(),
            row.artist.to_string(),
            row.title.to_string(),
            row.album.unwrap_or_default().to_string(),
            row.album_artist.unwrap_or_default().to_string(),
            row.duration_secs
                .map(|d| (d * 1000).to_string())
                .unwrap_or_default(),
            PLAYER.to_string(),
            PLAYER.to_string(),
            self.version.clone(),
            row.event.to_string(),
        ];
        fields
            .iter()
            .map(|f| escape(f))
            .collect::<Vec<_>>()
            .join(",")
    }
}

impl ScrobbleTarget for CsvLog {
    fn id(&self) -> TargetId {
        TargetId::CsvLog
    }

    fn max_batch(&self) -> usize {
        usize::MAX
    }

    fn now_playing(&self, _now_playing: &NowPlaying) -> Result<(), SubmitError> {
        Err(SubmitError::Unsupported)
    }

    fn submit(&self, items: &[Scrobble]) -> Result<(), SubmitError> {
        let rows: Vec<String> = items
            .iter()
            .map(|item| {
                self.row(Row {
                    timestamp: item.timestamp,
                    artist: &item.artist,
                    title: &item.title,
                    album: item.album.as_deref(),
                    album_artist: item.album_artist.as_deref(),
                    duration_secs: item.duration_secs,
                    event: "scrobble",
                })
            })
            .collect();
        self.append(&rows)
    }

    fn love(&self, artist: &str, title: &str, love: bool) -> Result<(), SubmitError> {
        let event = if love { "love" } else { "unlove" };
        let row = self.row(Row {
            timestamp: now_secs(),
            artist,
            title,
            album: None,
            album_artist: None,
            duration_secs: None,
            event,
        });
        self.append(&[row])
    }
}

struct Row<'a> {
    timestamp: u64,
    artist: &'a str,
    title: &'a str,
    album: Option<&'a str>,
    album_artist: Option<&'a str>,
    duration_secs: Option<u64>,
    event: &'a str,
}

fn escape(field: &str) -> String {
    if field.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn format_utc(timestamp: u64) -> String {
    let secs = timestamp as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "pawse-csv-{tag}-{}-{}.csv",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn scrobble(title: &str) -> Scrobble {
        Scrobble {
            artist: "Artist, The".to_string(),
            title: title.to_string(),
            album: Some("Album \"X\"".to_string()),
            album_artist: Some("AA".to_string()),
            track_number: Some(3),
            duration_secs: Some(200),
            timestamp: 1_700_000_000,
        }
    }

    #[test]
    fn header_is_written_once_and_rows_are_appended() {
        let path = temp_path("header");
        let log = CsvLog::new(path.clone(), "1.0".to_string());
        log.submit(&[scrobble("One")]).unwrap();
        log.submit(&[scrobble("Two")]).unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], HEADER);
        assert_eq!(contents.matches(HEADER).count(), 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_empty_or_pawse_written_file_may_be_continued() {
        let empty = temp_path("is-log-empty");
        std::fs::write(&empty, b"").unwrap();
        assert!(is_pawse_log(&empty));

        let bare = temp_path("is-log-bare");
        std::fs::write(&bare, HEADER.as_bytes()).unwrap();
        assert!(is_pawse_log(&bare));

        let written = temp_path("is-log-written");
        let log = CsvLog::new(written.clone(), "1.0".to_string());
        log.submit(&[scrobble("One")]).unwrap();
        assert!(is_pawse_log(&written));

        for path in [empty, bare, written] {
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn a_foreign_file_is_never_appended_to() {
        let prose = temp_path("is-log-prose");
        std::fs::write(&prose, b"Dear Mum,\nwe had a lovely time.\n").unwrap();
        assert!(!is_pawse_log(&prose));

        let binary = temp_path("is-log-binary");
        std::fs::write(&binary, [0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10]).unwrap();
        assert!(!is_pawse_log(&binary));

        let truncated = temp_path("is-log-truncated");
        std::fs::write(&truncated, &HEADER.as_bytes()[..20]).unwrap();
        assert!(!is_pawse_log(&truncated));

        let prefixed = temp_path("is-log-prefixed");
        std::fs::write(&prefixed, format!("{HEADER}extra\n").as_bytes()).unwrap();
        assert!(!is_pawse_log(&prefixed));

        assert!(!is_pawse_log(&temp_path("is-log-missing")));

        for path in [prose, binary, truncated, prefixed] {
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn commas_and_quotes_are_escaped() {
        let path = temp_path("escape");
        let log = CsvLog::new(path.clone(), "1.0".to_string());
        log.submit(&[scrobble("Title")]).unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains("\"Artist, The\""));
        assert!(contents.contains("\"Album \"\"X\"\"\""));
        assert!(contents.contains(",200000,"));
        assert!(contents.ends_with(",scrobble\n"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn newlines_inside_a_field_are_quoted() {
        let path = temp_path("newline");
        let log = CsvLog::new(path.clone(), "1.0".to_string());
        let mut item = scrobble("Line\r\nBreak");
        item.album = None;
        log.submit(&[item]).unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains("\"Line\r\nBreak\""));
        assert_eq!(contents.matches(HEADER).count(), 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn love_writes_its_own_event_row() {
        let path = temp_path("love");
        let log = CsvLog::new(path.clone(), "1.0".to_string());
        log.love("A", "T", false).unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.lines().nth(1).unwrap().ends_with(",unlove"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn now_playing_is_unsupported() {
        let log = CsvLog::new(temp_path("np"), "1.0".to_string());
        let np = NowPlaying {
            artist: "A".to_string(),
            title: "T".to_string(),
            album: None,
            album_artist: None,
            track_number: None,
            duration_secs: None,
        };
        assert!(matches!(
            log.now_playing(&np),
            Err(SubmitError::Unsupported)
        ));
    }

    #[test]
    fn utc_formatting_matches_known_timestamps() {
        assert_eq!(format_utc(0), "1970-01-01 00:00:00");
        assert_eq!(format_utc(1_700_000_000), "2023-11-14 22:13:20");
        assert_eq!(format_utc(951_782_400), "2000-02-29 00:00:00");
    }
}

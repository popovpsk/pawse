use anyhow::{Context as _, Result};
use serde::Deserialize;

use super::elrc::{self, Line, Syllable};
use super::{Candidate, LyricsQuery, http};

pub(super) const NAME: &str = "lrcmux";
const GET_URL: &str = "https://api.lrcmux.dev/get";
const ATTEMPTS: u32 = 1;

#[derive(Deserialize)]
struct Answer {
    track: Track,
    meta: Meta,
    #[serde(default)]
    lines: Vec<AnswerLine>,
}

#[derive(Deserialize)]
struct Track {
    #[serde(default)]
    title: String,
    #[serde(default)]
    artist: String,
    #[serde(default)]
    duration: Option<f64>,
}

#[derive(Deserialize)]
struct Meta {
    #[serde(default)]
    level: String,
    #[serde(default)]
    instrumental: bool,
}

#[derive(Deserialize)]
struct AnswerLine {
    #[serde(default)]
    text: String,
    #[serde(default)]
    start: Option<u64>,
    #[serde(default)]
    words: Vec<AnswerWord>,
}

#[derive(Deserialize)]
struct AnswerWord {
    #[serde(default)]
    text: String,
    start: u64,
    #[serde(default)]
    end: Option<u64>,
}

pub(super) fn candidates(session: &http::Session, q: &LyricsQuery) -> Result<Vec<Candidate>> {
    let mut response = session
        .call(ATTEMPTS, || {
            let mut req = session
                .get(GET_URL)
                .query("artist", &q.artist)
                .query("title", &q.title);
            if let Some(album) = q.album.as_deref().filter(|album| !album.is_empty()) {
                req = req.query("album", album);
            }
            if let Some(duration) = q.duration_secs {
                req = req.query("duration", duration.to_string());
            }
            req.call()
        })
        .context("lrcmux request failed")?;
    match response.status().as_u16() {
        404 => return Ok(Vec::new()),
        code if !(200..300).contains(&code) => {
            anyhow::bail!("lrcmux request failed: HTTP {code}")
        }
        _ => {}
    }
    let body = http::body(&mut response, "lrcmux")?;
    Ok(parse(&body)?.into_iter().collect())
}

fn parse(body: &str) -> Result<Option<Candidate>> {
    let answer: Answer = serde_json::from_str(body).context("lrcmux answer is not JSON")?;
    if answer.meta.instrumental {
        return Ok(None);
    }
    let lines: Vec<Line> = answer
        .lines
        .iter()
        .map(|line| sheet_line(line, &answer.meta.level))
        .collect();
    Ok(Some(Candidate {
        provider: NAME,
        title: answer.track.title,
        artist: answer.track.artist,
        duration_secs: answer.track.duration,
        text: elrc::write(&lines),
    }))
}

fn sheet_line(line: &AnswerLine, level: &str) -> Line {
    let start_ms = match level {
        "word" | "line" => line
            .start
            .or_else(|| line.words.first().map(|word| word.start))
            .and_then(ms),
        _ => None,
    };
    let syllables = if level == "word" && start_ms.is_some() {
        syllables(&line.words)
    } else {
        Vec::new()
    };
    Line {
        start_ms,
        text: line.text.clone(),
        syllables,
        ..Default::default()
    }
}

fn syllables(words: &[AnswerWord]) -> Vec<Syllable> {
    let mut out: Vec<Syllable> = Vec::with_capacity(words.len());
    let mut space = false;
    for word in words {
        let text = word.text.trim();
        let Some(start_ms) = ms(word.start) else {
            continue;
        };
        if text.is_empty() {
            space = true;
            continue;
        }
        out.push(Syllable {
            start_ms,
            end_ms: word.end.and_then(ms),
            text: text.to_string(),
            space_before: std::mem::take(&mut space) || word.text.starts_with(char::is_whitespace),
        });
        space = word.text.ends_with(char::is_whitespace);
    }
    out
}

fn ms(value: u64) -> Option<u32> {
    u32::try_from(value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_lrc;

    fn answer(level: &str, instrumental: bool, lines: &str) -> String {
        format!(
            r#"{{"track":{{"title":"Song","artist":"Band","album":"Record","duration":187}},
               "meta":{{"source":{{"id":"kugou"}},"level":"{level}","instrumental":{instrumental}}},
               "lines":{lines}}}"#
        )
    }

    #[test]
    fn word_level_becomes_enhanced_lrc() {
        let body = answer(
            "word",
            false,
            r#"[{"text":"I'm tired","start":21909,"end":24864,"words":[
                {"text":"I'm ","start":21909,"end":22076},
                {"text":"tired","start":22217,"end":22401}]}]"#,
        );
        let found = parse(&body).unwrap().unwrap();
        assert_eq!(
            found.text,
            "[00:21.90]<00:21.90>I'm<00:22.07> <00:22.21>tired<00:22.40>"
        );
        assert_eq!(found.duration_secs, Some(187.0));
        let line = &parse_lrc(&found.text).lines[0];
        assert_eq!(line.text, "I'm tired");
        assert_eq!(line.words.len(), 2);
    }

    #[test]
    fn line_and_unsynced_levels() {
        let lines = r#"[{"text":"first","start":1000,"end":2000},{"text":"second","start":3000}]"#;
        assert_eq!(
            parse(&answer("line", false, lines)).unwrap().unwrap().text,
            "[00:01.00]first\n[00:03.00]second"
        );
        assert_eq!(
            parse(&answer("none", false, lines)).unwrap().unwrap().text,
            "first\nsecond"
        );
    }

    #[test]
    fn instrumental_gives_nothing_and_garbage_is_an_error() {
        assert!(parse(&answer("none", true, "[]")).unwrap().is_none());
        assert!(parse("not json").is_err());
    }
}

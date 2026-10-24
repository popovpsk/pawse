use std::fmt::Write as _;

use crate::band::{Band, DEFAULT_Q, Kind, MAX_BANDS};
use crate::filter::preamp_db;

const EXPORT_RATE: f64 = 48_000.0;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    #[error("line {line}: unsupported filter type {kind}")]
    Unsupported { line: usize, kind: String },
    #[error("line {line}: unreadable filter")]
    Malformed { line: usize },
    #[error("line {line}: filters for separate channels are not supported")]
    Channels { line: usize },
    #[error("no filters found")]
    Empty,
    #[error("{count} filters, at most {MAX_BANDS} are supported")]
    TooMany { count: usize },
}

pub fn parse(text: &str) -> Result<Vec<Band>, ParseError> {
    let mut bands = Vec::new();
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    for (ix, raw) in text.lines().enumerate() {
        let Some((head, body)) = raw.trim().split_once(':') else {
            continue;
        };
        let head = head.trim().to_ascii_lowercase();
        if head == "channel" && !body.trim().eq_ignore_ascii_case("all") {
            return Err(ParseError::Channels { line: ix + 1 });
        }
        if head.starts_with("filter")
            && let Some(band) = parse_filter(body, ix + 1)?
        {
            bands.push(band);
        }
    }
    if bands.is_empty() {
        return Err(ParseError::Empty);
    }
    if bands.len() > MAX_BANDS {
        return Err(ParseError::TooMany { count: bands.len() });
    }
    Ok(bands)
}

fn parse_filter(body: &str, line: usize) -> Result<Option<Band>, ParseError> {
    let malformed = ParseError::Malformed { line };
    let mut tokens = body.split_whitespace();
    let enabled = match tokens.next().map(str::to_ascii_uppercase).as_deref() {
        Some("ON") => true,
        Some("OFF") => false,
        _ => return Err(malformed),
    };
    let code = tokens.next().ok_or(malformed.clone())?;
    let kind = match code.to_ascii_uppercase().as_str() {
        "PK" | "PEQ" => Kind::Peak,
        "LSC" | "LS" => Kind::LowShelf,
        "HSC" | "HS" => Kind::HighShelf,
        "LP" | "LPQ" => Kind::LowPass,
        "HP" | "HPQ" => Kind::HighPass,
        "NONE" => return Ok(None),
        _ => {
            return Err(ParseError::Unsupported {
                line,
                kind: code.to_string(),
            });
        }
    };
    let rest: Vec<&str> = tokens.collect();
    let number = |key: &str| {
        rest.iter()
            .position(|token| token.eq_ignore_ascii_case(key))
            .and_then(|at| rest.get(at + 1))
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| value.is_finite())
    };
    let freq = number("Fc").ok_or(malformed.clone())?;
    let gain = match (kind.has_gain(), number("Gain")) {
        (true, Some(gain)) => gain,
        (true, None) => return Err(malformed),
        (false, _) => 0.0,
    };
    let q = match (number("Q"), number("Oct")) {
        (Some(q), _) => q,
        (None, Some(octaves)) => q_from_octaves(octaves),
        (None, None) if kind == Kind::Peak => return Err(malformed),
        (None, None) => DEFAULT_Q,
    };
    Ok(Some(Band {
        enabled,
        ..Band::new(kind, freq, gain, q)
    }))
}

fn q_from_octaves(octaves: f64) -> f64 {
    let ratio = 2f64.powf(octaves);
    ratio.sqrt() / (ratio - 1.0)
}

pub fn write(bands: &[Band]) -> String {
    let mut text = String::new();
    let preamp = preamp_db(bands, EXPORT_RATE);
    let _ = writeln!(text, "Preamp: {:.1} dB", preamp + 0.0);
    for (ix, band) in bands.iter().enumerate() {
        let band = band.clamped();
        let state = if band.enabled { "ON" } else { "OFF" };
        let code = match band.kind {
            Kind::Peak => "PK",
            Kind::LowShelf => "LSC",
            Kind::HighShelf => "HSC",
            Kind::LowPass => "LPQ",
            Kind::HighPass => "HPQ",
        };
        let _ = write!(
            text,
            "Filter {}: {state} {code} Fc {} Hz",
            ix + 1,
            trimmed(band.freq)
        );
        if band.kind.has_gain() {
            let _ = write!(text, " Gain {:.1} dB", band.gain);
        }
        let _ = writeln!(text, " Q {:.2}", band.q);
    }
    text
}

fn trimmed(value: f64) -> String {
    let text = format!("{value:.1}");
    text.strip_suffix(".0").map(str::to_string).unwrap_or(text)
}

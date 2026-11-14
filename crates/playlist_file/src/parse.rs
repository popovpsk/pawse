use std::borrow::Cow;
use std::path::{Component, MAIN_SEPARATOR, Path, PathBuf};

pub const EXTENSIONS: &[&str] = &["m3u", "m3u8", "pls"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Playlist {
    pub name: String,
    pub entries: Vec<PathBuf>,
}

pub fn is_playlist(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            EXTENSIONS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(ext))
        })
}

pub fn read(path: &Path) -> std::io::Result<Playlist> {
    let bytes = std::fs::read(path)?;
    Ok(parse(&bytes, path))
}

pub fn parse(bytes: &[u8], path: &Path) -> Playlist {
    let utf8 = path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("m3u8"));
    let text = decode(bytes, utf8);
    let base = path.parent().unwrap_or_else(|| Path::new(""));
    let (title, lines) = if is_pls(&text) {
        (None, pls_lines(&text))
    } else {
        m3u_lines(&text)
    };
    let entries = lines
        .into_iter()
        .filter_map(|line| locate(line, base))
        .collect();
    let name = title
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            path.file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
    Playlist { name, entries }
}

fn decode(bytes: &[u8], utf8: bool) -> Cow<'_, str> {
    if let Some((encoding, bom)) = encoding_rs::Encoding::for_bom(bytes) {
        return encoding.decode_without_bom_handling(&bytes[bom..]).0;
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Cow::Borrowed(text);
    }
    if utf8 {
        return String::from_utf8_lossy(bytes);
    }
    let mut detector = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Deny);
    detector.feed(bytes, true);
    detector
        .guess(None, chardetng::Utf8Detection::Deny)
        .decode_without_bom_handling(bytes)
        .0
}

fn split_lines(text: &str) -> impl Iterator<Item = &str> {
    text.split(['\r', '\n'])
}

fn is_pls(text: &str) -> bool {
    split_lines(text)
        .map(str::trim)
        .find(|line| !line.is_empty())
        .is_some_and(|line| line.eq_ignore_ascii_case("[playlist]"))
}

fn m3u_lines(text: &str) -> (Option<&str>, Vec<&str>) {
    let mut title = None;
    let mut lines = Vec::new();
    for line in split_lines(text).map(str::trim) {
        if line.is_empty() {
            continue;
        }
        if let Some(directive) = line.strip_prefix('#') {
            if let Some(value) = strip_prefix_ignore_case(directive, "PLAYLIST:") {
                title = Some(value);
            }
            continue;
        }
        lines.push(line);
    }
    (title, lines)
}

fn pls_lines(text: &str) -> Vec<&str> {
    let mut numbered: Vec<(u64, &str)> = Vec::new();
    for line in split_lines(text) {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let Some(number) = strip_prefix_ignore_case(key.trim(), "File") else {
            continue;
        };
        let Ok(number) = number.parse::<u64>() else {
            continue;
        };
        let value = value.trim();
        if !value.is_empty() {
            numbered.push((number, value));
        }
    }
    numbered.sort_by_key(|(number, _)| *number);
    numbered.into_iter().map(|(_, value)| value).collect()
}

fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

fn locate(line: &str, base: &Path) -> Option<PathBuf> {
    let raw: Cow<'_, str> = match strip_prefix_ignore_case(line, "file:") {
        Some(rest) => Cow::Owned(file_uri_path(rest)),
        None if has_scheme(line) => return None,
        None => Cow::Borrowed(line),
    };
    if raw.is_empty() {
        return None;
    }
    let native = native_separators(&raw);
    let path = if !cfg!(windows) && has_drive(&raw) {
        PathBuf::from(native)
    } else {
        base.join(native)
    };
    Some(normalize(&path))
}

fn has_scheme(line: &str) -> bool {
    let Some((scheme, _)) = line.split_once("://") else {
        return false;
    };
    scheme.len() > 1
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

fn has_drive(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

fn file_uri_path(rest: &str) -> String {
    let path = match rest.strip_prefix("//") {
        Some(authority) if has_drive(authority) => authority.to_string(),
        Some(authority) => {
            let (host, path) = authority.split_at(authority.find('/').unwrap_or(authority.len()));
            if host.is_empty() || host.eq_ignore_ascii_case("localhost") {
                path.to_string()
            } else {
                format!("//{host}{path}")
            }
        }
        None => rest.to_string(),
    };
    let decoded = percent_decode(&path);
    match decoded.strip_prefix('/') {
        Some(tail) if has_drive(tail) => tail.to_string(),
        _ => decoded,
    }
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut ix = 0;
    while ix < bytes.len() {
        let escaped = (bytes[ix] == b'%')
            .then(|| bytes.get(ix + 1..ix + 3))
            .flatten()
            .filter(|hex| hex.iter().all(u8::is_ascii_hexdigit))
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match escaped {
            Some(byte) => {
                out.push(byte);
                ix += 3;
            }
            None => {
                out.push(bytes[ix]);
                ix += 1;
            }
        }
    }
    match String::from_utf8(out) {
        Ok(text) => text,
        Err(error) => String::from_utf8_lossy(error.as_bytes()).into_owned(),
    }
}

fn native_separators(path: &str) -> String {
    let foreign = if MAIN_SEPARATOR == '/' { '\\' } else { '/' };
    path.chars()
        .map(|c| if c == foreign { MAIN_SEPARATOR } else { c })
        .collect()
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match out.components().next_back() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => out.push(".."),
            },
            other => out.push(other.as_os_str()),
        }
    }
    out
}

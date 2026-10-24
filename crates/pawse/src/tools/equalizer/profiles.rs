use std::io;
use std::path::{Path, PathBuf};

use equalizer::Band;

const EXTENSION: &str = "txt";
const MAX_NAME_BYTES: usize = 120;
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

pub fn folder() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join("pawse").join("equalizer"))
}

pub fn list(folder: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == EXTENSION))
        .filter_map(|path| path.file_stem()?.to_str().map(str::to_string))
        .filter(|name| !name.starts_with('.'))
        .collect();
    names.sort_by_cached_key(|name| name.to_lowercase());
    names
}

pub fn read(folder: &Path, name: &str) -> Result<Vec<Band>, String> {
    let text = std::fs::read_to_string(path(folder, name)).map_err(|e| e.to_string())?;
    equalizer::parse(&text).map_err(|e| e.to_string())
}

pub fn write(folder: &Path, name: &str, bands: &[Band]) -> io::Result<()> {
    std::fs::create_dir_all(folder)?;
    let tmp = folder.join(format!(".{name}.{EXTENSION}.tmp"));
    std::fs::write(&tmp, equalizer::write(bands))?;
    std::fs::rename(&tmp, path(folder, name))
}

pub fn delete(folder: &Path, name: &str) -> io::Result<()> {
    match std::fs::remove_file(path(folder, name)) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

pub fn same_name(a: &str, b: &str) -> bool {
    if cfg!(target_os = "linux") {
        a == b
    } else {
        a.to_lowercase() == b.to_lowercase()
    }
}

pub fn contains(names: &[String], name: &str) -> bool {
    names.iter().any(|existing| same_name(existing, name))
}

pub fn clean_name(raw: &str) -> Option<String> {
    let replaced: String = raw
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '-'
            } else {
                c
            }
        })
        .collect();
    let mut name = String::new();
    for c in replaced
        .trim_matches(|c: char| c == '.' || c.is_whitespace())
        .chars()
    {
        if name.len() + c.len_utf8() > MAX_NAME_BYTES {
            break;
        }
        name.push(c);
    }
    let mut name = name
        .trim_end_matches(|c: char| c == '.' || c.is_whitespace())
        .to_string();
    let stem = name.split('.').next().unwrap_or_default().trim_end();
    if RESERVED
        .iter()
        .any(|reserved| reserved.eq_ignore_ascii_case(stem))
    {
        name.push('_');
    }
    (!name.is_empty()).then_some(name)
}

pub fn normalized(bands: &[Band]) -> Vec<Band> {
    equalizer::parse(&equalizer::write(bands)).unwrap_or_else(|_| bands.to_vec())
}

fn path(folder: &Path, name: &str) -> PathBuf {
    folder.join(format!("{name}.{EXTENSION}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use equalizer::Kind;

    fn temp_folder(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("pawse-eq-profiles-{tag}-{}", std::process::id()))
    }

    #[test]
    fn names_lose_what_a_file_name_cannot_hold() {
        assert_eq!(
            clean_name("  HD 600 / warm  "),
            Some("HD 600 - warm".into())
        );
        assert_eq!(clean_name("..hidden"), Some("hidden".into()));
        assert_eq!(clean_name(" .. "), None);
        assert_eq!(clean_name("a:b*c?"), Some("a-b-c-".into()));
        assert_eq!(
            clean_name(&"x".repeat(200)).map(|n| n.len()),
            Some(MAX_NAME_BYTES)
        );
        assert!(clean_name(&"🎧".repeat(80)).unwrap().len() <= MAX_NAME_BYTES);
        assert_eq!(clean_name("nul"), Some("nul_".into()));
        assert_eq!(clean_name("Com1.txt"), Some("Com1.txt_".into()));
        assert_eq!(clean_name("Console"), Some("Console".into()));
    }

    #[test]
    fn deleting_a_missing_profile_is_not_an_error() {
        let folder = temp_folder("missing");
        assert!(delete(&folder, "gone").is_ok());
    }

    #[test]
    fn profiles_round_trip_through_their_folder() {
        let folder = temp_folder("round-trip");
        let _ = std::fs::remove_dir_all(&folder);
        let bands = normalized(&[
            Band::new(Kind::LowShelf, 105.0, 6.5, 0.7),
            Band::new(Kind::Peak, 3158.4, -1.84, 3.671),
        ]);
        write(&folder, "Warm", &bands).unwrap();
        write(&folder, "air", &bands[..1]).unwrap();
        std::fs::write(folder.join("notes.md"), "x").unwrap();
        assert_eq!(list(&folder), ["air", "Warm"]);
        assert_eq!(read(&folder, "Warm").unwrap(), bands);
        assert!(contains(&list(&folder), "warm"));
        delete(&folder, "air").unwrap();
        assert_eq!(list(&folder), ["Warm"]);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn normalized_bands_survive_a_save_unchanged() {
        let bands = normalized(&[Band::new(Kind::Peak, 1234.56, 2.25, 1.005)]);
        assert_eq!(normalized(&bands), bands);
    }
}

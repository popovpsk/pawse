use std::path::Path;

pub const SCHEME: &str = "pawse-source://";
const LEGACY_SCHEMES: [&str; 1] = ["subsonic://"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteRef {
    pub source_id: i64,
    pub key: String,
    pub suffix: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location<'a> {
    File(&'a Path),
    Remote(RemoteRef),
    Invalid,
}

pub fn locator(source_id: i64, key: &str, suffix: &str) -> String {
    format!("{SCHEME}{source_id}/{key}.{suffix}")
}

fn strip_scheme(path: &str) -> Option<&str> {
    std::iter::once(SCHEME)
        .chain(LEGACY_SCHEMES)
        .find_map(|scheme| path.strip_prefix(scheme))
}

pub fn is_remote(path: &str) -> bool {
    strip_scheme(path).is_some()
}

pub fn parse(path: &str) -> Option<RemoteRef> {
    let rest = strip_scheme(path)?;
    let (source, file) = rest.split_once('/')?;
    let (key, suffix) = file.rsplit_once('.')?;
    if key.is_empty() || suffix.is_empty() {
        return None;
    }
    Some(RemoteRef {
        source_id: source.parse().ok()?,
        key: key.to_string(),
        suffix: suffix.to_string(),
    })
}

pub fn canonical(path: &str) -> Option<String> {
    if path.starts_with(SCHEME) {
        return None;
    }
    let reference = parse(path)?;
    Some(locator(
        reference.source_id,
        &reference.key,
        &reference.suffix,
    ))
}

pub fn location(path: &str) -> Location<'_> {
    if !is_remote(path) {
        return Location::File(Path::new(path));
    }
    parse(path).map_or(Location::Invalid, Location::Remote)
}

pub fn local_file(path: &str) -> Option<&Path> {
    match location(path) {
        Location::File(file) => Some(file),
        Location::Remote(_) | Location::Invalid => None,
    }
}

pub fn suffix_for(suffix: Option<&str>, content_type: Option<&str>) -> String {
    if let Some(suffix) = suffix
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.len() <= 8 && s.chars().all(|c| c.is_ascii_alphanumeric()))
    {
        return suffix.to_ascii_lowercase();
    }
    let from_type = match content_type.unwrap_or("") {
        "audio/flac" | "audio/x-flac" => "flac",
        "audio/ogg" | "application/ogg" => "ogg",
        "audio/opus" => "opus",
        "audio/mp4" | "audio/x-m4a" | "audio/aac" => "m4a",
        "audio/wav" | "audio/x-wav" => "wav",
        _ => "mp3",
    };
    from_type.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locators_round_trip_and_keep_dots_in_the_key() {
        let path = locator(4, "a.b-7", "flac");
        assert!(is_remote(&path));
        assert_eq!(
            parse(&path),
            Some(RemoteRef {
                source_id: 4,
                key: "a.b-7".into(),
                suffix: "flac".into()
            })
        );
        assert_eq!(parse("/music/a.flac"), None);
    }

    #[test]
    fn locators_written_before_the_rename_still_parse() {
        let old = "subsonic://4/abc.flac";
        assert!(is_remote(old));
        assert_eq!(parse(old), parse(&locator(4, "abc", "flac")));
        assert!(locator(4, "abc", "flac").starts_with(SCHEME));
    }

    #[test]
    fn only_legacy_locators_are_rewritten() {
        assert_eq!(
            canonical("subsonic://4/a.b.flac"),
            Some(locator(4, "a.b", "flac"))
        );
        assert_eq!(canonical(&locator(4, "a", "flac")), None);
        assert_eq!(canonical("/music/a.flac"), None);
        assert_eq!(canonical("subsonic://broken"), None);
    }

    #[test]
    fn locations_tell_files_from_server_tracks_and_broken_locators() {
        assert_eq!(
            location("/music/a.flac"),
            Location::File(Path::new("/music/a.flac"))
        );
        assert_eq!(
            local_file("C:\\Music\\a.flac"),
            Some(Path::new("C:\\Music\\a.flac"))
        );
        assert!(matches!(
            location(&locator(1, "k", "mp3")),
            Location::Remote(_)
        ));
        assert_eq!(local_file(&locator(1, "k", "mp3")), None);
        for broken in [
            "pawse-source://x/k.mp3",
            "pawse-source://1/k",
            "subsonic://1/.mp3",
            "pawse-source://1/k.",
        ] {
            assert_eq!(location(broken), Location::Invalid, "{broken}");
            assert_eq!(local_file(broken), None, "{broken}");
        }
    }

    #[test]
    fn the_suffix_falls_back_to_the_content_type() {
        assert_eq!(suffix_for(Some("FLAC"), None), "flac");
        assert_eq!(suffix_for(None, Some("audio/ogg")), "ogg");
        assert_eq!(suffix_for(Some(""), Some("audio/mpeg")), "mp3");
        assert_eq!(suffix_for(Some("../x"), Some("audio/flac")), "flac");
        assert_eq!(suffix_for(Some("tar.gz"), None), "mp3");
    }
}

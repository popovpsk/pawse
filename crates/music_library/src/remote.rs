pub const SCHEME: &str = "subsonic://";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteRef {
    pub source_id: i64,
    pub key: String,
    pub suffix: String,
}

pub fn locator(source_id: i64, key: &str, suffix: &str) -> String {
    format!("{SCHEME}{source_id}/{key}.{suffix}")
}

pub fn is_remote(path: &str) -> bool {
    path.starts_with(SCHEME)
}

pub fn parse(path: &str) -> Option<RemoteRef> {
    let rest = path.strip_prefix(SCHEME)?;
    let (source, file) = rest.split_once('/')?;
    let (key, suffix) = file.rsplit_once('.')?;
    Some(RemoteRef {
        source_id: source.parse().ok()?,
        key: key.to_string(),
        suffix: suffix.to_string(),
    })
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
    fn the_suffix_falls_back_to_the_content_type() {
        assert_eq!(suffix_for(Some("FLAC"), None), "flac");
        assert_eq!(suffix_for(None, Some("audio/ogg")), "ogg");
        assert_eq!(suffix_for(Some(""), Some("audio/mpeg")), "mp3");
        assert_eq!(suffix_for(Some("../x"), Some("audio/flac")), "flac");
        assert_eq!(suffix_for(Some("tar.gz"), None), "mp3");
    }
}

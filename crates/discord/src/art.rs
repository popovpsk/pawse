use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use ureq::Agent;

pub struct ArtCache {
    path: PathBuf,
    agent: Agent,
    map: HashMap<String, String>,
}

impl ArtCache {
    pub fn load(path: PathBuf) -> Self {
        let map = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let agent = Agent::new_with_config(
            Agent::config_builder()
                .timeout_connect(Some(Duration::from_secs(10)))
                .timeout_recv_response(Some(Duration::from_secs(10)))
                .timeout_recv_body(Some(Duration::from_secs(10)))
                .build(),
        );
        Self { path, agent, map }
    }

    pub fn resolve(&mut self, artist: &str, album: Option<&str>) -> Option<String> {
        let album = album?;
        if artist.is_empty() || album.is_empty() {
            return None;
        }
        let key = format!("{artist}|{album}");
        if let Some(url) = self.map.get(&key) {
            return non_empty(url);
        }
        let found = match lookup(&self.agent, artist, album) {
            Ok(found) => found.unwrap_or_default(),
            Err(()) => return None,
        };
        self.map.insert(key, found.clone());
        self.persist();
        non_empty(&found)
    }

    fn persist(&self) {
        let Ok(json) = serde_json::to_string(&self.map) else {
            return;
        };
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let tmp = self.path.with_extension("json.tmp");
        if std::fs::write(&tmp, &json).is_ok() {
            let _ = std::fs::rename(&tmp, &self.path);
        }
    }
}

fn non_empty(url: &str) -> Option<String> {
    (!url.is_empty()).then(|| url.to_string())
}

fn lookup(agent: &Agent, artist: &str, album: &str) -> Result<Option<String>, ()> {
    let term = format!("{artist} {album}");
    let body = agent
        .get("https://itunes.apple.com/search")
        .query("term", &term)
        .query("entity", "album")
        .query("limit", "1")
        .call()
        .map_err(|_| ())?
        .body_mut()
        .read_to_string()
        .map_err(|_| ())?;
    parse_artwork(&body)
}

fn parse_artwork(body: &str) -> Result<Option<String>, ()> {
    let json: serde_json::Value = serde_json::from_str(body).map_err(|_| ())?;
    let Some(art) = json
        .get("results")
        .and_then(|r| r.get(0))
        .and_then(|a| a.get("artworkUrl100"))
        .and_then(|v| v.as_str())
    else {
        return Ok(None);
    };
    let upscaled = art.replace("100x100bb", "512x512bb");
    Ok((upscaled.len() <= 254).then_some(upscaled))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upscales_artwork_url() {
        let body = r#"{"results":[{"artworkUrl100":"https://is1.mzstatic.com/a/100x100bb.jpg"}]}"#;
        assert_eq!(
            parse_artwork(body),
            Ok(Some("https://is1.mzstatic.com/a/512x512bb.jpg".to_string()))
        );
    }

    #[test]
    fn empty_results_are_a_definitive_miss() {
        assert_eq!(parse_artwork(r#"{"results":[]}"#), Ok(None));
    }

    #[test]
    fn invalid_body_is_a_failure_not_a_miss() {
        assert_eq!(parse_artwork("<html>throttled</html>"), Err(()));
    }

    #[test]
    fn cache_hit_and_negative_hit_skip_network() {
        let path = std::env::temp_dir().join(format!(
            "pawse-discord-art-{}-{:?}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(
            &path,
            r#"{"A|Hit":"https://img/512x512bb.jpg","A|Miss":""}"#,
        )
        .unwrap();

        let mut cache = ArtCache::load(path.clone());
        assert_eq!(
            cache.resolve("A", Some("Hit")).as_deref(),
            Some("https://img/512x512bb.jpg")
        );
        assert_eq!(cache.resolve("A", Some("Miss")), None);
        assert_eq!(cache.resolve("A", None), None);

        let _ = std::fs::remove_file(&path);
    }
}

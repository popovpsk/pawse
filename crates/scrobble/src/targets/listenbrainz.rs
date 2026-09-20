use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};

use crate::target::{ScrobbleTarget, SubmitError, TargetId};
use crate::targets::audioscrobbler::LovedTrack;
use crate::targets::{agent, read};
use crate::{NowPlaying, Scrobble};

pub const DEFAULT_ROOT: &str = "https://api.listenbrainz.org";

const CLIENT: &str = "pawse";
const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const BATCH: usize = 50;

fn urlencode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn parse_feedback(body: &str) -> Result<(Vec<LovedTrack>, usize)> {
    let parsed: Value = serde_json::from_str(body)
        .with_context(|| format!("parse listenbrainz feedback: {body}"))?;
    let total = parsed
        .get("total_count")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    let tracks = parsed
        .get("feedback")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let meta = item.get("track_metadata")?;
                    let artist = meta.get("artist_name").and_then(Value::as_str)?.trim();
                    let title = meta.get("track_name").and_then(Value::as_str)?.trim();
                    if artist.is_empty() || title.is_empty() {
                        return None;
                    }
                    Some(LovedTrack {
                        artist: artist.to_string(),
                        title: title.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok((tracks, total))
}

pub struct ListenBrainzClient {
    agent: ureq::Agent,
    root: String,
    token: String,
}

impl ListenBrainzClient {
    pub fn new(root: String, token: String) -> Self {
        let root = root.trim().trim_end_matches('/').to_string();
        let root = if root.is_empty() {
            DEFAULT_ROOT.to_string()
        } else {
            root
        };
        Self {
            agent: agent(),
            root,
            token,
        }
    }

    pub fn loved_tracks(
        &self,
        user: &str,
        count: usize,
        offset: usize,
    ) -> Result<(Vec<LovedTrack>, usize)> {
        let url = format!(
            "{}/1/feedback/user/{}/get-feedback?score=1&metadata=true&count={count}&offset={offset}",
            self.root,
            urlencode(user)
        );
        let (status, body) = read(
            self.agent
                .get(&url)
                .header("Authorization", self.auth())
                .call(),
        )
        .map_err(|e| anyhow!("listenbrainz {e}"))?;
        if status >= 400 {
            return Err(anyhow!("listenbrainz feedback failed: http {status}"));
        }
        parse_feedback(&body)
    }

    pub fn validate(&self) -> Result<String> {
        let url = format!("{}/1/validate-token", self.root);
        let (status, body) = read(
            self.agent
                .get(&url)
                .header("Authorization", self.auth())
                .call(),
        )
        .map_err(|e| anyhow!("listenbrainz {e}"))?;
        if status == 401 {
            return Err(anyhow!("listenbrainz rejected the token"));
        }
        let parsed: Value =
            serde_json::from_str(&body).with_context(|| format!("parse listenbrainz: {body}"))?;
        if parsed.get("valid").and_then(Value::as_bool) != Some(true) {
            let message = parsed
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("token is not valid");
            return Err(anyhow!("listenbrainz: {message}"));
        }
        parsed
            .get("user_name")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| anyhow!("listenbrainz did not return a user name"))
    }

    fn auth(&self) -> String {
        format!("Token {}", self.token)
    }

    fn post(&self, payload: &Value) -> Result<(), SubmitError> {
        let url = format!("{}/1/submit-listens", self.root);
        let body = match serde_json::to_string(payload) {
            Ok(body) => body,
            Err(e) => return Err(SubmitError::Permanent(format!("serialize listens: {e}"))),
        };
        match read(
            self.agent
                .post(&url)
                .header("Authorization", self.auth())
                .header("Content-Type", "application/json")
                .send(&body),
        ) {
            Ok((status, body)) => match classify(status, &body) {
                Some(err) => Err(err),
                None => Ok(()),
            },
            Err(e) => Err(SubmitError::Transient(e)),
        }
    }
}

impl ScrobbleTarget for ListenBrainzClient {
    fn id(&self) -> TargetId {
        TargetId::ListenBrainz
    }

    fn max_batch(&self) -> usize {
        BATCH
    }

    fn now_playing(&self, np: &NowPlaying) -> Result<(), SubmitError> {
        self.post(&playing_now_payload(np))
    }

    fn submit(&self, items: &[Scrobble]) -> Result<(), SubmitError> {
        if items.is_empty() {
            return Ok(());
        }
        self.post(&listens_payload(items))
    }

    fn accepts_loves(&self) -> bool {
        false
    }

    fn love(&self, _artist: &str, _title: &str, _love: bool, _at: u64) -> Result<(), SubmitError> {
        Err(SubmitError::Unsupported)
    }
}

fn playing_now_payload(np: &NowPlaying) -> Value {
    let metadata = track_metadata(
        &np.artist,
        &np.title,
        np.album.as_deref(),
        np.track_number,
        np.duration_secs,
    );
    json!({
        "listen_type": "playing_now",
        "payload": [{ "track_metadata": metadata }],
    })
}

fn listens_payload(items: &[Scrobble]) -> Value {
    let payload: Vec<Value> = items
        .iter()
        .map(|item| {
            json!({
                "listened_at": item.timestamp,
                "track_metadata": track_metadata(
                    &item.artist,
                    &item.title,
                    item.album.as_deref(),
                    item.track_number,
                    item.duration_secs,
                ),
            })
        })
        .collect();
    let listen_type = if payload.len() == 1 {
        "single"
    } else {
        "import"
    };
    json!({ "listen_type": listen_type, "payload": payload })
}

fn track_metadata(
    artist: &str,
    title: &str,
    album: Option<&str>,
    track_number: Option<u32>,
    duration_secs: Option<u64>,
) -> Value {
    let mut additional = json!({
        "media_player": CLIENT,
        "submission_client": CLIENT,
        "submission_client_version": CLIENT_VERSION,
    });
    if let Some(map) = additional.as_object_mut() {
        if let Some(duration) = duration_secs.filter(|d| *d > 0) {
            map.insert("duration_ms".to_string(), json!(duration * 1000));
        }
        if let Some(track_number) = track_number {
            map.insert("tracknumber".to_string(), json!(track_number));
        }
    }
    let mut metadata = json!({
        "artist_name": artist,
        "track_name": title,
        "additional_info": additional,
    });
    if let (Some(map), Some(album)) = (metadata.as_object_mut(), album.filter(|a| !a.is_empty())) {
        map.insert("release_name".to_string(), json!(album));
    }
    metadata
}

fn classify(status: u16, body: &str) -> Option<SubmitError> {
    if status < 400 {
        return None;
    }
    let message = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            v.get("error")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| v.get("message").and_then(Value::as_str).map(str::to_owned))
        })
        .unwrap_or_else(|| format!("http {status}"));
    Some(match status {
        401 | 403 => SubmitError::Auth(message),
        400 | 404 | 413 => SubmitError::Permanent(message),
        _ => SubmitError::Transient(message),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_is_normalized_and_defaulted() {
        let client = ListenBrainzClient::new("  https://lb.example.org/  ".to_string(), "t".into());
        assert_eq!(client.root, "https://lb.example.org");
        let client = ListenBrainzClient::new(String::new(), "t".into());
        assert_eq!(client.root, DEFAULT_ROOT);
    }

    #[test]
    fn metadata_carries_duration_in_milliseconds_and_track_number() {
        let meta = track_metadata("A", "T", Some("Alb"), Some(4), Some(180));
        assert_eq!(meta["artist_name"], "A");
        assert_eq!(meta["release_name"], "Alb");
        assert_eq!(meta["additional_info"]["duration_ms"], 180_000);
        assert_eq!(meta["additional_info"]["tracknumber"], 4);
        assert_eq!(meta["additional_info"]["submission_client"], CLIENT);
    }

    #[test]
    fn metadata_omits_empty_optional_fields() {
        let meta = track_metadata("A", "T", None, None, Some(0));
        assert!(meta.get("release_name").is_none());
        assert!(meta["additional_info"].get("duration_ms").is_none());
        assert!(meta["additional_info"].get("tracknumber").is_none());
    }

    fn item(i: u64) -> Scrobble {
        Scrobble {
            artist: format!("A{i}"),
            title: format!("T{i}"),
            album: Some("Alb".to_string()),
            album_artist: None,
            track_number: Some(2),
            duration_secs: Some(180),
            timestamp: 1_700_000_000 + i,
        }
    }

    #[test]
    fn a_single_listen_is_submitted_as_single() {
        let payload = listens_payload(&[item(0)]);

        assert_eq!(payload["listen_type"], "single");
        assert_eq!(payload["payload"].as_array().unwrap().len(), 1);
        assert_eq!(payload["payload"][0]["listened_at"], 1_700_000_000u64);
        assert_eq!(payload["payload"][0]["track_metadata"]["track_name"], "T0");
        assert_eq!(payload["payload"][0]["track_metadata"]["artist_name"], "A0");
        assert_eq!(
            payload["payload"][0]["track_metadata"]["release_name"],
            "Alb"
        );
    }

    #[test]
    fn several_listens_are_submitted_as_import() {
        let payload = listens_payload(&[item(0), item(1)]);

        assert_eq!(payload["listen_type"], "import");
        assert_eq!(payload["payload"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn playing_now_carries_no_timestamp() {
        let np = NowPlaying {
            artist: "A".to_string(),
            title: "T".to_string(),
            album: None,
            album_artist: None,
            track_number: None,
            duration_secs: Some(180),
        };
        let payload = playing_now_payload(&np);

        assert_eq!(payload["listen_type"], "playing_now");
        assert!(payload["payload"][0].get("listened_at").is_none());
        assert_eq!(payload["payload"][0]["track_metadata"]["track_name"], "T");
    }

    #[test]
    fn metadata_reports_the_client_version() {
        let meta = track_metadata("A", "T", None, None, None);
        assert_eq!(
            meta["additional_info"]["submission_client_version"],
            CLIENT_VERSION
        );
    }

    #[test]
    fn status_maps_to_error_class() {
        assert!(matches!(
            classify(401, r#"{"error":"Invalid authorization token."}"#),
            Some(SubmitError::Auth(_))
        ));
        assert!(matches!(
            classify(400, r#"{"error":"bad listen"}"#),
            Some(SubmitError::Permanent(_))
        ));
        assert!(matches!(classify(429, ""), Some(SubmitError::Transient(_))));
        assert!(matches!(classify(503, ""), Some(SubmitError::Transient(_))));
        assert!(classify(200, r#"{"status":"ok"}"#).is_none());
    }
}

#[cfg(test)]
mod feedback_tests {
    use super::*;

    #[test]
    fn feedback_with_metadata_yields_loved_tracks() {
        let body = r#"{"count":2,"total_count":7,"offset":0,"feedback":[
            {"score":1,"recording_mbid":"m1","track_metadata":{"artist_name":"Tool","track_name":"Pneuma"}},
            {"score":1,"recording_mbid":"m2","track_metadata":{"artist_name":"Gojira","track_name":"Stranded"}}
        ]}"#;
        let (tracks, total) = parse_feedback(body).unwrap();
        assert_eq!(total, 7);
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].artist, "Tool");
        assert_eq!(tracks[1].title, "Stranded");
    }

    #[test]
    fn feedback_without_metadata_is_skipped_not_fatal() {
        let body = r#"{"count":2,"total_count":2,"offset":0,"feedback":[
            {"score":1,"recording_mbid":"m1"},
            {"score":1,"recording_mbid":"m2","track_metadata":{"artist_name":"Tool","track_name":"Pneuma"}}
        ]}"#;
        let (tracks, _) = parse_feedback(body).unwrap();
        assert_eq!(
            tracks.len(),
            1,
            "an entry we cannot match to a local track is skipped, the rest still import"
        );
    }

    #[test]
    fn an_empty_feedback_page_is_not_an_error() {
        let (tracks, total) =
            parse_feedback(r#"{"count":0,"total_count":0,"feedback":[]}"#).unwrap();
        assert!(tracks.is_empty());
        assert_eq!(total, 0);
    }

    #[test]
    fn a_user_name_with_spaces_is_escaped() {
        assert_eq!(urlencode("a b/c"), "a%20b%2Fc");
        assert_eq!(urlencode("plain-name_1.0~"), "plain-name_1.0~");
    }
}

use std::collections::BTreeMap;

use anyhow::{Context, Result, anyhow, bail};
use md5::{Digest, Md5};
use serde::Deserialize;

use crate::target::{ScrobbleTarget, SubmitError, TargetId};
use crate::targets::{agent, read};
use crate::{NowPlaying, Scrobble, Session};

const USER_AGENT: &str = "pawse-scrobbler";
const LIBREFM_KEY: &str = "pawse";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    pub id: TargetId,
    pub endpoint: String,
    pub auth_url: String,
    pub api_key: String,
    pub api_secret: String,
}

impl Profile {
    pub fn lastfm(api_key: String, api_secret: String) -> Self {
        Self {
            id: TargetId::Lastfm,
            endpoint: "https://ws.audioscrobbler.com/2.0/".to_string(),
            auth_url: "https://www.last.fm/api/auth/".to_string(),
            api_key,
            api_secret,
        }
    }

    pub fn librefm() -> Self {
        Self {
            id: TargetId::Librefm,
            endpoint: "https://libre.fm/2.0/".to_string(),
            auth_url: "https://libre.fm/api/auth/".to_string(),
            api_key: LIBREFM_KEY.to_string(),
            api_secret: LIBREFM_KEY.to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LovedTrack {
    pub artist: String,
    pub title: String,
}

pub struct AudioscrobblerClient {
    agent: ureq::Agent,
    profile: Profile,
    session: Option<String>,
}

impl AudioscrobblerClient {
    pub fn new(profile: Profile) -> Self {
        Self {
            agent: agent(),
            profile,
            session: None,
        }
    }

    pub fn with_session(mut self, session: String) -> Self {
        self.session = Some(session);
        self
    }

    pub fn auth_url(&self, token: &str) -> String {
        format!(
            "{}?api_key={}&token={token}",
            self.profile.auth_url, self.profile.api_key
        )
    }

    pub fn get_token(&self) -> Result<String> {
        let mut params = BTreeMap::new();
        params.insert("method".to_string(), "auth.getToken".to_string());
        let body = self.request(params, false, true)?;
        parse::<TokenResp>(&body).map(|t| t.token)
    }

    pub fn get_session(&self, token: &str) -> Result<Session, SessionError> {
        let mut params = BTreeMap::new();
        params.insert("method".to_string(), "auth.getSession".to_string());
        params.insert("token".to_string(), token.to_string());
        let body = self.request(params, true, true)?;
        if let Some(err) = session_error(&body) {
            return Err(err);
        }
        let resp = parse::<SessionResp>(&body)?;
        Ok(Session {
            key: resp.session.key,
            name: resp.session.name,
        })
    }

    pub fn loved_tracks(
        &self,
        user: &str,
        page: u32,
        limit: u32,
    ) -> Result<(Vec<LovedTrack>, u32)> {
        let mut params = BTreeMap::new();
        params.insert("method".to_string(), "user.getLovedTracks".to_string());
        params.insert("user".to_string(), user.to_string());
        params.insert("page".to_string(), page.to_string());
        params.insert("limit".to_string(), limit.to_string());
        let body = self.request(params, false, false)?;
        check_error(&body)?;
        parse_loved(&body)
    }

    fn request(
        &self,
        mut params: BTreeMap<String, String>,
        post: bool,
        signed: bool,
    ) -> Result<String> {
        params.insert("api_key".to_string(), self.profile.api_key.clone());
        let sig = signed.then(|| self.sign(&params));
        let (_, body) = self
            .send(params, post, sig)
            .map_err(|e| anyhow!("{} request failed: {e}", self.profile.id.label()))?;
        Ok(body)
    }

    fn send(
        &self,
        params: BTreeMap<String, String>,
        post: bool,
        sig: Option<String>,
    ) -> Result<(u16, String), String> {
        if post {
            let mut form: Vec<(&str, &str)> = params
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            if let Some(sig) = &sig {
                form.push(("api_sig", sig.as_str()));
            }
            form.push(("format", "json"));
            read(
                self.agent
                    .post(&self.profile.endpoint)
                    .header("User-Agent", USER_AGENT)
                    .send_form(form),
            )
        } else {
            let mut req = self
                .agent
                .get(&self.profile.endpoint)
                .header("User-Agent", USER_AGENT);
            for (k, v) in &params {
                req = req.query(k, v);
            }
            if let Some(sig) = &sig {
                req = req.query("api_sig", sig);
            }
            req = req.query("format", "json");
            read(req.call())
        }
    }

    fn sign(&self, params: &BTreeMap<String, String>) -> String {
        let mut hasher = Md5::new();
        for (k, v) in params {
            hasher.update(k.as_bytes());
            hasher.update(v.as_bytes());
        }
        hasher.update(self.profile.api_secret.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    fn session(&self) -> Result<&str, SubmitError> {
        self.session
            .as_deref()
            .ok_or_else(|| SubmitError::Auth("no session".to_string()))
    }

    fn post(&self, mut params: BTreeMap<String, String>) -> Result<(), SubmitError> {
        params.insert("api_key".to_string(), self.profile.api_key.clone());
        let sig = self.sign(&params);
        match self.send(params, true, Some(sig)) {
            Ok((status, body)) => match classify(status, &body) {
                Some(err) => Err(err),
                None => Ok(()),
            },
            Err(e) => Err(SubmitError::Transient(e)),
        }
    }
}

impl ScrobbleTarget for AudioscrobblerClient {
    fn id(&self) -> TargetId {
        self.profile.id
    }

    fn max_batch(&self) -> usize {
        50
    }

    fn now_playing(&self, np: &NowPlaying) -> Result<(), SubmitError> {
        self.post(now_playing_params(self.session()?, np))
    }

    fn submit(&self, items: &[Scrobble]) -> Result<(), SubmitError> {
        if items.is_empty() {
            return Ok(());
        }
        self.post(scrobble_params(self.session()?, items))
    }

    fn love(&self, artist: &str, title: &str, love: bool) -> Result<(), SubmitError> {
        self.post(love_params(self.session()?, artist, title, love))
    }
}

fn authed(session: &str, method: &str) -> BTreeMap<String, String> {
    let mut params = BTreeMap::new();
    params.insert("method".to_string(), method.to_string());
    params.insert("sk".to_string(), session.to_string());
    params
}

fn now_playing_params(session: &str, np: &NowPlaying) -> BTreeMap<String, String> {
    let mut params = authed(session, "track.updateNowPlaying");
    params.insert("artist".to_string(), np.artist.clone());
    params.insert("track".to_string(), np.title.clone());
    if let Some(album) = &np.album {
        params.insert("album".to_string(), album.clone());
    }
    if let Some(album_artist) = &np.album_artist {
        params.insert("albumArtist".to_string(), album_artist.clone());
    }
    if let Some(track_number) = np.track_number {
        params.insert("trackNumber".to_string(), track_number.to_string());
    }
    if let Some(duration) = np.duration_secs {
        params.insert("duration".to_string(), duration.to_string());
    }
    params
}

fn scrobble_params(session: &str, items: &[Scrobble]) -> BTreeMap<String, String> {
    let mut params = authed(session, "track.scrobble");
    for (i, item) in items.iter().enumerate() {
        params.insert(format!("artist[{i}]"), item.artist.clone());
        params.insert(format!("track[{i}]"), item.title.clone());
        params.insert(format!("timestamp[{i}]"), item.timestamp.to_string());
        if let Some(album) = &item.album {
            params.insert(format!("album[{i}]"), album.clone());
        }
        if let Some(album_artist) = &item.album_artist {
            params.insert(format!("albumArtist[{i}]"), album_artist.clone());
        }
        if let Some(track_number) = item.track_number {
            params.insert(format!("trackNumber[{i}]"), track_number.to_string());
        }
        if let Some(duration) = item.duration_secs {
            params.insert(format!("duration[{i}]"), duration.to_string());
        }
    }
    params
}

fn love_params(session: &str, artist: &str, title: &str, love: bool) -> BTreeMap<String, String> {
    let method = if love { "track.love" } else { "track.unlove" };
    let mut params = authed(session, method);
    params.insert("artist".to_string(), artist.to_string());
    params.insert("track".to_string(), title.to_string());
    params
}

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("the token has not been authorized yet")]
    NotAuthorized,
    #[error("the authorization token is no longer valid")]
    TokenExpired,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

fn session_error(body: &str) -> Option<SessionError> {
    let err = serde_json::from_str::<ErrorResp>(body).ok()?;
    Some(match err.error {
        14 => SessionError::NotAuthorized,
        4 | 15 => SessionError::TokenExpired,
        code => SessionError::Other(anyhow!("error {code}: {}", err.message)),
    })
}

fn classify(status: u16, body: &str) -> Option<SubmitError> {
    if let Ok(err) = serde_json::from_str::<ErrorResp>(body) {
        let message = format!("{} ({})", err.message, err.error);
        return Some(match err.error {
            4 | 9 | 10 | 13 | 14 | 15 | 26 => SubmitError::Auth(message),
            6 | 7 => SubmitError::Permanent(message),
            _ => SubmitError::Transient(message),
        });
    }
    if status >= 400 {
        return Some(SubmitError::Transient(format!("http {status}")));
    }
    None
}

fn parse<T: for<'de> Deserialize<'de>>(body: &str) -> Result<T> {
    check_error(body)?;
    serde_json::from_str(body).with_context(|| format!("parse response: {body}"))
}

fn check_error(body: &str) -> Result<()> {
    if let Ok(err) = serde_json::from_str::<ErrorResp>(body) {
        bail!("error {}: {}", err.error, err.message);
    }
    Ok(())
}

fn parse_loved(body: &str) -> Result<(Vec<LovedTrack>, u32)> {
    let root: serde_json::Value =
        serde_json::from_str(body).with_context(|| format!("parse loved tracks: {body}"))?;
    let loved = root
        .get("lovedtracks")
        .ok_or_else(|| anyhow!("no lovedtracks in response"))?;
    let total_pages = loved
        .get("@attr")
        .and_then(|a| a.get("totalPages"))
        .and_then(|p| match p {
            serde_json::Value::String(s) => s.parse().ok(),
            serde_json::Value::Number(n) => n.as_u64().map(|n| n as u32),
            _ => None,
        })
        .unwrap_or(1);
    let entries = match loved.get("track") {
        Some(serde_json::Value::Array(items)) => items.clone(),
        Some(single @ serde_json::Value::Object(_)) => vec![single.clone()],
        _ => Vec::new(),
    };
    let tracks = entries
        .iter()
        .filter_map(|t| {
            let title = t.get("name")?.as_str()?.to_string();
            let artist = t.get("artist")?.get("name")?.as_str()?.to_string();
            (!title.is_empty() && !artist.is_empty()).then_some(LovedTrack { artist, title })
        })
        .collect();
    Ok((tracks, total_pages))
}

#[derive(Deserialize)]
struct TokenResp {
    token: String,
}

#[derive(Deserialize)]
struct SessionResp {
    session: SessionInner,
}

#[derive(Deserialize)]
struct SessionInner {
    name: String,
    key: String,
}

#[derive(Deserialize)]
struct ErrorResp {
    error: u32,
    message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client() -> AudioscrobblerClient {
        AudioscrobblerClient::new(Profile::lastfm(
            "thekey".to_string(),
            "thesecret".to_string(),
        ))
    }

    #[test]
    fn signature_is_sorted_key_value_pairs_then_secret() {
        let mut params = BTreeMap::new();
        params.insert("method".to_string(), "auth.getToken".to_string());
        params.insert("api_key".to_string(), "thekey".to_string());

        let expected = format!(
            "{:x}",
            Md5::digest("api_keythekeymethodauth.getTokenthesecret".as_bytes())
        );
        assert_eq!(client().sign(&params), expected);
    }

    fn item(i: u32) -> Scrobble {
        Scrobble {
            artist: format!("A{i}"),
            title: format!("T{i}"),
            album: Some(format!("Alb{i}")),
            album_artist: Some(format!("AA{i}")),
            track_number: Some(i + 1),
            duration_secs: Some(180),
            timestamp: 100 + i as u64,
        }
    }

    #[test]
    fn scrobble_params_carry_every_indexed_field() {
        let params = scrobble_params("thesession", &[item(0), item(1)]);

        assert_eq!(params["method"], "track.scrobble");
        assert_eq!(params["sk"], "thesession");
        for i in 0..2u32 {
            assert_eq!(params[&format!("artist[{i}]")], format!("A{i}"));
            assert_eq!(params[&format!("track[{i}]")], format!("T{i}"));
            assert_eq!(params[&format!("album[{i}]")], format!("Alb{i}"));
            assert_eq!(params[&format!("albumArtist[{i}]")], format!("AA{i}"));
            assert_eq!(params[&format!("trackNumber[{i}]")], (i + 1).to_string());
            assert_eq!(params[&format!("timestamp[{i}]")], (100 + i).to_string());
            assert_eq!(params[&format!("duration[{i}]")], "180");
        }
        assert!(!params.contains_key("api_sig"));
        assert!(!params.contains_key("format"));
        assert!(!params.contains_key("api_key"));
    }

    #[test]
    fn scrobble_params_omit_absent_optional_fields() {
        let bare = Scrobble {
            artist: "A".to_string(),
            title: "T".to_string(),
            album: None,
            album_artist: None,
            track_number: None,
            duration_secs: None,
            timestamp: 7,
        };
        let params = scrobble_params("s", &[bare]);

        assert_eq!(params.len(), 5);
        assert!(!params.contains_key("album[0]"));
        assert!(!params.contains_key("albumArtist[0]"));
        assert!(!params.contains_key("trackNumber[0]"));
        assert!(!params.contains_key("duration[0]"));
    }

    #[test]
    fn now_playing_and_love_params_use_the_right_methods() {
        let np = NowPlaying {
            artist: "A".to_string(),
            title: "T".to_string(),
            album: Some("Alb".to_string()),
            album_artist: Some("AA".to_string()),
            track_number: Some(3),
            duration_secs: Some(200),
        };
        let params = now_playing_params("s", &np);
        assert_eq!(params["method"], "track.updateNowPlaying");
        assert_eq!(params["albumArtist"], "AA");
        assert_eq!(params["trackNumber"], "3");
        assert_eq!(params["duration"], "200");

        assert_eq!(love_params("s", "A", "T", true)["method"], "track.love");
        assert_eq!(love_params("s", "A", "T", false)["method"], "track.unlove");
    }

    #[test]
    fn a_client_without_a_session_reports_an_auth_error() {
        let err = client().submit(&[item(0)]).unwrap_err();
        assert!(matches!(err, SubmitError::Auth(_)));
    }

    #[test]
    fn batch_keys_sign_in_lexicographic_order() {
        let params = scrobble_params("s", &[item(0), item(1)]);
        let keys: Vec<&str> = params.keys().map(|k| k.as_str()).collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();

        assert_eq!(keys, sorted);
        assert_eq!(keys[0], "albumArtist[0]");
        assert!(keys.contains(&"album[0]"));
        assert!(keys.contains(&"trackNumber[1]"));
    }

    #[test]
    fn keys_and_signatures_never_drop_the_queue() {
        for code in [4, 9, 10, 13, 14, 15, 26] {
            let body = format!(r#"{{"error":{code},"message":"nope"}}"#);
            assert!(
                matches!(classify(200, &body), Some(SubmitError::Auth(_))),
                "code {code} must disable the target, not drop its items"
            );
        }
    }

    #[test]
    fn service_trouble_is_transient() {
        for code in [8, 11, 16, 29] {
            let body = format!(r#"{{"error":{code},"message":"later"}}"#);
            assert!(matches!(
                classify(200, &body),
                Some(SubmitError::Transient(_))
            ));
        }
    }

    #[test]
    fn an_unapproved_token_is_reported_separately() {
        let err = session_error(
            r#"{"error":14,"message":"Unauthorized Token - This token has not been authorized"}"#,
        )
        .unwrap();
        assert!(matches!(err, SessionError::NotAuthorized));
    }

    #[test]
    fn a_stale_token_asks_for_a_new_one() {
        for code in [4, 15] {
            let body = format!(r#"{{"error":{code},"message":"gone"}}"#);
            assert!(
                matches!(session_error(&body), Some(SessionError::TokenExpired)),
                "code {code} must send the user back through the browser"
            );
        }
    }

    #[test]
    fn other_session_failures_keep_their_text() {
        let err = session_error(r#"{"error":29,"message":"Rate limit exceeded"}"#).unwrap();
        assert!(matches!(err, SessionError::Other(_)));
        assert!(err.to_string().contains("Rate limit exceeded"));
    }

    #[test]
    fn a_successful_session_body_is_not_an_error() {
        assert!(
            session_error(r#"{"session":{"key":"k","name":"n"}}"#).is_none(),
            "a real session must not be mistaken for a failure"
        );
    }

    #[test]
    fn invalid_session_is_an_auth_error() {
        let err = classify(200, r#"{"error":9,"message":"Invalid session key"}"#).unwrap();
        assert!(matches!(err, SubmitError::Auth(_)));
    }

    #[test]
    fn rate_limit_is_transient() {
        let err = classify(200, r#"{"error":29,"message":"Rate limit exceeded"}"#).unwrap();
        assert!(matches!(err, SubmitError::Transient(_)));
    }

    #[test]
    fn invalid_parameters_are_permanent() {
        let err = classify(200, r#"{"error":6,"message":"Invalid parameters"}"#).unwrap();
        assert!(matches!(err, SubmitError::Permanent(_)));
    }

    #[test]
    fn server_error_without_a_json_body_is_transient() {
        let err = classify(503, "<html>nope</html>").unwrap();
        assert!(matches!(err, SubmitError::Transient(_)));
    }

    #[test]
    fn success_body_is_not_an_error() {
        assert!(classify(200, r#"{"scrobbles":{"@attr":{"accepted":1}}}"#).is_none());
    }

    #[test]
    fn loved_tracks_parse_array_and_single_object() {
        let many = r#"{"lovedtracks":{"track":[{"name":"T1","artist":{"name":"A1"}},{"name":"T2","artist":{"name":"A2"}}],"@attr":{"totalPages":"3"}}}"#;
        let (tracks, pages) = parse_loved(many).unwrap();
        assert_eq!(pages, 3);
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[1].artist, "A2");

        let one = r#"{"lovedtracks":{"track":{"name":"T","artist":{"name":"A"}},"@attr":{"totalPages":"1"}}}"#;
        let (tracks, pages) = parse_loved(one).unwrap();
        assert_eq!(pages, 1);
        assert_eq!(tracks.len(), 1);

        let none = r#"{"lovedtracks":{"@attr":{"totalPages":"1"}}}"#;
        let (tracks, _) = parse_loved(none).unwrap();
        assert!(tracks.is_empty());
    }

    #[test]
    fn librefm_profile_needs_no_configured_keys() {
        let profile = Profile::librefm();
        assert_eq!(profile.id, TargetId::Librefm);
        assert!(!profile.api_key.is_empty());
        assert!(profile.endpoint.starts_with("https://libre.fm"));
    }
}

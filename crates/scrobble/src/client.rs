use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use md5::{Digest, Md5};
use serde::Deserialize;

use crate::{NowPlaying, Scrobble, Session};

const ENDPOINT: &str = "https://ws.audioscrobbler.com/2.0/";
const USER_AGENT: &str = "pawse-scrobbler";

pub struct LastfmClient {
    agent: ureq::Agent,
    api_key: String,
    api_secret: String,
}

impl LastfmClient {
    pub fn new(api_key: String, api_secret: String) -> Self {
        let agent = ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .timeout_connect(Some(Duration::from_secs(15)))
                .timeout_recv_response(Some(Duration::from_secs(20)))
                .timeout_recv_body(Some(Duration::from_secs(20)))
                .http_status_as_error(false)
                .build(),
        );
        Self {
            agent,
            api_key,
            api_secret,
        }
    }

    pub fn get_token(&self) -> Result<String> {
        let mut params = BTreeMap::new();
        params.insert("method".to_string(), "auth.getToken".to_string());
        let body = self.call(params, false)?;
        parse::<TokenResp>(&body).map(|t| t.token)
    }

    pub fn get_session(&self, token: &str) -> Result<Session> {
        let mut params = BTreeMap::new();
        params.insert("method".to_string(), "auth.getSession".to_string());
        params.insert("token".to_string(), token.to_string());
        let body = self.call(params, true)?;
        let resp = parse::<SessionResp>(&body)?;
        Ok(Session {
            key: resp.session.key,
            name: resp.session.name,
        })
    }

    pub fn now_playing(&self, session: &str, np: &NowPlaying) -> Result<()> {
        let mut params = BTreeMap::new();
        params.insert("method".to_string(), "track.updateNowPlaying".to_string());
        params.insert("sk".to_string(), session.to_string());
        params.insert("artist".to_string(), np.artist.clone());
        params.insert("track".to_string(), np.title.clone());
        if let Some(album) = &np.album {
            params.insert("album".to_string(), album.clone());
        }
        if let Some(duration) = np.duration_secs {
            params.insert("duration".to_string(), duration.to_string());
        }
        let body = self.call(params, true)?;
        check_error(&body)
    }

    pub fn scrobble_batch(&self, session: &str, items: &[Scrobble]) -> Result<()> {
        if items.is_empty() {
            return Ok(());
        }
        let mut params = BTreeMap::new();
        params.insert("method".to_string(), "track.scrobble".to_string());
        params.insert("sk".to_string(), session.to_string());
        for (i, item) in items.iter().enumerate() {
            params.insert(format!("artist[{i}]"), item.artist.clone());
            params.insert(format!("track[{i}]"), item.title.clone());
            params.insert(format!("timestamp[{i}]"), item.timestamp.to_string());
            if let Some(album) = &item.album {
                params.insert(format!("album[{i}]"), album.clone());
            }
            if let Some(duration) = item.duration_secs {
                params.insert(format!("duration[{i}]"), duration.to_string());
            }
        }
        let body = self.call(params, true)?;
        check_error(&body)
    }

    pub fn love(&self, session: &str, artist: &str, track: &str, love: bool) -> Result<()> {
        let mut params = BTreeMap::new();
        let method = if love { "track.love" } else { "track.unlove" };
        params.insert("method".to_string(), method.to_string());
        params.insert("sk".to_string(), session.to_string());
        params.insert("artist".to_string(), artist.to_string());
        params.insert("track".to_string(), track.to_string());
        let body = self.call(params, true)?;
        check_error(&body)
    }

    fn call(&self, mut params: BTreeMap<String, String>, post: bool) -> Result<String> {
        params.insert("api_key".to_string(), self.api_key.clone());
        let sig = self.sign(&params);

        if post {
            let mut form: Vec<(&str, &str)> = params
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            form.push(("api_sig", sig.as_str()));
            form.push(("format", "json"));
            read(
                self.agent
                    .post(ENDPOINT)
                    .header("User-Agent", USER_AGENT)
                    .send_form(form),
            )
        } else {
            let mut req = self.agent.get(ENDPOINT).header("User-Agent", USER_AGENT);
            for (k, v) in &params {
                req = req.query(k, v);
            }
            req = req.query("api_sig", &sig).query("format", "json");
            read(req.call())
        }
    }

    fn sign(&self, params: &BTreeMap<String, String>) -> String {
        let mut hasher = Md5::new();
        for (k, v) in params {
            hasher.update(k.as_bytes());
            hasher.update(v.as_bytes());
        }
        hasher.update(self.api_secret.as_bytes());
        format!("{:x}", hasher.finalize())
    }
}

fn read(result: Result<ureq::http::Response<ureq::Body>, ureq::Error>) -> Result<String> {
    match result {
        Ok(mut resp) => resp
            .body_mut()
            .read_to_string()
            .context("read last.fm response"),
        Err(e) => Err(anyhow!("last.fm request failed: {e}")),
    }
}

fn parse<T: for<'de> Deserialize<'de>>(body: &str) -> Result<T> {
    check_error(body)?;
    serde_json::from_str(body).with_context(|| format!("parse last.fm response: {body}"))
}

fn check_error(body: &str) -> Result<()> {
    if let Ok(err) = serde_json::from_str::<ErrorResp>(body) {
        bail!("last.fm error {}: {}", err.error, err.message);
    }
    Ok(())
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

    #[test]
    fn signature_is_sorted_key_value_pairs_then_secret() {
        let client = LastfmClient::new("thekey".to_string(), "thesecret".to_string());
        let mut params = BTreeMap::new();
        params.insert("method".to_string(), "auth.getToken".to_string());
        params.insert("api_key".to_string(), "thekey".to_string());

        let expected = format!(
            "{:x}",
            Md5::digest("api_keythekeymethodauth.getTokenthesecret".as_bytes())
        );
        assert_eq!(client.sign(&params), expected);
    }

    #[test]
    fn check_error_surfaces_lastfm_error() {
        let body = r#"{"error":9,"message":"Invalid session key"}"#;
        let err = check_error(body).unwrap_err();
        assert!(err.to_string().contains("Invalid session key"));
    }

    #[test]
    fn check_error_passes_success_body() {
        let body = r#"{"scrobbles":{"@attr":{"accepted":1}}}"#;
        assert!(check_error(body).is_ok());
    }
}

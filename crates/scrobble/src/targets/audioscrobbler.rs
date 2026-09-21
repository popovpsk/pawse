use std::collections::BTreeMap;
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use md5::{Digest, Md5};
use percent_encoding::{NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use serde::Deserialize;

use crate::target::{ScrobbleTarget, SubmitError, TargetId};
use crate::targets::{agent, read};
use crate::{NowPlaying, Scrobble, Session};

const USER_AGENT: &str = "pawse-scrobbler";
const LIBREFM_KEY: &str = "pawse";
const CALLBACK_POLL_INTERVAL: Duration = Duration::from_millis(50);
pub const CALLBACK_TIMEOUT: Duration = Duration::from_secs(300);
const CALLBACK_PATH: &str = "/pawse-auth";
const CALLBACK_READ_TIMEOUT: Duration = Duration::from_secs(2);
const CALLBACK_DRAIN_TIMEOUT: Duration = Duration::from_millis(500);

const CALLBACK_SUCCESS_PAGE: &str = r#"<!doctype html>
<html>
<head><meta charset="utf-8"><title>pawse</title><style>
:root { color-scheme: light dark; }
* { box-sizing: border-box; }
html, body { height: 100%; margin: 0; }
body {
    display: flex;
    align-items: center;
    justify-content: center;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
    background: #fafafa;
    color: #16181d;
}
@media (prefers-color-scheme: dark) {
    body { background: #16181d; color: #f2f2f2; }
}
.card { text-align: center; padding: 48px; }
.check {
    width: 72px;
    height: 72px;
    border-radius: 50%;
    background: #22c55e;
    display: flex;
    align-items: center;
    justify-content: center;
    margin: 0 auto 24px;
}
h1 { font-size: 26px; font-weight: 600; margin: 0 0 8px; }
p { font-size: 16px; margin: 0; opacity: 0.65; }
</style></head>
<body>
<div class="card">
    <div class="check">
        <svg width="34" height="34" viewBox="0 0 24 24" fill="none" stroke="white"
            stroke-width="3" stroke-linecap="round" stroke-linejoin="round">
            <path d="M4 12l5 5L20 7"/>
        </svg>
    </div>
    <h1>You're signed in</h1>
    <p>You can close this tab and go back to pawse.</p>
</div>
</body>
</html>"#;

const CALLBACK_WAITING_PAGE: &str = r#"<!doctype html>
<html>
<head><meta charset="utf-8"><title>pawse</title><style>
:root { color-scheme: light dark; }
html, body { height: 100%; margin: 0; }
body {
    display: flex;
    align-items: center;
    justify-content: center;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
    background: #fafafa;
    color: #16181d;
}
@media (prefers-color-scheme: dark) {
    body { background: #16181d; color: #f2f2f2; }
}
p { font-size: 16px; opacity: 0.65; }
</style></head>
<body><p>Waiting for the pawse sign-in redirect…</p></body>
</html>"#;

const CALLBACK_DENIED_PAGE: &str = r#"<!doctype html>
<html>
<head><meta charset="utf-8"><title>pawse</title><style>
:root { color-scheme: light dark; }
html, body { height: 100%; margin: 0; }
body {
    display: flex;
    align-items: center;
    justify-content: center;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
    background: #fafafa;
    color: #16181d;
}
@media (prefers-color-scheme: dark) {
    body { background: #16181d; color: #f2f2f2; }
}
p { font-size: 16px; opacity: 0.65; }
</style></head>
<body><p>Sign-in was not approved. You can close this tab and try again in pawse.</p></body>
</html>"#;

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
    send_loves: bool,
}

impl AudioscrobblerClient {
    pub fn new(profile: Profile) -> Self {
        Self {
            agent: agent(),
            profile,
            session: None,
            send_loves: true,
        }
    }

    pub fn with_session(mut self, session: String) -> Self {
        self.session = Some(session);
        self
    }

    pub fn with_loves(mut self, send_loves: bool) -> Self {
        self.send_loves = send_loves;
        self
    }

    pub fn auth_url(&self, callback_port: u16, callback_state: &str) -> String {
        let callback =
            format!("http://127.0.0.1:{callback_port}{CALLBACK_PATH}?state={callback_state}");
        format!(
            "{}?api_key={}&cb={}",
            self.profile.auth_url,
            self.profile.api_key,
            percent_encode(&callback)
        )
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

    fn accepts_loves(&self) -> bool {
        self.send_loves
    }

    fn love(&self, artist: &str, title: &str, love: bool, _at: u64) -> Result<(), SubmitError> {
        if !self.send_loves {
            return Err(SubmitError::Unsupported);
        }
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

pub fn bind_callback_listener() -> std::io::Result<(TcpListener, u16, String)> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    Ok((listener, port, random_state()))
}

fn random_state() -> String {
    let a = RandomState::new().build_hasher().finish();
    let b = RandomState::new().build_hasher().finish();
    format!("{a:016x}{b:016x}")
}

pub fn wait_for_callback(
    listener: TcpListener,
    expected_state: &str,
    timeout: Duration,
    cancel: &AtomicBool,
) -> Result<String, SessionError> {
    let deadline = Instant::now() + timeout;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(SessionError::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(SessionError::TimedOut);
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_nonblocking(false);
                match handle_callback_connection(&mut stream, expected_state) {
                    CallbackHit::Token(token) => return Ok(token),
                    CallbackHit::Denied => return Err(SessionError::Denied),
                    CallbackHit::Stray => {}
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(CALLBACK_POLL_INTERVAL);
            }
            Err(e) => {
                return Err(SessionError::Other(anyhow!(
                    "callback listener failed: {e}"
                )));
            }
        }
    }
}

pub fn wait_for_callback_async(
    listener: TcpListener,
    expected_state: String,
    timeout: Duration,
    cancel: Arc<AtomicBool>,
) -> flume::Receiver<Result<String, SessionError>> {
    let (tx, rx) = flume::bounded(1);
    thread::spawn(move || {
        let result = wait_for_callback(listener, &expected_state, timeout, &cancel);
        let _ = tx.send(result);
    });
    rx
}

enum CallbackHit {
    Token(String),
    Denied,
    Stray,
}

fn handle_callback_connection(stream: &mut TcpStream, expected_state: &str) -> CallbackHit {
    let Some(request) = read_request_line(stream) else {
        return CallbackHit::Stray;
    };
    let Some(parsed) = parse_callback_request(&request) else {
        respond(stream, CALLBACK_WAITING_PAGE);
        return CallbackHit::Stray;
    };
    if parsed.path != CALLBACK_PATH || parsed.state.as_deref() != Some(expected_state) {
        respond(stream, CALLBACK_WAITING_PAGE);
        return CallbackHit::Stray;
    }
    match parsed.token {
        Some(token) => {
            respond(stream, CALLBACK_SUCCESS_PAGE);
            CallbackHit::Token(token)
        }
        None => {
            respond(stream, CALLBACK_DENIED_PAGE);
            CallbackHit::Denied
        }
    }
}

fn read_request_line(stream: &mut TcpStream) -> Option<String> {
    stream.set_read_timeout(Some(CALLBACK_READ_TIMEOUT)).ok()?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 512];
    for _ in 0..8 {
        let Ok(n) = stream.read(&mut chunk) else {
            break;
        };
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() >= 8192 {
            break;
        }
    }
    if buf.is_empty() {
        return None;
    }
    Some(String::from_utf8_lossy(&buf).into_owned())
}

fn respond(stream: &mut TcpStream, body: &str) {
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
    close_gracefully(stream);
}

fn close_gracefully(stream: &mut TcpStream) {
    let _ = stream.shutdown(Shutdown::Write);
    if stream
        .set_read_timeout(Some(CALLBACK_DRAIN_TIMEOUT))
        .is_err()
    {
        return;
    }
    let mut sink = [0u8; 512];
    let deadline = Instant::now() + CALLBACK_DRAIN_TIMEOUT;
    while Instant::now() < deadline {
        match stream.read(&mut sink) {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => break,
        }
    }
}

fn percent_encode(value: &str) -> String {
    utf8_percent_encode(value, NON_ALPHANUMERIC).to_string()
}

struct CallbackRequest {
    path: String,
    token: Option<String>,
    state: Option<String>,
}

fn parse_callback_request(request: &str) -> Option<CallbackRequest> {
    let request_line = request.lines().next()?;
    let target = request_line.split_whitespace().nth(1)?;
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut token = None;
    let mut state = None;
    for pair in query.split('&') {
        if let Some(value) = pair.strip_prefix("token=") {
            token = Some(percent_decode_str(value).decode_utf8_lossy().into_owned());
        } else if let Some(value) = pair.strip_prefix("state=") {
            state = Some(percent_decode_str(value).decode_utf8_lossy().into_owned());
        }
    }
    Some(CallbackRequest {
        path: path.to_string(),
        token,
        state,
    })
}

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("the token has not been authorized yet")]
    NotAuthorized,
    #[error("the authorization token is no longer valid")]
    TokenExpired,
    #[error("sign-in was cancelled")]
    Cancelled,
    #[error("timed out waiting for authorization")]
    TimedOut,
    #[error("sign-in was not approved")]
    Denied,
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

    #[test]
    fn callback_url_carries_an_encoded_callback_with_the_state() {
        let url = client().auth_url(12345, "thestate");
        let cb_encoded = url.split("cb=").nth(1).expect("cb param present");
        let cb = percent_decode_str(cb_encoded)
            .decode_utf8_lossy()
            .into_owned();
        assert_eq!(cb, "http://127.0.0.1:12345/pawse-auth?state=thestate");
        assert!(!url.contains("token="));
    }

    #[test]
    fn two_random_states_are_not_the_same() {
        assert_ne!(random_state(), random_state());
    }

    #[test]
    fn token_and_state_are_parsed_from_the_callback_request_line() {
        let request = "GET /pawse-auth?state=xyz&token=abc123 HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
        let parsed = parse_callback_request(request).unwrap();
        assert_eq!(parsed.path, "/pawse-auth");
        assert_eq!(parsed.token.as_deref(), Some("abc123"));
        assert_eq!(parsed.state.as_deref(), Some("xyz"));
    }

    #[test]
    fn token_is_found_even_when_not_the_first_query_param() {
        let request = "GET /pawse-auth?foo=bar&state=xyz&token=abc123 HTTP/1.1\r\n\r\n";
        let parsed = parse_callback_request(request).unwrap();
        assert_eq!(parsed.token.as_deref(), Some("abc123"));
    }

    #[test]
    fn a_denied_request_carries_the_state_but_no_token() {
        let request = "GET /pawse-auth?state=xyz HTTP/1.1\r\n\r\n";
        let parsed = parse_callback_request(request).unwrap();
        assert_eq!(parsed.state.as_deref(), Some("xyz"));
        assert!(parsed.token.is_none());
    }

    #[test]
    fn a_stray_request_never_matches_the_callback_path_or_state() {
        let favicon = parse_callback_request("GET /favicon.ico HTTP/1.1\r\n\r\n").unwrap();
        assert_ne!(favicon.path, CALLBACK_PATH);

        let request = "GET /pawse-auth?state=other&token=abc123 HTTP/1.1\r\n\r\n";
        let parsed = parse_callback_request(request).unwrap();
        assert_ne!(parsed.state.as_deref(), Some("expected"));

        assert!(parse_callback_request("").is_none());
    }

    fn browser_request(state: &str, token: Option<&str>) -> String {
        let query = match token {
            Some(t) => format!("state={state}&token={t}"),
            None => format!("state={state}"),
        };
        let padding = "x".repeat(900);
        format!(
            "GET {CALLBACK_PATH}?{query} HTTP/1.1\r\n\
             Host: 127.0.0.1\r\n\
             Connection: keep-alive\r\n\
             Upgrade-Insecure-Requests: 1\r\n\
             Referer: https://www.last.fm/\r\n\
             Accept-Encoding: gzip, deflate, br\r\n\
             Accept-Language: en-US,en;q=0.9\r\n\
             User-Agent: {padding}\r\n\r\n"
        )
    }

    #[test]
    fn the_success_page_reaches_the_browser_in_full() {
        let (listener, port, state) = bind_callback_listener().unwrap();
        let rx = wait_for_callback_async(
            listener,
            state.clone(),
            Duration::from_secs(5),
            Arc::new(AtomicBool::new(false)),
        );

        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        client
            .write_all(browser_request(&state, Some("abc123")).as_bytes())
            .unwrap();

        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();

        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(response.ends_with(CALLBACK_SUCCESS_PAGE));
        assert_eq!(
            response.len(),
            response.find("\r\n\r\n").unwrap() + 4 + CALLBACK_SUCCESS_PAGE.len()
        );
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap().unwrap(),
            "abc123"
        );
    }

    #[test]
    fn a_stray_request_is_answered_and_the_wait_continues() {
        let (listener, port, state) = bind_callback_listener().unwrap();
        let rx = wait_for_callback_async(
            listener,
            state.clone(),
            Duration::from_secs(5),
            Arc::new(AtomicBool::new(false)),
        );

        let mut stray = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stray
            .write_all(browser_request("wrong-state", Some("nope")).as_bytes())
            .unwrap();
        let mut stray_response = String::new();
        stray.read_to_string(&mut stray_response).unwrap();
        assert!(stray_response.ends_with(CALLBACK_WAITING_PAGE));

        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        client
            .write_all(browser_request(&state, Some("abc123")).as_bytes())
            .unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.ends_with(CALLBACK_SUCCESS_PAGE));
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap().unwrap(),
            "abc123"
        );
    }

    #[test]
    fn the_token_is_percent_decoded() {
        let request = "GET /pawse-auth?state=xyz&token=a%2Fb%3Dc HTTP/1.1\r\n\r\n";
        let parsed = parse_callback_request(request).unwrap();
        assert_eq!(parsed.token.as_deref(), Some("a/b=c"));
    }
}

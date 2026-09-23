use std::collections::HashSet;
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use md5::{Digest, Md5};
use serde::{Deserialize, Deserializer};

const API_VERSION: &str = "1.16.1";
const CLIENT_NAME: &str = "pawse";
const PAGE_SIZE: usize = 500;
const MAX_PAGES: usize = 10_000;
const MAX_COVER_BYTES: u64 = 32 * 1024 * 1024;
const ERROR_WRONG_CREDENTIALS: i64 = 40;
const ERROR_TOKEN_AUTH_UNSUPPORTED: i64 = 41;
const ERROR_NOT_AUTHORIZED: i64 = 50;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub url: String,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("server unreachable: {0}")]
    Transient(String),
    #[error("wrong username or password")]
    Auth,
    #[error("{0}")]
    Server(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Song {
    #[serde(deserialize_with = "id")]
    pub id: String,
    #[serde(default, deserialize_with = "lenient_text")]
    pub title: String,
    #[serde(default, deserialize_with = "lenient_opt_text")]
    pub album: Option<String>,
    #[serde(default, deserialize_with = "lenient_opt_text")]
    pub artist: Option<String>,
    #[serde(
        default,
        alias = "displayAlbumArtist",
        deserialize_with = "lenient_opt_text"
    )]
    pub album_artist: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub track: Option<u32>,
    #[serde(default, deserialize_with = "lenient")]
    pub disc_number: Option<u32>,
    #[serde(default, deserialize_with = "lenient")]
    pub year: Option<i32>,
    #[serde(default, deserialize_with = "lenient_opt_text")]
    pub genre: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub duration: Option<u64>,
    #[serde(default, deserialize_with = "lenient")]
    pub size: Option<u64>,
    #[serde(default, deserialize_with = "lenient_opt_text")]
    pub suffix: Option<String>,
    #[serde(default, deserialize_with = "lenient_opt_text")]
    pub content_type: Option<String>,
    #[serde(default, deserialize_with = "optional_id")]
    pub cover_art: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub bit_rate: Option<u32>,
    #[serde(default, deserialize_with = "lenient_opt_text")]
    pub path: Option<String>,
    #[serde(default, deserialize_with = "lenient_list")]
    pub artists: Vec<Named>,
    #[serde(default, deserialize_with = "lenient_list")]
    pub album_artists: Vec<Named>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Named {
    #[serde(default)]
    pub name: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawId {
    Text(String),
    Number(i64),
}

impl From<RawId> for String {
    fn from(raw: RawId) -> Self {
        match raw {
            RawId::Text(text) => text,
            RawId::Number(number) => number.to_string(),
        }
    }
}

fn id<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    Ok(RawId::deserialize(deserializer)?.into())
}

fn optional_id<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    Ok(Option::<RawId>::deserialize(deserializer)?.map(String::from))
}

fn lenient<'de, D: Deserializer<'de>, T: std::str::FromStr>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::Number(number) => number
            .as_i64()
            .map(|n| n.to_string())
            .or_else(|| number.as_f64().map(|f| (f.round() as i64).to_string()))
            .and_then(|text| text.parse().ok()),
        serde_json::Value::String(text) => text.trim().parse().ok(),
        _ => None,
    })
}

fn lenient_opt_text<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::String(text) => Some(text),
        serde_json::Value::Number(number) => Some(number.to_string()),
        _ => None,
    })
}

fn lenient_text<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    Ok(lenient_opt_text(deserializer)?.unwrap_or_default())
}

fn lenient_list<'de, D: Deserializer<'de>, T: for<'a> Deserialize<'a>>(
    deserializer: D,
) -> Result<Vec<T>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::Array(items) => items
            .into_iter()
            .filter_map(|item| serde_json::from_value(item).ok())
            .collect(),
        _ => Vec::new(),
    })
}

#[derive(Deserialize)]
struct AlbumRef {
    #[serde(deserialize_with = "id")]
    id: String,
}

pub struct Client {
    base: String,
    username: String,
    password: String,
    agent: ureq::Agent,
    legacy_auth: AtomicBool,
    page_size: usize,
}

enum Payload {
    Json(serde_json::Value),
    Binary(ureq::Body),
}

impl Client {
    pub fn new(config: &Config) -> Self {
        Self {
            base: config.url.trim().trim_end_matches('/').to_string(),
            username: config.username.clone(),
            password: config.password.clone(),
            agent: ureq::Agent::new_with_config(
                ureq::Agent::config_builder()
                    .timeout_connect(Some(Duration::from_secs(10)))
                    .timeout_recv_response(Some(Duration::from_secs(60)))
                    .timeout_recv_body(Some(Duration::from_secs(600)))
                    .http_status_as_error(false)
                    .build(),
            ),
            legacy_auth: AtomicBool::new(false),
            page_size: PAGE_SIZE,
        }
    }

    pub fn ping(&self) -> Result<(), Error> {
        self.json("ping", &[]).map(|_| ())
    }

    pub fn songs(&self) -> Result<Vec<Song>, Error> {
        let search_error = match self.search_all() {
            Ok(songs) if !songs.is_empty() => return Ok(songs),
            Ok(_) => None,
            Err(Error::Server(message)) => {
                log::info!("subsonic: search3 listing unavailable ({message}), walking albums");
                Some(Error::Server(message))
            }
            Err(e) => return Err(e),
        };
        let walked = self.walk_albums()?;
        match search_error {
            Some(error) if walked.is_empty() => Err(error),
            _ => Ok(walked),
        }
    }

    pub fn starred_songs(&self) -> Result<Vec<Song>, Error> {
        let response = self.json("getStarred2", &[])?;
        songs_at(&response, &["starred2", "song"])
    }

    pub fn cover_art(&self, cover_id: &str) -> Result<Vec<u8>, Error> {
        let body = self.binary("getCoverArt", &[("id", cover_id)])?;
        let mut bytes = Vec::new();
        body.into_reader()
            .take(MAX_COVER_BYTES)
            .read_to_end(&mut bytes)
            .map_err(|e| Error::Transient(e.to_string()))?;
        Ok(bytes)
    }

    pub fn download_to(&self, song_id: &str, dest: &Path) -> Result<(), Error> {
        let body = self.binary("download", &[("id", song_id)])?;
        let mut partial = dest.as_os_str().to_owned();
        partial.push(".partial");
        let partial = std::path::PathBuf::from(partial);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::Server(e.to_string()))?;
        }
        let written = std::fs::File::create(&partial)
            .and_then(|mut file| std::io::copy(&mut body.into_reader(), &mut file));
        if let Err(e) = written {
            let _ = std::fs::remove_file(&partial);
            return Err(Error::Transient(e.to_string()));
        }
        std::fs::rename(&partial, dest).map_err(|e| Error::Server(e.to_string()))
    }

    fn search_all(&self) -> Result<Vec<Song>, Error> {
        let mut songs = Vec::new();
        let mut seen = HashSet::new();
        let count = self.page_size.to_string();
        let mut offset = 0usize;
        for _ in 0..MAX_PAGES {
            let offset_text = offset.to_string();
            let response = self.json(
                "search3",
                &[
                    ("query", ""),
                    ("artistCount", "0"),
                    ("albumCount", "0"),
                    ("songCount", &count),
                    ("songOffset", &offset_text),
                ],
            )?;
            let batch = songs_at(&response, &["searchResult3", "song"])?;
            if batch.is_empty() {
                return Ok(songs);
            }
            offset += batch.len();
            let before = seen.len();
            for song in batch {
                if seen.insert(song.id.clone()) {
                    songs.push(song);
                }
            }
            if seen.len() == before {
                return Err(Error::Server("search3 ignores songOffset".into()));
            }
        }
        Err(Error::Server("search3 did not finish paging".into()))
    }

    fn walk_albums(&self) -> Result<Vec<Song>, Error> {
        let mut album_ids = Vec::new();
        let mut seen_albums = HashSet::new();
        let size = self.page_size.to_string();
        let mut offset = 0usize;
        for _ in 0..MAX_PAGES {
            let offset_text = offset.to_string();
            let response = self.json(
                "getAlbumList2",
                &[
                    ("type", "alphabeticalByName"),
                    ("size", &size),
                    ("offset", &offset_text),
                ],
            )?;
            let batch: Vec<AlbumRef> = list_at(&response, &["albumList2", "album"])?;
            if batch.is_empty() {
                break;
            }
            offset += batch.len();
            let before = seen_albums.len();
            for album in batch {
                if seen_albums.insert(album.id.clone()) {
                    album_ids.push(album.id);
                }
            }
            if seen_albums.len() == before {
                return Err(Error::Server("getAlbumList2 ignores offset".into()));
            }
        }
        let mut songs = Vec::new();
        let mut seen = HashSet::new();
        for album_id in album_ids {
            let response = self.json("getAlbum", &[("id", &album_id)])?;
            for song in songs_at(&response, &["album", "song"])? {
                if seen.insert(song.id.clone()) {
                    songs.push(song);
                }
            }
        }
        Ok(songs)
    }

    fn json(&self, method: &str, params: &[(&str, &str)]) -> Result<serde_json::Value, Error> {
        match self.request(method, params)? {
            Payload::Json(value) => Ok(value),
            Payload::Binary(_) => Err(Error::Server(format!("{method}: unexpected binary reply"))),
        }
    }

    fn binary(&self, method: &str, params: &[(&str, &str)]) -> Result<ureq::Body, Error> {
        match self.request(method, params)? {
            Payload::Binary(body) => Ok(body),
            Payload::Json(_) => Err(Error::Server(format!("{method}: unexpected reply"))),
        }
    }

    fn request(&self, method: &str, params: &[(&str, &str)]) -> Result<Payload, Error> {
        match self.request_once(method, params) {
            Err(ServerFailure::TokenAuthUnsupported)
                if !self.legacy_auth.load(Ordering::Relaxed) =>
            {
                self.legacy_auth.store(true, Ordering::Relaxed);
                self.request_once(method, params)
                    .map_err(ServerFailure::into_error)
            }
            other => other.map_err(ServerFailure::into_error),
        }
    }

    fn request_once(
        &self,
        method: &str,
        params: &[(&str, &str)],
    ) -> Result<Payload, ServerFailure> {
        let mut request = self
            .agent
            .get(format!("{}/rest/{method}", self.base))
            .query("u", &self.username)
            .query("v", API_VERSION)
            .query("c", CLIENT_NAME)
            .query("f", "json");
        if self.legacy_auth.load(Ordering::Relaxed) {
            request = request.query("p", format!("enc:{}", hex(self.password.as_bytes())));
        } else {
            let salt = salt();
            let token = hex(&Md5::digest(format!("{}{salt}", self.password).as_bytes()));
            request = request.query("t", token).query("s", salt);
        }
        for (key, value) in params {
            request = request.query(*key, *value);
        }
        let response = request
            .call()
            .map_err(|e| ServerFailure::Error(Error::Transient(redact(&e.to_string()))))?;
        let status = response.status().as_u16();
        if status == 401 || status == 403 {
            return Err(ServerFailure::Error(Error::Auth));
        }
        if status >= 500 {
            return Err(ServerFailure::Error(Error::Transient(format!(
                "HTTP {status}"
            ))));
        }
        if !(200..300).contains(&status) {
            return Err(ServerFailure::Error(Error::Server(format!(
                "HTTP {status}"
            ))));
        }
        let is_json = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("json") || value.contains("xml"));
        let body = response.into_body();
        if !is_json {
            return Ok(Payload::Binary(body));
        }
        let value: serde_json::Value = serde_json::from_reader(body.into_reader())
            .map_err(|e| ServerFailure::Error(Error::Server(format!("{method}: {e}"))))?;
        let inner = value.get("subsonic-response").cloned().ok_or_else(|| {
            ServerFailure::Error(Error::Server(format!("{method}: not a Subsonic server")))
        })?;
        if inner.get("status").and_then(|s| s.as_str()) == Some("ok") {
            return Ok(Payload::Json(inner));
        }
        let code = inner
            .pointer("/error/code")
            .and_then(|c| c.as_i64())
            .unwrap_or(0);
        let message = inner
            .pointer("/error/message")
            .and_then(|m| m.as_str())
            .unwrap_or("request failed")
            .to_string();
        Err(match code {
            ERROR_TOKEN_AUTH_UNSUPPORTED => ServerFailure::TokenAuthUnsupported,
            ERROR_WRONG_CREDENTIALS | ERROR_NOT_AUTHORIZED => ServerFailure::Error(Error::Auth),
            _ => ServerFailure::Error(Error::Server(format!("{method}: {message}"))),
        })
    }
}

enum ServerFailure {
    TokenAuthUnsupported,
    Error(Error),
}

impl ServerFailure {
    fn into_error(self) -> Error {
        match self {
            ServerFailure::TokenAuthUnsupported => Error::Auth,
            ServerFailure::Error(error) => error,
        }
    }
}

fn list_at<T: for<'de> Deserialize<'de>>(
    response: &serde_json::Value,
    path: &[&str],
) -> Result<Vec<T>, Error> {
    let mut node = response;
    for key in path {
        match node.get(key) {
            Some(next) => node = next,
            None => return Ok(Vec::new()),
        }
    }
    serde_json::from_value(node.clone()).map_err(|e| Error::Server(e.to_string()))
}

fn songs_at(response: &serde_json::Value, path: &[&str]) -> Result<Vec<Song>, Error> {
    list_at(response, path)
}

fn redact(message: &str) -> String {
    let mut out = String::with_capacity(message.len());
    let mut rest = message;
    while let Some(start) = rest.find('?') {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let end = tail
            .find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ')')
            .unwrap_or(tail.len());
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

fn salt() -> String {
    let a = RandomState::new().build_hasher().finish();
    format!("{a:016x}")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests;

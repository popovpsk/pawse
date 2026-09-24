use serde::Deserialize;
use server_http::{Status, lenient};
use std::collections::HashSet;

pub use server_http::RangeBody;

const CLIENT_NAME: &str = "Pawse";
const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const PAGE_SIZE: usize = 500;
const MAX_PAGES: usize = 10_000;
const MAX_COVER_BYTES: u64 = 32 * 1024 * 1024;
const COVER_WIDTH: &str = "1200";
const ITEM_FIELDS: &str = "MediaSources,Genres,Path";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub url: String,
    pub user_id: String,
    pub token: String,
    pub device_id: String,
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
#[serde(rename_all = "PascalCase")]
pub struct Item {
    #[serde(deserialize_with = "lenient::id")]
    pub id: String,
    #[serde(default, deserialize_with = "lenient::text")]
    pub name: String,
    #[serde(default, deserialize_with = "lenient::opt_text")]
    pub album: Option<String>,
    #[serde(default, deserialize_with = "lenient::opt_text")]
    pub album_id: Option<String>,
    #[serde(default, deserialize_with = "lenient::opt_text")]
    pub album_artist: Option<String>,
    #[serde(default, deserialize_with = "lenient::list")]
    pub artists: Vec<String>,
    #[serde(default, deserialize_with = "lenient::list")]
    pub album_artists: Vec<Named>,
    #[serde(default, deserialize_with = "lenient::number")]
    pub index_number: Option<u32>,
    #[serde(default, deserialize_with = "lenient::number")]
    pub parent_index_number: Option<u32>,
    #[serde(default, deserialize_with = "lenient::number")]
    pub production_year: Option<i32>,
    #[serde(default, deserialize_with = "lenient::list")]
    pub genres: Vec<String>,
    #[serde(default, deserialize_with = "lenient::number")]
    pub run_time_ticks: Option<u64>,
    #[serde(default, deserialize_with = "lenient::opt_text")]
    pub container: Option<String>,
    #[serde(default, deserialize_with = "lenient::opt_text")]
    pub path: Option<String>,
    #[serde(default, deserialize_with = "lenient::list")]
    pub media_sources: Vec<MediaSource>,
    #[serde(default, deserialize_with = "lenient::object")]
    pub image_tags: ImageTags,
    #[serde(default, deserialize_with = "lenient::opt_text")]
    pub album_primary_image_tag: Option<String>,
}

impl Item {
    pub fn size(&self) -> Option<u64> {
        self.media_sources.iter().find_map(|source| source.size)
    }

    pub fn bitrate(&self) -> Option<u32> {
        self.media_sources.iter().find_map(|source| source.bitrate)
    }

    pub fn file_path(&self) -> Option<&str> {
        self.path
            .as_deref()
            .or_else(|| self.media_sources.iter().find_map(|s| s.path.as_deref()))
    }

    pub fn duration_ms(&self) -> Option<u64> {
        self.run_time_ticks.map(|ticks| ticks / 10_000)
    }

    pub fn cover_key(&self) -> Option<&str> {
        match (&self.album_id, &self.album_primary_image_tag) {
            (Some(album), Some(_)) => Some(album),
            _ => self.image_tags.primary.as_ref().map(|_| self.id.as_str()),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Named {
    #[serde(default, deserialize_with = "lenient::text")]
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct MediaSource {
    #[serde(default, deserialize_with = "lenient::number")]
    pub size: Option<u64>,
    #[serde(default, deserialize_with = "lenient::number")]
    pub bitrate: Option<u32>,
    #[serde(default, deserialize_with = "lenient::opt_text")]
    pub container: Option<String>,
    #[serde(default, deserialize_with = "lenient::opt_text")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ImageTags {
    #[serde(default, deserialize_with = "lenient::opt_text")]
    pub primary: Option<String>,
}

pub fn authenticate(
    url: &str,
    username: &str,
    password: &str,
    device_id: &str,
) -> Result<Config, Error> {
    let base = normalize(url);
    let body = serde_json::json!({ "Username": username, "Pw": password }).to_string();
    let response = server_http::agent()
        .post(format!("{base}/Users/AuthenticateByName"))
        .header("Authorization", authorization(device_id, None))
        .header("Content-Type", "application/json")
        .send(body)
        .map_err(|e| Error::Transient(server_http::redact(&e.to_string())))?;
    let value = read_json(check_status(response)?, "AuthenticateByName")?;
    let token = value
        .get("AccessToken")
        .and_then(|t| t.as_str())
        .filter(|t| !t.is_empty());
    let user_id = value
        .pointer("/User/Id")
        .and_then(|u| u.as_str())
        .filter(|u| !u.is_empty());
    match (token, user_id) {
        (Some(token), Some(user_id)) => Ok(Config {
            url: base,
            user_id: user_id.to_string(),
            token: token.to_string(),
            device_id: device_id.to_string(),
        }),
        _ => Err(Error::Server("not a Jellyfin server".into())),
    }
}

pub struct Client {
    base: String,
    user_id: String,
    authorization: String,
    agent: ureq::Agent,
    page_size: usize,
}

impl Client {
    pub fn new(config: &Config) -> Self {
        Self {
            base: normalize(&config.url),
            user_id: config.user_id.clone(),
            authorization: authorization(&config.device_id, Some(&config.token)),
            agent: server_http::agent(),
            page_size: PAGE_SIZE,
        }
    }

    pub fn ping(&self) -> Result<(), Error> {
        let info = self.json("/System/Info/Public", &[])?;
        if info.get("Id").and_then(|id| id.as_str()).is_none() {
            return Err(Error::Server("not a Jellyfin server".into()));
        }
        self.json("/Users/Me", &[]).map(|_| ())
    }

    pub fn songs(&self) -> Result<Vec<Item>, Error> {
        self.audio_items(&[])
    }

    pub fn favorites(&self) -> Result<Vec<Item>, Error> {
        self.audio_items(&[("Filters", "IsFavorite")])
    }

    pub fn cover_art(&self, item_id: &str) -> Result<Vec<u8>, Error> {
        let response = self.get(
            &format!("/Items/{}/Images/Primary", encode(item_id)),
            &[("maxWidth", COVER_WIDTH)],
            None,
        )?;
        server_http::read_capped(response.into_body(), MAX_COVER_BYTES)
            .map_err(|e| Error::Transient(e.to_string()))
    }

    pub fn fetch_range(
        &self,
        item_id: &str,
        start: u64,
        end: Option<u64>,
    ) -> Result<RangeBody, Error> {
        let range = server_http::range_header(start, end);
        let response = self.get(
            &format!("/Audio/{}/stream", encode(item_id)),
            &[("static", "true")],
            Some(&range),
        )?;
        server_http::range_body(response)
            .map_err(|message| Error::Server(format!("stream: {message}")))
    }

    fn audio_items(&self, extra: &[(&str, &str)]) -> Result<Vec<Item>, Error> {
        let (items, total) = self.paged_items(extra)?;
        match total {
            Some(total) if items.len() < total => {
                log::info!(
                    "jellyfin: paging returned {} of {total} items, listing in one request",
                    items.len()
                );
                let (items, _, _) = self.page(extra, None)?;
                Ok(dedupe(items))
            }
            _ => Ok(items),
        }
    }

    fn paged_items(&self, extra: &[(&str, &str)]) -> Result<(Vec<Item>, Option<usize>), Error> {
        let mut items = Vec::new();
        let mut seen = HashSet::new();
        let mut offset = 0usize;
        let mut total = None;
        for _ in 0..MAX_PAGES {
            let (batch, received, reported) = self.page(extra, Some(offset))?;
            total = reported.or(total);
            if received == 0 {
                return Ok((items, total));
            }
            offset += received;
            let parsed = batch.len();
            let before = seen.len();
            for item in batch {
                if seen.insert(item.id.clone()) {
                    items.push(item);
                }
            }
            if parsed > 0 && seen.len() == before {
                return Err(Error::Server("Items ignores StartIndex".into()));
            }
            if total.is_some_and(|total| offset >= total) {
                return Ok((items, total));
            }
        }
        Err(Error::Server("Items did not finish paging".into()))
    }

    fn page(
        &self,
        extra: &[(&str, &str)],
        offset: Option<usize>,
    ) -> Result<(Vec<Item>, usize, Option<usize>), Error> {
        let start = offset.map(|offset| offset.to_string());
        let limit = self.page_size.to_string();
        let mut params: Vec<(&str, &str)> = vec![
            ("userId", &self.user_id),
            ("IncludeItemTypes", "Audio"),
            ("Recursive", "true"),
            ("Fields", ITEM_FIELDS),
            ("EnableImageTypes", "Primary"),
            ("SortBy", "SortName,Album,ParentIndexNumber,IndexNumber"),
            ("SortOrder", "Ascending"),
        ];
        if let Some(start) = &start {
            params.push(("StartIndex", start));
            params.push(("Limit", &limit));
        }
        params.extend_from_slice(extra);
        let response = self.json("/Items", &params)?;
        let total = response
            .get("TotalRecordCount")
            .and_then(|n| n.as_u64())
            .map(|n| n as usize);
        let raw = match response.get("Items") {
            Some(serde_json::Value::Array(raw)) => raw.as_slice(),
            Some(_) => return Err(Error::Server("Items: unexpected reply".into())),
            None => &[],
        };
        let items = raw
            .iter()
            .filter_map(|item| match serde_json::from_value::<Item>(item.clone()) {
                Ok(item) => Some(item),
                Err(e) => {
                    log::warn!("jellyfin: skipping an item: {e}");
                    None
                }
            })
            .collect();
        Ok((items, raw.len(), total))
    }

    fn json(&self, path: &str, params: &[(&str, &str)]) -> Result<serde_json::Value, Error> {
        read_json(self.get(path, params, None)?, path)
    }

    fn get(
        &self,
        path: &str,
        params: &[(&str, &str)],
        range: Option<&str>,
    ) -> Result<ureq::http::Response<ureq::Body>, Error> {
        let mut request = self
            .agent
            .get(format!("{}{path}", self.base))
            .header("Authorization", &self.authorization);
        for (key, value) in params {
            request = request.query(*key, *value);
        }
        if let Some(range) = range {
            request = server_http::with_range(request, range);
        }
        let response = request
            .call()
            .map_err(|e| Error::Transient(server_http::redact(&e.to_string())))?;
        check_status(response)
    }
}

fn dedupe(items: Vec<Item>) -> Vec<Item> {
    let mut seen = HashSet::new();
    items
        .into_iter()
        .filter(|item| seen.insert(item.id.clone()))
        .collect()
}

fn normalize(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

fn header_value(text: &str) -> String {
    text.chars()
        .filter(|c| *c != '"' && *c != ',' && !c.is_control())
        .collect()
}

fn device_name() -> String {
    ["COMPUTERNAME", "HOSTNAME"]
        .iter()
        .filter_map(|key| std::env::var(key).ok())
        .map(|name| header_value(name.trim()))
        .find(|name| !name.is_empty())
        .unwrap_or_else(|| CLIENT_NAME.to_string())
}

fn authorization(device_id: &str, token: Option<&str>) -> String {
    let mut value = format!(
        "MediaBrowser Client=\"{CLIENT_NAME}\", Device=\"{}\", DeviceId=\"{}\", Version=\"{CLIENT_VERSION}\"",
        device_name(),
        header_value(device_id)
    );
    if let Some(token) = token {
        value.push_str(&format!(", Token=\"{}\"", header_value(token)));
    }
    value
}

fn check_status(
    response: ureq::http::Response<ureq::Body>,
) -> Result<ureq::http::Response<ureq::Body>, Error> {
    let status = response.status().as_u16();
    match server_http::classify(status) {
        Status::Success => Ok(response),
        Status::Auth => Err(Error::Auth),
        Status::Transient => Err(Error::Transient(format!("HTTP {status}"))),
        Status::Failed => Err(Error::Server(format!("HTTP {status}"))),
    }
}

fn read_json(
    response: ureq::http::Response<ureq::Body>,
    what: &str,
) -> Result<serde_json::Value, Error> {
    let is_json = server_http::is_json(&response);
    if !is_json {
        return Err(Error::Server(format!("{what}: not a Jellyfin server")));
    }
    serde_json::from_reader(response.into_body().into_reader())
        .map_err(|e| Error::Server(format!("{what}: {e}")))
}

fn encode(segment: &str) -> String {
    segment
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;

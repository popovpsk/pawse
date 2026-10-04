use std::collections::{HashSet, VecDeque};
use std::io::Read;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use server_http::Status;

mod address;
mod device;
mod didl;
mod renderer;
mod soap;
mod ssdp;
mod xml;

pub use didl::{Item, Res};
pub use renderer::{
    PositionInfo, Renderer, TrackMetadata, TransportState, describe_renderer, discover_renderers,
    track_didl,
};
pub use server_http::RangeBody;
pub use ssdp::{Notification, NotifyListener};

use device::Description;
use soap::Failure;

pub const DISCOVER_TIMEOUT: Duration = Duration::from_secs(3);
const FIND_TIMEOUT: Duration = Duration::from_secs(3);
const FIND_BACKOFF: Duration = Duration::from_secs(30);
const DISCOVER_HTTP_TIMEOUT: Duration = Duration::from_secs(3);
const PAGE_SIZE: &str = "200";
const MAX_PAGES: usize = 10_000;
const MAX_CONTAINERS: usize = 100_000;
const MAX_XML_BYTES: u64 = 64 * 1024 * 1024;
const MAX_COVER_BYTES: u64 = 32 * 1024 * 1024;
const RANGE_NOT_SATISFIABLE: u16 = 416;
const AUDIO_CRITERIA: &str = "upnp:class derivedfrom \"object.item.audioItem\"";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub udn: String,
    pub location: String,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("server unreachable: {0}")]
    Transient(String),
    #[error("access denied")]
    Auth,
    #[error("{0}")]
    Server(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub udn: String,
    pub name: String,
    pub model: Option<String>,
    pub location: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Endpoint {
    location: String,
    control: String,
    service_type: String,
}

pub struct Client {
    agent: ureq::Agent,
    udn: String,
    location: Mutex<String>,
    endpoint: Mutex<Option<Endpoint>>,
    missed: Mutex<Option<Instant>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn read_text(body: ureq::Body) -> Result<String, Error> {
    let mut bytes = Vec::new();
    body.into_reader()
        .take(MAX_XML_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|e| Error::Transient(e.to_string()))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn fetch_description(agent: &ureq::Agent, location: &str) -> Result<Description, Error> {
    let response = agent
        .get(location)
        .call()
        .map_err(|e| Error::Transient(e.to_string()))?;
    let status = response.status().as_u16();
    match server_http::classify(status) {
        Status::Success => {}
        Status::Auth => return Err(Error::Auth),
        Status::Transient => return Err(Error::Transient(format!("HTTP {status}"))),
        Status::Failed => return Err(Error::Server(format!("HTTP {status}"))),
    }
    let text = read_text(response.into_body())?;
    device::parse(location, &text).map_err(Error::Server)
}

pub fn describe(address: &str) -> Result<Device, Error> {
    let agent = server_http::agent();
    let mut last = Error::Server("no address".into());
    for location in address::candidates(address) {
        match fetch_description(&agent, &location) {
            Ok(description) => return Ok(description.device),
            Err(error) => last = error,
        }
    }
    Err(last)
}

pub fn discover(timeout: Duration) -> Vec<Device> {
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(DISCOVER_HTTP_TIMEOUT))
            .http_status_as_error(false)
            .build(),
    );
    let mut devices: Vec<Device> = Vec::new();
    for reply in ssdp::search(&ssdp::TARGETS, &[], timeout, None) {
        if devices
            .iter()
            .any(|device| device.udn.eq_ignore_ascii_case(&reply.udn))
        {
            continue;
        }
        match fetch_description(&agent, &reply.location) {
            Ok(description)
                if !devices
                    .iter()
                    .any(|device| device.udn.eq_ignore_ascii_case(&description.device.udn)) =>
            {
                devices.push(description.device);
            }
            Ok(_) => {}
            Err(e) => log::debug!("SSDP: {} did not describe itself: {e}", reply.location),
        }
    }
    devices
}

impl Client {
    pub fn new(config: &Config) -> Self {
        Self {
            agent: server_http::agent(),
            udn: config.udn.clone(),
            location: Mutex::new(config.location.clone()),
            endpoint: Mutex::new(None),
            missed: Mutex::new(None),
        }
    }

    pub fn location(&self) -> String {
        lock(&self.location).clone()
    }

    pub fn ping(&self) -> Result<(), Error> {
        self.resolve(true).map(|_| ())
    }

    fn forget_endpoint(&self) {
        *lock(&self.endpoint) = None;
    }

    fn endpoint(&self) -> Result<Endpoint, Error> {
        if let Some(endpoint) = lock(&self.endpoint).clone() {
            return Ok(endpoint);
        }
        self.resolve(false)
    }

    fn ours(&self, description: &Description) -> bool {
        description.device.udn.eq_ignore_ascii_case(&self.udn)
    }

    fn resolve(&self, always_search: bool) -> Result<Endpoint, Error> {
        let location = lock(&self.location).clone();
        let description = match fetch_description(&self.agent, &location) {
            Ok(description) if self.ours(&description) => description,
            other => {
                self.forget_endpoint();
                let error = match other {
                    Ok(description) => Error::Transient(format!(
                        "another server answers at {location}: {}",
                        description.device.name
                    )),
                    Err(error) => error,
                };
                let Some(found) = self.find(always_search) else {
                    return Err(error);
                };
                let description = fetch_description(&self.agent, &found)?;
                if !self.ours(&description) {
                    return Err(error);
                }
                log::info!("DLNA: {} moved from {location} to {found}", self.udn);
                *lock(&self.location) = found;
                description
            }
        };
        let endpoint = Endpoint {
            location: description.device.location,
            control: description.control,
            service_type: description.service_type,
        };
        *lock(&self.endpoint) = Some(endpoint.clone());
        Ok(endpoint)
    }

    fn find(&self, always: bool) -> Option<String> {
        let recently_missed = lock(&self.missed).is_some_and(|at| at.elapsed() < FIND_BACKOFF);
        if recently_missed && !always {
            return None;
        }
        let targets = [self.udn.as_str(), ssdp::TARGETS[0], ssdp::TARGETS[1]];
        let found = ssdp::search(&targets, &[], FIND_TIMEOUT, Some(&self.udn))
            .into_iter()
            .find(|reply| reply.udn.eq_ignore_ascii_case(&self.udn))
            .map(|reply| reply.location);
        *lock(&self.missed) = found.is_none().then(Instant::now);
        found
    }

    fn call(
        &self,
        endpoint: &Endpoint,
        action: &str,
        args: &[(&str, &str)],
    ) -> Result<soap::Args, Failure> {
        let result = soap::call(
            &self.agent,
            &endpoint.control,
            &endpoint.service_type,
            action,
            args,
        );
        if let Err(Failure::Other(Error::Transient(_))) = &result {
            self.forget_endpoint();
        }
        result
    }

    pub fn items(&self) -> Result<Vec<Item>, Error> {
        let endpoint = self.endpoint()?;
        let items = match self.search_all(&endpoint)? {
            Some(items) => items,
            None => self.browse_all(&endpoint)?,
        };
        Ok(unique(items))
    }

    fn search_all(&self, endpoint: &Endpoint) -> Result<Option<Vec<Item>>, Error> {
        let caps = match self.call(endpoint, "GetSearchCapabilities", &[]) {
            Ok(out) => out.get("SearchCaps").cloned().unwrap_or_default(),
            Err(Failure::Upnp(..)) => return Ok(None),
            Err(Failure::Other(error)) => return Err(error),
        };
        if !searchable(&caps) {
            return Ok(None);
        }
        let listed = self.collect(endpoint, "Search", |start| {
            vec![
                ("ContainerID", "0".into()),
                ("SearchCriteria", AUDIO_CRITERIA.into()),
                ("Filter", "*".into()),
                ("StartingIndex", start),
                ("RequestedCount", PAGE_SIZE.into()),
                ("SortCriteria", String::new()),
            ]
        });
        match listed {
            Ok(page) if page.items.is_empty() => Ok(None),
            Ok(page) => Ok(Some(page.items)),
            Err(Failure::Upnp(code, message)) => {
                log::info!("DLNA: search failed ({code}: {message}), browsing instead");
                Ok(None)
            }
            Err(Failure::Other(error)) => Err(error),
        }
    }

    fn browse_all(&self, endpoint: &Endpoint) -> Result<Vec<Item>, Error> {
        let mut queue = VecDeque::from(["0".to_string()]);
        let mut visited: HashSet<String> = queue.iter().cloned().collect();
        let mut items = Vec::new();
        while let Some(id) = queue.pop_front() {
            let page = self
                .collect(endpoint, "Browse", |start| {
                    vec![
                        ("ObjectID", id.clone()),
                        ("BrowseFlag", "BrowseDirectChildren".into()),
                        ("Filter", "*".into()),
                        ("StartingIndex", start),
                        ("RequestedCount", PAGE_SIZE.into()),
                        ("SortCriteria", String::new()),
                    ]
                })
                .map_err(Failure::into_error)?;
            items.extend(page.items);
            for container in page.containers {
                if visited.insert(container.clone()) {
                    if visited.len() > MAX_CONTAINERS {
                        return Err(Error::Server("too many folders on the server".into()));
                    }
                    queue.push_back(container);
                }
            }
        }
        Ok(items)
    }

    fn collect(
        &self,
        endpoint: &Endpoint,
        action: &str,
        args: impl Fn(String) -> Vec<(&'static str, String)>,
    ) -> Result<didl::Page, Failure> {
        let mut page = didl::Page::default();
        let mut seen = HashSet::new();
        let mut start: u64 = 0;
        for _ in 0..MAX_PAGES {
            let owned = args(start.to_string());
            let borrowed: Vec<(&str, &str)> = owned
                .iter()
                .map(|(name, value)| (*name, value.as_str()))
                .collect();
            let out = self.call(endpoint, action, &borrowed)?;
            let chunk = didl::parse(
                out.get("Result").map_or("", String::as_str),
                &endpoint.location,
            )
            .map_err(|e| Failure::Other(Error::Server(e)))?;
            let number = |name: &str| out.get(name).and_then(|v| v.trim().parse::<u64>().ok());
            let returned = number("NumberReturned")
                .filter(|n| *n > 0)
                .unwrap_or(chunk.entries.len() as u64);
            if returned == 0 {
                return Ok(page);
            }
            let fresh = chunk
                .entries
                .iter()
                .filter(|id| seen.insert((*id).clone()))
                .count()
                > 0;
            if !chunk.entries.is_empty() && !fresh {
                return Err(Failure::Other(Error::Server(
                    "the server keeps returning the same page".into(),
                )));
            }
            start += returned;
            page.items.extend(chunk.items);
            page.containers.extend(chunk.containers);
            if number("TotalMatches").is_some_and(|total| total > 0 && start >= total) {
                return Ok(page);
            }
        }
        Err(Failure::Other(Error::Server(
            "the listing does not end".into(),
        )))
    }

    pub fn fetch_range(&self, key: &str, start: u64, end: Option<u64>) -> Result<RangeBody, Error> {
        let url = self.url(key)?;
        let response = self.get_range(&url, start, end)?;
        if response.status().as_u16() != RANGE_NOT_SATISFIABLE {
            return self.range_body(response);
        }
        let Some(end) = end else {
            return self.range_body(response);
        };
        let mut range = self.range_body(self.get_range(&url, start, None)?)?;
        if range.ranged {
            let wanted = end.saturating_sub(range.offset);
            range.body = Box::new(range.body.take(wanted));
        }
        Ok(range)
    }

    fn get_range(
        &self,
        url: &str,
        start: u64,
        end: Option<u64>,
    ) -> Result<ureq::http::Response<ureq::Body>, Error> {
        let request = self
            .agent
            .get(url)
            .header("getcontentFeatures.dlna.org", "1");
        server_http::with_range(request, &server_http::range_header(start, end))
            .call()
            .map_err(|e| self.unreachable(e))
    }

    fn range_body(&self, response: ureq::http::Response<ureq::Body>) -> Result<RangeBody, Error> {
        self.check(response.status().as_u16())?;
        server_http::range_body(response).map_err(Error::Server)
    }

    pub fn cover(&self, key: &str) -> Result<Vec<u8>, Error> {
        let url = self.url(key)?;
        let response = self
            .agent
            .get(&url)
            .call()
            .map_err(|e| self.unreachable(e))?;
        self.check(response.status().as_u16())?;
        server_http::read_capped(response.into_body(), MAX_COVER_BYTES)
            .map_err(|e| Error::Transient(e.to_string()))
    }

    fn url(&self, key: &str) -> Result<String, Error> {
        let endpoint = self.endpoint()?;
        address::url(&endpoint.location, key)
            .ok_or_else(|| Error::Server(format!("bad media address {key}")))
    }

    fn unreachable(&self, error: ureq::Error) -> Error {
        self.forget_endpoint();
        Error::Transient(error.to_string())
    }

    fn check(&self, status: u16) -> Result<(), Error> {
        match server_http::classify(status) {
            Status::Success => Ok(()),
            Status::Auth => Err(Error::Auth),
            Status::Transient => {
                self.forget_endpoint();
                Err(Error::Transient(format!("HTTP {status}")))
            }
            Status::Failed => Err(Error::Server(format!("HTTP {status}"))),
        }
    }
}

fn searchable(caps: &str) -> bool {
    let caps = caps.trim();
    caps == "*"
        || caps
            .split(',')
            .any(|cap| cap.trim().eq_ignore_ascii_case("upnp:class"))
}

fn unique(items: Vec<Item>) -> Vec<Item> {
    let mut keys = HashSet::new();
    items
        .into_iter()
        .filter(|item| item.pick().is_some_and(|res| keys.insert(res.key.clone())))
        .collect()
}

#[cfg(test)]
mod tests;

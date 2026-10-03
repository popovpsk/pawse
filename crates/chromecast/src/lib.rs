use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

mod connection;
mod proto;
#[cfg(any(test, feature = "test-support"))]
pub mod testing;

use connection::Connection;

pub const SERVICE_TYPE: &str = "_googlecast._tcp.local.";
pub const DEFAULT_MEDIA_RECEIVER: &str = "CC1AD845";

pub(crate) const SENDER: &str = "sender-pawse";
pub(crate) const RECEIVER: &str = "receiver-0";
pub(crate) const NS_CONNECTION: &str = "urn:x-cast:com.google.cast.tp.connection";
pub(crate) const NS_HEARTBEAT: &str = "urn:x-cast:com.google.cast.tp.heartbeat";
const NS_RECEIVER: &str = "urn:x-cast:com.google.cast.receiver";
const NS_MEDIA: &str = "urn:x-cast:com.google.cast.media";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const LOAD_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("{0}")]
    Io(String),
    #[error("the device did not answer in time")]
    Timeout,
    #[error("the device refused: {0}")]
    Rejected(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub model: Option<String>,
    pub address: SocketAddr,
}

impl Device {
    pub fn from_service<'a>(
        addresses: impl IntoIterator<Item = IpAddr>,
        port: u16,
        txt: impl Fn(&str) -> Option<&'a str>,
    ) -> Option<Self> {
        let mut addresses: Vec<IpAddr> = addresses.into_iter().collect();
        addresses.sort_by_key(|address| !address.is_ipv4());
        let address = *addresses.first()?;
        let id = txt("id").filter(|id| !id.is_empty())?.to_string();
        let name = txt("fn")
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or(&id)
            .to_string();
        let model = txt("md")
            .map(str::trim)
            .filter(|model| !model.is_empty())
            .map(str::to_string);
        let audio_capable = txt("ca")
            .and_then(|caps| caps.parse::<u32>().ok())
            .is_none_or(|caps| caps & 4 != 0);
        audio_capable.then(|| Device {
            id,
            name,
            model,
            address: SocketAddr::new(address, port),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Application {
    pub app_id: String,
    pub session_id: String,
    pub transport_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Volume {
    pub level: f32,
    pub muted: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReceiverStatus {
    pub applications: Vec<Application>,
    pub volume: Option<Volume>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerState {
    Idle,
    Playing,
    Paused,
    Buffering,
    Loading,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleReason {
    Finished,
    Cancelled,
    Interrupted,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MediaStatus {
    pub media_session_id: i64,
    pub state: PlayerState,
    pub idle_reason: Option<IdleReason>,
    pub current_time: Option<f64>,
    pub duration: Option<f64>,
    pub content_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Receiver(ReceiverStatus),
    Media(Vec<MediaStatus>),
    LoadFailed(String),
    TransportClosed(String),
    Disconnected(String),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Media {
    pub url: String,
    pub content_type: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub image: Option<String>,
    pub duration: Option<f64>,
}

fn application(value: &Value) -> Option<Application> {
    Some(Application {
        app_id: value["appId"].as_str()?.to_string(),
        session_id: value["sessionId"].as_str()?.to_string(),
        transport_id: value["transportId"].as_str()?.to_string(),
    })
}

fn receiver_status(payload: &Value) -> ReceiverStatus {
    let status = &payload["status"];
    ReceiverStatus {
        applications: status["applications"]
            .as_array()
            .map(|apps| apps.iter().filter_map(application).collect())
            .unwrap_or_default(),
        volume: status["volume"]["level"].as_f64().map(|level| Volume {
            level: level as f32,
            muted: status["volume"]["muted"].as_bool().unwrap_or(false),
        }),
    }
}

fn media_status(value: &Value) -> Option<MediaStatus> {
    let state = match value["playerState"].as_str()? {
        "IDLE" => PlayerState::Idle,
        "PLAYING" => PlayerState::Playing,
        "PAUSED" => PlayerState::Paused,
        "BUFFERING" => PlayerState::Buffering,
        "LOADING" => PlayerState::Loading,
        _ => return None,
    };
    let idle_reason = value["idleReason"]
        .as_str()
        .and_then(|reason| match reason {
            "FINISHED" => Some(IdleReason::Finished),
            "CANCELLED" => Some(IdleReason::Cancelled),
            "INTERRUPTED" => Some(IdleReason::Interrupted),
            "ERROR" => Some(IdleReason::Error),
            _ => None,
        });
    Some(MediaStatus {
        media_session_id: value["mediaSessionId"].as_i64()?,
        state,
        idle_reason,
        current_time: value["currentTime"].as_f64(),
        duration: value["media"]["duration"].as_f64(),
        content_id: value["media"]["contentId"].as_str().map(str::to_string),
    })
}

fn media_statuses(payload: &Value) -> Vec<MediaStatus> {
    payload["status"]
        .as_array()
        .map(|all| all.iter().filter_map(media_status).collect())
        .unwrap_or_default()
}

fn failure(payload: &Value) -> Option<String> {
    let kind = payload["type"].as_str()?;
    matches!(
        kind,
        "LOAD_FAILED"
            | "LOAD_CANCELLED"
            | "INVALID_REQUEST"
            | "LAUNCH_ERROR"
            | "INVALID_PLAYER_STATE"
    )
    .then(|| match payload["reason"].as_str() {
        Some(reason) => format!("{kind} ({reason})"),
        None => kind.to_string(),
    })
}

impl Event {
    pub(crate) fn from_payload(payload: &Value) -> Option<Self> {
        match payload["type"].as_str()? {
            "RECEIVER_STATUS" => Some(Event::Receiver(receiver_status(payload))),
            "MEDIA_STATUS" => Some(Event::Media(media_statuses(payload))),
            _ => failure(payload).map(Event::LoadFailed),
        }
    }
}

fn load_payload(app: &Application, media: &Media, autoplay: bool, start: f64) -> Value {
    let mut metadata = json!({"metadataType": 3});
    if let Some(title) = &media.title {
        metadata["title"] = json!(title);
    }
    if let Some(artist) = &media.artist {
        metadata["artist"] = json!(artist);
    }
    if let Some(album) = &media.album {
        metadata["albumName"] = json!(album);
    }
    if let Some(image) = &media.image {
        metadata["images"] = json!([{ "url": image }]);
    }
    let mut info = json!({
        "contentId": media.url,
        "contentUrl": media.url,
        "streamType": "BUFFERED",
        "contentType": media.content_type,
        "metadata": metadata,
    });
    if let Some(duration) = media.duration {
        info["duration"] = json!(duration);
    }
    json!({
        "type": "LOAD",
        "sessionId": app.session_id,
        "media": info,
        "autoplay": autoplay,
        "currentTime": start,
    })
}

pub struct Client {
    connection: Connection,
    next_request: AtomicU32,
}

impl Client {
    pub fn connect(address: SocketAddr) -> Result<(Self, flume::Receiver<Event>), Error> {
        let (events, receiver) = flume::unbounded();
        let connection = Connection::open(address, events)?;
        Ok((
            Self {
                connection,
                next_request: AtomicU32::new(1),
            },
            receiver,
        ))
    }

    pub fn is_closed(&self) -> bool {
        self.connection.is_closed()
    }

    fn request(
        &self,
        destination: &str,
        namespace: &str,
        mut payload: Value,
        timeout: Duration,
    ) -> Result<Value, Error> {
        if self.connection.is_closed() {
            return Err(Error::Io("the connection to the device is closed".into()));
        }
        let id = self.next_request.fetch_add(1, Ordering::Relaxed);
        payload["requestId"] = json!(id);
        let (sender, answer) = flume::bounded(1);
        self.connection
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id, sender);
        self.connection.send(destination, namespace, payload);
        let reply = answer.recv_timeout(timeout);
        self.connection
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&id);
        match reply {
            Ok(reply) => match failure(&reply) {
                Some(reason) => Err(Error::Rejected(reason)),
                None => Ok(reply),
            },
            Err(flume::RecvTimeoutError::Timeout) => Err(Error::Timeout),
            Err(flume::RecvTimeoutError::Disconnected) => {
                Err(Error::Io("the connection to the device is closed".into()))
            }
        }
    }

    pub fn receiver_status(&self) -> Result<ReceiverStatus, Error> {
        let reply = self.request(
            RECEIVER,
            NS_RECEIVER,
            json!({"type": "GET_STATUS"}),
            REQUEST_TIMEOUT,
        )?;
        Ok(receiver_status(&reply))
    }

    pub fn launch(&self, app_id: &str) -> Result<Application, Error> {
        if let Some(app) = self
            .receiver_status()?
            .applications
            .into_iter()
            .find(|app| app.app_id == app_id)
        {
            self.join(&app);
            return Ok(app);
        }
        let reply = self.request(
            RECEIVER,
            NS_RECEIVER,
            json!({"type": "LAUNCH", "appId": app_id}),
            LOAD_TIMEOUT,
        )?;
        let app = receiver_status(&reply)
            .applications
            .into_iter()
            .find(|app| app.app_id == app_id)
            .ok_or_else(|| Error::Rejected("the receiver app did not start".into()))?;
        self.join(&app);
        Ok(app)
    }

    pub fn join(&self, app: &Application) {
        self.connection
            .send(&app.transport_id, NS_CONNECTION, json!({"type": "CONNECT"}));
    }

    pub fn load(
        &self,
        app: &Application,
        media: &Media,
        autoplay: bool,
        start: f64,
    ) -> Result<Option<MediaStatus>, Error> {
        let payload = load_payload(app, media, autoplay, start);
        let reply = self.request(&app.transport_id, NS_MEDIA, payload, LOAD_TIMEOUT)?;
        Ok(media_statuses(&reply).into_iter().next())
    }

    fn media_command(
        &self,
        app: &Application,
        media_session_id: i64,
        kind: &str,
        extra: Option<(&str, Value)>,
    ) -> Result<(), Error> {
        let mut payload = json!({"type": kind, "mediaSessionId": media_session_id});
        if let Some((key, value)) = extra {
            payload[key] = value;
        }
        self.request(&app.transport_id, NS_MEDIA, payload, REQUEST_TIMEOUT)
            .map(|_| ())
    }

    pub fn play(&self, app: &Application, media_session_id: i64) -> Result<(), Error> {
        self.media_command(app, media_session_id, "PLAY", None)
    }

    pub fn pause(&self, app: &Application, media_session_id: i64) -> Result<(), Error> {
        self.media_command(app, media_session_id, "PAUSE", None)
    }

    pub fn stop(&self, app: &Application, media_session_id: i64) -> Result<(), Error> {
        self.media_command(app, media_session_id, "STOP", None)
    }

    pub fn seek(
        &self,
        app: &Application,
        media_session_id: i64,
        seconds: f64,
    ) -> Result<(), Error> {
        self.media_command(
            app,
            media_session_id,
            "SEEK",
            Some(("currentTime", json!(seconds))),
        )
    }

    pub fn media_status(&self, app: &Application) -> Result<Vec<MediaStatus>, Error> {
        let reply = self.request(
            &app.transport_id,
            NS_MEDIA,
            json!({"type": "GET_STATUS"}),
            REQUEST_TIMEOUT,
        )?;
        Ok(media_statuses(&reply))
    }

    pub fn set_volume(&self, level: f32) -> Result<(), Error> {
        self.request(
            RECEIVER,
            NS_RECEIVER,
            json!({"type": "SET_VOLUME", "volume": {"level": level.clamp(0.0, 1.0)}}),
            REQUEST_TIMEOUT,
        )
        .map(|_| ())
    }

    pub fn stop_app(&self, app: &Application) -> Result<(), Error> {
        self.request(
            RECEIVER,
            NS_RECEIVER,
            json!({"type": "STOP", "sessionId": app.session_id}),
            REQUEST_TIMEOUT,
        )
        .map(|_| ())
    }
}

#[cfg(test)]
mod tests;

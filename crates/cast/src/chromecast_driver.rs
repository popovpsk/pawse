use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use audio_decoder::Codec;
use chromecast::{
    Application, Client, DEFAULT_MEDIA_RECEIVER, Event, IdleReason, Media, MediaStatus, PlayerState,
};

use crate::media::{Accepts, Probe, chromecast_mime};
use crate::pcm::Container;
use crate::session::{Driver, Loading, State, Status};

pub(crate) struct ChromecastDriver {
    client: Client,
    events: flume::Receiver<Event>,
    app: Application,
    peer: IpAddr,
    media_session: Option<i64>,
    content: Option<String>,
    volume: Option<f32>,
    lost: Option<String>,
    load_error: Option<String>,
}

impl ChromecastDriver {
    pub fn connect(address: SocketAddr) -> Result<Self, String> {
        let (client, events) = Client::connect(address).map_err(|e| e.to_string())?;
        let volume = client
            .receiver_status()
            .ok()
            .and_then(|status| status.volume)
            .map(|volume| volume.level);
        let app = client
            .launch(DEFAULT_MEDIA_RECEIVER)
            .map_err(|e| e.to_string())?;
        while events.try_recv().is_ok() {}
        Ok(Self {
            client,
            events,
            app,
            peer: address.ip(),
            media_session: None,
            content: None,
            volume,
            lost: None,
            load_error: None,
        })
    }

    fn drain_events(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Disconnected(reason) => self.lost = Some(reason),
                Event::TransportClosed(transport) if transport == self.app.transport_id => {
                    self.lost = Some("the receiver app was closed".into());
                }
                Event::TransportClosed(_) => {}
                Event::Receiver(status) => {
                    if let Some(volume) = status.volume {
                        self.volume = Some(volume.level);
                    }
                    let ours = status
                        .applications
                        .iter()
                        .any(|app| app.session_id == self.app.session_id);
                    if !ours {
                        self.lost = Some("another app took over the device".into());
                    }
                }
                Event::Media(statuses) => {
                    if let Some(status) = self.ours(&statuses) {
                        self.media_session = Some(status.media_session_id);
                    }
                }
                Event::LoadFailed(reason) => self.load_error = Some(reason),
            }
        }
    }

    fn ours<'a>(&self, statuses: &'a [MediaStatus]) -> Option<&'a MediaStatus> {
        statuses
            .iter()
            .find(|status| match (&status.content_id, &self.content) {
                (Some(id), Some(ours)) => id == ours,
                _ => true,
            })
    }

    fn session(&self) -> Result<i64, String> {
        self.media_session
            .ok_or_else(|| "nothing is loaded on the device".to_string())
    }
}

impl Accepts for ChromecastDriver {
    fn original(&self, codec: Codec, extension: &str, probe: &Probe) -> Option<String> {
        chromecast_mime(codec, extension, probe).map(str::to_string)
    }

    fn pcm(&self) -> Container {
        Container::Wav
    }

    fn pcm_limits(&self) -> (u32, u16) {
        (96_000, 2)
    }

    fn wants_seek_table(&self) -> bool {
        true
    }
}

impl Driver for ChromecastDriver {
    fn peer(&self) -> IpAddr {
        self.peer
    }

    fn load(&mut self, loading: &Loading) -> Result<(), String> {
        self.drain_events();
        self.load_error = None;
        let media = Media {
            url: loading.url.to_string(),
            content_type: loading.mime.to_string(),
            title: Some(loading.info.title.clone()).filter(|title| !title.is_empty()),
            artist: loading.info.artist.clone(),
            album: loading.info.album.clone(),
            image: loading.cover_url.map(str::to_string),
            duration: loading.duration.map(|d| d.as_secs_f64()),
        };
        self.content = Some(media.url.clone());
        let status = self
            .client
            .load(
                &self.app,
                &media,
                loading.autoplay,
                loading.start.as_secs_f64(),
            )
            .map_err(|e| e.to_string())?;
        self.media_session = status.map(|status| status.media_session_id);
        Ok(())
    }

    fn play(&mut self) -> Result<(), String> {
        let session = self.session()?;
        self.client
            .play(&self.app, session)
            .map_err(|e| e.to_string())
    }

    fn pause(&mut self) -> Result<(), String> {
        let session = self.session()?;
        self.client
            .pause(&self.app, session)
            .map_err(|e| e.to_string())
    }

    fn seek(&mut self, position: Duration) -> Result<(), String> {
        let session = self.session()?;
        self.client
            .seek(&self.app, session, position.as_secs_f64())
            .map_err(|e| e.to_string())
    }

    fn stop(&mut self) -> Result<(), String> {
        let Some(session) = self.media_session.take() else {
            return Ok(());
        };
        self.content = None;
        self.client
            .stop(&self.app, session)
            .map_err(|e| e.to_string())
    }

    fn set_volume(&mut self, volume: f32) -> Result<(), String> {
        self.volume = Some(volume);
        self.client.set_volume(volume).map_err(|e| e.to_string())
    }

    fn status(&mut self) -> Result<Status, String> {
        self.drain_events();
        let statuses = self
            .client
            .media_status(&self.app)
            .map_err(|e| e.to_string())?;
        self.drain_events();
        let Some(status) = self.ours(&statuses).cloned() else {
            let state = self.content.is_some().then_some(State::Idle);
            return Ok(Status {
                state,
                volume: self.volume,
                error: self.load_error.take(),
                ..Status::default()
            });
        };
        self.media_session = Some(status.media_session_id);
        let state = match status.state {
            PlayerState::Playing => State::Playing,
            PlayerState::Paused => State::Paused,
            PlayerState::Buffering => State::Buffering,
            PlayerState::Loading => State::Loading,
            PlayerState::Idle => match status.idle_reason {
                Some(IdleReason::Finished) => State::Finished,
                Some(IdleReason::Error) => State::Failed,
                _ => State::Idle,
            },
        };
        Ok(Status {
            state: Some(state),
            position: status.current_time.map(Duration::from_secs_f64),
            duration: status
                .duration
                .filter(|d| d.is_finite() && *d > 0.0)
                .map(Duration::from_secs_f64),
            volume: self.volume,
            error: self.load_error.take(),
        })
    }

    fn lost(&mut self) -> Option<String> {
        self.drain_events();
        if self.lost.is_none() && self.client.is_closed() {
            self.lost = Some("the connection to the device closed".into());
        }
        self.lost.clone()
    }

    fn volume(&mut self) -> Option<f32> {
        self.volume
    }

    fn seeks_on_load(&self) -> bool {
        true
    }

    fn close(&mut self) {
        if self.lost.is_none() {
            let _ = self.client.stop_app(&self.app);
        }
    }
}

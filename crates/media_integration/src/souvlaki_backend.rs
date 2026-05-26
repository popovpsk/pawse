use std::cell::RefCell;
use std::ffi::c_void;
use std::time::Duration;

use flume::Sender;
use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, PlatformConfig};

use crate::{MediaCommand, MediaPlaybackState, NowPlayingInfo, SystemMediaIntegration};

/// Shared implementation for Windows (System Media Transport Controls) and
/// Linux (MPRIS over D-Bus), backed by the `souvlaki` crate.
///
/// `MediaControls` is `!Send`/`!Sync`, so this lives on the GPUI main thread.
/// Only the `attach` callback crosses threads, and it merely forwards into the
/// command channel.
pub struct SouvlakiIntegration {
    controls: RefCell<MediaControls>,
}

impl SouvlakiIntegration {
    pub fn new(sender: Sender<MediaCommand>, hwnd: Option<*mut c_void>) -> Option<Self> {
        let config = PlatformConfig {
            dbus_name: "pawse",
            display_name: "Pawse",
            hwnd,
        };

        let mut controls = match MediaControls::new(config) {
            Ok(controls) => controls,
            Err(err) => {
                eprintln!("media_integration: failed to create media controls: {err:?}");
                return None;
            }
        };

        let attach_result = controls.attach(move |event| {
            if let Some(command) = map_event(event) {
                let _ = sender.send(command);
            }
        });
        if let Err(err) = attach_result {
            eprintln!("media_integration: failed to attach media control handler: {err:?}");
            return None;
        }

        Some(Self {
            controls: RefCell::new(controls),
        })
    }
}

fn map_event(event: MediaControlEvent) -> Option<MediaCommand> {
    match event {
        MediaControlEvent::Play => Some(MediaCommand::Play),
        MediaControlEvent::Pause => Some(MediaCommand::Pause),
        MediaControlEvent::Toggle => Some(MediaCommand::TogglePlayPause),
        MediaControlEvent::Next => Some(MediaCommand::Next),
        MediaControlEvent::Previous => Some(MediaCommand::Previous),
        MediaControlEvent::SetPosition(position) => {
            Some(MediaCommand::Seek(position.0.as_secs_f64()))
        }
        _ => None,
    }
}

fn playback(state: MediaPlaybackState, progress: Option<f64>) -> MediaPlayback {
    let progress = progress
        .filter(|secs| *secs >= 0.0)
        .map(|secs| souvlaki::MediaPosition(Duration::from_secs_f64(secs)));
    match state {
        MediaPlaybackState::Playing => MediaPlayback::Playing { progress },
        MediaPlaybackState::Paused => MediaPlayback::Paused { progress },
        MediaPlaybackState::Stopped => MediaPlayback::Stopped,
    }
}

impl SystemMediaIntegration for SouvlakiIntegration {
    fn update_now_playing(&self, info: NowPlayingInfo) {
        let cover_url = info
            .artwork_path
            .as_ref()
            .map(|path| format!("file://{}", path.to_string_lossy()));
        let duration =
            (info.duration_secs > 0.0).then(|| Duration::from_secs_f64(info.duration_secs));

        let metadata = MediaMetadata {
            title: (!info.title.is_empty()).then_some(info.title.as_str()),
            artist: (!info.artist.is_empty()).then_some(info.artist.as_str()),
            album: (!info.album.is_empty()).then_some(info.album.as_str()),
            cover_url: cover_url.as_deref(),
            duration,
        };

        if let Err(err) = self.controls.borrow_mut().set_metadata(metadata) {
            eprintln!("media_integration: failed to set metadata: {err:?}");
        }
    }

    fn set_playback_state(&self, state: MediaPlaybackState) {
        if let Err(err) = self
            .controls
            .borrow_mut()
            .set_playback(playback(state, None))
        {
            eprintln!("media_integration: failed to set playback state: {err:?}");
        }
    }

    fn update_position(&self, elapsed_secs: f64, state: MediaPlaybackState) {
        if let Err(err) = self
            .controls
            .borrow_mut()
            .set_playback(playback(state, Some(elapsed_secs)))
        {
            eprintln!("media_integration: failed to update position: {err:?}");
        }
    }
}

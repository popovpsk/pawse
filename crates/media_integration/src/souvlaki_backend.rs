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
                log::warn!("media_integration: failed to create media controls: {err:?}");
                return None;
            }
        };

        let attach_result = controls.attach(move |event| {
            if let Some(command) = map_event(event) {
                let _ = sender.send(command);
            }
        });
        if let Err(err) = attach_result {
            log::warn!("media_integration: failed to attach media control handler: {err:?}");
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
        .filter(|secs| secs.is_finite() && *secs >= 0.0)
        .map(|secs| souvlaki::MediaPosition(Duration::from_secs_f64(secs)));
    match state {
        MediaPlaybackState::Playing => MediaPlayback::Playing { progress },
        MediaPlaybackState::Paused => MediaPlayback::Paused { progress },
        MediaPlaybackState::Stopped => MediaPlayback::Stopped,
    }
}

impl SystemMediaIntegration for SouvlakiIntegration {
    fn update_now_playing(&self, info: NowPlayingInfo, _state: MediaPlaybackState) {
        let cover_url = info
            .artwork_path
            .as_ref()
            .map(|path| format!("file://{}", path.to_string_lossy()));
        let duration = (info.duration_secs.is_finite() && info.duration_secs > 0.0)
            .then(|| Duration::from_secs_f64(info.duration_secs));

        let metadata = MediaMetadata {
            title: (!info.title.is_empty()).then_some(info.title.as_str()),
            artist: (!info.artist.is_empty()).then_some(info.artist.as_str()),
            album: (!info.album.is_empty()).then_some(info.album.as_str()),
            cover_url: cover_url.as_deref(),
            duration,
        };

        if let Err(err) = self.controls.borrow_mut().set_metadata(metadata) {
            log::warn!("media_integration: failed to set metadata: {err:?}");
        }
    }

    fn set_playback_state(&self, state: MediaPlaybackState) {
        if let Err(err) = self
            .controls
            .borrow_mut()
            .set_playback(playback(state, None))
        {
            log::warn!("media_integration: failed to set playback state: {err:?}");
        }
    }

    fn update_position(&self, elapsed_secs: f64, state: MediaPlaybackState) {
        if let Err(err) = self
            .controls
            .borrow_mut()
            .set_playback(playback(state, Some(elapsed_secs)))
        {
            log::warn!("media_integration: failed to update position: {err:?}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use souvlaki::{MediaControlEvent, MediaPosition, SeekDirection};

    #[test]
    fn map_event_maps_transport_commands() {
        assert_eq!(map_event(MediaControlEvent::Play), Some(MediaCommand::Play));
        assert_eq!(
            map_event(MediaControlEvent::Pause),
            Some(MediaCommand::Pause)
        );
        assert_eq!(
            map_event(MediaControlEvent::Toggle),
            Some(MediaCommand::TogglePlayPause)
        );
        assert_eq!(map_event(MediaControlEvent::Next), Some(MediaCommand::Next));
        assert_eq!(
            map_event(MediaControlEvent::Previous),
            Some(MediaCommand::Previous)
        );
    }

    #[test]
    fn map_event_converts_set_position_to_seek_seconds() {
        let event = MediaControlEvent::SetPosition(MediaPosition(Duration::from_millis(2500)));
        assert_eq!(map_event(event), Some(MediaCommand::Seek(2.5)));
    }

    #[test]
    fn map_event_ignores_unsupported_events() {
        assert_eq!(map_event(MediaControlEvent::Stop), None);
        assert_eq!(map_event(MediaControlEvent::Raise), None);
        assert_eq!(map_event(MediaControlEvent::Quit), None);
        assert_eq!(map_event(MediaControlEvent::SetVolume(0.5)), None);
        assert_eq!(
            map_event(MediaControlEvent::Seek(SeekDirection::Forward)),
            None
        );
    }

    #[test]
    fn playback_carries_non_negative_progress() {
        assert_eq!(
            playback(MediaPlaybackState::Playing, Some(12.5)),
            MediaPlayback::Playing {
                progress: Some(MediaPosition(Duration::from_secs_f64(12.5)))
            }
        );
        assert_eq!(
            playback(MediaPlaybackState::Paused, Some(3.0)),
            MediaPlayback::Paused {
                progress: Some(MediaPosition(Duration::from_secs_f64(3.0)))
            }
        );
    }

    #[test]
    fn playback_drops_invalid_progress() {
        for secs in [
            Some(-1.0),
            Some(f64::NAN),
            Some(f64::INFINITY),
            Some(f64::NEG_INFINITY),
            None,
        ] {
            assert_eq!(
                playback(MediaPlaybackState::Playing, secs),
                MediaPlayback::Playing { progress: None },
                "progress {secs:?} should be dropped"
            );
        }
    }

    #[test]
    fn playback_stopped_ignores_progress() {
        assert_eq!(
            playback(MediaPlaybackState::Stopped, Some(9.0)),
            MediaPlayback::Stopped
        );
    }
}

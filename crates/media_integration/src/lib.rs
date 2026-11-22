#![cfg_attr(not(target_os = "macos"), forbid(unsafe_code))]

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::rc::Rc;

use flume::Sender;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(any(target_os = "linux", test))]
mod mpris;
#[cfg(target_os = "windows")]
mod souvlaki_backend;

/// Snapshot of the currently playing track for system media widgets.
#[derive(Debug, Clone, Default)]
pub struct NowPlayingInfo {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub artwork_path: Option<PathBuf>,
    pub duration_secs: f64,
    pub elapsed_secs: Option<f64>,
}

/// Playback state exposed to the OS media controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaPlaybackState {
    Playing,
    Paused,
    Stopped,
}

/// Commands the OS can send to the player.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MediaCommand {
    Play,
    Pause,
    TogglePlayPause,
    Next,
    Previous,
    Seek(f64),
    SetVolume(f64),
}

/// Platform-agnostic interface for system media integration.
///
/// All implementations are expected to be used on the main thread only
/// (GPUI's requirement for AppKit interop).
pub trait SystemMediaIntegration {
    /// Update the metadata shown in the system Now Playing widget.
    fn update_now_playing(&self, info: NowPlayingInfo, state: MediaPlaybackState);

    /// Update the playback state shown in system UI.
    fn set_playback_state(&self, state: MediaPlaybackState);

    /// Update just the elapsed playback time in the Now Playing widget.
    /// Called frequently (e.g. every second) so it should be lightweight.
    fn update_position(&self, elapsed_secs: f64, state: MediaPlaybackState);

    /// Publish the player's volume (0.0..=1.0) where the OS surface shows one.
    fn set_volume(&self, _volume: f64) {}

    /// Like `set_volume`, but announced even when the value did not change:
    /// the answer to a client's write the player did not take.
    fn restate_volume(&self, _volume: f64) {}
}

pub type Driver = Pin<Box<dyn Future<Output = ()> + Send>>;

pub struct Integration {
    pub controls: Rc<dyn SystemMediaIntegration>,
    pub driver: Option<Driver>,
}

/// Create the platform's system media integration.
///
/// `hwnd` is the native window handle, required by the Windows System Media
/// Transport Controls; it is ignored on every other platform. Returns `None`
/// when the current platform has no integration or it failed to initialize.
///
/// Must be called on the main thread.
pub fn create_integration(
    #[allow(unused)] command_sender: Sender<MediaCommand>,
    #[allow(unused)] hwnd: Option<*mut std::ffi::c_void>,
) -> Option<Integration> {
    #[cfg(target_os = "macos")]
    {
        let integration = macos::MacOsIntegration::new(command_sender)?;
        Some(Integration {
            controls: Rc::new(integration),
            driver: None,
        })
    }
    #[cfg(target_os = "windows")]
    {
        let integration = souvlaki_backend::SouvlakiIntegration::new(command_sender, hwnd)?;
        Some(Integration {
            controls: Rc::new(integration),
            driver: None,
        })
    }
    #[cfg(target_os = "linux")]
    {
        let (integration, driver) = mpris::MprisIntegration::new(command_sender);
        Some(Integration {
            controls: Rc::new(integration),
            driver: Some(driver),
        })
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        None
    }
}

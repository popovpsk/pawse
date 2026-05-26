use std::path::PathBuf;
use std::rc::Rc;

use flume::Sender;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(any(target_os = "windows", target_os = "linux"))]
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
}

/// Platform-agnostic interface for system media integration.
///
/// All implementations are expected to be used on the main thread only
/// (GPUI's requirement for AppKit interop).
pub trait SystemMediaIntegration {
    /// Update the metadata shown in the system Now Playing widget.
    fn update_now_playing(&self, info: NowPlayingInfo);

    /// Update the playback state shown in system UI.
    fn set_playback_state(&self, state: MediaPlaybackState);

    /// Update just the elapsed playback time in the Now Playing widget.
    /// Called frequently (e.g. every second) so it should be lightweight.
    fn update_position(&self, elapsed_secs: f64, state: MediaPlaybackState);
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
) -> Option<Rc<dyn SystemMediaIntegration>> {
    #[cfg(target_os = "macos")]
    {
        let integration = macos::MacOsIntegration::new(command_sender)?;
        Some(Rc::new(integration))
    }
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        let integration = souvlaki_backend::SouvlakiIntegration::new(command_sender, hwnd)?;
        Some(Rc::new(integration))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        None
    }
}

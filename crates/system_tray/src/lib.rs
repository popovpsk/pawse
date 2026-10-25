#![cfg(any(target_os = "linux", target_os = "windows"))]

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as platform;

use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayEvent {
    Show,
    Quit,
}

pub struct IconImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub struct TrayLabels {
    pub show: String,
    pub quit: String,
}

pub struct TrayConfig {
    pub id: String,
    pub title: String,
    pub labels: TrayLabels,
    pub icons: Vec<IconImage>,
}

pub type EventSink = Arc<dyn Fn(TrayEvent) + Send + Sync>;

#[derive(Debug, thiserror::Error)]
pub enum TrayError {
    #[error("tray icon: {0}")]
    Platform(String),
}

pub struct Tray(platform::Tray);

impl Tray {
    pub fn new(config: TrayConfig, sink: EventSink) -> Result<Self, TrayError> {
        platform::Tray::new(config, sink).map(Self)
    }

    pub fn is_shown(&self) -> bool {
        self.0.is_shown()
    }

    pub fn set_labels(&self, labels: &TrayLabels) {
        self.0.set_labels(labels);
    }

    #[cfg(target_os = "windows")]
    pub fn window_handle(&self) -> *mut std::ffi::c_void {
        self.0.window_handle()
    }
}

use audio_common::StreamParams;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PlaybackState {
    Stopped = 0,
    Playing = 1,
    Paused = 2,
}

#[derive(Debug, Clone)]
pub enum Command {
    SetLocalTrack(PathBuf),
    Play,
    Pause,
    Seek(f32),
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineEvent {
    Loaded {
        params: StreamParams,
        duration: Duration,
    },
    Playing,
    Paused,
    Stopped,
    PositionChanged(Duration),
    TrackEnded,
    Error(String),
}

#[derive(Debug, Clone)]
pub struct TrackInfo {
    pub params: StreamParams,
    pub duration: Duration,
}

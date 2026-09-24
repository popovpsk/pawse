use crate::StreamingSource;
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

#[derive(Debug)]
pub struct StreamTrack {
    pub source: StreamingSource,
    pub start_offset: Option<Duration>,
    pub track_duration: Option<Duration>,
}

#[derive(Debug)]
pub enum Command {
    SetLocalTrack {
        path: PathBuf,
        start_offset: Option<Duration>,
        track_duration: Option<Duration>,
        prepared: bool,
    },
    Prepare {
        play: Option<bool>,
    },
    SetStreamTrack(Box<StreamTrack>),
    Play {
        fade_in: bool,
    },
    Pause,
    Seek(f32),
    Stop,
    Fail(String),
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
    Buffering(bool),
    Error(String),
}

#[derive(Debug, Clone)]
pub struct TrackInfo {
    pub params: StreamParams,
    pub duration: Duration,
}

use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelCount {
    Mono,
    Stereo,
    Multi(u8),
}

impl ChannelCount {
    pub fn from_u8(n: u8) -> Self {
        match n {
            1 => ChannelCount::Mono,
            2 => ChannelCount::Stereo,
            n => ChannelCount::Multi(n),
        }
    }

    pub fn to_u8(self) -> u8 {
        match self {
            ChannelCount::Mono => 1,
            ChannelCount::Stereo => 2,
            ChannelCount::Multi(n) => n,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamParams {
    pub sample_rate: u32,
    pub channels: ChannelCount,
    pub bit_depth: u8,
}

impl StreamParams {
    pub fn new(sample_rate: u32, channels: ChannelCount, bit_depth: u8) -> Self {
        Self {
            sample_rate,
            channels,
            bit_depth,
        }
    }

    pub fn channels_count(&self) -> u8 {
        self.channels.to_u8()
    }
}

#[derive(Clone, Debug)]
pub struct AudioBuffer {
    data: Vec<f32>,
}

impl AudioBuffer {
    pub fn new(data: Vec<f32>) -> Self {
        Self { data }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn as_slice(&self) -> &[f32] {
        &self.data
    }

    pub fn as_slice_mut(&mut self) -> &mut [f32] {
        &mut self.data
    }

    pub fn as_ptr(&self) -> *const f32 {
        self.data.as_ptr()
    }
}

impl From<Vec<f32>> for AudioBuffer {
    fn from(data: Vec<f32>) -> Self {
        Self::new(data)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Decoder error: {0}")]
    Decoder(String),

    #[error("Output error: {0}")]
    Output(String),

    #[error("DSP error: {0}")]
    Dsp(String),

    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),

    #[error("Device not found: {0}")]
    DeviceNotFound(String),

    #[error("Stream closed")]
    StreamClosed,

    #[error("Invalid state: {0}")]
    InvalidState(String),
}

pub trait AudioSource: Send {
    fn params(&self) -> StreamParams;

    fn next_buffer(&mut self) -> Result<Option<AudioBuffer>, AudioError>;

    fn seek(&mut self, position: Duration) -> Result<Duration, AudioError>;

    fn duration(&self) -> Option<Duration>;
}

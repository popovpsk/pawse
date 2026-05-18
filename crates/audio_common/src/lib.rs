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

/// 24-bit signed sample
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct I24(i32);

impl I24 {
    pub fn new(val: i32) -> Self {
        // Sign-extend from 24 bits
        let extended = (val << 8) >> 8;
        I24(extended)
    }

    pub fn into_i32(self) -> i32 {
        self.0
    }
}

impl From<I24> for i32 {
    fn from(val: I24) -> i32 {
        val.0
    }
}

/// Enum для хранения аудио сэмплов разных типов (как AudioBufferRef в Symphonia)
#[derive(Clone, Debug)]
pub enum AudioSamples {
    S16(Vec<i16>),
    S24(Vec<I24>),
    S32(Vec<i32>),
    F32(Vec<f32>),
}

impl AudioSamples {
    /// Возвращает количество сэмплов
    pub fn len(&self) -> usize {
        match self {
            AudioSamples::S16(data) => data.len(),
            AudioSamples::S24(data) => data.len(),
            AudioSamples::S32(data) => data.len(),
            AudioSamples::F32(data) => data.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn copy_from_offset(&self, start: usize) -> Self {
        match self {
            AudioSamples::S16(data) => AudioSamples::S16(data[start..].to_vec()),
            AudioSamples::S24(data) => AudioSamples::S24(data[start..].to_vec()),
            AudioSamples::S32(data) => AudioSamples::S32(data[start..].to_vec()),
            AudioSamples::F32(data) => AudioSamples::F32(data[start..].to_vec()),
        }
    }

    /// Конвертирует любой формат сэмплов в f32
    pub fn to_f32(&self) -> Vec<f32> {
        match self {
            AudioSamples::S16(data) => data.iter().map(|&s| s as f32 / 32768.0).collect(),
            AudioSamples::S24(data) => data
                .iter()
                .map(|&s| s.into_i32() as f32 / 8388608.0)
                .collect(),
            AudioSamples::S32(data) => data.iter().map(|&s| s as f32 / 2147483648.0).collect(),
            AudioSamples::F32(data) => data.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AudioBatch {
    pub data: AudioSamples,
    pub metadata: Metadata,
}

#[derive(Debug, Clone)]
pub struct Metadata {
    pub sample_rate: u32,
    pub channels: ChannelCount,
    pub bit_depth: u8,
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

    #[error("Device busy: {0}")]
    DeviceBusy(String),
}

pub trait AudioSource: Send {
    fn params(&self) -> StreamParams;

    fn next_buffer(&mut self) -> Result<Option<AudioBatch>, AudioError>;

    fn seek(&mut self, position: f32) -> Result<Duration, AudioError>;

    fn duration(&self) -> Option<Duration>;
}

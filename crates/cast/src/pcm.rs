use std::io::{self, Read};
use std::time::Duration;

use audio_common::{AudioSamples, AudioSource};
use audio_decoder::Decoder;
use rubato::{FftFixedIn, Resampler};

use crate::media::Source;

const WAV_HEADER: u64 = 44;
const RESAMPLE_CHUNK: usize = 1024;
const WAV_EXTENSIBLE_HEADER: u64 = 68;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container {
    Wav,
    L16,
}

#[derive(Clone)]
pub struct PcmSpec {
    pub source: Source,
    pub extension: String,
    pub start: Duration,
    pub frames: u64,
    pub sample_rate: u32,
    pub channels: u16,
    pub source_rate: u32,
    pub source_channels: u16,
    pub bits: u16,
    pub container: Container,
}

impl std::fmt::Debug for PcmSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PcmSpec")
            .field("extension", &self.extension)
            .field("start", &self.start)
            .field("frames", &self.frames)
            .field("sample_rate", &self.sample_rate)
            .field("channels", &self.channels)
            .field("source_rate", &self.source_rate)
            .field("source_channels", &self.source_channels)
            .field("bits", &self.bits)
            .field("container", &self.container)
            .finish()
    }
}

impl PcmSpec {
    fn converts(&self) -> bool {
        self.source_rate != self.sample_rate || self.source_channels != self.channels
    }

    pub fn block_align(&self) -> u64 {
        u64::from(self.channels) * u64::from(self.bits / 8)
    }

    fn extensible(&self) -> bool {
        self.channels > 2
    }

    pub fn header_len(&self) -> u64 {
        match self.container {
            Container::L16 => 0,
            Container::Wav if self.extensible() => WAV_EXTENSIBLE_HEADER,
            Container::Wav => WAV_HEADER,
        }
    }

    pub fn data_len(&self) -> u64 {
        self.frames * self.block_align()
    }

    pub fn len(&self) -> u64 {
        self.header_len() + self.data_len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames == 0
    }

    pub fn duration(&self) -> Duration {
        Duration::from_secs_f64(self.frames as f64 / f64::from(self.sample_rate))
    }

    pub fn mime(&self) -> String {
        match self.container {
            Container::Wav => "audio/wav".to_string(),
            Container::L16 => format!(
                "audio/L16;rate={};channels={}",
                self.sample_rate, self.channels
            ),
        }
    }

    pub fn header(&self) -> Vec<u8> {
        if self.container == Container::L16 {
            return Vec::new();
        }
        let data = u32::try_from(self.data_len()).unwrap_or(u32::MAX);
        let byte_rate = self.sample_rate * u32::from(self.channels) * u32::from(self.bits / 8);
        let align = self.channels * (self.bits / 8);
        let mut header = Vec::with_capacity(self.header_len() as usize);
        let riff = data.saturating_add(self.header_len() as u32 - 8);
        header.extend_from_slice(b"RIFF");
        header.extend_from_slice(&riff.to_le_bytes());
        header.extend_from_slice(b"WAVE");
        header.extend_from_slice(b"fmt ");
        if self.extensible() {
            header.extend_from_slice(&40u32.to_le_bytes());
            header.extend_from_slice(&0xfffeu16.to_le_bytes());
        } else {
            header.extend_from_slice(&16u32.to_le_bytes());
            header.extend_from_slice(&1u16.to_le_bytes());
        }
        header.extend_from_slice(&self.channels.to_le_bytes());
        header.extend_from_slice(&self.sample_rate.to_le_bytes());
        header.extend_from_slice(&byte_rate.to_le_bytes());
        header.extend_from_slice(&align.to_le_bytes());
        header.extend_from_slice(&self.bits.to_le_bytes());
        if self.extensible() {
            header.extend_from_slice(&22u16.to_le_bytes());
            header.extend_from_slice(&self.bits.to_le_bytes());
            header.extend_from_slice(&channel_mask(self.channels).to_le_bytes());
            header.extend_from_slice(&[
                0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xaa, 0x00, 0x38,
                0x9b, 0x71,
            ]);
        }
        header.extend_from_slice(b"data");
        header.extend_from_slice(&data.to_le_bytes());
        header
    }
}

fn channel_mask(channels: u16) -> u32 {
    match channels {
        3 => 0x7,
        4 => 0x33,
        5 => 0x37,
        6 => 0x3f,
        7 => 0x13f,
        8 => 0x63f,
        n => (1u32 << n.min(18)) - 1,
    }
}

pub(crate) fn open_decoder(source: &Source, extension: &str) -> io::Result<Decoder> {
    let decoder = match source {
        Source::File(path) => Decoder::open(path),
        Source::Stream(open) => Decoder::open_stream(open()?, Some(extension)),
    };
    decoder.map_err(|e| io::Error::other(e.to_string()))
}

pub struct PcmReader {
    spec: PcmSpec,
    decoder: Decoder,
    header: Vec<u8>,
    position: u64,
    frame: u64,
    skip_frames: u64,
    skip_bytes: usize,
    pending: Vec<u8>,
    pending_at: usize,
    exhausted: bool,
    converter: Option<Converter>,
}

struct Converter {
    resampler: Option<FftFixedIn<f32>>,
    input: Vec<Vec<f32>>,
    output: Vec<Vec<f32>>,
    delay: usize,
    flushed: bool,
}

impl Converter {
    fn new(spec: &PcmSpec) -> io::Result<Self> {
        let channels = usize::from(spec.channels);
        let resampler = if spec.source_rate == spec.sample_rate {
            None
        } else {
            Some(
                FftFixedIn::new(
                    spec.source_rate as usize,
                    spec.sample_rate as usize,
                    RESAMPLE_CHUNK,
                    2,
                    channels,
                )
                .map_err(|e| io::Error::other(e.to_string()))?,
            )
        };
        let output = resampler
            .as_ref()
            .map(|r| r.output_buffer_allocate(true))
            .unwrap_or_default();
        let delay = resampler.as_ref().map_or(0, |r| r.output_delay());
        Ok(Self {
            resampler,
            input: vec![Vec::new(); channels],
            output,
            delay,
            flushed: false,
        })
    }

    fn push(&mut self, frames: &[f32], out: &mut Vec<f32>) {
        let channels = self.input.len();
        let Some(resampler) = &mut self.resampler else {
            out.extend_from_slice(frames);
            return;
        };
        for frame in frames.chunks_exact(channels) {
            for (plane, sample) in self.input.iter_mut().zip(frame) {
                plane.push(*sample);
            }
        }
        loop {
            let needed = resampler.input_frames_next();
            if self.input[0].len() < needed {
                return;
            }
            let chunk: Vec<&[f32]> = self.input.iter().map(|plane| &plane[..needed]).collect();
            match resampler.process_into_buffer(&chunk, &mut self.output, None) {
                Ok((_, produced)) => {
                    let skip = self.delay.min(produced);
                    self.delay -= skip;
                    for i in skip..produced {
                        for plane in &self.output {
                            out.push(plane[i]);
                        }
                    }
                }
                Err(e) => log::warn!("cast: resampling failed: {e}"),
            }
            for plane in &mut self.input {
                plane.drain(..needed);
            }
        }
    }

    fn flush(&mut self, out: &mut Vec<f32>) {
        if self.flushed || self.resampler.is_none() {
            return;
        }
        self.flushed = true;
        let channels = self.input.len();
        let tail = self.input[0].len() + self.delay + RESAMPLE_CHUNK;
        self.push(&vec![0.0; tail * channels], out);
    }
}

fn mix(frame: &[f32], channels: usize, out: &mut Vec<f32>) {
    if frame.len() == channels {
        out.extend_from_slice(frame);
        return;
    }
    if channels == 1 {
        out.push(frame.iter().sum::<f32>() / frame.len() as f32);
        return;
    }
    if frame.len() == 1 {
        out.extend(std::iter::repeat_n(frame[0], channels));
        return;
    }
    let center = frame.get(2).copied().unwrap_or(0.0) * 0.707;
    let left_surround = frame.get(4).copied().unwrap_or(0.0) * 0.707;
    let right_surround = frame.get(5).copied().unwrap_or(0.0) * 0.707;
    out.push((frame[0] + center + left_surround) * 0.5);
    out.push((frame[1] + center + right_surround) * 0.5);
    out.extend(std::iter::repeat_n(0.0, channels.saturating_sub(2)));
}

impl PcmReader {
    pub fn open(spec: PcmSpec, offset: u64) -> io::Result<Self> {
        let mut decoder = open_decoder(&spec.source, &spec.extension)?;
        let header = spec.header();
        let header_len = header.len() as u64;
        let align = spec.block_align();
        let data_offset = offset.saturating_sub(header_len);
        let frame = (data_offset / align).min(spec.frames);
        let skip_bytes = (data_offset % align) as usize;
        let target =
            spec.start + Duration::from_secs_f64(frame as f64 / f64::from(spec.sample_rate));
        let mut skip_frames = frame_count(target, spec.source_rate);
        if !target.is_zero()
            && let Some(total) = decoder.duration().filter(|total| !total.is_zero())
        {
            let early = target.saturating_sub(SEEK_MARGIN);
            let ratio = (early.as_secs_f64() / total.as_secs_f64()).clamp(0.0, 1.0) as f32;
            match decoder.seek(ratio) {
                Ok(landed) => {
                    skip_frames = frame_count(target.saturating_sub(landed), spec.source_rate);
                }
                Err(e) => {
                    log::debug!("cast: seek for a PCM range failed, decoding from the start: {e}")
                }
            }
        }
        let converter = if spec.converts() {
            Some(Converter::new(&spec)?)
        } else {
            None
        };
        Ok(Self {
            spec,
            decoder,
            header,
            position: offset,
            frame,
            skip_frames,
            skip_bytes,
            pending: Vec::new(),
            pending_at: 0,
            exhausted: false,
            converter,
        })
    }

    fn refill(&mut self) -> io::Result<()> {
        self.pending.clear();
        self.pending_at = 0;
        let channels = usize::from(self.spec.channels);
        let remaining = self.spec.frames - self.frame;
        if remaining == 0 {
            return Ok(());
        }
        if self.exhausted
            && let Some(converter) = &mut self.converter
            && !converter.flushed
        {
            let mut floats = Vec::new();
            converter.flush(&mut floats);
            self.emit_floats(&floats, remaining);
            return Ok(());
        }
        if self.exhausted {
            let frames = remaining.min(4096) as usize;
            self.pending
                .resize(frames * self.spec.block_align() as usize, 0);
            self.frame += frames as u64;
            return Ok(());
        }
        let batch = match self.decoder.next_buffer() {
            Ok(Some(batch)) => batch,
            Ok(None) => {
                self.exhausted = true;
                return self.refill();
            }
            Err(e) => {
                log::warn!("cast: decoding for a PCM stream failed: {e}");
                self.exhausted = true;
                return self.refill();
            }
        };
        let source_channels = usize::from(batch.metadata.channels.to_u8()).max(1);
        let total = batch.data.len() / source_channels;
        let skip = (self.skip_frames.min(total as u64)) as usize;
        self.skip_frames -= skip as u64;
        let frames = (total - skip).min(remaining as usize);
        if frames == 0 {
            return Ok(());
        }
        let from = skip * source_channels;
        if self.converter.is_some() {
            let floats = batch.data.to_f32();
            let mut mixed = Vec::with_capacity((total - skip) * channels);
            for frame in floats[from..].chunks_exact(source_channels) {
                mix(frame, channels, &mut mixed);
            }
            let mut converted = Vec::with_capacity(mixed.len());
            if let Some(converter) = &mut self.converter {
                converter.push(&mixed, &mut converted);
            }
            self.emit_floats(&converted, remaining);
            return Ok(());
        }
        let to = from + frames * source_channels;
        encode(
            &batch.data,
            from..to,
            source_channels,
            channels,
            self.spec.bits,
            self.spec.container == Container::L16,
            &mut self.pending,
        );
        self.frame += frames as u64;
        if self.skip_bytes > 0 {
            let cut = self.skip_bytes.min(self.pending.len());
            self.pending_at = cut;
            self.skip_bytes -= cut;
        }
        Ok(())
    }
}

impl PcmReader {
    fn emit_floats(&mut self, samples: &[f32], remaining: u64) {
        let channels = usize::from(self.spec.channels);
        let frames = (samples.len() / channels).min(remaining as usize);
        let width = usize::from(self.spec.bits / 8);
        let big_endian = self.spec.container == Container::L16;
        let max = ((1i64 << (self.spec.bits - 1)) - 1) as f32;
        self.pending.reserve(frames * channels * width);
        for sample in &samples[..frames * channels] {
            let bytes = ((sample.clamp(-1.0, 1.0) * max).round() as i32).to_le_bytes();
            if big_endian {
                self.pending.extend(bytes[..width].iter().rev());
            } else {
                self.pending.extend_from_slice(&bytes[..width]);
            }
        }
        self.frame += frames as u64;
        if self.skip_bytes > 0 {
            let cut = self.skip_bytes.min(self.pending.len());
            self.pending_at = cut;
            self.skip_bytes -= cut;
        }
    }
}

const SEEK_MARGIN: Duration = Duration::from_millis(50);

fn frame_count(duration: Duration, sample_rate: u32) -> u64 {
    (duration.as_secs_f64() * f64::from(sample_rate)).round() as u64
}

fn sample_i32(samples: &AudioSamples, index: usize, bits: u16) -> i32 {
    let shift_to = |value: i32, from: u16| -> i32 {
        if from > bits {
            value >> (from - bits)
        } else {
            value << (bits - from)
        }
    };
    match samples {
        AudioSamples::S16(data) => shift_to(i32::from(data[index]), 16),
        AudioSamples::S24(data) => shift_to(data[index].into_i32(), 24),
        AudioSamples::S32(data) => shift_to(data[index], 32),
        AudioSamples::F32(data) => {
            let max = ((1i64 << (bits - 1)) - 1) as f32;
            (data[index].clamp(-1.0, 1.0) * max).round() as i32
        }
    }
}

fn encode(
    samples: &AudioSamples,
    range: std::ops::Range<usize>,
    source_channels: usize,
    channels: usize,
    bits: u16,
    big_endian: bool,
    out: &mut Vec<u8>,
) {
    let width = usize::from(bits / 8);
    out.reserve((range.len() / source_channels) * channels * width);
    let mut index = range.start;
    while index < range.end {
        for channel in 0..channels {
            let value = sample_i32(samples, index + channel.min(source_channels - 1), bits);
            let bytes = value.to_le_bytes();
            if big_endian {
                out.extend(bytes[..width].iter().rev());
            } else {
                out.extend_from_slice(&bytes[..width]);
            }
        }
        index += source_channels;
    }
}

impl Read for PcmReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let header_len = self.header.len() as u64;
        if self.position < header_len {
            let from = self.position as usize;
            let n = (self.header.len() - from).min(buf.len());
            buf[..n].copy_from_slice(&self.header[from..from + n]);
            self.position += n as u64;
            return Ok(n);
        }
        while self.pending_at >= self.pending.len() {
            if self.frame >= self.spec.frames {
                return Ok(0);
            }
            self.refill()?;
        }
        let n = (self.pending.len() - self.pending_at).min(buf.len());
        buf[..n].copy_from_slice(&self.pending[self.pending_at..self.pending_at + n]);
        self.pending_at += n;
        self.position += n as u64;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(channels: u16, bits: u16, container: Container) -> PcmSpec {
        PcmSpec {
            source: Source::File("/nonexistent".into()),
            extension: "flac".into(),
            start: Duration::ZERO,
            frames: 44_100,
            sample_rate: 44_100,
            channels,
            source_rate: 44_100,
            source_channels: channels,
            bits,
            container,
        }
    }

    #[test]
    fn a_stereo_wav_header_describes_the_data() {
        let spec = spec(2, 16, Container::Wav);
        let header = spec.header();
        assert_eq!(header.len() as u64, spec.header_len());
        assert_eq!(&header[..4], b"RIFF");
        assert_eq!(&header[8..16], b"WAVEfmt ");
        assert_eq!(u16::from_le_bytes([header[20], header[21]]), 1);
        assert_eq!(u16::from_le_bytes([header[22], header[23]]), 2);
        assert_eq!(
            u32::from_le_bytes(header[24..28].try_into().unwrap()),
            44_100
        );
        assert_eq!(
            u32::from_le_bytes(header[40..44].try_into().unwrap()),
            44_100 * 4
        );
        assert_eq!(
            u32::from_le_bytes(header[4..8].try_into().unwrap()) as u64,
            spec.len() - 8
        );
    }

    #[test]
    fn multichannel_wav_uses_the_extensible_format() {
        let spec = spec(6, 24, Container::Wav);
        let header = spec.header();
        assert_eq!(header.len() as u64, spec.header_len());
        assert_eq!(u16::from_le_bytes([header[20], header[21]]), 0xfffe);
        assert_eq!(&header[header.len() - 8..header.len() - 4], b"data");
        assert_eq!(spec.block_align(), 18);
    }

    #[test]
    fn l16_has_no_header_and_names_its_format() {
        let spec = spec(2, 16, Container::L16);
        assert!(spec.header().is_empty());
        assert_eq!(spec.mime(), "audio/L16;rate=44100;channels=2");
        assert_eq!(spec.len(), 44_100 * 4);
    }

    #[test]
    fn samples_are_scaled_to_the_target_width() {
        let samples = AudioSamples::S16(vec![0x1234, -1]);
        let mut out = Vec::new();
        encode(&samples, 0..2, 2, 2, 24, false, &mut out);
        assert_eq!(out, vec![0x00, 0x34, 0x12, 0x00, 0xff, 0xff]);
        out.clear();
        encode(&samples, 0..2, 2, 2, 16, true, &mut out);
        assert_eq!(out, vec![0x12, 0x34, 0xff, 0xff]);
        out.clear();
        encode(
            &AudioSamples::F32(vec![1.0, -1.0]),
            0..2,
            2,
            2,
            16,
            false,
            &mut out,
        );
        assert_eq!(out, vec![0xff, 0x7f, 0x01, 0x80]);
    }
}

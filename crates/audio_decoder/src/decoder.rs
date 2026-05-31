use audio_common::{
    AudioBatch, AudioError, AudioSamples, AudioSource, ChannelCount, I24, Metadata, StreamParams,
};
use std::fs::File;
use std::path::Path;
use std::time::Duration;
use symphonia::core::audio::{AudioBufferRef, SampleBuffer, Signal};
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

// ============================================================================
// APE source — uses ape-decoder crate for Monkey's Audio (.ape) files
// ============================================================================

const APE_CHUNK_SAMPLES: usize = 4096;

struct ApeSource {
    ape_decoder: ape_decoder::ApeDecoder<File>,
    sample_rate: u32,
    channels: ChannelCount,
    bit_depth: u8,
    total_samples: u64,
    total_duration: Duration,
    block_align: u16,
    current_frame: u32,
    total_frames: u32,
    pcm_buffer: Vec<u8>,
    pcm_offset: usize,
    skip_after_seek: usize,
}

impl ApeSource {
    fn open(path: &Path) -> Result<Self, AudioError> {
        let file = File::open(path).map_err(AudioError::Io)?;
        let decoder = ape_decoder::ApeDecoder::new(file)
            .map_err(|e| AudioError::Decoder(format!("APE: {}", e)))?;

        let info = decoder.info();
        let sample_rate = info.sample_rate;
        let channels = ChannelCount::from_u8(info.channels as u8);
        let bit_depth = info.bits_per_sample as u8;
        let total_samples = info.total_samples;
        let total_duration = Duration::from_millis(info.duration_ms);
        let block_align = info.block_align;
        let total_frames = info.total_frames;

        Ok(Self {
            ape_decoder: decoder,
            sample_rate,
            channels,
            bit_depth,
            total_samples,
            total_duration,
            block_align,
            current_frame: 0,
            total_frames,
            pcm_buffer: Vec::new(),
            pcm_offset: 0,
            skip_after_seek: 0,
        })
    }
}

fn pcm_to_samples(pcm: &[u8], bit_depth: u8) -> AudioSamples {
    match bit_depth {
        16 => {
            let mut samples = Vec::with_capacity(pcm.len() / 2);
            for chunk in pcm.chunks_exact(2) {
                samples.push(i16::from_le_bytes([chunk[0], chunk[1]]));
            }
            AudioSamples::S16(samples)
        }
        24 => {
            let mut samples = Vec::with_capacity(pcm.len() / 3);
            for chunk in pcm.chunks_exact(3) {
                let sign = if chunk[2] & 0x80 != 0 { 0xFFu8 } else { 0x00u8 };
                let raw = i32::from_le_bytes([chunk[0], chunk[1], chunk[2], sign]);
                samples.push(I24::new(raw));
            }
            AudioSamples::S24(samples)
        }
        32 => {
            let mut samples = Vec::with_capacity(pcm.len() / 4);
            for chunk in pcm.chunks_exact(4) {
                samples.push(i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
            }
            AudioSamples::S32(samples)
        }
        _ => {
            let mut samples = Vec::with_capacity(pcm.len() / 2);
            for chunk in pcm.chunks_exact(2) {
                samples.push(i16::from_le_bytes([chunk[0], chunk[1]]));
            }
            AudioSamples::S16(samples)
        }
    }
}

impl AudioSource for ApeSource {
    fn params(&self) -> StreamParams {
        StreamParams::new(self.sample_rate, self.channels, self.bit_depth)
    }

    fn next_buffer(&mut self) -> Result<Option<AudioBatch>, AudioError> {
        let channels = self.channels.to_u8() as usize;
        let bytes_per_sample = (self.bit_depth / 8) as usize;
        let sample_size = bytes_per_sample * channels;
        let chunk_bytes = APE_CHUNK_SAMPLES * sample_size;

        loop {
            let available = self.pcm_buffer.len().saturating_sub(self.pcm_offset);
            if available > 0 {
                let end = (self.pcm_offset + chunk_bytes).min(self.pcm_buffer.len());
                let chunk = &self.pcm_buffer[self.pcm_offset..end];
                self.pcm_offset = end;

                return Ok(Some(AudioBatch {
                    data: pcm_to_samples(chunk, self.bit_depth),
                    metadata: Metadata {
                        sample_rate: self.sample_rate,
                        channels: self.channels,
                        bit_depth: self.bit_depth,
                    },
                }));
            }

            if self.current_frame >= self.total_frames {
                return Ok(None);
            }

            let frame_pcm = self
                .ape_decoder
                .decode_frame(self.current_frame)
                .map_err(|e| AudioError::Decoder(format!("APE: {}", e)))?;
            self.current_frame += 1;

            self.pcm_buffer = frame_pcm;
            self.pcm_offset = 0;

            if self.skip_after_seek > 0 {
                self.pcm_offset = self.skip_after_seek.min(self.pcm_buffer.len());
                self.skip_after_seek = 0;
            }
        }
    }

    fn seek(&mut self, position: f32) -> Result<Duration, AudioError> {
        let position = position.clamp(0.0, 1.0);
        let target_sample = (self.total_samples as f64 * position as f64) as u64;

        let result = self
            .ape_decoder
            .seek(target_sample)
            .map_err(|e| AudioError::Decoder(format!("APE seek: {}", e)))?;

        self.current_frame = result.frame_index;
        self.pcm_buffer.clear();
        self.pcm_offset = 0;
        self.skip_after_seek = result.skip_samples as usize * self.block_align as usize;

        let position_secs = target_sample as f64 / self.sample_rate as f64;
        Ok(Duration::from_secs_f64(position_secs))
    }

    fn duration(&self) -> Option<Duration> {
        Some(self.total_duration)
    }
}

// ============================================================================
// Symphonia decoder — handles all other formats via Symphonia
// ============================================================================

struct SymphoniaDecoder {
    format: Box<dyn symphonia::core::formats::FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::Decoder>,
    track_id: u32,
    codec_params: CodecParameters,
    duration: Option<Duration>,
}

impl SymphoniaDecoder {
    fn open(path: &Path) -> Result<Self, AudioError> {
        let file = File::open(path).map_err(AudioError::Io)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        let mut hint = Hint::new();
        if let Some(ext) = path.extension() {
            hint.with_extension(ext.to_str().unwrap_or(""));
        }

        let probed = symphonia::default::get_probe()
            .format(
                &hint,
                mss,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .map_err(|e| AudioError::Decoder(e.to_string()))?;

        let format = probed.format;

        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or(AudioError::Decoder("No audio track found".to_string()))?;

        let track_id = track.id;
        let codec_params = track.codec_params.clone();

        let sample_rate = codec_params
            .sample_rate
            .ok_or(AudioError::Decoder("No sample rate".to_string()))?;

        let duration = codec_params.n_frames.map(|frames| {
            let secs = frames as f64 / sample_rate as f64;
            Duration::from_secs_f64(secs)
        });

        let decoder_opts = DecoderOptions::default();
        let decoder = symphonia::default::get_codecs()
            .make(&codec_params, &decoder_opts)
            .map_err(|e| AudioError::Decoder(e.to_string()))?;

        Ok(Self {
            format,
            decoder,
            track_id,
            codec_params,
            duration,
        })
    }

    fn decode_next(&mut self) -> Result<Option<AudioBatch>, AudioError> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(p) => p,
                Err(symphonia::core::errors::Error::IoError(ref e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    return Ok(None);
                }
                Err(e) => return Err(AudioError::Decoder(e.to_string())),
            };

            let decoded = match self.decoder.decode(&packet) {
                Ok(decoded_buffer) => decoded_buffer,
                Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
                Err(e) => return Err(AudioError::Decoder(e.to_string())),
            };

            let symphonia_spec = decoded.spec();
            let sample_rate = symphonia_spec.rate;
            let channels = ChannelCount::from_u8(symphonia_spec.channels.count() as u8);

            let audio_sample = map_audio_buffer_ref(decoded);

            return Ok(Some(AudioBatch {
                data: audio_sample,
                metadata: Metadata {
                    sample_rate,
                    channels,
                    bit_depth: self.codec_params.bits_per_sample.unwrap_or(32) as u8,
                },
            }));
        }
    }
}

impl AudioSource for SymphoniaDecoder {
    fn params(&self) -> StreamParams {
        let sample_rate = self.codec_params.sample_rate.unwrap_or(44100);
        let channels = self
            .codec_params
            .channels
            .map(|c: symphonia::core::audio::Channels| ChannelCount::from_u8(c.count() as u8))
            .unwrap_or(ChannelCount::Stereo);

        let bit_depth = self.codec_params.bits_per_sample.unwrap_or(32);

        StreamParams::new(sample_rate, channels, bit_depth as u8)
    }

    fn next_buffer(&mut self) -> Result<Option<AudioBatch>, AudioError> {
        self.decode_next()
    }

    fn seek(&mut self, position: f32) -> Result<Duration, AudioError> {
        let duration = self.duration.unwrap().mul_f32(position);

        let time: symphonia::core::units::Time = duration.into();

        let seeked = self
            .format
            .seek(
                symphonia::core::formats::SeekMode::Coarse,
                symphonia::core::formats::SeekTo::Time {
                    time,
                    track_id: Some(self.track_id),
                },
            )
            .map_err(|e| AudioError::Decoder(e.to_string()))?;

        let sample_rate = self
            .codec_params
            .sample_rate
            .ok_or_else(|| AudioError::Decoder("Sample rate unknown after seek".to_string()))?;
        let actual_ts = seeked.actual_ts as f64 / sample_rate as f64;
        Ok(Duration::from_secs_f64(actual_ts))
    }

    fn duration(&self) -> Option<Duration> {
        self.duration
    }
}

// ============================================================================
// Combined Decoder — selects APE or Symphonia based on file extension
// ============================================================================

#[allow(private_interfaces)]
pub enum Decoder {
    Symphonia(Box<SymphoniaDecoder>),
    Ape(Box<ApeSource>),
}

impl Decoder {
    pub fn open(path: &Path) -> Result<Self, AudioError> {
        match path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase()
            .as_str()
        {
            "ape" => Ok(Decoder::Ape(Box::new(ApeSource::open(path)?))),
            _ => Ok(Decoder::Symphonia(Box::new(SymphoniaDecoder::open(path)?))),
        }
    }
}

impl AudioSource for Decoder {
    fn params(&self) -> StreamParams {
        match self {
            Decoder::Symphonia(d) => d.params(),
            Decoder::Ape(d) => d.params(),
        }
    }

    fn next_buffer(&mut self) -> Result<Option<AudioBatch>, AudioError> {
        match self {
            Decoder::Symphonia(d) => d.next_buffer(),
            Decoder::Ape(d) => d.next_buffer(),
        }
    }

    fn seek(&mut self, position: f32) -> Result<Duration, AudioError> {
        match self {
            Decoder::Symphonia(d) => d.seek(position),
            Decoder::Ape(d) => d.seek(position),
        }
    }

    fn duration(&self) -> Option<Duration> {
        match self {
            Decoder::Symphonia(d) => d.duration(),
            Decoder::Ape(d) => d.duration(),
        }
    }
}

// ============================================================================
// map_audio_buffer_ref — Symphonia planar → interleaved
// ============================================================================

fn map_audio_buffer_ref(decoded: AudioBufferRef<'_>) -> AudioSamples {
    let spec = *decoded.spec();
    let frames = decoded.frames();
    let channels = spec.channels.count();
    let total_samples = frames * channels;

    match decoded {
        AudioBufferRef::S16(buf) => {
            let mut interleaved = Vec::with_capacity(total_samples);
            for frame in 0..frames {
                for ch in 0..channels {
                    interleaved.push(buf.chan(ch)[frame]);
                }
            }
            AudioSamples::S16(interleaved)
        }
        AudioBufferRef::S24(buf) => {
            let mut interleaved: Vec<I24> = Vec::with_capacity(total_samples);
            for frame in 0..frames {
                for ch in 0..channels {
                    interleaved.push(I24::new(buf.chan(ch)[frame].inner()));
                }
            }
            AudioSamples::S24(interleaved)
        }
        AudioBufferRef::S32(buf) => {
            let mut interleaved = Vec::with_capacity(total_samples);
            for frame in 0..frames {
                for ch in 0..channels {
                    interleaved.push(buf.chan(ch)[frame]);
                }
            }
            AudioSamples::S32(interleaved)
        }
        AudioBufferRef::F32(buf) => {
            let mut interleaved = Vec::with_capacity(total_samples);
            for frame in 0..frames {
                for ch in 0..channels {
                    interleaved.push(buf.chan(ch)[frame]);
                }
            }
            AudioSamples::F32(interleaved)
        }
        _ => {
            let mut sample_buf = SampleBuffer::<f32>::new(frames as u64, spec);
            sample_buf.copy_interleaved_ref(decoded);
            AudioSamples::F32(sample_buf.samples().to_vec())
        }
    }
}

// ============================================================================
// Тесты
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use std::path::PathBuf;

    fn fixture_path(filename: &str) -> PathBuf {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("..");
        path.push("..");
        path.push("fixtures");
        for part in filename.split('/') {
            path.push(part);
        }
        path
    }

    #[rstest]
    #[case::sine_440_16_44_mono("sine_440_16_44_mono.wav", 44100, 16, ChannelCount::Mono)]
    #[case::sine_440_16_48_mono("sine_440_16_48_mono.wav", 48000, 16, ChannelCount::Mono)]
    #[case::sine_440_16_96_mono("sine_440_16_96_mono.wav", 96000, 16, ChannelCount::Mono)]
    #[case::sine_440_24_44_mono("sine_440_24_44_mono.wav", 44100, 24, ChannelCount::Mono)]
    #[case::sine_440_32_44_mono("sine_440_32_44_mono.wav", 44100, 32, ChannelCount::Mono)]
    #[case::sine_440_16_44_stereo("sine_440_16_44_stereo.wav", 44100, 16, ChannelCount::Stereo)]
    #[case::silence("silence_16_44_mono.wav", 44100, 16, ChannelCount::Mono)]
    #[case::original_1khz("1khz_16_44_1.wav", 44100, 16, ChannelCount::Mono)]
    fn test_decoder_params(
        #[case] filename: &str,
        #[case] sample_rate: u32,
        #[case] bit_depth: u8,
        #[case] channels: ChannelCount,
    ) {
        let path = fixture_path(filename);
        let decoder =
            Decoder::open(&path).unwrap_or_else(|_| panic!("Failed to open {}", filename));

        let params = decoder.params();
        assert_eq!(
            params.sample_rate, sample_rate,
            "Sample rate mismatch for {}",
            filename
        );
        assert_eq!(
            params.bit_depth, bit_depth,
            "Bit depth mismatch for {}",
            filename
        );
        assert_eq!(
            params.channels, channels,
            "Channels mismatch for {}",
            filename
        );
    }

    #[rstest]
    #[case::sine_440_16_44_mono("sine_440_16_44_mono.wav")]
    #[case::sine_440_16_48_mono("sine_440_16_48_mono.wav")]
    #[case::sine_440_16_96_mono("sine_440_16_96_mono.wav")]
    #[case::sine_440_24_44_mono("sine_440_24_44_mono.wav")]
    #[case::sine_440_32_44_mono("sine_440_32_44_mono.wav")]
    #[case::sine_440_16_44_stereo("sine_440_16_44_stereo.wav")]
    #[case::silence("silence_16_44_mono.wav")]
    #[case::original_1khz("1khz_16_44_1.wav")]
    fn test_decode_buffer_not_empty(#[case] filename: &str) {
        let path = fixture_path(filename);
        let mut decoder =
            Decoder::open(&path).unwrap_or_else(|_| panic!("Failed to open {}", filename));

        let buffer = decoder
            .next_buffer()
            .unwrap_or_else(|_| panic!("Failed to decode {}", filename));
        assert!(
            buffer.is_some(),
            "Buffer should not be None for {}",
            filename
        );

        let audio_batch = buffer.unwrap();
        let samples = audio_batch.data;
        assert!(
            !samples.is_empty(),
            "Samples should not be empty for {}",
            filename
        );
    }

    #[rstest]
    #[case::sine_440_16_44_mono("sine_440_16_44_mono.wav")]
    #[case::sine_440_16_48_mono("sine_440_16_48_mono.wav")]
    #[case::sine_440_16_96_mono("sine_440_16_96_mono.wav")]
    #[case::sine_440_24_44_mono("sine_440_24_44_mono.wav")]
    #[case::sine_440_32_44_mono("sine_440_32_44_mono.wav")]
    #[case::sine_440_16_44_stereo("sine_440_16_44_stereo.wav")]
    #[case::silence("silence_16_44_mono.wav")]
    #[case::original_1khz("1khz_16_44_1.wav")]
    fn test_samples_in_valid_range(#[case] filename: &str) {
        let path = fixture_path(filename);
        let mut decoder =
            Decoder::open(&path).unwrap_or_else(|_| panic!("Failed to open {}", filename));

        let buffer = decoder
            .next_buffer()
            .unwrap_or_else(|_| panic!("Failed to decode {}", filename));
        let audio_batch = buffer.unwrap();

        if let AudioSamples::F32(samples) = audio_batch.data {
            for (i, &sample) in samples.iter().enumerate() {
                assert!(
                    (-1.0..=1.0).contains(&sample),
                    "Sample {} out of range [-1.0, 1.0]: {} in {}",
                    i,
                    sample,
                    filename
                );
            }
        }
    }

    #[test]
    fn test_silence_samples_are_zero() {
        let path = fixture_path("silence_16_44_mono.wav");
        let mut decoder = Decoder::open(&path).expect("Failed to open silence file");

        let buffer = decoder.next_buffer().expect("Failed to decode silence");
        let audio_batch = buffer.unwrap();

        match audio_batch.data {
            AudioSamples::S16(samples) => {
                for sample in samples.iter() {
                    assert_eq!(*sample, 0, "Silence sample should be 0");
                }
            }
            _ => panic!("Expected S16 format for silence file"),
        }
    }

    #[rstest]
    #[case::sine_440_16_44_mono("sine_440_16_44_mono.wav")]
    #[case::sine_440_16_48_mono("sine_440_16_48_mono.wav")]
    #[case::sine_440_16_96_mono("sine_440_16_96_mono.wav")]
    #[case::sine_440_24_44_mono("sine_440_24_44_mono.wav")]
    #[case::sine_440_32_44_mono("sine_440_32_44_mono.wav")]
    #[case::sine_440_16_44_stereo("sine_440_16_44_stereo.wav")]
    #[case::silence("silence_16_44_mono.wav")]
    #[case::original_1khz("1khz_16_44_1.wav")]
    fn test_seek_to_beginning(#[case] filename: &str) {
        let path = fixture_path(filename);
        let mut decoder =
            Decoder::open(&path).unwrap_or_else(|_| panic!("Failed to open {}", filename));

        let result = decoder
            .seek(0.0)
            .unwrap_or_else(|_| panic!("Failed to seek in {}", filename));
        assert_eq!(result, Duration::ZERO, "Seek to zero should return zero");

        let buffer = decoder
            .next_buffer()
            .unwrap_or_else(|_| panic!("Failed to decode after seek in {}", filename));
        assert!(
            buffer.is_some(),
            "Buffer should exist after seek in {}",
            filename
        );
    }

    #[rstest]
    #[case::sine_440_16_44_mono("sine_440_16_44_mono.wav")]
    #[case::sine_440_16_48_mono("sine_440_16_48_mono.wav")]
    #[case::original_1khz("1khz_16_44_1.wav")]
    fn test_multiple_buffers(#[case] filename: &str) {
        let path = fixture_path(filename);
        let mut decoder =
            Decoder::open(&path).unwrap_or_else(|_| panic!("Failed to open {}", filename));

        let mut buffer_count = 0;
        while let Ok(Some(_buffer)) = decoder.next_buffer() {
            buffer_count += 1;
            if buffer_count >= 10 {
                break;
            }
        }

        assert!(
            buffer_count >= 1,
            "Should read at least 1 buffer, got {}",
            buffer_count
        );
    }

    #[rstest]
    #[case::original_1khz("1khz_16_44_1.wav", 2.0)]
    #[case::sine_440_16_44_mono("sine_440_16_44_mono.wav", 0.5)]
    #[case::sine_440_16_44_mono("sine_440_16_44_mono.wav", 0.5)]
    #[case::sine_440_16_48_mono("sine_440_16_48_mono.wav", 0.5)]
    #[case::sine_440_16_96_mono("sine_440_16_96_mono.wav", 0.5)]
    fn test_duration_exact(#[case] filename: &str, #[case] expected_secs: f64) {
        let path = fixture_path(filename);
        let decoder =
            Decoder::open(&path).unwrap_or_else(|_| panic!("Failed to open {}", filename));

        let duration = decoder
            .duration()
            .unwrap_or_else(|| panic!("Duration should exist for {}", filename));

        let actual_secs = duration.as_secs_f64();
        assert!(
            (actual_secs - expected_secs).abs() < 0.01,
            "Duration mismatch for {}: expected {}s, got {:?}",
            filename,
            expected_secs,
            duration
        );
    }

    #[test]
    fn test_pcm_to_samples_s16() {
        let pcm = vec![0x00, 0x00, 0xFF, 0x7F, 0x00, 0x80];
        let result = pcm_to_samples(&pcm, 16);
        match result {
            AudioSamples::S16(samples) => {
                assert_eq!(samples.len(), 3);
                assert_eq!(samples[0], 0);
                assert_eq!(samples[1], i16::MAX);
                assert_eq!(samples[2], i16::MIN);
            }
            _ => panic!("Expected S16"),
        }
    }

    #[test]
    fn test_pcm_to_samples_s24() {
        let pcm = vec![0x00, 0x00, 0x00, 0xFF, 0xFF, 0x7F, 0x00, 0x00, 0x80];
        let result = pcm_to_samples(&pcm, 24);
        match result {
            AudioSamples::S24(samples) => {
                assert_eq!(samples.len(), 3);
                assert_eq!(samples[0].into_i32(), 0);
                assert_eq!(samples[1].into_i32(), (1 << 23) - 1);
                assert_eq!(samples[2].into_i32(), -(1 << 23));
            }
            _ => panic!("Expected S24"),
        }
    }

    #[test]
    fn test_pcm_to_samples_s32() {
        let pcm = vec![
            0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0x7F, 0x00, 0x00, 0x00, 0x80,
        ];
        let result = pcm_to_samples(&pcm, 32);
        match result {
            AudioSamples::S32(samples) => {
                assert_eq!(samples.len(), 3);
                assert_eq!(samples[0], 0);
                assert_eq!(samples[1], i32::MAX);
                assert_eq!(samples[2], i32::MIN);
            }
            _ => panic!("Expected S32"),
        }
    }
}

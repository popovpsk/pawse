use audio_common::{
    AudioBatch, AudioError, AudioSamples, AudioSource, ChannelCount, I24, Metadata, StreamParams,
};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::LazyLock;
use std::time::Duration;
use symphonia::core::audio::{Audio, GenericAudioBufferRef};
use symphonia::core::codecs::audio::well_known::CODEC_ID_OPUS;
use symphonia::core::codecs::audio::{AudioCodecParameters, AudioDecoderOptions};
use symphonia::core::codecs::registry::CodecRegistry;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::packet::Packet;
use symphonia::core::units::{Time, Timestamp};

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
            for chunk in pcm.as_chunks::<2>().0 {
                samples.push(i16::from_le_bytes(*chunk));
            }
            AudioSamples::S16(samples)
        }
        24 => {
            let mut samples = Vec::with_capacity(pcm.len() / 3);
            for chunk in pcm.as_chunks::<3>().0 {
                let sign = if chunk[2] & 0x80 != 0 { 0xFFu8 } else { 0x00u8 };
                let raw = i32::from_le_bytes([chunk[0], chunk[1], chunk[2], sign]);
                samples.push(I24::new(raw));
            }
            AudioSamples::S24(samples)
        }
        32 => {
            let mut samples = Vec::with_capacity(pcm.len() / 4);
            for chunk in pcm.as_chunks::<4>().0 {
                samples.push(i32::from_le_bytes(*chunk));
            }
            AudioSamples::S32(samples)
        }
        _ => {
            let mut samples = Vec::with_capacity(pcm.len() / 2);
            for chunk in pcm.as_chunks::<2>().0 {
                samples.push(i16::from_le_bytes(*chunk));
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
// DSD source — uses the dsd crate for DSF/DFF (DSD -> PCM decimation)
// ============================================================================

struct DsdAdapter {
    inner: dsd::DsdSource,
}

impl DsdAdapter {
    fn open(path: &Path) -> Result<Self, AudioError> {
        let inner = dsd::DsdSource::open(path).map_err(|e| AudioError::Decoder(e.to_string()))?;
        Ok(Self { inner })
    }
}

impl AudioSource for DsdAdapter {
    fn params(&self) -> StreamParams {
        let p = self.inner.params();
        StreamParams::new(
            p.pcm_sample_rate,
            ChannelCount::from_u8(p.channels),
            p.pcm_bit_depth,
        )
        .with_dsd_rate(p.dsd_rate)
    }

    fn next_buffer(&mut self) -> Result<Option<AudioBatch>, AudioError> {
        let p = self.inner.params();
        let interleaved = self
            .inner
            .next_buffer()
            .map_err(|e| AudioError::Decoder(e.to_string()))?;

        Ok(interleaved.map(|data| AudioBatch {
            data: AudioSamples::F32(data),
            metadata: Metadata {
                sample_rate: p.pcm_sample_rate,
                channels: ChannelCount::from_u8(p.channels),
                bit_depth: p.pcm_bit_depth,
            },
        }))
    }

    fn seek(&mut self, position: f32) -> Result<Duration, AudioError> {
        self.inner
            .seek(position)
            .map_err(|e| AudioError::Decoder(e.to_string()))
    }

    fn duration(&self) -> Option<Duration> {
        self.inner.duration()
    }
}

// ============================================================================
// Symphonia decoder — handles all other formats via Symphonia
// ============================================================================

static CODECS: LazyLock<CodecRegistry> = LazyLock::new(|| {
    let mut registry = CodecRegistry::new();
    symphonia::default::register_enabled_codecs(&mut registry);
    registry.register_audio_decoder::<symphonia_adapter_libopus::OpusDecoder>();
    registry
});

const OPUS_PREROLL_MS: i64 = 320;

fn opus_output_gain(params: &AudioCodecParameters) -> Option<f32> {
    let head = params.extra_data.as_deref()?;
    if !head.starts_with(b"OpusHead") {
        return None;
    }
    let raw = i16::from_le_bytes([*head.get(16)?, *head.get(17)?]);
    (raw != 0).then(|| 10f32.powf(f32::from(raw) / (20.0 * 256.0)))
}

struct SymphoniaDecoder {
    format: Box<dyn symphonia::core::formats::FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::audio::AudioDecoder>,
    track_id: u32,
    codec_params: AudioCodecParameters,
    duration: Option<Duration>,
    start_time: Option<Time>,
    opus: bool,
    gain: Option<f32>,
    pending: Option<Packet>,
}

impl SymphoniaDecoder {
    fn open(path: &Path) -> Result<Self, AudioError> {
        let file = File::open(path).map_err(AudioError::Io)?;
        Self::from_source(
            Box::new(file),
            path.extension().and_then(|ext| ext.to_str()),
        )
    }

    fn from_source(
        source: Box<dyn symphonia::core::io::MediaSource>,
        extension: Option<&str>,
    ) -> Result<Self, AudioError> {
        let mss = MediaSourceStream::new(source, Default::default());

        let mut hint = Hint::new();
        if let Some(ext) = extension {
            hint.with_extension(ext);
        }

        let format = symphonia::default::get_probe()
            .probe(
                &hint,
                mss,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .map_err(|e| AudioError::Decoder(e.to_string()))?;

        let track = format
            .default_track(TrackType::Audio)
            .or_else(|| format.first_track(TrackType::Audio))
            .ok_or(AudioError::Decoder("No audio track found".to_string()))?;

        let track_id = track.id;
        let num_frames = track.num_frames;
        let start_time = track
            .time_base
            .and_then(|time_base| time_base.calc_time(track.start_ts));
        let codec_params = track
            .codec_params
            .as_ref()
            .and_then(|params| params.audio())
            .cloned()
            .ok_or(AudioError::Decoder("No audio codec parameters".to_string()))?;

        let sample_rate = codec_params
            .sample_rate
            .ok_or(AudioError::Decoder("No sample rate".to_string()))?;

        let duration = num_frames.map(|frames| {
            let secs = frames as f64 / sample_rate as f64;
            Duration::from_secs_f64(secs)
        });

        let decoder_opts = AudioDecoderOptions::default();
        let decoder = CODECS
            .make_audio_decoder(&codec_params, &decoder_opts)
            .map_err(|e| AudioError::Decoder(e.to_string()))?;

        let opus = codec_params.codec == CODEC_ID_OPUS;
        let gain = if opus {
            opus_output_gain(&codec_params)
        } else {
            None
        };

        Ok(Self {
            format,
            decoder,
            track_id,
            codec_params,
            duration,
            start_time,
            opus,
            gain,
            pending: None,
        })
    }

    fn discard_before(&mut self, target: Timestamp) -> Result<Option<Timestamp>, AudioError> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => return Ok(None),
                Err(symphonia::core::errors::Error::IoError(ref e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    return Ok(None);
                }
                Err(e) => return Err(AudioError::Decoder(e.to_string())),
            };
            if packet.track_id != self.track_id {
                continue;
            }
            if packet
                .pts
                .checked_add(packet.dur)
                .is_some_and(|end| end <= target)
            {
                let _ = self.decoder.decode(&packet);
                continue;
            }
            let first = packet.pts;
            self.pending = Some(packet);
            return Ok(Some(first));
        }
    }

    fn decode_next(&mut self) -> Result<Option<AudioBatch>, AudioError> {
        loop {
            let packet = match self.pending.take() {
                Some(packet) => packet,
                None => match self.format.next_packet() {
                    Ok(Some(p)) => p,
                    Ok(None) => return Ok(None),
                    Err(symphonia::core::errors::Error::IoError(ref e))
                        if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                    {
                        return Ok(None);
                    }
                    Err(e) => return Err(AudioError::Decoder(e.to_string())),
                },
            };

            if packet.track_id != self.track_id {
                continue;
            }

            let decoded = match self.decoder.decode(&packet) {
                Ok(decoded_buffer) => decoded_buffer,
                Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
                Err(e) => return Err(AudioError::Decoder(e.to_string())),
            };

            let symphonia_spec = decoded.spec();
            let sample_rate = symphonia_spec.rate();
            let channels = ChannelCount::from_u8(symphonia_spec.channels().count() as u8);

            let mut audio_sample = map_audio_buffer_ref(decoded);
            if let (Some(gain), AudioSamples::F32(samples)) = (self.gain, &mut audio_sample) {
                samples.iter_mut().for_each(|sample| *sample *= gain);
            }

            return Ok(Some(AudioBatch {
                data: audio_sample,
                metadata: Metadata {
                    sample_rate,
                    channels,
                    bit_depth: self.codec_params.bits_per_sample.unwrap_or(16) as u8,
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
            .as_ref()
            .map(|c| ChannelCount::from_u8(c.count() as u8))
            .unwrap_or(ChannelCount::Stereo);

        let bit_depth = self.codec_params.bits_per_sample.unwrap_or(16);

        StreamParams::new(sample_rate, channels, bit_depth as u8)
    }

    fn next_buffer(&mut self) -> Result<Option<AudioBatch>, AudioError> {
        self.decode_next()
    }

    fn seek(&mut self, position: f32) -> Result<Duration, AudioError> {
        let duration = self
            .duration
            .ok_or_else(|| AudioError::Decoder("Seek needs a known duration".to_string()))?
            .mul_f32(position);

        let target = Time::try_new(duration.as_secs() as i64, duration.subsec_nanos())
            .ok_or_else(|| AudioError::Decoder("Seek position out of range".to_string()))?;
        let target = self.start_time.map_or(target, |start| target.max(start));
        let seek_time = if self.opus {
            let earlier = target.checked_sub_millis(OPUS_PREROLL_MS).unwrap_or(target);
            self.start_time.map_or(earlier, |start| earlier.max(start))
        } else {
            target
        };

        let seeked = self
            .format
            .seek(
                symphonia::core::formats::SeekMode::Coarse,
                symphonia::core::formats::SeekTo::Time {
                    time: seek_time,
                    track_id: Some(self.track_id),
                },
            )
            .map_err(|e| AudioError::Decoder(e.to_string()))?;
        self.pending = None;

        let sample_rate = self
            .codec_params
            .sample_rate
            .ok_or_else(|| AudioError::Decoder("Sample rate unknown after seek".to_string()))?;
        let mut landed = seeked.actual_ts;
        if self.opus {
            self.decoder = CODECS
                .make_audio_decoder(&self.codec_params, &AudioDecoderOptions::default())
                .map_err(|e| AudioError::Decoder(e.to_string()))?;
            let target_ts =
                Timestamp::new((target.as_secs_f64() * sample_rate as f64).round() as i64);
            if let Some(first) = self.discard_before(target_ts)? {
                landed = first;
            }
        } else {
            self.decoder.reset();
        }

        let seconds = (landed.get() as f64 / sample_rate as f64).max(0.0);
        Ok(Duration::from_secs_f64(seconds))
    }

    fn duration(&self) -> Option<Duration> {
        self.duration
    }
}

// ============================================================================
// Combined Decoder — selects APE or Symphonia based on file extension
// ============================================================================

pub type Superseded = Box<dyn Fn() -> bool + Send + Sync>;

pub trait MediaStream: Read + Seek + Send + Sync {
    fn byte_len(&self) -> Option<u64>;

    fn give_up_waiting_when(&mut self, _superseded: Superseded) {}
}

struct StreamSource(Box<dyn MediaStream>);

impl Read for StreamSource {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf)
    }
}

impl Seek for StreamSource {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.0.seek(pos)
    }
}

impl symphonia::core::io::MediaSource for StreamSource {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        self.0.byte_len()
    }
}

pub fn can_stream(extension: &str) -> bool {
    !matches!(
        extension.to_ascii_lowercase().as_str(),
        "ape" | "dsf" | "dff"
    )
}

#[allow(private_interfaces)]
pub enum Decoder {
    Symphonia(Box<SymphoniaDecoder>),
    Ape(Box<ApeSource>),
    Dsd(Box<DsdAdapter>),
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
            "dsf" | "dff" => Ok(Decoder::Dsd(Box::new(DsdAdapter::open(path)?))),
            _ => Ok(Decoder::Symphonia(Box::new(SymphoniaDecoder::open(path)?))),
        }
    }

    pub fn open_stream(
        stream: Box<dyn MediaStream>,
        extension: Option<&str>,
    ) -> Result<Self, AudioError> {
        if extension.is_some_and(|ext| !can_stream(ext)) {
            return Err(AudioError::Decoder(format!(
                "{} files cannot be streamed",
                extension.unwrap_or_default()
            )));
        }
        Ok(Decoder::Symphonia(Box::new(SymphoniaDecoder::from_source(
            Box::new(StreamSource(stream)),
            extension,
        )?)))
    }
}

impl AudioSource for Decoder {
    fn params(&self) -> StreamParams {
        match self {
            Decoder::Symphonia(d) => d.params(),
            Decoder::Ape(d) => d.params(),
            Decoder::Dsd(d) => d.params(),
        }
    }

    fn next_buffer(&mut self) -> Result<Option<AudioBatch>, AudioError> {
        match self {
            Decoder::Symphonia(d) => d.next_buffer(),
            Decoder::Ape(d) => d.next_buffer(),
            Decoder::Dsd(d) => d.next_buffer(),
        }
    }

    fn seek(&mut self, position: f32) -> Result<Duration, AudioError> {
        match self {
            Decoder::Symphonia(d) => d.seek(position),
            Decoder::Ape(d) => d.seek(position),
            Decoder::Dsd(d) => d.seek(position),
        }
    }

    fn duration(&self) -> Option<Duration> {
        match self {
            Decoder::Symphonia(d) => d.duration(),
            Decoder::Ape(d) => d.duration(),
            Decoder::Dsd(d) => d.duration(),
        }
    }
}

// ============================================================================
// map_audio_buffer_ref — Symphonia planar → interleaved
// ============================================================================

fn map_audio_buffer_ref(decoded: GenericAudioBufferRef<'_>) -> AudioSamples {
    let frames = decoded.frames();
    let channels = decoded.spec().channels().count();
    let total_samples = frames * channels;

    match decoded {
        GenericAudioBufferRef::S16(buf) => {
            let mut interleaved = Vec::with_capacity(total_samples);
            interleaved.extend(buf.iter_interleaved());
            AudioSamples::S16(interleaved)
        }
        GenericAudioBufferRef::S24(buf) => {
            let mut interleaved: Vec<I24> = Vec::with_capacity(total_samples);
            interleaved.extend(buf.iter_interleaved().map(|s| I24::new(s.inner())));
            AudioSamples::S24(interleaved)
        }
        GenericAudioBufferRef::S32(buf) => {
            let mut interleaved = Vec::with_capacity(total_samples);
            interleaved.extend(buf.iter_interleaved());
            AudioSamples::S32(interleaved)
        }
        GenericAudioBufferRef::F32(buf) => {
            let mut interleaved = Vec::with_capacity(total_samples);
            interleaved.extend(buf.iter_interleaved());
            AudioSamples::F32(interleaved)
        }
        _ => {
            let mut interleaved: Vec<f32> = Vec::with_capacity(total_samples);
            decoded.copy_to_vec_interleaved(&mut interleaved);
            AudioSamples::F32(interleaved)
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

    #[test]
    fn test_vorbis_can_yield_an_empty_batch() {
        let mut decoder = Decoder::open(&fixture_path("tagged_ogg.ogg")).unwrap();
        let first = decoder.next_buffer().unwrap().unwrap();
        assert!(first.data.is_empty());
        let second = decoder.next_buffer().unwrap().unwrap();
        assert!(!second.data.is_empty());
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

    fn drain_f32(decoder: &mut Decoder) -> Vec<f32> {
        let mut out = Vec::new();
        while let Some(batch) = decoder.next_buffer().unwrap() {
            match batch.data {
                AudioSamples::F32(samples) => out.extend(samples),
                _ => panic!("expected F32 from opus"),
            }
        }
        out
    }

    #[test]
    fn opus_decodes_to_48k_stereo_float() {
        let mut decoder = Decoder::open(&fixture_path("tagged_opus.opus")).unwrap();
        let params = decoder.params();
        assert_eq!(params.sample_rate, 48_000);
        assert_eq!(params.channels, ChannelCount::Stereo);

        let samples = drain_f32(&mut decoder);
        let frames = samples.len() / 2;
        assert!(
            (frames as i64 - 96_000).abs() < 2_000,
            "expected about two seconds, got {frames} frames"
        );
        let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.05 && peak < 0.2, "peak {peak}");
    }

    #[test]
    fn opus_reports_its_duration() {
        let decoder = Decoder::open(&fixture_path("tagged_opus.opus")).unwrap();
        let secs = decoder.duration().unwrap().as_secs_f64();
        assert!((secs - 2.0).abs() < 0.05, "duration {secs}");
    }

    #[test]
    fn opus_seeks_and_keeps_decoding() {
        let mut decoder = Decoder::open(&fixture_path("tagged_opus.opus")).unwrap();
        let position = decoder.seek(0.5).unwrap().as_secs_f64();
        assert!((position - 1.0).abs() < 0.1, "landed at {position}");

        let samples = drain_f32(&mut decoder);
        let remaining = samples.len() as f64 / 2.0 / 48_000.0;
        assert!(
            (remaining - (2.0 - position)).abs() < 0.1,
            "remaining {remaining} after landing at {position}"
        );
        let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.05, "silent after seek");
    }

    #[test]
    fn opus_replays_identically_when_its_timeline_starts_late() {
        let mut decoder = Decoder::open(&fixture_path("late_start_opus.opus")).unwrap();
        let first = drain_f32(&mut decoder);

        let position = decoder.seek(0.0).unwrap().as_secs_f64();
        assert!(position < 0.05, "landed at {position}");
        assert_eq!(drain_f32(&mut decoder), first);
    }

    fn frame_error(a: &[f32], b: &[f32]) -> f64 {
        let diff: f64 = a.iter().zip(b).map(|(x, y)| f64::from(x - y).powi(2)).sum();
        let power: f64 = b.iter().map(|y| f64::from(*y).powi(2)).sum();
        (diff / power.max(1e-12)).sqrt()
    }

    #[test]
    fn opus_seek_matches_a_straight_decode() {
        let name = "late_start_opus.opus";
        let mut straight = Decoder::open(&fixture_path(name)).unwrap();
        let reference = drain_f32(&mut straight);

        for fraction in [0.2f32, 0.5, 0.8] {
            let mut decoder = Decoder::open(&fixture_path(name)).unwrap();
            let landed = decoder.seek(fraction).unwrap().as_secs_f64();
            let samples = drain_f32(&mut decoder);

            let expected = (landed * 48_000.0) as i64 - 936;
            let window = 960 * 2;
            let best = (expected - 40..expected + 40)
                .filter(|start| *start >= 0)
                .map(|start| {
                    let start = start as usize * 2;
                    (
                        frame_error(&samples[..window], &reference[start..start + window]),
                        start,
                    )
                })
                .fold(
                    (f64::MAX, 0),
                    |best, cur| if cur.0 < best.0 { cur } else { best },
                );
            assert!(
                best.0 < 0.01,
                "seek to {fraction} deviates by {:.2}% from a straight decode",
                best.0 * 100.0
            );
        }
    }

    fn opus_with_header_gain(raw: i16) -> Vec<u8> {
        fn ogg_crc(data: &[u8]) -> u32 {
            let mut crc = 0u32;
            for byte in data {
                crc ^= u32::from(*byte) << 24;
                for _ in 0..8 {
                    crc = if crc & 0x8000_0000 != 0 {
                        (crc << 1) ^ 0x04C1_1DB7
                    } else {
                        crc << 1
                    };
                }
            }
            crc
        }

        let mut bytes = std::fs::read(fixture_path("tagged_opus.opus")).unwrap();
        assert_eq!(&bytes[..4], b"OggS");
        let segments = bytes[26] as usize;
        let page_len = 27
            + segments
            + bytes[27..27 + segments]
                .iter()
                .map(|s| *s as usize)
                .sum::<usize>();
        let head = bytes[..page_len]
            .windows(8)
            .position(|w| w == b"OpusHead")
            .unwrap();
        bytes[head + 16..head + 18].copy_from_slice(&raw.to_le_bytes());
        bytes[22..26].copy_from_slice(&[0; 4]);
        let crc = ogg_crc(&bytes[..page_len]);
        bytes[22..26].copy_from_slice(&crc.to_le_bytes());
        bytes
    }

    #[test]
    fn opus_applies_the_header_output_gain() {
        let mut plain = Decoder::open(&fixture_path("tagged_opus.opus")).unwrap();
        let plain_peak = drain_f32(&mut plain)
            .iter()
            .fold(0.0f32, |m, s| m.max(s.abs()));

        let quieter = opus_with_header_gain(-1541);
        let stream = Box::new(MemoryStream(std::io::Cursor::new(quieter)));
        let mut decoder = Decoder::open_stream(stream, Some("opus")).unwrap();
        let peak = drain_f32(&mut decoder)
            .iter()
            .fold(0.0f32, |m, s| m.max(s.abs()));

        let ratio = peak / plain_peak;
        assert!((ratio - 0.5).abs() < 0.01, "gain ratio {ratio}");
    }

    #[test]
    fn opus_decodes_from_a_stream() {
        let mut from_file = Decoder::open(&fixture_path("tagged_opus.opus")).unwrap();
        let mut from_stream =
            Decoder::open_stream(memory_stream("tagged_opus.opus"), Some("opus")).unwrap();
        assert_eq!(from_stream.params(), from_file.params());
        assert_eq!(drain_f32(&mut from_stream), drain_f32(&mut from_file));
    }

    struct MemoryStream(std::io::Cursor<Vec<u8>>);

    impl Read for MemoryStream {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.0.read(buf)
        }
    }

    impl Seek for MemoryStream {
        fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
            self.0.seek(pos)
        }
    }

    impl MediaStream for MemoryStream {
        fn byte_len(&self) -> Option<u64> {
            Some(self.0.get_ref().len() as u64)
        }
    }

    fn memory_stream(filename: &str) -> Box<dyn MediaStream> {
        Box::new(MemoryStream(std::io::Cursor::new(
            std::fs::read(fixture_path(filename)).unwrap(),
        )))
    }

    #[test]
    fn a_stream_decodes_like_the_file_it_came_from() {
        let name = "sine_440_16_44_stereo.wav";
        let mut from_file = Decoder::open(&fixture_path(name)).unwrap();
        let mut from_stream = Decoder::open_stream(memory_stream(name), Some("wav")).unwrap();
        assert_eq!(from_stream.params(), from_file.params());
        assert_eq!(from_stream.duration(), from_file.duration());
        from_file.seek(0.5).unwrap();
        from_stream.seek(0.5).unwrap();
        let a = from_file.next_buffer().unwrap().unwrap();
        let b = from_stream.next_buffer().unwrap().unwrap();
        match (a.data, b.data) {
            (AudioSamples::S16(a), AudioSamples::S16(b)) => assert_eq!(a, b),
            _ => panic!("expected S16 from both"),
        }

        let unhinted = Decoder::open_stream(memory_stream(name), None).unwrap();
        assert_eq!(unhinted.params(), from_file.params());
    }

    #[test]
    fn formats_pinned_to_files_are_refused_as_streams() {
        assert!(!can_stream("APE"));
        assert!(!can_stream("dsf"));
        assert!(can_stream("flac"));
        assert!(
            Decoder::open_stream(memory_stream("sine_440_16_44_stereo.wav"), Some("dff")).is_err()
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

    fn write_minimal_dsf(path: &std::path::Path, channels: u32, dsd_rate: u32) {
        const BLOCK_SIZE: u32 = 64;
        let sample_count = (BLOCK_SIZE * 8) as u64;
        let data_size = BLOCK_SIZE as u64 * channels as u64;

        let mut buf = Vec::new();
        buf.extend_from_slice(b"DSD ");
        buf.extend_from_slice(&28u64.to_le_bytes());
        buf.extend_from_slice(&(28 + 52 + 12 + data_size).to_le_bytes());
        buf.extend_from_slice(&0u64.to_le_bytes());

        buf.extend_from_slice(b"fmt ");
        buf.extend_from_slice(&52u64.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&2u32.to_le_bytes());
        buf.extend_from_slice(&channels.to_le_bytes());
        buf.extend_from_slice(&dsd_rate.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&sample_count.to_le_bytes());
        buf.extend_from_slice(&BLOCK_SIZE.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());

        buf.extend_from_slice(b"data");
        buf.extend_from_slice(&(12 + data_size).to_le_bytes());
        buf.extend(std::iter::repeat_n(0x69u8, data_size as usize));

        std::fs::write(path, buf).unwrap();
    }

    #[test]
    fn test_dsd_decoder_reports_source_rate_and_target_pcm_rate() {
        let path = std::env::temp_dir().join("pawse_audio_decoder_test.dsf");
        write_minimal_dsf(&path, 2, 2_822_400);

        let mut decoder = Decoder::open(&path).expect("open dsf");
        let params = decoder.params();
        assert_eq!(params.dsd_rate, Some(2_822_400));
        assert_eq!(params.sample_rate, 352_800);
        assert_eq!(params.channels, ChannelCount::Stereo);

        let batch = decoder
            .next_buffer()
            .expect("decode")
            .expect("at least one batch");
        assert!(matches!(batch.data, AudioSamples::F32(_)));

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_dsd256_still_targets_dsd64s_native_pcm_rate_through_decoder() {
        let path = std::env::temp_dir().join("pawse_audio_decoder_test_dsd256.dsf");
        write_minimal_dsf(&path, 2, 2_822_400 * 4);

        let decoder = Decoder::open(&path).expect("open dsf");
        let params = decoder.params();
        assert_eq!(params.dsd_rate, Some(11_289_600));
        assert_eq!(
            params.sample_rate, 352_800,
            "DSD256 must reach the Decoder layer at the same fixed rate as DSD64"
        );

        std::fs::remove_file(&path).ok();
    }
}

use audio_common::{AudioBuffer, AudioError, AudioSource, ChannelCount, StreamParams};
use std::path::Path;
use std::time::Duration;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

pub struct Decoder {
    format: Box<dyn symphonia::core::formats::FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::Decoder>,
    track_id: u32,
    codec_params: CodecParameters,
    duration: Option<Duration>,
    sample_buffer: Option<SampleBuffer<f32>>,
}

impl Decoder {
    /// Открывает файл, инициализирует format reader и decoder.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, AudioError> {
        let path_ref = path.as_ref();

        // Открытие файла
        let file = std::fs::File::open(path_ref).map_err(AudioError::Io)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        // Определение формата (WAV, FLAC, MP3...)
        let mut hint = Hint::new();
        if let Some(ext) = path_ref.extension() {
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

        // Поиск первого аудио-трека
        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or(AudioError::Decoder("No audio track found".to_string()))?;

        let track_id = track.id;
        let codec_params = track.codec_params.clone();

        // Вычисление duration
        let sample_rate = codec_params
            .sample_rate
            .ok_or(AudioError::Decoder("No sample rate".to_string()))?;

        let duration = codec_params.n_frames.map(|frames| {
            let secs = frames as f64 / sample_rate as f64;
            Duration::from_secs_f64(secs)
        });

        // Создание декодера
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
            sample_buffer: None,
        })
    }

    fn decode_next(&mut self) -> Result<Option<AudioBuffer>, AudioError> {
        loop {
            // Читаем следующий пакет из файла
            let packet = match self.format.next_packet() {
                Ok(p) => p,
                Err(symphonia::core::errors::Error::IoError(ref e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    return Ok(None);
                }
                Err(e) => return Err(AudioError::Decoder(e.to_string())),
            };

            // Фильтруем пакеты других треков
            if packet.track_id() != self.track_id {
                continue;
            }

            // Декодируем пакет в сэмплы
            let decoded = match self.decoder.decode(&packet) {
                Ok(decoded_buffer) => decoded_buffer,
                Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
                Err(e) => return Err(AudioError::Decoder(e.to_string())),
            };

            let spec = *decoded.spec();
            let duration = decoded.capacity() as u64;

            // Конвертируем в f32, переиспользуем буфер
            let mut sample_buf = match self.sample_buffer.take() {
                Some(buf) => buf,
                None => SampleBuffer::<f32>::new(duration, spec),
            };

            sample_buf.copy_interleaved_ref(decoded);
            let samples = sample_buf.samples().to_vec();
            self.sample_buffer = Some(sample_buf);

            return Ok(Some(AudioBuffer::new(samples)));
        }
    }
}

impl AudioSource for Decoder {
    fn params(&self) -> StreamParams {
        let sample_rate = self.codec_params.sample_rate.unwrap_or(44100);
        let channels = self
            .codec_params
            .channels
            .map(|c: symphonia::core::audio::Channels| ChannelCount::from_u8(c.count() as u8))
            .unwrap_or(ChannelCount::Stereo);

        StreamParams::new(sample_rate, channels, 32)
    }

    fn next_buffer(&mut self) -> Result<Option<AudioBuffer>, AudioError> {
        self.decode_next()
    }

    fn seek(&mut self, position: Duration) -> Result<Duration, AudioError> {
        // SeekTo::Time - Symphonia сама выбирает способ поиска
        let time: symphonia::core::units::Time = position.into();

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

        // Конвертируем timestamp → Duration
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
        path.push(filename);
        path
    }

    #[rstest]
    #[case::sine_440_16_44_mono("sine_440_16_44_mono.wav", 44100, 32, ChannelCount::Mono)]
    #[case::sine_440_16_48_mono("sine_440_16_48_mono.wav", 48000, 32, ChannelCount::Mono)]
    #[case::sine_440_16_96_mono("sine_440_16_96_mono.wav", 96000, 32, ChannelCount::Mono)]
    #[case::sine_440_24_44_mono("sine_440_24_44_mono.wav", 44100, 32, ChannelCount::Mono)]
    #[case::sine_440_32_44_mono("sine_440_32_44_mono.wav", 44100, 32, ChannelCount::Mono)]
    #[case::sine_440_16_44_stereo("sine_440_16_44_stereo.wav", 44100, 32, ChannelCount::Stereo)]
    #[case::silence("silence_16_44_mono.wav", 44100, 32, ChannelCount::Mono)]
    #[case::original_1khz("1khz_16_44_1.wav", 44100, 32, ChannelCount::Mono)]
    fn test_decoder_params(
        #[case] filename: &str,
        #[case] sample_rate: u32,
        #[case] bit_depth: u8,
        #[case] channels: ChannelCount,
    ) {
        let path = fixture_path(filename);
        let decoder = Decoder::open(&path).expect(&format!("Failed to open {}", filename));

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
        let mut decoder = Decoder::open(&path).expect(&format!("Failed to open {}", filename));

        let buffer = decoder
            .next_buffer()
            .expect(&format!("Failed to decode {}", filename));
        assert!(
            buffer.is_some(),
            "Buffer should not be None for {}",
            filename
        );

        let audio_buffer = buffer.unwrap();
        let samples = audio_buffer.as_slice();
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
    #[case::original_1khz("1khz_16_44_1.wav")]
    fn test_samples_in_valid_range(#[case] filename: &str) {
        let path = fixture_path(filename);
        let mut decoder = Decoder::open(&path).expect(&format!("Failed to open {}", filename));

        let buffer = decoder
            .next_buffer()
            .expect(&format!("Failed to decode {}", filename));
        let audio_buffer = buffer.unwrap();
        let samples = audio_buffer.as_slice();

        for (i, &sample) in samples.iter().enumerate() {
            assert!(
                sample >= -1.0 && sample <= 1.0,
                "Sample {} out of range [-1.0, 1.0]: {} in {}",
                i,
                sample,
                filename
            );
        }
    }

    #[test]
    fn test_silence_samples_are_zero() {
        let path = fixture_path("silence_16_44_mono.wav");
        let mut decoder = Decoder::open(&path).expect("Failed to open silence file");

        let buffer = decoder.next_buffer().expect("Failed to decode silence");
        let audio_buffer = buffer.unwrap();
        let samples = audio_buffer.as_slice();

        for (i, &sample) in samples.iter().enumerate() {
            assert!(
                sample.abs() < 0.001,
                "Silence sample {} should be ~0, got {}",
                i,
                sample
            );
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
        let mut decoder = Decoder::open(&path).expect(&format!("Failed to open {}", filename));

        let result = decoder
            .seek(Duration::ZERO)
            .expect(&format!("Failed to seek in {}", filename));
        assert_eq!(result, Duration::ZERO, "Seek to zero should return zero");

        let buffer = decoder
            .next_buffer()
            .expect(&format!("Failed to decode after seek in {}", filename));
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
        let mut decoder = Decoder::open(&path).expect(&format!("Failed to open {}", filename));

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

    // Тест: проверка точной продолжительности
    #[rstest]
    #[case::original_1khz("1khz_16_44_1.wav", 2.0)]
    #[case::sine_440_16_44_mono("sine_440_16_44_mono.wav", 0.5)]
    #[case::sine_440_16_44_mono("sine_440_16_44_mono.wav", 0.5)]
    #[case::sine_440_16_48_mono("sine_440_16_48_mono.wav", 0.5)]
    #[case::sine_440_16_96_mono("sine_440_16_96_mono.wav", 0.5)]
    fn test_duration_exact(#[case] filename: &str, #[case] expected_secs: f64) {
        let path = fixture_path(filename);
        let decoder = Decoder::open(&path).expect(&format!("Failed to open {}", filename));

        let duration = decoder
            .duration()
            .expect(&format!("Duration should exist for {}", filename));

        let actual_secs = duration.as_secs_f64();
        assert!(
            (actual_secs - expected_secs).abs() < 0.01,
            "Duration mismatch for {}: expected {}s, got {:?}",
            filename,
            expected_secs,
            duration
        );
    }
}

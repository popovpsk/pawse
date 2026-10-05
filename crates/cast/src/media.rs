use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use audio_common::AudioSource;
use audio_decoder::{Codec, MediaStream};

use crate::pcm::{Container, PcmSpec, open_decoder};

pub type StreamOpener = Arc<dyn Fn() -> io::Result<Box<dyn MediaStream>> + Send + Sync>;

#[derive(Clone)]
pub enum Source {
    File(PathBuf),
    Stream(StreamOpener),
}

impl Source {
    pub fn byte_len(&self) -> io::Result<Option<u64>> {
        match self {
            Source::File(path) => Ok(Some(std::fs::metadata(path)?.len())),
            Source::Stream(open) => Ok(open()?.byte_len()),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrackInfo {
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
}

#[derive(Clone, Default)]
pub struct Cover {
    pub bytes: Arc<Vec<u8>>,
    pub mime: String,
}

#[derive(Clone)]
pub struct Media {
    pub source: Source,
    pub extension: String,
    pub start: Duration,
    pub length: Option<Duration>,
    pub info: TrackInfo,
    pub cover: Option<Cover>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Probe {
    pub codec: Codec,
    pub sample_rate: u32,
    pub channels: u16,
    pub bit_depth: u8,
    pub duration: Option<Duration>,
}

pub fn probe(source: &Source, extension: &str) -> Result<Probe, String> {
    let decoder = open_decoder(source, extension).map_err(|e| e.to_string())?;
    let params = decoder.params();
    Ok(Probe {
        codec: decoder.codec(),
        sample_rate: params.sample_rate,
        channels: u16::from(params.channels_count()).max(1),
        bit_depth: params.bit_depth,
        duration: decoder.duration(),
    })
}

#[derive(Debug, Clone)]
pub enum Delivery {
    Original { mime: String },
    Pcm(PcmSpec),
}

pub trait Accepts {
    fn original(&self, codec: Codec, extension: &str, probe: &Probe) -> Option<String>;
    fn pcm(&self) -> Container;
    fn pcm_limits(&self) -> (u32, u16);
    fn wants_seek_table(&self) -> bool {
        false
    }
    fn takes_pcm(&self) -> bool {
        true
    }
}

fn reduced_rate(rate: u32, max: u32) -> u32 {
    let mut out = rate;
    while out > max && out.is_multiple_of(2) {
        out /= 2;
    }
    out.min(max)
}

pub fn plan(media: &Media, probe: &Probe, accepts: &dyn Accepts) -> Result<Delivery, String> {
    let segment = !media.start.is_zero() || media.length.is_some();
    if !segment && let Some(mime) = accepts.original(probe.codec, &media.extension, probe) {
        return Ok(Delivery::Original { mime });
    }
    let container = accepts.pcm();
    let length = media
        .length
        .or_else(|| {
            probe
                .duration
                .map(|total| total.saturating_sub(media.start))
        })
        .ok_or("the track length is unknown, so it cannot be converted for this device")?;
    let bits = if container == Container::L16
        || (probe.bit_depth <= 16 && !matches!(probe.codec, Codec::Dsd))
    {
        16
    } else {
        24
    };
    let (max_rate, max_channels) = accepts.pcm_limits();
    let sample_rate = reduced_rate(probe.sample_rate, max_rate);
    let channels = probe.channels.min(max_channels.max(1));
    let frames = (length.as_secs_f64() * f64::from(sample_rate)).round() as u64;
    Ok(Delivery::Pcm(PcmSpec {
        source: media.source.clone(),
        extension: media.extension.clone(),
        start: media.start,
        frames,
        sample_rate,
        channels,
        source_rate: probe.sample_rate,
        source_channels: probe.channels,
        bits,
        container,
    }))
}

pub fn dlna_mimes(codec: Codec, extension: &str) -> &'static [&'static str] {
    match codec {
        Codec::Mp3 => &["audio/mpeg", "audio/mp3", "audio/x-mpeg"],
        Codec::Flac if extension == "flac" => &["audio/flac", "audio/x-flac"],
        Codec::Aac if extension == "aac" => &["audio/aac", "audio/x-aac", "audio/vnd.dlna.adts"],
        Codec::Aac => &[
            "audio/mp4",
            "audio/x-m4a",
            "audio/m4a",
            "audio/aac",
            "audio/mp4a-latm",
        ],
        Codec::Alac => &["audio/x-alac", "audio/alac"],
        Codec::Vorbis | Codec::Opus | Codec::Flac
            if matches!(extension, "ogg" | "oga" | "opus") =>
        {
            &["audio/ogg", "application/ogg", "audio/x-ogg", "audio/opus"]
        }
        Codec::Pcm if extension == "wav" => &["audio/wav", "audio/x-wav", "audio/wave"],
        Codec::Pcm if matches!(extension, "aif" | "aiff") => &["audio/aiff", "audio/x-aiff"],
        _ => &[],
    }
}

pub fn chromecast_mime(codec: Codec, extension: &str, probe: &Probe) -> Option<&'static str> {
    if probe.sample_rate > 96_000 || probe.channels > 2 {
        return None;
    }
    match (codec, extension) {
        (Codec::Mp3, _) => Some("audio/mpeg"),
        (Codec::Aac, "aac") => Some("audio/aac"),
        (Codec::Aac, "m4a" | "mp4" | "m4b") => Some("audio/mp4"),
        (Codec::Flac, "flac") if probe.bit_depth <= 24 => Some("audio/flac"),
        (Codec::Opus, "webm") => Some("audio/webm"),
        (Codec::Pcm, "wav") => Some("audio/wav"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed(Option<&'static str>, Container);

    impl Accepts for Fixed {
        fn original(&self, _: Codec, _: &str, _: &Probe) -> Option<String> {
            self.0.map(str::to_string)
        }
        fn pcm(&self) -> Container {
            self.1
        }
        fn pcm_limits(&self) -> (u32, u16) {
            (96_000, 2)
        }
    }

    fn media(start: Duration, length: Option<Duration>) -> Media {
        Media {
            source: Source::File("/nonexistent.flac".into()),
            extension: "flac".into(),
            start,
            length,
            info: TrackInfo::default(),
            cover: None,
        }
    }

    fn probe(bit_depth: u8, codec: Codec) -> Probe {
        Probe {
            codec,
            sample_rate: 48_000,
            channels: 2,
            bit_depth,
            duration: Some(Duration::from_secs(100)),
        }
    }

    #[test]
    fn a_whole_file_the_device_reads_is_sent_as_it_is() {
        let delivery = plan(
            &media(Duration::ZERO, None),
            &probe(16, Codec::Flac),
            &Fixed(Some("audio/flac"), Container::Wav),
        )
        .unwrap();
        assert!(matches!(delivery, Delivery::Original { mime } if mime == "audio/flac"));
    }

    #[test]
    fn a_cue_track_is_cut_into_pcm_even_when_the_codec_is_supported() {
        let delivery = plan(
            &media(Duration::from_secs(30), Some(Duration::from_secs(10))),
            &probe(24, Codec::Flac),
            &Fixed(Some("audio/flac"), Container::Wav),
        )
        .unwrap();
        let Delivery::Pcm(spec) = delivery else {
            panic!("expected PCM");
        };
        assert_eq!(spec.start, Duration::from_secs(30));
        assert_eq!(spec.frames, 480_000);
        assert_eq!(spec.bits, 24);
    }

    #[test]
    fn the_first_cue_track_is_cut_too() {
        let delivery = plan(
            &media(Duration::ZERO, Some(Duration::from_secs(10))),
            &probe(16, Codec::Flac),
            &Fixed(Some("audio/flac"), Container::Wav),
        )
        .unwrap();
        let Delivery::Pcm(spec) = delivery else {
            panic!("expected PCM");
        };
        assert_eq!(spec.frames, 480_000);
    }

    #[test]
    fn unsupported_codecs_become_pcm_of_the_remaining_length() {
        let Delivery::Pcm(spec) = plan(
            &media(Duration::ZERO, None),
            &probe(16, Codec::Ape),
            &Fixed(None, Container::L16),
        )
        .unwrap() else {
            panic!("expected PCM");
        };
        assert_eq!(spec.frames, 4_800_000);
        assert_eq!(spec.bits, 16);
        assert_eq!(spec.container, Container::L16);
    }

    #[test]
    fn hi_res_and_surround_are_reduced_to_the_device_limits() {
        let mut big = probe(24, Codec::Flac);
        big.sample_rate = 192_000;
        big.channels = 6;
        let Delivery::Pcm(spec) = plan(
            &media(Duration::ZERO, None),
            &big,
            &Fixed(None, Container::Wav),
        )
        .unwrap() else {
            panic!("expected PCM");
        };
        assert_eq!((spec.sample_rate, spec.channels), (96_000, 2));
        assert_eq!((spec.source_rate, spec.source_channels), (192_000, 6));
        assert_eq!(spec.frames, 9_600_000);
        assert_eq!(reduced_rate(352_800, 96_000), 88_200);
        assert_eq!(reduced_rate(48_000, 96_000), 48_000);
        assert_eq!(reduced_rate(99_999, 96_000), 96_000);
    }

    #[test]
    fn chromecast_takes_common_formats_up_to_96_khz() {
        let mut stereo = probe(16, Codec::Flac);
        assert_eq!(
            chromecast_mime(Codec::Flac, "flac", &stereo),
            Some("audio/flac")
        );
        assert_eq!(chromecast_mime(Codec::Alac, "m4a", &stereo), None);
        assert_eq!(
            chromecast_mime(Codec::Aac, "m4a", &stereo),
            Some("audio/mp4")
        );
        stereo.sample_rate = 192_000;
        assert_eq!(chromecast_mime(Codec::Flac, "flac", &stereo), None);
    }

    #[test]
    fn chromecast_gets_ogg_as_pcm_and_opus_in_webm_as_it_is() {
        let opus = Probe {
            sample_rate: 48_000,
            ..probe(16, Codec::Opus)
        };
        assert_eq!(chromecast_mime(Codec::Opus, "opus", &opus), None);
        assert_eq!(chromecast_mime(Codec::Vorbis, "ogg", &opus), None);
        assert_eq!(
            chromecast_mime(Codec::Opus, "webm", &opus),
            Some("audio/webm")
        );
        let Delivery::Pcm(spec) = plan(
            &media(Duration::ZERO, None),
            &opus,
            &Fixed(chromecast_mime(Codec::Opus, "opus", &opus), Container::Wav),
        )
        .unwrap() else {
            panic!("expected PCM");
        };
        assert_eq!((spec.sample_rate, spec.bits), (48_000, 16));
    }
}

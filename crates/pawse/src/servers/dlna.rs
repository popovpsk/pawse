use music_library::RemoteSong;

use super::{
    RemoteConfig, RemoteError, ServerClient, joined_genres, real_album, real_artist,
    real_track_number,
};

const MAX_KBPS: u64 = 20_000;
const AUDIO_EXTENSIONS: [&str; 18] = [
    "mp3", "flac", "m4a", "mp4", "aac", "alac", "ogg", "oga", "opus", "wma", "wav", "aif", "aiff",
    "dsf", "dff", "ape", "wv", "mka",
];

pub struct Dlna(dlna::Client, dlna::Config);

impl Dlna {
    pub fn new(config: &dlna::Config) -> Self {
        Self(dlna::Client::new(config), config.clone())
    }
}

pub fn error(error: dlna::Error) -> RemoteError {
    match error {
        dlna::Error::Auth => RemoteError::Auth,
        dlna::Error::Transient(message) => RemoteError::Unreachable(message),
        dlna::Error::Server(message) => RemoteError::Other(message),
    }
}

pub fn describe(address: &str) -> Result<dlna::Device, RemoteError> {
    dlna::describe(address).map_err(error)
}

impl ServerClient for Dlna {
    fn ping(&self) -> Result<(), RemoteError> {
        self.0.ping().map_err(error)
    }

    fn songs(&self) -> Result<Vec<RemoteSong>, RemoteError> {
        Ok(self
            .0
            .items()
            .map_err(error)?
            .into_iter()
            .filter_map(song)
            .collect())
    }

    fn favorite_keys(&self) -> Result<Vec<String>, RemoteError> {
        Ok(Vec::new())
    }

    fn cover_art(&self, key: &str, _max_size: u32) -> Result<Vec<u8>, RemoteError> {
        self.0.cover(key).map_err(error)
    }

    fn fetch_range(
        &self,
        key: &str,
        start: u64,
        end: Option<u64>,
    ) -> Result<server_http::RangeBody, RemoteError> {
        self.0.fetch_range(key, start, end).map_err(error)
    }

    fn moved(&self) -> Option<RemoteConfig> {
        let location = self.0.location();
        (location != self.1.location).then(|| {
            RemoteConfig::Dlna(dlna::Config {
                udn: self.1.udn.clone(),
                location,
            })
        })
    }
}

fn song(item: dlna::Item) -> Option<RemoteSong> {
    let res = item.pick()?.clone();
    let cover_key = item.cover_id();
    let mut artists = item
        .artists
        .into_iter()
        .filter_map(|name| real_artist(Some(name)));
    let artist = artists.next();
    let mime = mime(&res.protocol_info);
    Some(RemoteSong {
        key: res.id(),
        title: item.title,
        artist,
        artist_aliases: artists.collect(),
        album: real_album(item.album),
        album_artist: real_artist(item.album_artists.into_iter().next()),
        track_number: real_track_number(item.track_number),
        disc_number: item.disc_number,
        year: item.date.as_deref().and_then(year),
        genre: joined_genres(&item.genres),
        duration_ms: res.duration_ms.map(|ms| ms as i64),
        size: res.size.map(|size| size as i64),
        suffix: suffix(&res.key, mime),
        content_type: (!mime.is_empty()).then(|| mime.to_string()),
        bitrate_kbps: bitrate_kbps(&res),
        cover_key,
        cover_hash: None,
        start_offset_ms: None,
    })
}

fn mime(protocol_info: &str) -> &str {
    let mime = protocol_info.split(':').nth(2).unwrap_or("");
    mime.split(';').next().unwrap_or("").trim()
}

fn year(date: &str) -> Option<i32> {
    let digits = date.trim().get(..4)?;
    digits
        .bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| digits.parse().ok())
        .flatten()
        .filter(|year| *year > 0)
}

fn bitrate_kbps(res: &dlna::Res) -> Option<u32> {
    let measured = match (res.size, res.duration_ms) {
        (Some(size), Some(ms)) if size > 0 && ms > 0 => Some(size * 8 / ms),
        _ => None,
    };
    let declared = res.bitrate.filter(|raw| *raw > 0).map(|raw| {
        let from_bytes = raw * 8 / 1000;
        if from_bytes > MAX_KBPS {
            raw / 1000
        } else {
            from_bytes
        }
    });
    measured
        .or(declared)
        .and_then(|kbps| u32::try_from(kbps).ok())
        .filter(|kbps| *kbps > 0)
}

fn suffix(key: &str, mime: &str) -> Option<String> {
    let path = key.split(['?', '#']).next().unwrap_or(key);
    let from_path = path
        .rsplit('/')
        .next()
        .and_then(|name| name.rsplit_once('.'))
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .filter(|extension| AUDIO_EXTENSIONS.contains(&extension.as_str()));
    if from_path.is_some() {
        return from_path;
    }
    let from_mime = match mime.to_ascii_lowercase().as_str() {
        "audio/mpeg" | "audio/mp3" | "audio/x-mpeg" => "mp3",
        "audio/flac" | "audio/x-flac" => "flac",
        "audio/mp4" | "audio/x-m4a" | "audio/m4a" => "m4a",
        "audio/aac" | "audio/x-aac" | "audio/aacp" => "aac",
        "audio/ogg" | "audio/x-ogg" | "application/ogg" | "audio/vorbis" => "ogg",
        "audio/opus" => "opus",
        "audio/x-ms-wma" | "audio/wma" => "wma",
        "audio/wav" | "audio/x-wav" | "audio/wave" | "audio/vnd.wave" => "wav",
        "audio/aiff" | "audio/x-aiff" => "aiff",
        "audio/dsf" | "audio/x-dsf" => "dsf",
        "audio/dff" | "audio/x-dff" => "dff",
        "audio/ape" | "audio/x-ape" | "audio/x-monkeys-audio" => "ape",
        "audio/wavpack" | "audio/x-wavpack" => "wv",
        _ => return None,
    };
    Some(from_mime.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn res(key: &str, protocol_info: &str) -> dlna::Res {
        dlna::Res {
            key: key.into(),
            protocol_info: protocol_info.into(),
            ..Default::default()
        }
    }

    #[test]
    fn an_item_becomes_a_song_keyed_by_its_media_path() {
        let converted = song(dlna::Item {
            id: "64$1".into(),
            title: "Song".into(),
            artists: vec!["Singer".into(), "Guest".into()],
            album_artists: vec!["Band".into()],
            album: Some("Record".into()),
            track_number: Some(3),
            date: Some("1997-05-01".into()),
            genres: vec!["Rock".into(), "Pop".into()],
            album_art: Some("/AlbumArt/7-12.jpg".into()),
            res: vec![dlna::Res {
                size: Some(30_000_000),
                duration_ms: Some(250_000),
                ..res("/MediaItems/12.flac", "http-get:*:audio/x-flac:*")
            }],
            ..Default::default()
        })
        .unwrap();
        assert_eq!(converted.key, "/MediaItems/12.flac#30000000");
        assert_eq!(converted.artist.as_deref(), Some("Singer"));
        assert_eq!(converted.artist_aliases, vec!["Guest".to_string()]);
        assert_eq!(converted.album_artist.as_deref(), Some("Band"));
        assert_eq!(converted.year, Some(1997));
        assert_eq!(converted.genre.as_deref(), Some("Rock; Pop"));
        assert_eq!(converted.duration_ms, Some(250_000));
        assert_eq!(converted.size, Some(30_000_000));
        assert_eq!(converted.bitrate_kbps, Some(960));
        assert_eq!(converted.suffix.as_deref(), Some("flac"));
        assert_eq!(converted.content_type.as_deref(), Some("audio/x-flac"));
        assert_eq!(
            converted.cover_key.as_deref(),
            Some("/AlbumArt/7-12.jpg#30000000")
        );
    }

    #[test]
    fn an_item_with_nothing_playable_is_skipped() {
        let item = dlna::Item {
            res: vec![res("/pcm", "http-get:*:audio/L16;rate=44100:*")],
            ..Default::default()
        };
        assert_eq!(song(item), None);
    }

    #[test]
    fn server_placeholders_for_missing_tags_are_dropped() {
        let converted = song(dlna::Item {
            artists: vec!["[Unknown Artist]".into()],
            album: Some("[Unknown Album]".into()),
            track_number: Some(1997),
            res: vec![res("/a.mp3", "http-get:*:audio/mpeg:*")],
            ..Default::default()
        })
        .unwrap();
        assert_eq!(converted.artist, None);
        assert_eq!(converted.album, None);
        assert_eq!(converted.track_number, None);
    }

    #[test]
    fn the_extension_comes_from_the_path_then_the_mime_type() {
        assert_eq!(
            suffix(
                "/content/media/object_id/5/res_id/0/ext/file.FLAC",
                "audio/mpeg"
            )
            .as_deref(),
            Some("flac")
        );
        assert_eq!(
            suffix("/resource/123/MEDIA_ITEM/FLAC-0/ORIGINAL", "audio/x-flac").as_deref(),
            Some("flac")
        );
        assert_eq!(
            suffix("/get/12.bin?x=1.mp3", "audio/mp4").as_deref(),
            Some("m4a")
        );
        assert_eq!(suffix("/stream", "video/mp4"), None);
        assert_eq!(
            suffix("/MediaItems/32.dat", "audio/ogg").as_deref(),
            Some("ogg")
        );
        assert_eq!(
            mime("http-get:*:audio/L16;rate=44100;channels=2:DLNA.ORG_PN=LPCM"),
            "audio/L16"
        );
    }

    #[test]
    fn the_bitrate_is_measured_or_read_in_bytes_per_second() {
        let measured = dlna::Res {
            size: Some(8_000_000),
            duration_ms: Some(200_000),
            bitrate: Some(1),
            ..Default::default()
        };
        assert_eq!(bitrate_kbps(&measured), Some(320));
        let bytes = dlna::Res {
            bitrate: Some(40_000),
            ..Default::default()
        };
        assert_eq!(bitrate_kbps(&bytes), Some(320));
        let bits = dlna::Res {
            bitrate: Some(4_608_000),
            ..Default::default()
        };
        assert_eq!(bitrate_kbps(&bits), Some(4_608));
        let at_threshold = dlna::Res {
            bitrate: Some(2_500_000),
            ..Default::default()
        };
        assert_eq!(bitrate_kbps(&at_threshold), Some(20_000));
        let past_threshold = dlna::Res {
            bitrate: Some(2_500_125),
            ..Default::default()
        };
        assert_eq!(bitrate_kbps(&past_threshold), Some(2_500));
        assert_eq!(bitrate_kbps(&dlna::Res::default()), None);
    }

    #[test]
    fn only_a_leading_four_digit_year_counts() {
        assert_eq!(year("2003"), Some(2003));
        assert_eq!(year("2003-01-01T00:00:00"), Some(2003));
        assert_eq!(year("03-01-01"), None);
        assert_eq!(year("0000"), None);
    }

    #[test]
    fn a_server_that_did_not_move_asks_for_nothing_to_be_saved() {
        let client = Dlna::new(&dlna::Config {
            udn: "uuid:a".into(),
            location: "http://10.0.0.5:8200/rootDesc.xml".into(),
        });
        assert_eq!(client.moved(), None);
    }

    #[test]
    fn errors_keep_the_auth_and_unreachable_split() {
        assert_eq!(error(dlna::Error::Auth), RemoteError::Auth);
        assert_eq!(
            error(dlna::Error::Transient("down".into())),
            RemoteError::Unreachable("down".into())
        );
        assert_eq!(
            error(dlna::Error::Server("x".into())),
            RemoteError::Other("x".into())
        );
    }
}

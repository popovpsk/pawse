use music_library::RemoteSong;

use super::{RemoteError, ServerClient, real_album, real_artist, real_track_number};

const KNOWN_CONTAINERS: [&str; 14] = [
    "flac", "mp3", "m4a", "mp4", "aac", "ogg", "opus", "wav", "aiff", "aif", "ape", "wv", "dsf",
    "dff",
];

pub struct Jellyfin(jellyfin::Client);

impl Jellyfin {
    pub fn new(config: &jellyfin::Config) -> Self {
        Self(jellyfin::Client::new(config))
    }
}

fn error(error: jellyfin::Error) -> RemoteError {
    match error {
        jellyfin::Error::Auth => RemoteError::Auth,
        jellyfin::Error::Transient(message) => RemoteError::Unreachable(message),
        jellyfin::Error::Server(message) => RemoteError::Other(message),
    }
}

pub fn authenticate(
    url: &str,
    username: &str,
    password: &str,
    device_id: &str,
) -> Result<jellyfin::Config, RemoteError> {
    jellyfin::authenticate(url, username, password, device_id).map_err(error)
}

impl ServerClient for Jellyfin {
    fn ping(&self) -> Result<(), RemoteError> {
        self.0.ping().map_err(error)
    }

    fn songs(&self) -> Result<Vec<RemoteSong>, RemoteError> {
        Ok(self
            .0
            .songs()
            .map_err(error)?
            .into_iter()
            .map(song)
            .collect())
    }

    fn favorite_keys(&self) -> Result<Vec<String>, RemoteError> {
        Ok(self
            .0
            .favorites()
            .map_err(error)?
            .into_iter()
            .map(|item| item.id)
            .collect())
    }

    fn cover_art(&self, key: &str) -> Result<Vec<u8>, RemoteError> {
        self.0.cover_art(key).map_err(error)
    }

    fn fetch_range(
        &self,
        key: &str,
        start: u64,
        end: Option<u64>,
    ) -> Result<server_http::RangeBody, RemoteError> {
        self.0.fetch_range(key, start, end).map_err(error)
    }
}

fn extension(item: &jellyfin::Item) -> Option<String> {
    let from_path = item
        .file_path()
        .and_then(|path| path.rsplit(['/', '\\']).next())
        .and_then(|name| name.rsplit_once('.'))
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .filter(|ext| !ext.is_empty());
    let from_container = || {
        let container = item.container.as_deref().or_else(|| {
            item.media_sources
                .iter()
                .find_map(|source| source.container.as_deref())
        })?;
        let tokens: Vec<String> = container
            .split(',')
            .map(|token| token.trim().to_ascii_lowercase())
            .filter(|token| !token.is_empty())
            .collect();
        KNOWN_CONTAINERS
            .iter()
            .find(|known| tokens.iter().any(|token| token == *known))
            .map(|known| known.to_string())
            .or_else(|| tokens.first().cloned())
    };
    from_path.or_else(from_container)
}

fn song(item: jellyfin::Item) -> RemoteSong {
    let artists: Vec<String> = item
        .artists
        .iter()
        .filter_map(|name| real_artist(Some(name.clone())))
        .collect();
    let album_artist = real_artist(
        item.album_artists
            .iter()
            .map(|named| named.name.trim())
            .find(|name| !name.is_empty())
            .map(str::to_string)
            .or(item.album_artist.clone()),
    );
    RemoteSong {
        title: item.name.clone(),
        artist: artists.first().cloned().or_else(|| album_artist.clone()),
        artist_aliases: artists.iter().skip(1).cloned().collect(),
        album: real_album(item.album.clone()),
        album_artist,
        track_number: real_track_number(item.index_number),
        disc_number: item.parent_index_number,
        year: item.production_year,
        genre: item.genres.first().cloned(),
        duration_ms: item.duration_ms().map(|ms| ms as i64),
        size: item.size().map(|size| size as i64),
        suffix: extension(&item),
        content_type: None,
        bitrate_kbps: item.bitrate().map(|bps| bps / 1000),
        cover_key: item.cover_key().map(str::to_string),
        cover_hash: None,
        key: item.id,
        start_offset_ms: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item() -> jellyfin::Item {
        jellyfin::Item {
            id: "1".into(),
            name: "Song".into(),
            ..Default::default()
        }
    }

    #[test]
    fn the_extension_comes_from_the_file_then_from_the_container() {
        let with_path = jellyfin::Item {
            path: Some("/m/Some.Dir/track.FLAC".into()),
            container: Some("mp3".into()),
            ..item()
        };
        assert_eq!(song(with_path).suffix.as_deref(), Some("flac"));
        let mp4_family = jellyfin::Item {
            container: Some("mov,mp4,m4a,3gp".into()),
            ..item()
        };
        assert_eq!(song(mp4_family).suffix.as_deref(), Some("m4a"));
        let from_source = jellyfin::Item {
            media_sources: vec![jellyfin::MediaSource {
                container: Some("opus".into()),
                ..Default::default()
            }],
            ..item()
        };
        assert_eq!(song(from_source).suffix.as_deref(), Some("opus"));
        let unknown = jellyfin::Item {
            container: Some("weird".into()),
            ..item()
        };
        assert_eq!(song(unknown).suffix.as_deref(), Some("weird"));
        let windows = jellyfin::Item {
            path: Some("D:\\Music\\a.ogg".into()),
            ..item()
        };
        assert_eq!(song(windows).suffix.as_deref(), Some("ogg"));
        assert_eq!(song(item()).suffix, None);
    }

    #[test]
    fn artists_come_from_the_split_list_and_fall_back_to_the_album_artist() {
        let split = jellyfin::Item {
            artists: vec!["A".into(), " ".into(), "B".into()],
            album_artist: Some("A & B".into()),
            ..item()
        };
        let converted = song(split);
        assert_eq!(converted.artist.as_deref(), Some("A"));
        assert_eq!(converted.artist_aliases, vec!["B".to_string()]);
        assert_eq!(converted.album_artist.as_deref(), Some("A & B"));

        let only_album_artist = jellyfin::Item {
            album_artists: vec![jellyfin::Named {
                name: "Band".into(),
            }],
            ..item()
        };
        assert_eq!(song(only_album_artist).artist.as_deref(), Some("Band"));
    }

    #[test]
    fn units_are_converted_to_the_library_ones() {
        let converted = song(jellyfin::Item {
            run_time_ticks: Some(1_855_000_000),
            media_sources: vec![jellyfin::MediaSource {
                size: Some(10),
                bitrate: Some(256_000),
                ..Default::default()
            }],
            index_number: Some(1997),
            parent_index_number: Some(2),
            ..item()
        });
        assert_eq!(converted.duration_ms, Some(185_500));
        assert_eq!(converted.bitrate_kbps, Some(256));
        assert_eq!(converted.size, Some(10));
        assert_eq!(converted.track_number, None);
        assert_eq!(converted.disc_number, Some(2));
    }

    #[test]
    fn covers_are_keyed_by_album_when_the_album_has_art() {
        let album = jellyfin::Item {
            album_id: Some("al".into()),
            album_primary_image_tag: Some("t".into()),
            image_tags: jellyfin::ImageTags {
                primary: Some("own".into()),
            },
            ..item()
        };
        assert_eq!(song(album).cover_key.as_deref(), Some("al"));
        let own = jellyfin::Item {
            album_id: Some("al".into()),
            image_tags: jellyfin::ImageTags {
                primary: Some("own".into()),
            },
            ..item()
        };
        assert_eq!(song(own).cover_key.as_deref(), Some("1"));
        assert_eq!(song(item()).cover_key, None);
    }

    #[test]
    fn errors_keep_the_auth_and_unreachable_split() {
        assert_eq!(error(jellyfin::Error::Auth), RemoteError::Auth);
        assert_eq!(
            error(jellyfin::Error::Transient("down".into())),
            RemoteError::Unreachable("down".into())
        );
        assert_eq!(
            error(jellyfin::Error::Server("x".into())),
            RemoteError::Other("x".into())
        );
    }
}

use music_library::RemoteSong;

use super::{RemoteError, ServerClient, real_album, real_artist, real_track_number};

pub struct Subsonic(subsonic::Client);

impl Subsonic {
    pub fn new(config: &subsonic::Config) -> Self {
        Self(subsonic::Client::new(config))
    }
}

fn error(error: subsonic::Error) -> RemoteError {
    match error {
        subsonic::Error::Auth => RemoteError::Auth,
        subsonic::Error::Transient(message) => RemoteError::Unreachable(message),
        subsonic::Error::Server(message) => RemoteError::Other(message),
    }
}

impl ServerClient for Subsonic {
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
            .starred_songs()
            .map_err(error)?
            .into_iter()
            .map(|song| song.id)
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

fn first_name(names: &[subsonic::Named]) -> Option<String> {
    names
        .iter()
        .map(|named| named.name.trim())
        .find(|name| !name.is_empty())
        .map(str::to_string)
}

fn song(song: subsonic::Song) -> RemoteSong {
    RemoteSong {
        key: song.id,
        title: song.title,
        artist: real_artist(first_name(&song.artists).or(song.artist.clone())),
        artist_aliases: song
            .artist
            .iter()
            .cloned()
            .chain(song.artists.iter().skip(1).map(|named| named.name.clone()))
            .filter_map(|name| real_artist(Some(name)))
            .collect(),
        album: real_album(song.album),
        album_artist: real_artist(first_name(&song.album_artists).or(song.album_artist)),
        track_number: real_track_number(song.track),
        disc_number: song.disc_number,
        year: song.year,
        genre: song.genre,
        duration_ms: song.duration.map(|secs| (secs * 1000) as i64),
        size: song.size.map(|size| size as i64),
        suffix: song.suffix,
        content_type: song.content_type,
        bitrate_kbps: song.bit_rate,
        cover_key: song.cover_art,
        cover_hash: None,
        start_offset_ms: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_credited_artist_is_used_instead_of_the_joined_display_name() {
        let converted = song(subsonic::Song {
            id: "1".into(),
            title: "Moonlight".into(),
            artist: Some("Daniel Lanois • Daryl Johnson".into()),
            artists: vec![
                subsonic::Named {
                    name: "Daniel Lanois".into(),
                },
                subsonic::Named {
                    name: "Daryl Johnson".into(),
                },
            ],
            ..Default::default()
        });
        assert_eq!(converted.artist.as_deref(), Some("Daniel Lanois"));
        assert_eq!(
            converted.artist_aliases,
            vec![
                "Daniel Lanois • Daryl Johnson".to_string(),
                "Daryl Johnson".to_string()
            ]
        );
    }

    #[test]
    fn server_placeholders_for_missing_tags_are_dropped() {
        let converted = song(subsonic::Song {
            id: "1".into(),
            title: "Whole Album Image".into(),
            artist: Some("[Unknown Artist]".into()),
            album_artist: Some(" [unknown artist] ".into()),
            album: Some("[Unknown Album]".into()),
            track: Some(1997),
            ..Default::default()
        });
        assert_eq!(converted.artist, None);
        assert_eq!(converted.album_artist, None);
        assert_eq!(converted.album, None);
        assert_eq!(converted.track_number, None);
    }

    #[test]
    fn units_are_converted_to_the_library_ones() {
        let converted = song(subsonic::Song {
            id: "1".into(),
            duration: Some(185),
            size: Some(4_000_000),
            bit_rate: Some(320),
            suffix: Some("flac".into()),
            ..Default::default()
        });
        assert_eq!(converted.duration_ms, Some(185_000));
        assert_eq!(converted.size, Some(4_000_000));
        assert_eq!(converted.bitrate_kbps, Some(320));
        assert_eq!(converted.suffix.as_deref(), Some("flac"));
    }

    #[test]
    fn errors_keep_the_auth_and_unreachable_split() {
        assert_eq!(error(subsonic::Error::Auth), RemoteError::Auth);
        assert_eq!(
            error(subsonic::Error::Transient("down".into())),
            RemoteError::Unreachable("down".into())
        );
        assert_eq!(
            error(subsonic::Error::Server("x".into())),
            RemoteError::Other("x".into())
        );
    }
}

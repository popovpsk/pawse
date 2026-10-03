use std::sync::Arc;

use music_library::RemoteSong;

mod dlna;
mod jellyfin;
mod subsonic;
pub mod torrent;

pub use self::dlna::describe as describe_dlna;
pub use self::jellyfin::authenticate as authenticate_jellyfin;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ServerKind {
    Subsonic,
    Jellyfin,
    Dlna,
    Torrent,
}

impl ServerKind {
    pub const ALL: [ServerKind; 4] = [
        ServerKind::Subsonic,
        ServerKind::Jellyfin,
        ServerKind::Dlna,
        ServerKind::Torrent,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ServerKind::Subsonic => "subsonic",
            ServerKind::Jellyfin => "jellyfin",
            ServerKind::Dlna => "dlna",
            ServerKind::Torrent => "torrent",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            ServerKind::Subsonic => "Subsonic",
            ServerKind::Jellyfin => "Jellyfin",
            ServerKind::Dlna => "DLNA",
            ServerKind::Torrent => "Torrent",
        }
    }

    pub fn manual_sync(self) -> bool {
        match self {
            ServerKind::Subsonic | ServerKind::Jellyfin | ServerKind::Dlna => true,
            ServerKind::Torrent => false,
        }
    }

    pub fn imports_favorites(self) -> bool {
        match self {
            ServerKind::Subsonic | ServerKind::Jellyfin => true,
            ServerKind::Dlna | ServerKind::Torrent => false,
        }
    }

    pub fn reports_plays(self) -> bool {
        match self {
            ServerKind::Subsonic => true,
            ServerKind::Jellyfin | ServerKind::Dlna | ServerKind::Torrent => false,
        }
    }

    pub fn sends_favorites(self) -> bool {
        match self {
            ServerKind::Subsonic | ServerKind::Jellyfin => true,
            ServerKind::Dlna | ServerKind::Torrent => false,
        }
    }

    pub fn syncs_alone(self) -> bool {
        match self {
            ServerKind::Subsonic | ServerKind::Jellyfin | ServerKind::Dlna => false,
            ServerKind::Torrent => true,
        }
    }

    pub fn titled_by_name(self) -> bool {
        match self {
            ServerKind::Subsonic | ServerKind::Jellyfin => false,
            ServerKind::Dlna | ServerKind::Torrent => true,
        }
    }

    pub fn has_peers(self) -> bool {
        match self {
            ServerKind::Subsonic | ServerKind::Jellyfin | ServerKind::Dlna => false,
            ServerKind::Torrent => true,
        }
    }

    pub fn parse(kind: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|known| known.as_str() == kind)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoteConfig {
    Subsonic(::subsonic::Config),
    Jellyfin(::jellyfin::Config),
    Dlna(::dlna::Config),
    Torrent(torrent::Config),
}

impl RemoteConfig {
    pub fn kind(&self) -> ServerKind {
        match self {
            RemoteConfig::Subsonic(_) => ServerKind::Subsonic,
            RemoteConfig::Jellyfin(_) => ServerKind::Jellyfin,
            RemoteConfig::Dlna(_) => ServerKind::Dlna,
            RemoteConfig::Torrent(_) => ServerKind::Torrent,
        }
    }

    pub fn web_url(&self) -> Option<&str> {
        match self {
            RemoteConfig::Subsonic(config) => Some(&config.url),
            RemoteConfig::Jellyfin(config) => Some(&config.url),
            RemoteConfig::Dlna(_) | RemoteConfig::Torrent(_) => None,
        }
    }

    pub fn client(&self) -> Arc<dyn ServerClient> {
        match self {
            RemoteConfig::Subsonic(config) => Arc::new(subsonic::Subsonic::new(config)),
            RemoteConfig::Jellyfin(config) => Arc::new(jellyfin::Jellyfin::new(config)),
            RemoteConfig::Dlna(config) => Arc::new(dlna::Dlna::new(config)),
            RemoteConfig::Torrent(config) => Arc::new(torrent::Torrent::new(config)),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteServer {
    pub uri: String,
    pub name: String,
    pub config: RemoteConfig,
}

impl RemoteServer {
    pub fn kind(&self) -> ServerKind {
        self.config.kind()
    }

    pub fn key(&self) -> String {
        source_key(self.kind(), &self.uri)
    }
}

pub fn source_key(kind: ServerKind, uri: &str) -> String {
    format!("{}:{uri}", kind.as_str())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoteError {
    Auth,
    Unreachable(String),
    NotFound(String),
    Other(String),
}

impl From<music_library::LibraryError> for RemoteError {
    fn from(error: music_library::LibraryError) -> Self {
        RemoteError::Other(error.to_string())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Peers {
    pub connected: u32,
    pub known: u32,
}

pub trait ServerClient: Send + Sync {
    fn ping(&self) -> Result<(), RemoteError>;
    fn songs(&self) -> Result<Vec<RemoteSong>, RemoteError>;
    fn favorite_keys(&self) -> Result<Vec<String>, RemoteError>;
    fn cover_art(&self, key: &str, max_size: u32) -> Result<Vec<u8>, RemoteError>;
    fn fetch_range(
        &self,
        key: &str,
        start: u64,
        end: Option<u64>,
    ) -> Result<server_http::RangeBody, RemoteError>;
    fn scrobble(&self, _key: &str, _played_at: u64) -> Result<(), RemoteError> {
        Err(unsupported())
    }
    fn now_playing(&self, _key: &str) -> Result<(), RemoteError> {
        Err(unsupported())
    }
    fn set_favorite(&self, _key: &str, _favorite: bool) -> Result<(), RemoteError> {
        Err(unsupported())
    }
    fn lyrics(&self, _key: &str) -> Result<Option<lyrics::Lyrics>, RemoteError> {
        Ok(None)
    }
    fn forget(&self) {}
    fn moved(&self) -> Option<RemoteConfig> {
        None
    }
    fn peers(&self) -> Option<Peers> {
        None
    }
}

fn unsupported() -> RemoteError {
    RemoteError::Other("not supported by this server".into())
}

const UNKNOWN_ALBUM: &str = "[unknown album]";
const MAX_TRACK_NUMBER: u32 = 999;
const GENRE_JOIN: &str = "; ";

fn real_artist(name: Option<String>) -> Option<String> {
    name.map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .filter(|name| !crate::library_service::is_placeholder_artist(name))
}

fn real_album(album: Option<String>) -> Option<String> {
    album.filter(|album| !album.trim().eq_ignore_ascii_case(UNKNOWN_ALBUM))
}

fn real_track_number(number: Option<u32>) -> Option<u32> {
    number.filter(|n| (1..=MAX_TRACK_NUMBER).contains(n))
}

fn one_line(text: &str) -> String {
    text.split(['\n', '\r'])
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn server_lyrics(mut lines: Vec<lyrics::LyricLine>) -> Option<lyrics::Lyrics> {
    for line in &mut lines {
        line.text = one_line(&line.text);
        line.background = line
            .background
            .as_deref()
            .map(one_line)
            .filter(|text| !text.is_empty());
    }
    let synced = lines.iter().any(|line| line.time_ms.is_some());
    if synced {
        lines.retain(|line| line.time_ms.is_some());
        lines.sort_by_key(|line| line.time_ms);
    } else {
        lines.retain(|line| !line.text.is_empty());
    }
    lines
        .iter()
        .any(|line| !line.text.is_empty())
        .then_some(lyrics::Lyrics { synced, lines })
}

fn joined_genres(genres: &[String]) -> Option<String> {
    let kept: Vec<&str> = genres
        .iter()
        .map(String::as_str)
        .filter(|genre| !genre.trim().is_empty())
        .collect();
    (!kept.is_empty()).then(|| kept.join(GENRE_JOIN))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_round_trip_through_their_names() {
        for kind in ServerKind::ALL {
            assert_eq!(ServerKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(ServerKind::parse("local"), None);
    }

    #[test]
    fn the_same_address_under_two_kinds_gives_two_keys() {
        assert_ne!(
            source_key(ServerKind::Subsonic, "me@http://nas"),
            source_key(ServerKind::Jellyfin, "me@http://nas")
        );
    }

    #[test]
    fn placeholders_and_impossible_track_numbers_are_dropped() {
        assert_eq!(real_artist(Some(" [Unknown Artist] ".into())), None);
        assert_eq!(real_artist(Some("  ".into())), None);
        assert_eq!(real_artist(Some(" Band ".into())).as_deref(), Some("Band"));
        assert_eq!(real_album(Some("[Unknown Album]".into())), None);
        assert_eq!(real_track_number(Some(1997)), None);
        assert_eq!(real_track_number(Some(0)), None);
        assert_eq!(real_track_number(Some(12)), Some(12));
    }

    #[test]
    fn a_genre_list_is_joined_for_the_library_to_split() {
        let genres = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<Vec<_>>();
        assert_eq!(joined_genres(&genres(&[])), None);
        assert_eq!(joined_genres(&genres(&["", "  "])), None);
        assert_eq!(joined_genres(&genres(&["Rock"])).as_deref(), Some("Rock"));
        assert_eq!(
            joined_genres(&genres(&["Rock", " ", "Pop"])).as_deref(),
            Some("Rock; Pop")
        );
    }
    fn line(time_ms: Option<u32>, text: &str) -> lyrics::LyricLine {
        lyrics::LyricLine {
            time_ms,
            text: text.into(),
            background: None,
        }
    }

    #[test]
    fn server_lyrics_are_sorted_kept_on_one_line_and_none_when_blank() {
        let mut echoed = line(Some(3_000), " two\nlines ");
        echoed.background = Some("\n".into());
        let synced = server_lyrics(vec![
            echoed,
            line(Some(1_000), "one"),
            line(None, "untimed"),
            line(Some(2_000), ""),
        ])
        .unwrap();
        assert!(synced.synced);
        assert_eq!(
            synced.lines,
            vec![
                line(Some(1_000), "one"),
                line(Some(2_000), ""),
                line(Some(3_000), "two lines"),
            ]
        );

        let plain = server_lyrics(vec![line(None, "a"), line(None, " "), line(None, "b")]).unwrap();
        assert!(!plain.synced);
        assert_eq!(plain.lines, vec![line(None, "a"), line(None, "b")]);

        assert_eq!(
            server_lyrics(vec![line(Some(0), " "), line(None, "")]),
            None
        );
        assert_eq!(server_lyrics(Vec::new()), None);
    }
}

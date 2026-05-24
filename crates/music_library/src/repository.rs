use std::collections::HashMap;

use crate::error::Result;
use crate::models::{
    AlbumSearchEntry, AlbumSummary, ArtistSummary, CoverArt, NewTrack, PlaylistSummary,
    PlaylistTrackRef, Track,
};

pub trait LibraryRepository: Send + Sync {
    fn upsert_artist(&self, name: &str) -> Result<i64>;
    fn upsert_album(
        &self,
        title: &str,
        year: Option<i32>,
        cover_art_id: Option<i64>,
    ) -> Result<i64>;
    fn set_album_artists(&self, album_id: i64, artist_ids: &[(i64, i32)]) -> Result<()>;
    fn upsert_track(
        &self,
        track: &NewTrack,
        album_id: Option<i64>,
        artist_ids: &[(i64, i32)],
    ) -> Result<i64>;
    fn albums(&self) -> Result<Vec<AlbumSummary>>;
    fn album_search_entries(&self) -> Result<Vec<AlbumSearchEntry>>;
    fn tracks_for_album(&self, album_id: i64) -> Result<Vec<Track>>;
    fn track_artists(&self, track_id: i64) -> Result<Vec<String>>;
    fn track_artists_with_ids(&self, track_id: i64) -> Result<Vec<(i64, String)>>;
    fn track_artists_map(&self, track_ids: &[i64]) -> Result<HashMap<i64, Vec<String>>>;
    fn album_title(&self, album_id: i64) -> Result<Option<String>>;
    fn search(&self, query: &str) -> Result<Vec<Track>>;
    fn clear(&self) -> Result<()>;
    fn has_tracks(&self) -> Result<bool>;
    fn delete_orphaned_albums_and_artists(&self) -> Result<()>;
    fn save_cover_art(&self, data: &[u8]) -> Result<i64>;
    fn get_cover_art(&self, id: i64) -> Result<Option<CoverArt>>;
    fn get_cover_art_small(&self, id: i64) -> Result<Option<Vec<u8>>>;
    fn get_cover_art_large(&self, id: i64) -> Result<Option<Vec<u8>>>;
    fn album_has_artists(&self, album_id: i64) -> Result<bool>;
    fn artists(&self) -> Result<Vec<ArtistSummary>>;
    fn tracks_by_artist(&self, artist_id: i64) -> Result<Vec<Track>>;
    fn liked_tracks(&self) -> Result<Vec<Track>>;
    fn set_liked(&self, track_id: i64, liked: bool) -> Result<()>;

    fn create_playlist(&self, name: &str) -> Result<i64>;
    fn delete_playlist(&self, playlist_id: i64) -> Result<()>;
    fn playlists(&self) -> Result<Vec<PlaylistSummary>>;
    fn add_track_to_playlist(&self, playlist_id: i64, track_id: i64) -> Result<()>;
    fn remove_track_from_playlist(&self, playlist_id: i64, track_id: i64) -> Result<()>;
    fn tracks_for_playlist(&self, playlist_id: i64) -> Result<Vec<Track>>;
    fn playlists_containing_track(&self, track_id: i64) -> Result<Vec<i64>>;

    /// Capture every `playlist_tracks` row by content key (path +
    /// start_offset_ms) instead of by `track_id`. Survives a `clear()` where
    /// tracks get fresh ids on the next scan.
    fn playlist_track_refs(&self) -> Result<Vec<PlaylistTrackRef>>;
    /// Re-insert playlist memberships from snapshots taken before a rescan.
    /// Refs whose (path, start_offset_ms) no longer resolve to a track are
    /// dropped silently; the surviving refs are renumbered into a dense
    /// position sequence per playlist.
    fn restore_playlist_track_refs(&self, refs: &[PlaylistTrackRef]) -> Result<()>;
}

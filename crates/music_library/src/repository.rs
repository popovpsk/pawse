use crate::models::{AlbumSummary, NewTrack, Track};
use crate::error::Result;

pub trait LibraryRepository: Send + Sync {
    fn upsert_artist(&self, name: &str) -> Result<i64>;
    fn upsert_album(
        &self,
        title: &str,
        year: Option<i32>,
        cover_art_path: Option<&str>,
    ) -> Result<i64>;
    fn set_album_artists(&self, album_id: i64, artist_ids: &[(i64, i32)]) -> Result<()>;
    fn upsert_track(&self, track: &NewTrack, album_id: Option<i64>, artist_ids: &[(i64, i32)]) -> Result<i64>;
    fn albums(&self) -> Result<Vec<AlbumSummary>>;
    fn tracks_for_album(&self, album_id: i64) -> Result<Vec<Track>>;
    fn track_artists(&self, track_id: i64) -> Result<Vec<String>>;
    fn album_title(&self, album_id: i64) -> Result<Option<String>>;
    fn search(&self, query: &str) -> Result<Vec<Track>>;
    fn clear(&self) -> Result<()>;
    fn has_tracks(&self) -> Result<bool>;
    fn delete_orphaned_albums_and_artists(&self) -> Result<()>;
    fn save_cover_art(&self, data: &[u8]) -> Result<String>;
    fn album_has_artists(&self, album_id: i64) -> Result<bool>;
}

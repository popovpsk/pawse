use std::collections::HashMap;

use crate::error::Result;
use crate::models::{
    AlbumSearchEntry, AlbumSummary, ArtistSummary, CoverArt, LyricsRef, NewTrack, PlaylistSummary,
    PlaylistTrackRef, ScanTrack, StoredLyrics, Track,
};

/// A batched, single-transaction sink for a full rescan. Implementations own a
/// dedicated DB connection (separate from the repository's read connection) so
/// the UI can keep reading while a scan writes. Covers and tracks arrive in any
/// order; a track whose cover hash has not been seen yet is buffered until the
/// matching [`add_cover`](ScanWrite::add_cover) (or inserted cover-less at
/// [`finish`](ScanWrite::finish)).
pub trait ScanWrite: Send {
    /// Wipe tracks/artists/albums (keeping `cover_art`, like [`LibraryRepository::clear`]).
    fn clear(&mut self) -> Result<()>;
    /// Register a freshly generated cover thumbnail by content hash.
    fn add_cover(
        &mut self,
        hash: &str,
        small: Vec<u8>,
        large: Vec<u8>,
        source_path: &str,
        embedded: bool,
    ) -> Result<()>;
    /// Insert one scanned track, resolving artists/album/cover from caches.
    fn add_track(&mut self, track: ScanTrack) -> Result<()>;
    /// Flush any buffered tracks and commit.
    fn finish(self: Box<Self>) -> Result<()>;
}

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
    fn album_track_counts(&self) -> Result<HashMap<i64, i64>>;
    fn track_artists(&self, track_id: i64) -> Result<Vec<String>>;
    fn track_artists_with_ids(&self, track_id: i64) -> Result<Vec<(i64, String)>>;
    fn track_artists_map(&self, track_ids: &[i64]) -> Result<HashMap<i64, Vec<String>>>;
    fn album_title(&self, album_id: i64) -> Result<Option<String>>;
    fn album_genres(&self, album_id: i64) -> Result<Vec<String>>;
    fn album_genres_map(&self) -> Result<HashMap<i64, Vec<String>>>;
    fn clear(&self) -> Result<()>;
    fn has_tracks(&self) -> Result<bool>;
    fn delete_orphaned_albums_and_artists(&self) -> Result<()>;
    fn save_cover_art(&self, data: &[u8]) -> Result<i64>;
    fn get_cover_art(&self, id: i64) -> Result<Option<CoverArt>>;
    fn get_cover_art_small(&self, id: i64) -> Result<Option<Vec<u8>>>;
    fn get_cover_art_large(&self, id: i64) -> Result<Option<Vec<u8>>>;
    fn get_cover_art_source(&self, id: i64) -> Result<Option<(String, bool)>>;
    fn album_has_artists(&self, album_id: i64) -> Result<bool>;
    fn artists(&self) -> Result<Vec<ArtistSummary>>;
    fn artist_album_covers(&self) -> Result<HashMap<i64, Vec<i64>>>;
    fn tracks_by_artist(&self, artist_id: i64) -> Result<Vec<Track>>;
    fn liked_tracks(&self) -> Result<Vec<Track>>;
    fn all_tracks(&self) -> Result<Vec<Track>>;
    fn track_count(&self) -> Result<i64>;
    fn set_liked(&self, track_id: i64, liked: bool) -> Result<()>;

    fn create_playlist(&self, name: &str) -> Result<i64>;
    fn delete_playlist(&self, playlist_id: i64) -> Result<()>;
    fn playlists(&self) -> Result<Vec<PlaylistSummary>>;
    fn add_track_to_playlist(&self, playlist_id: i64, track_id: i64) -> Result<()>;
    fn remove_track_from_playlist(&self, playlist_id: i64, track_id: i64) -> Result<()>;
    fn move_track_in_playlist(&self, playlist_id: i64, from: usize, to: usize) -> Result<()>;
    fn move_liked_track(&self, from: usize, to: usize) -> Result<()>;
    fn tracks_for_playlist(&self, playlist_id: i64) -> Result<Vec<Track>>;
    fn playlists_containing_track(&self, track_id: i64) -> Result<Vec<i64>>;

    fn lyrics_for_track(&self, track_id: i64) -> Result<Option<StoredLyrics>>;
    fn upsert_lyrics(&self, track_id: i64, text: &str, source: &str, not_found: bool)
    -> Result<()>;

    /// Capture lyrics that can't be re-derived from disk (network fetches) by
    /// content key, so a `clear()` + rescan doesn't drop them. Disk-backed
    /// sources (`lrc`, `embedded`) are excluded — the scan re-reads those.
    fn lyrics_refs(&self) -> Result<Vec<LyricsRef>>;
    /// Re-insert snapshotted lyrics for tracks that don't already have a row
    /// after the rescan (the scan's fresh disk lyrics win). Refs whose
    /// (path, start_offset_ms) no longer resolve to a track are dropped.
    fn restore_lyrics_refs(&self, refs: &[LyricsRef]) -> Result<()>;

    fn tracks_by_keys(&self, keys: &[(String, i32)]) -> Result<Vec<Track>>;

    /// Capture every `playlist_tracks` row by content key (path +
    /// start_offset_ms) instead of by `track_id`. Survives a `clear()` where
    /// tracks get fresh ids on the next scan.
    fn playlist_track_refs(&self) -> Result<Vec<PlaylistTrackRef>>;
    /// Re-insert playlist memberships from snapshots taken before a rescan.
    /// Refs whose (path, start_offset_ms) no longer resolve to a track are
    /// dropped silently; the surviving refs are renumbered into a dense
    /// position sequence per playlist.
    fn restore_playlist_track_refs(&self, refs: &[PlaylistTrackRef]) -> Result<()>;

    /// All `(hash, id)` pairs in `cover_art`. Lets the scan pipeline skip
    /// thumbnail generation for covers that already exist (they survive
    /// `clear()`), which is why the 2nd scan is much faster than the 1st.
    fn cover_art_hashes(&self) -> Result<Vec<(String, i64)>>;

    /// Open a batched scan-write session on a dedicated connection.
    fn open_scan_session(&self) -> Result<Box<dyn ScanWrite>>;

    /// Fingerprint of the filesystem state captured at the last successful
    /// scan, or `None` if never scanned. See [`set_scan_meta`](LibraryRepository::set_scan_meta).
    fn scan_fingerprint(&self) -> Result<Option<String>>;
    /// Serialized folder set from the last successful scan.
    fn scan_folders(&self) -> Result<Option<String>>;
    /// Persist the fingerprint + folder set after a successful scan.
    fn set_scan_meta(&self, fingerprint: &str, folders: &str) -> Result<()>;

    fn vacuum(&self) -> Result<()>;
}

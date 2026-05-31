use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{Image, SharedString};

use crate::cover_art_cache::CoverArtCache;
use crate::library_service::LibraryService;
use crate::track_list::TrackRowBase;

pub(crate) struct CoverTrackRow {
    pub(crate) base: TrackRowBase,
    pub(crate) track_all_ix: usize,
    pub(crate) artist: SharedString,
    pub(crate) cover: Option<Arc<Image>>,
}

impl CoverTrackRow {
    pub(crate) fn from_track(
        track: &music_library::Track,
        track_all_ix: usize,
        artist_by_track: &HashMap<i64, SharedString>,
        cover_cache: &mut CoverArtCache,
        library: &LibraryService,
    ) -> Self {
        Self {
            base: TrackRowBase::from_track(track),
            track_all_ix,
            artist: artist_by_track.get(&track.id).cloned().unwrap_or_default(),
            cover: cover_cache.get_small(track.cover_art_id, library),
        }
    }
}

pub(crate) fn build_artist_map(
    library: &LibraryService,
    tracks: &[Rc<music_library::Track>],
) -> HashMap<i64, SharedString> {
    let ids: Vec<i64> = tracks.iter().map(|t| t.id).collect();
    library
        .track_artists_map(&ids)
        .into_iter()
        .map(|(id, names)| (id, names.join(", ").into()))
        .collect()
}

pub(crate) fn build_haystacks(
    tracks: &[Rc<music_library::Track>],
    artist_by_track: &HashMap<i64, SharedString>,
) -> Vec<String> {
    tracks
        .iter()
        .map(|t| {
            let artist = artist_by_track
                .get(&t.id)
                .map(SharedString::as_str)
                .unwrap_or("");
            format!("{} {}", t.title, artist)
        })
        .collect()
}

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    Div, Hsla, Image, ParentElement, SharedString, Styled, div, prelude::FluentBuilder, px, rems,
};
use ui_resources::i18n::view_menu_strings;

use crate::cover_art_cache::CoverArtCache;
use crate::library_service::LibraryService;
use crate::settings_store::SettingsStore;
use crate::track_list::TrackRowBase;

pub(crate) const TITLE_MIN_WIDTH: f32 = 8.;
const ARTIST_COLUMN_WIDTH: f32 = 8.75;
const ALBUM_COLUMN_WIDTH: f32 = 10.;
const YEAR_COLUMN_WIDTH: f32 = 2.5;

pub(crate) struct CoverTrackRow {
    pub(crate) base: TrackRowBase,
    pub(crate) track_all_ix: usize,
    pub(crate) artist: SharedString,
    pub(crate) album: SharedString,
    pub(crate) year: SharedString,
    pub(crate) cover: Option<Arc<Image>>,
}

impl CoverTrackRow {
    pub(crate) fn from_track(
        track: &music_library::Track,
        track_all_ix: usize,
        names: &TrackNames,
        cover_cache: &mut CoverArtCache,
        library: &LibraryService,
    ) -> Self {
        Self {
            base: TrackRowBase::from_track(track),
            track_all_ix,
            artist: names.artists.get(&track.id).cloned().unwrap_or_default(),
            album: names.albums.get(&track.id).cloned().unwrap_or_default(),
            year: track
                .year
                .filter(|year| *year > 0)
                .map(|year| year.to_string().into())
                .unwrap_or_default(),
            cover: cover_cache.get_small(track.cover_art_id, library),
        }
    }
}

#[derive(Default)]
pub(crate) struct TrackNames {
    pub(crate) artists: HashMap<i64, SharedString>,
    albums: HashMap<i64, SharedString>,
}

impl TrackNames {
    pub(crate) fn load(library: &LibraryService, tracks: &[Rc<music_library::Track>]) -> Self {
        let artists = build_artist_map(library, tracks);
        let ids: Vec<i64> = tracks.iter().map(|t| t.id).collect();
        let albums = library
            .track_albums_map(&ids)
            .into_iter()
            .map(|(id, title)| (id, title.into()))
            .collect();
        Self { artists, albums }
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct TrackListPrefs {
    pub(crate) artist: bool,
    pub(crate) album: bool,
    pub(crate) year: bool,
    pub(crate) unavailable: bool,
}

impl TrackListPrefs {
    pub(crate) fn read(settings: &SettingsStore) -> Self {
        Self {
            artist: settings.playlists_show_artist(),
            album: settings.playlists_show_album(),
            year: settings.playlists_show_year(),
            unavailable: settings.playlists_show_unavailable(),
        }
    }
}

pub(crate) fn track_columns(
    row: Div,
    track: &CoverTrackRow,
    prefs: TrackListPrefs,
    muted_fg: Hsla,
) -> Div {
    let column = |width: f32, text: &SharedString| {
        div()
            .w(rems(width))
            .min_w(px(0.))
            .overflow_hidden()
            .text_ellipsis()
            .text_sm()
            .text_color(muted_fg)
            .child(text.clone())
    };
    row.when(prefs.artist, |row| {
        row.child(column(ARTIST_COLUMN_WIDTH, &track.artist))
    })
    .when(prefs.album, |row| {
        row.child(column(ALBUM_COLUMN_WIDTH, &track.album))
    })
    .when(prefs.year, |row| {
        row.child(
            column(YEAR_COLUMN_WIDTH, &track.year)
                .flex_shrink_0()
                .whitespace_nowrap()
                .text_right(),
        )
    })
}

pub(crate) fn unavailable_label(
    tracks: &[Rc<music_library::Track>],
    show_unavailable: bool,
) -> Option<SharedString> {
    let count = tracks.iter().filter(|t| !t.available).count();
    (count > 0).then(|| {
        if show_unavailable {
            crate::localization::tr().unavailable_count(count).into()
        } else {
            view_menu_strings().unavailable_hidden(count).into()
        }
    })
}

pub(crate) fn build_haystacks(
    tracks: &[Rc<music_library::Track>],
    names: &TrackNames,
) -> Vec<String> {
    tracks
        .iter()
        .map(|t| {
            let artist = names
                .artists
                .get(&t.id)
                .map(SharedString::as_str)
                .unwrap_or("");
            format!("{} {}", t.title, artist)
        })
        .collect()
}

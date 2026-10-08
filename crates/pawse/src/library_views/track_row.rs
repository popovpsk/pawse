use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    Div, ElementId, Hsla, Image, InteractiveElement, ParentElement, SharedString, Stateful,
    StatefulInteractiveElement, Styled, div, prelude::FluentBuilder, px, rems,
};
use gpui_component::tooltip::Tooltip;
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

fn column(width: f32, text: &SharedString, muted_fg: Hsla) -> Div {
    div()
        .w(rems(width))
        .min_w(px(0.))
        .overflow_hidden()
        .text_ellipsis()
        .text_sm()
        .text_color(muted_fg)
        .child(text.clone())
}

pub(crate) fn artist_column(track_id: i64, text: &SharedString, muted_fg: Hsla) -> Stateful<Div> {
    let tip = text.clone();
    column(ARTIST_COLUMN_WIDTH, text, muted_fg)
        .id(ElementId::NamedInteger(
            "artist-column".into(),
            track_id as u64,
        ))
        .when(!tip.is_empty(), |el| {
            el.tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
        })
}

pub(crate) fn track_columns(
    row: Div,
    track: &CoverTrackRow,
    prefs: TrackListPrefs,
    muted_fg: Hsla,
) -> Div {
    row.when(prefs.artist, |row| {
        row.child(artist_column(track.base.id, &track.artist, muted_fg))
    })
    .when(prefs.album, |row| {
        row.child(column(ALBUM_COLUMN_WIDTH, &track.album, muted_fg))
    })
    .when(prefs.year, |row| {
        row.child(
            column(YEAR_COLUMN_WIDTH, &track.year, muted_fg)
                .flex_shrink_0()
                .whitespace_nowrap()
                .text_right(),
        )
    })
}

pub(crate) fn guest_credits(
    library: &LibraryService,
    tracks: &[Rc<music_library::Track>],
    page_artist: Option<&str>,
) -> HashMap<i64, SharedString> {
    let track_ids: Vec<i64> = tracks.iter().map(|t| t.id).collect();
    let mut album_ids: Vec<i64> = tracks.iter().filter_map(|t| t.album_id).collect();
    album_ids.sort_unstable();
    album_ids.dedup();
    let track_artists = library.track_artists_map(&track_ids);
    let album_artists = library.known_album_artists_map(&album_ids);
    tracks
        .iter()
        .filter_map(|track| {
            let heads = album_artists
                .get(&track.album_id?)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let credited = track_artists.get(&track.id)?;
            is_guest_credit(credited, heads, page_artist)
                .then(|| (track.id, credited.join(", ").into()))
        })
        .collect()
}

fn is_guest_credit(
    credited: &[String],
    album_artists: &[String],
    page_artist: Option<&str>,
) -> bool {
    !credited.is_empty()
        && !same_people(credited, album_artists)
        && page_artist.is_none_or(|page| !same_people(credited, &[page]))
}

fn same_people<S: AsRef<str>>(credited: &[String], names: &[S]) -> bool {
    credited.len() == names.len()
        && credited
            .iter()
            .all(|name| names.iter().any(|other| other.as_ref() == name))
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
    artists: &HashMap<i64, SharedString>,
) -> Vec<String> {
    tracks
        .iter()
        .map(|t| {
            let artist = artists.get(&t.id).map(SharedString::as_str).unwrap_or("");
            format!("{} {}", t.title, artist)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::is_guest_credit;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[rstest]
    #[case::same_as_album(&["Gorillaz"], &["Gorillaz"], None, false)]
    #[case::guest(&["Gorillaz & Lou Reed"], &["Gorillaz"], None, true)]
    #[case::compilation(&["Mick Gordon"], &["Various Artists"], None, true)]
    #[case::page_artist_on_compilation(&["Mick Gordon"], &["Various Artists"], Some("Mick Gordon"), false)]
    #[case::other_artist_on_page(&["Martin Stig Andersen"], &["Various Artists"], Some("Mick Gordon"), true)]
    #[case::same_people_other_order(&["B", "A"], &["A", "B"], Some("A"), false)]
    #[case::subset_of_album_artists(&["A"], &["A", "B"], None, true)]
    #[case::no_credit(&[], &["A"], None, false)]
    #[case::album_without_artist(&["A"], &[], None, true)]
    fn guest_credit_rule(
        #[case] credited: &[&str],
        #[case] album: &[&str],
        #[case] page: Option<&str>,
        #[case] expected: bool,
    ) {
        assert_eq!(
            is_guest_credit(&names(credited), &names(album), page),
            expected
        );
    }
}

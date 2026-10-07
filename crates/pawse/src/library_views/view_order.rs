use std::cmp::Ordering;
use std::ops::Range;

use gpui::SharedString;
use ui_resources::i18n::view_menu_strings;

use crate::settings_store::{AlbumsSort, ArtistsSort};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SectionKey {
    Letter(char),
    Decade(i32),
    Undated,
    Other,
}

impl SectionKey {
    pub(super) fn label(self) -> SharedString {
        match self {
            SectionKey::Letter(c) => c.to_string().into(),
            SectionKey::Decade(decade) => view_menu_strings().decade(decade).into(),
            SectionKey::Undated => view_menu_strings().undated.clone(),
            SectionKey::Other => SharedString::new_static("#"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Section {
    pub(super) key: SectionKey,
    pub(super) rows: Range<usize>,
}

fn sort_key(name: &str) -> String {
    music_library::compute_sort_name(name).to_lowercase()
}

fn letter_of(key: &str) -> SectionKey {
    let Some(c) = key.chars().next() else {
        return SectionKey::Other;
    };
    let mut upper = c.to_uppercase();
    match (upper.next(), upper.next()) {
        (Some(letter), None) if letter != c && letter.to_lowercase().eq(std::iter::once(c)) => {
            SectionKey::Letter(letter)
        }
        _ => SectionKey::Other,
    }
}

fn goes_last(section: SectionKey) -> bool {
    matches!(section, SectionKey::Other | SectionKey::Undated)
}

fn directed(ordering: Ordering, desc: bool) -> Ordering {
    if desc { ordering.reverse() } else { ordering }
}

#[derive(Debug, Clone)]
pub(crate) struct AlbumKey {
    artist: String,
    title: String,
    year: Option<i32>,
    artist_section: SectionKey,
    title_section: SectionKey,
    pinned_last: bool,
}

impl AlbumKey {
    pub(super) fn new(artist: &str, title: &str, year: Option<i32>, pinned_last: bool) -> Self {
        let artist = sort_key(artist);
        let title = sort_key(title);
        Self {
            artist_section: letter_of(&artist),
            title_section: letter_of(&title),
            artist,
            title,
            year: year.filter(|y| *y > 0),
            pinned_last,
        }
    }

    pub(crate) fn of(album: &music_library::AlbumSummary) -> Self {
        Self::new(
            &album.artist_name,
            &album.title,
            album.year,
            album.id == music_library::NO_METADATA_ALBUM_ID,
        )
    }

    pub(super) fn section(&self, sort: AlbumsSort) -> SectionKey {
        match sort {
            AlbumsSort::Year => match self.year {
                Some(year) => SectionKey::Decade(year.div_euclid(10) * 10),
                None => SectionKey::Undated,
            },
            _ if self.pinned_last => SectionKey::Other,
            AlbumsSort::Artist => self.artist_section,
            AlbumsSort::Title => self.title_section,
        }
    }
}

fn compare_albums(a: &AlbumKey, b: &AlbumKey, sort: AlbumsSort, desc: bool) -> Ordering {
    let sections =
        |a: &AlbumKey, b: &AlbumKey| goes_last(a.section(sort)).cmp(&goes_last(b.section(sort)));
    a.pinned_last
        .cmp(&b.pinned_last)
        .then_with(|| sections(a, b))
        .then_with(|| match sort {
            AlbumsSort::Artist => directed(a.artist.cmp(&b.artist), desc)
                .then_with(|| a.year.is_none().cmp(&b.year.is_none()))
                .then_with(|| a.year.cmp(&b.year))
                .then_with(|| a.title.cmp(&b.title)),
            AlbumsSort::Title => {
                directed(a.title.cmp(&b.title), desc).then_with(|| a.artist.cmp(&b.artist))
            }
            AlbumsSort::Year => directed(a.year.cmp(&b.year), desc)
                .then_with(|| a.artist.cmp(&b.artist))
                .then_with(|| a.title.cmp(&b.title)),
        })
}

pub(crate) fn order_albums(keys: &[AlbumKey], sort: AlbumsSort, desc: bool) -> Vec<usize> {
    let mut order: Vec<usize> = (0..keys.len()).collect();
    order.sort_by(|&a, &b| compare_albums(&keys[a], &keys[b], sort, desc));
    order
}

#[derive(Debug, Clone)]
pub(crate) struct ArtistKey {
    name: String,
    section: SectionKey,
    tracks: i64,
    pinned_last: bool,
}

impl ArtistKey {
    pub(super) fn new(name: &str, sort_name: &str, tracks: i64, pinned_last: bool) -> Self {
        let name = if sort_name.is_empty() {
            sort_key(name)
        } else {
            sort_name.to_lowercase()
        };
        Self {
            section: if pinned_last {
                SectionKey::Other
            } else {
                letter_of(&name)
            },
            name,
            tracks,
            pinned_last,
        }
    }

    pub(crate) fn of(artist: &music_library::ArtistSummary) -> Self {
        Self::new(
            &artist.name,
            &artist.sort_name,
            artist.track_count,
            artist.id == music_library::NO_METADATA_ARTIST_ID,
        )
    }

    pub(super) fn section(&self) -> SectionKey {
        self.section
    }
}

fn compare_artists(a: &ArtistKey, b: &ArtistKey, sort: ArtistsSort, desc: bool) -> Ordering {
    a.pinned_last.cmp(&b.pinned_last).then_with(|| match sort {
        ArtistsSort::Name => goes_last(a.section)
            .cmp(&goes_last(b.section))
            .then_with(|| directed(a.name.cmp(&b.name), desc)),
        ArtistsSort::Tracks => {
            directed(a.tracks.cmp(&b.tracks), desc).then_with(|| a.name.cmp(&b.name))
        }
    })
}

pub(crate) fn order_artists(keys: &[ArtistKey], sort: ArtistsSort, desc: bool) -> Vec<usize> {
    let mut order: Vec<usize> = (0..keys.len()).collect();
    order.sort_by(|&a, &b| compare_artists(&keys[a], &keys[b], sort, desc));
    order
}

pub(super) fn sections(order: &[usize], key_of: impl Fn(usize) -> SectionKey) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    for (row, &ix) in order.iter().enumerate() {
        let key = key_of(ix);
        match out.last_mut() {
            Some(last) if last.key == key => last.rows.end = row + 1,
            _ => out.push(Section {
                key,
                rows: row..row + 1,
            }),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn album(artist: &str, title: &str, year: Option<i32>) -> AlbumKey {
        AlbumKey::new(artist, title, year, false)
    }

    fn artist(name: &str, tracks: i64) -> ArtistKey {
        ArtistKey::new(name, "", tracks, false)
    }

    fn albums_in_order(
        albums: &[(&str, &str, Option<i32>)],
        sort: AlbumsSort,
        desc: bool,
    ) -> Vec<String> {
        let keys: Vec<AlbumKey> = albums.iter().map(|(a, t, y)| album(a, t, *y)).collect();
        order_albums(&keys, sort, desc)
            .into_iter()
            .map(|ix| albums[ix].1.to_string())
            .collect()
    }

    #[rstest]
    #[case::the("The Smile", SectionKey::Letter('S'))]
    #[case::a("A Perfect Circle", SectionKey::Letter('P'))]
    #[case::plain("Radiohead", SectionKey::Letter('R'))]
    #[case::cyrillic("Кино", SectionKey::Letter('К'))]
    #[case::cased_non_ascii("Ólafur Arnalds", SectionKey::Letter('Ó'))]
    #[case::digit("30 Seconds to Mars", SectionKey::Other)]
    #[case::symbol("(hed) p.e.", SectionKey::Other)]
    #[case::uncased_script("坂本龍一", SectionKey::Other)]
    #[case::dotless_i("ılık", SectionKey::Other)]
    #[case::sharp_s("ßand", SectionKey::Other)]
    #[case::empty("", SectionKey::Other)]
    fn a_name_lands_in_its_letter_without_the_article(
        #[case] name: &str,
        #[case] expected: SectionKey,
    ) {
        assert_eq!(artist(name, 1).section(), expected);
        assert_eq!(album(name, "x", None).section(AlbumsSort::Artist), expected);
    }

    #[test]
    fn artist_sort_ignores_articles_and_keeps_albums_chronological() {
        let albums = [
            ("Radiohead", "In Rainbows", Some(2007)),
            ("The Smile", "Wall of Eyes", Some(2024)),
            ("Radiohead", "OK Computer", Some(1997)),
            ("A Perfect Circle", "Mer de Noms", Some(2000)),
        ];
        assert_eq!(
            albums_in_order(&albums, AlbumsSort::Artist, false),
            ["Mer de Noms", "OK Computer", "In Rainbows", "Wall of Eyes"]
        );
    }

    #[test]
    fn an_artists_undated_albums_follow_the_dated_ones() {
        let albums = [
            ("Muse", "Untitled", None),
            ("Muse", "Absolution", Some(2003)),
            ("Muse", "Zero", Some(0)),
            ("Muse", "Showbiz", Some(1999)),
        ];
        for desc in [false, true] {
            assert_eq!(
                albums_in_order(&albums, AlbumsSort::Artist, desc),
                ["Showbiz", "Absolution", "Untitled", "Zero"]
            );
        }
    }

    #[test]
    fn descending_flips_only_the_primary_key() {
        let albums = [
            ("Radiohead", "In Rainbows", Some(2007)),
            ("Muse", "Absolution", Some(2003)),
            ("Radiohead", "OK Computer", Some(1997)),
        ];
        assert_eq!(
            albums_in_order(&albums, AlbumsSort::Artist, true),
            ["OK Computer", "In Rainbows", "Absolution"]
        );
    }

    #[rstest]
    #[case::ascending(false)]
    #[case::descending(true)]
    fn non_letters_stay_at_the_end_in_both_directions(#[case] desc: bool) {
        let albums = [
            ("30 Seconds to Mars", "A Beautiful Lie", Some(2005)),
            ("Muse", "Absolution", Some(2003)),
            ("坂本龍一", "async", Some(2017)),
            ("Gojira", "Magma", Some(2016)),
        ];
        let order = albums_in_order(&albums, AlbumsSort::Artist, desc);
        let mut tail = order[2..].to_vec();
        tail.sort();
        assert_eq!(tail, ["A Beautiful Lie", "async"]);
    }

    #[test]
    fn title_sort_ignores_articles_and_breaks_ties_by_artist() {
        let albums = [
            ("Foo Fighters", "The Colour and the Shape", Some(1997)),
            ("Gorillaz", "Demon Days", Some(2005)),
            ("Blur", "Demon Days", Some(2001)),
        ];
        assert_eq!(
            albums_in_order(&albums, AlbumsSort::Title, false),
            ["The Colour and the Shape", "Demon Days", "Demon Days"]
        );
        let keys: Vec<AlbumKey> = albums.iter().map(|(a, t, y)| album(a, t, *y)).collect();
        let order = order_albums(&keys, AlbumsSort::Title, false);
        assert_eq!(albums[order[1]].0, "Blur");
    }

    #[rstest]
    #[case::oldest_first(false, ["OK Computer", "Absolution", "Untitled", "Zero"])]
    #[case::newest_first(true, ["Absolution", "OK Computer", "Untitled", "Zero"])]
    fn undated_albums_go_last_whatever_the_direction(
        #[case] desc: bool,
        #[case] expected: [&str; 4],
    ) {
        let albums = [
            ("Muse", "Absolution", Some(2003)),
            ("Band", "Untitled", None),
            ("Radiohead", "OK Computer", Some(1997)),
            ("Band", "Zero", Some(0)),
        ];
        assert_eq!(albums_in_order(&albums, AlbumsSort::Year, desc), expected);
    }

    #[test]
    fn the_no_metadata_album_is_always_last() {
        let keys = vec![
            AlbumKey::new("", "No metadata", None, true),
            album("30 Seconds to Mars", "A Beautiful Lie", Some(2005)),
            album("Muse", "Absolution", Some(2003)),
        ];
        for sort in AlbumsSort::ALL {
            for desc in [false, true] {
                assert_eq!(order_albums(&keys, sort, desc).last(), Some(&0), "{sort:?}");
            }
        }
    }

    #[test]
    fn year_sections_are_decades_then_undated() {
        let keys = vec![
            album("A", "x", Some(1999)),
            album("B", "x", None),
            album("C", "x", Some(1990)),
            album("D", "x", Some(2001)),
        ];
        let order = order_albums(&keys, AlbumsSort::Year, false);
        let found = sections(&order, |ix| keys[ix].section(AlbumsSort::Year));
        assert_eq!(
            found,
            vec![
                Section {
                    key: SectionKey::Decade(1990),
                    rows: 0..2
                },
                Section {
                    key: SectionKey::Decade(2000),
                    rows: 2..3
                },
                Section {
                    key: SectionKey::Undated,
                    rows: 3..4
                },
            ]
        );
    }

    #[test]
    fn letter_sections_cover_every_row_once() {
        let names = [
            "Muse",
            "Gojira",
            "Gorillaz",
            "The Smile",
            "Soundgarden",
            "4 Non Blondes",
        ];
        let keys: Vec<ArtistKey> = names.iter().map(|n| artist(n, 1)).collect();
        let order = order_artists(&keys, ArtistsSort::Name, false);
        let found = sections(&order, |ix| keys[ix].section());
        let labels: Vec<SectionKey> = found.iter().map(|s| s.key).collect();
        assert_eq!(
            labels,
            vec![
                SectionKey::Letter('G'),
                SectionKey::Letter('M'),
                SectionKey::Letter('S'),
                SectionKey::Other,
            ]
        );
        assert_eq!(found.first().map(|s| s.rows.start), Some(0));
        assert_eq!(found.last().map(|s| s.rows.end), Some(names.len()));
        assert!(found.windows(2).all(|w| w[0].rows.end == w[1].rows.start));
    }

    #[test]
    fn track_count_sorts_most_first_and_breaks_ties_by_name() {
        let keys = vec![
            artist("Muse", 26),
            artist("Gorillaz", 80),
            artist("Audioslave", 26),
            artist("4 Non Blondes", 90),
        ];
        let names: Vec<usize> = order_artists(&keys, ArtistsSort::Tracks, true);
        assert_eq!(names, vec![3, 1, 2, 0]);
        let names: Vec<usize> = order_artists(&keys, ArtistsSort::Tracks, false);
        assert_eq!(names, vec![2, 0, 1, 3]);
    }

    #[test]
    fn the_stored_sort_name_wins_over_the_display_name() {
        let keys = vec![
            ArtistKey::new("Zed", "aaa", 1, false),
            ArtistKey::new("Beta", "", 1, false),
        ];
        assert_eq!(order_artists(&keys, ArtistsSort::Name, false), vec![0, 1]);
        assert_eq!(keys[0].section(), SectionKey::Letter('A'));
    }
}

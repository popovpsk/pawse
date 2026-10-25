use gpui::{
    App, AppContext, Context, Entity, EventEmitter, IntoElement, ParentElement, Render, Styled,
    Subscription, Window, div,
};
use gpui_component::v_flex;

use crate::library_views::albums_view::{AlbumSelectedEvent, AlbumsView, OpenLibrarySettings};
use crate::library_views::artists_view::{ArtistSelectedEvent, ArtistsView};
use crate::library_views::genres_view::{GenreSelectedEvent, GenresView};
use crate::library_views::grouped_tracks_view::GroupedTracksView;
use crate::library_views::liked_view::LikedView;
use crate::library_views::playlist_tracks_view::PlaylistTracksView;
use crate::library_views::playlists_view::{
    AllTracksSelectedEvent, PlaylistSelectedEvent, PlaylistsView,
};
use crate::library_views::tracks_view::TracksView;
use crate::library_views::view_menu::ViewMenuTab;
use crate::localization::tr;
use crate::now_playing::{NavigateToAlbumRequested, NavigateToArtistRequested};
use crate::playback_queue::QueueSource;
use crate::services::Services;
use crate::settings_store::SettingsStore;
use music_library::ArtistGrouping;

#[derive(Clone, Debug)]
pub enum LibraryViewEvent {
    StateChanged,
    OpenLibrarySettings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LibraryRootTab {
    Albums,
    Artists,
    Genres,
    Liked,
    Playlists,
}

#[derive(Clone, Debug)]
pub enum NavKey {
    Root(LibraryRootTab),
    Album(i64),
    Artist(i64, ArtistGrouping),
    Genre(music_library::GenreSummary),
    Playlist(i64),
    AllTracks,
}

enum NavEntry {
    Root(LibraryRootTab),
    AlbumTracks {
        album_id: i64,
        view: Entity<TracksView>,
        _sub: Subscription,
    },
    ArtistTracks {
        artist_id: i64,
        grouping: ArtistGrouping,
        view: Entity<GroupedTracksView>,
        _subs: [Subscription; 2],
    },
    GenreTracks {
        genre: music_library::GenreSummary,
        view: Entity<GroupedTracksView>,
        _subs: [Subscription; 2],
    },
    PlaylistTracks {
        playlist_id: Option<i64>,
        view: Entity<PlaylistTracksView>,
    },
}

impl NavEntry {
    fn key(&self) -> NavKey {
        match self {
            NavEntry::Root(tab) => NavKey::Root(*tab),
            NavEntry::AlbumTracks { album_id, .. } => NavKey::Album(*album_id),
            NavEntry::ArtistTracks {
                artist_id,
                grouping,
                ..
            } => NavKey::Artist(*artist_id, *grouping),
            NavEntry::GenreTracks { genre, .. } => NavKey::Genre(genre.clone()),
            NavEntry::PlaylistTracks {
                playlist_id: Some(id),
                ..
            } => NavKey::Playlist(*id),
            NavEntry::PlaylistTracks {
                playlist_id: None, ..
            } => NavKey::AllTracks,
        }
    }
}

#[derive(Clone, Copy)]
struct OptionalTabs {
    liked: bool,
    playlists: bool,
    genres: bool,
}

impl OptionalTabs {
    fn read(cx: &App) -> Self {
        let store = cx.global::<SettingsStore>();
        Self {
            liked: store.liked_enabled(),
            playlists: store.playlists_enabled(),
            genres: store.genres_enabled(),
        }
    }

    fn allow(self, key: &NavKey) -> bool {
        match key {
            NavKey::Root(LibraryRootTab::Liked) => self.liked,
            NavKey::Root(LibraryRootTab::Playlists) | NavKey::Playlist(_) | NavKey::AllTracks => {
                self.playlists
            }
            NavKey::Root(LibraryRootTab::Genres) | NavKey::Genre(_) => self.genres,
            NavKey::Root(LibraryRootTab::Albums | LibraryRootTab::Artists)
            | NavKey::Album(_)
            | NavKey::Artist(..) => true,
        }
    }
}

struct GenresRoot {
    view: Entity<GenresView>,
    _subs: [Subscription; 2],
}

pub struct LibraryView {
    stack: Vec<NavEntry>,
    albums_view: Entity<AlbumsView>,
    artists_view: Entity<ArtistsView>,
    genres: Option<GenresRoot>,
    liked_view: Entity<LikedView>,
    playlists_view: Entity<PlaylistsView>,
    _album_subscription: Subscription,
    _artist_subscription: Subscription,
    _playlist_subscription: Subscription,
    _all_tracks_subscription: Subscription,
    _settings_subscriptions: [Subscription; 2],
    _settings_observer: Subscription,
    _app_events: crate::view_cache::AppEvents,
}

impl LibraryView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let albums_view = cx.new(|cx| AlbumsView::new(window, cx));
        let artists_view = cx.new(|cx| ArtistsView::new(window, cx));
        let liked_view = cx.new(|cx| LikedView::new(window, cx));
        let playlists_view = cx.new(|cx| PlaylistsView::new(window, cx));

        let album_subscription =
            cx.subscribe(&albums_view, |this, _, event: &AlbumSelectedEvent, cx| {
                this.show_album_tracks(event.album.clone(), cx);
            });

        let artist_subscription =
            cx.subscribe(&artists_view, |this, _, event: &ArtistSelectedEvent, cx| {
                let grouping = cx.global::<SettingsStore>().artists_grouping();
                this.show_artist_tracks(event.artist.clone(), grouping, cx);
            });

        let playlist_subscription = cx.subscribe_in(
            &playlists_view,
            window,
            |this, _, event: &PlaylistSelectedEvent, window, cx| {
                this.show_playlist_tracks(event.playlist.clone(), window, cx);
            },
        );

        let all_tracks_subscription = cx.subscribe_in(
            &playlists_view,
            window,
            |this, _, _: &AllTracksSelectedEvent, window, cx| {
                this.show_all_tracks(window, cx);
            },
        );

        let settings_subscriptions = [
            cx.subscribe(&albums_view, |_, _, _: &OpenLibrarySettings, cx| {
                cx.emit(LibraryViewEvent::OpenLibrarySettings);
            }),
            cx.subscribe(&artists_view, |_, _, _: &OpenLibrarySettings, cx| {
                cx.emit(LibraryViewEvent::OpenLibrarySettings);
            }),
        ];

        let settings_observer = cx.observe_global::<SettingsStore>(|this, cx| {
            let tabs = OptionalTabs::read(cx);
            if !tabs.genres {
                this.genres = None;
            }
            let before = this.stack.len();
            this.stack.retain(|entry| tabs.allow(&entry.key()));
            if this.stack.len() == before {
                return;
            }
            if !matches!(this.stack.first(), Some(NavEntry::Root(_))) {
                this.stack = vec![NavEntry::Root(LibraryRootTab::Albums)];
            }
            cx.emit(LibraryViewEvent::StateChanged);
            cx.notify();
        });

        Self {
            stack: vec![NavEntry::Root(LibraryRootTab::Albums)],
            albums_view,
            artists_view,
            genres: None,
            liked_view,
            playlists_view,
            _album_subscription: album_subscription,
            _artist_subscription: artist_subscription,
            _playlist_subscription: playlist_subscription,
            _all_tracks_subscription: all_tracks_subscription,
            _settings_subscriptions: settings_subscriptions,
            _settings_observer: settings_observer,
            _app_events: crate::view_cache::follow_app_events(cx),
        }
    }

    pub fn is_drilled_in(&self) -> bool {
        self.stack.len() > 1
    }

    pub fn view_menu_tab(&self) -> Option<ViewMenuTab> {
        match self.stack.last()? {
            NavEntry::Root(LibraryRootTab::Albums) => Some(ViewMenuTab::Albums),
            NavEntry::Root(LibraryRootTab::Artists) => Some(ViewMenuTab::Artists),
            NavEntry::Root(LibraryRootTab::Liked) | NavEntry::PlaylistTracks { .. } => {
                Some(ViewMenuTab::Tracks)
            }
            NavEntry::GenreTracks { .. } => Some(ViewMenuTab::Genre),
            _ => None,
        }
    }

    pub fn root_tab(&self) -> LibraryRootTab {
        match self.stack.first() {
            Some(NavEntry::Root(tab)) => *tab,
            _ => LibraryRootTab::Albums,
        }
    }

    pub fn current_tab(&self) -> Option<LibraryRootTab> {
        if self.is_drilled_in() {
            return None;
        }
        match self.stack.first() {
            Some(NavEntry::Root(t)) => Some(*t),
            _ => None,
        }
    }

    pub fn select_tab(&mut self, tab: LibraryRootTab, cx: &mut Context<Self>) {
        let same_root =
            self.stack.len() == 1 && matches!(self.stack[0], NavEntry::Root(t) if t == tab);
        if same_root {
            return;
        }
        if tab == LibraryRootTab::Genres {
            self.ensure_genres_view(cx);
        }
        self.stack = vec![NavEntry::Root(tab)];
        cx.emit(LibraryViewEvent::StateChanged);
        cx.notify();
    }

    pub fn apply_search(&mut self, query: &str, cx: &mut Context<Self>) {
        match self.stack.last() {
            Some(NavEntry::Root(LibraryRootTab::Albums)) => {
                self.albums_view.update(cx, |v, cx| v.set_filter(query, cx));
            }
            Some(NavEntry::Root(LibraryRootTab::Artists)) => {
                self.artists_view
                    .update(cx, |v, cx| v.set_filter(query, cx));
            }
            Some(NavEntry::Root(LibraryRootTab::Genres)) => {
                if let Some(genres) = &self.genres {
                    genres.view.update(cx, |v, cx| v.set_filter(query, cx));
                }
            }
            Some(NavEntry::Root(LibraryRootTab::Liked)) => {
                self.liked_view.update(cx, |v, cx| v.set_filter(query, cx));
            }
            Some(NavEntry::Root(LibraryRootTab::Playlists)) => {
                self.playlists_view
                    .update(cx, |v, cx| v.set_filter(query, cx));
            }
            Some(NavEntry::AlbumTracks { view, .. }) => {
                view.update(cx, |v, cx| v.set_filter(query, cx));
            }
            Some(NavEntry::ArtistTracks { view, .. } | NavEntry::GenreTracks { view, .. }) => {
                view.update(cx, |v, cx| v.set_filter(query, cx));
            }
            Some(NavEntry::PlaylistTracks { view, .. }) => {
                view.update(cx, |v, cx| v.set_filter(query, cx));
            }
            None => {}
        }
    }

    pub fn go_back(&mut self, cx: &mut Context<Self>) {
        if self.stack.len() > 1 {
            self.stack.pop();
            cx.emit(LibraryViewEvent::StateChanged);
            cx.notify();
        }
    }

    pub fn nav_keys(&self) -> Vec<NavKey> {
        self.stack.iter().map(NavEntry::key).collect()
    }

    pub fn restore(&mut self, keys: &[NavKey], window: &mut Window, cx: &mut Context<Self>) {
        let tabs = OptionalTabs::read(cx);
        let Some((root @ NavKey::Root(tab), pages)) = keys.split_first() else {
            return;
        };
        if !tabs.allow(root) {
            return;
        }
        self.select_tab(*tab, cx);
        let library = cx.global::<Services>().library.clone();
        for key in pages.iter().filter(|key| tabs.allow(key)) {
            match key {
                NavKey::Album(id) => self.navigate_to_album(*id, cx),
                NavKey::Artist(id, grouping) => {
                    if let Some(artist) = library.artist_summary(*id, *grouping) {
                        self.show_artist_tracks(artist, *grouping, cx);
                    }
                }
                NavKey::Genre(genre) => self.show_genre_tracks(genre.clone(), cx),
                NavKey::Playlist(id) => {
                    if let Some(playlist) = library.playlists().into_iter().find(|p| p.id == *id) {
                        self.show_playlist_tracks(playlist, window, cx);
                    }
                }
                NavKey::AllTracks => self.show_all_tracks(window, cx),
                NavKey::Root(_) => {}
            }
        }
    }

    pub fn navigate_to_album(&mut self, album_id: i64, cx: &mut Context<Self>) {
        let services = cx.global::<Services>();
        if let Some(album) = services
            .library
            .albums()
            .into_iter()
            .find(|a| a.id == album_id)
        {
            self.show_album_tracks(album, cx);
        }
    }

    pub fn navigate_to_artist(
        &mut self,
        artist_id: i64,
        from_track: Option<i64>,
        cx: &mut Context<Self>,
    ) {
        let preferred = cx.global::<SettingsStore>().artists_grouping();
        let library = &cx.global::<Services>().library;
        let found = resolve_artist_page(
            artist_id,
            preferred,
            || {
                from_track.and_then(|track_id| {
                    library.listed_artist_for_credit(track_id, artist_id, preferred)
                })
            },
            |id, grouping| library.artist_summary(id, grouping),
        );
        if let Some((artist, grouping)) = found {
            self.show_artist_tracks(artist, grouping, cx);
        }
    }

    fn show_album_tracks(&mut self, album: music_library::AlbumSummary, cx: &mut Context<Self>) {
        let view = cx.new(|cx| TracksView::new(&album, cx));
        let sub = cx.subscribe(&view, |this, _, event: &NavigateToArtistRequested, cx| {
            this.navigate_to_artist(event.artist_id, event.track_id, cx);
        });
        self.stack.push(NavEntry::AlbumTracks {
            album_id: album.id,
            view,
            _sub: sub,
        });
        cx.emit(LibraryViewEvent::StateChanged);
        cx.notify();
    }

    fn show_artist_tracks(
        &mut self,
        artist: music_library::ArtistSummary,
        grouping: ArtistGrouping,
        cx: &mut Context<Self>,
    ) {
        let view = cx.new(|cx| GroupedTracksView::artist(&artist, grouping, cx));
        let subs = Self::grouped_tracks_subscriptions(&view, cx);
        self.stack.push(NavEntry::ArtistTracks {
            artist_id: artist.id,
            grouping,
            view,
            _subs: subs,
        });
        cx.emit(LibraryViewEvent::StateChanged);
        cx.notify();
    }

    fn show_genre_tracks(&mut self, genre: music_library::GenreSummary, cx: &mut Context<Self>) {
        let sort = cx.global::<SettingsStore>().genres_sort();
        let view = cx.new(|cx| GroupedTracksView::genre(&genre, sort, cx));
        let subs = Self::grouped_tracks_subscriptions(&view, cx);
        self.stack.push(NavEntry::GenreTracks {
            genre,
            view,
            _subs: subs,
        });
        cx.emit(LibraryViewEvent::StateChanged);
        cx.notify();
    }

    fn grouped_tracks_subscriptions(
        view: &Entity<GroupedTracksView>,
        cx: &mut Context<Self>,
    ) -> [Subscription; 2] {
        [
            cx.subscribe(view, |this, _, event: &NavigateToAlbumRequested, cx| {
                this.navigate_to_album(event.album_id, cx);
            }),
            cx.subscribe(view, |this, _, event: &NavigateToArtistRequested, cx| {
                this.navigate_to_artist(event.artist_id, event.track_id, cx);
            }),
        ]
    }

    fn ensure_genres_view(&mut self, cx: &mut Context<Self>) {
        if self.genres.is_some() {
            return;
        }
        let view = cx.new(GenresView::new);
        let subs = [
            cx.subscribe(&view, |this, _, event: &GenreSelectedEvent, cx| {
                this.show_genre_tracks(event.genre.clone(), cx);
            }),
            cx.subscribe(&view, |_, _, _: &OpenLibrarySettings, cx| {
                cx.emit(LibraryViewEvent::OpenLibrarySettings);
            }),
        ];
        self.genres = Some(GenresRoot { view, _subs: subs });
    }

    fn show_playlist_tracks(
        &mut self,
        playlist: music_library::PlaylistSummary,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.new(|cx| {
            PlaylistTracksView::new(
                playlist.name.clone().into(),
                QueueSource::Playlist(playlist.id),
                window,
                cx,
            )
        });
        self.stack.push(NavEntry::PlaylistTracks {
            playlist_id: Some(playlist.id),
            view,
        });
        cx.emit(LibraryViewEvent::StateChanged);
        cx.notify();
    }

    fn show_all_tracks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.new(|cx| {
            PlaylistTracksView::new(tr().all_tracks.clone(), QueueSource::AllTracks, window, cx)
        });
        self.stack.push(NavEntry::PlaylistTracks {
            playlist_id: None,
            view,
        });
        cx.emit(LibraryViewEvent::StateChanged);
        cx.notify();
    }
}

fn resolve_artist_page(
    artist_id: i64,
    preferred: ArtistGrouping,
    listed_for_credit: impl FnOnce() -> Option<i64>,
    summary: impl Fn(i64, ArtistGrouping) -> Option<music_library::ArtistSummary>,
) -> Option<(music_library::ArtistSummary, ArtistGrouping)> {
    let other = match preferred {
        ArtistGrouping::TrackArtist => ArtistGrouping::AlbumArtist,
        ArtistGrouping::AlbumArtist => ArtistGrouping::TrackArtist,
    };
    let open = |id, grouping| summary(id, grouping).map(|artist| (artist, grouping));
    open(artist_id, preferred)
        .or_else(|| {
            listed_for_credit()
                .filter(|&listed| listed != artist_id)
                .and_then(|listed| open(listed, preferred))
        })
        .or_else(|| open(artist_id, other))
}

impl EventEmitter<LibraryViewEvent> for LibraryView {}

impl Render for LibraryView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().relative().size_full().child(match self.stack.last() {
            Some(NavEntry::Root(LibraryRootTab::Albums)) => {
                v_flex().size_full().child(self.albums_view.clone())
            }
            Some(NavEntry::Root(LibraryRootTab::Artists)) => {
                v_flex().size_full().child(self.artists_view.clone())
            }
            Some(NavEntry::Root(LibraryRootTab::Genres)) => match &self.genres {
                Some(genres) => v_flex().size_full().child(genres.view.clone()),
                None => v_flex().size_full(),
            },
            Some(NavEntry::Root(LibraryRootTab::Liked)) => {
                v_flex().size_full().child(self.liked_view.clone())
            }
            Some(NavEntry::Root(LibraryRootTab::Playlists)) => {
                v_flex().size_full().child(self.playlists_view.clone())
            }
            Some(NavEntry::AlbumTracks { view, .. }) => v_flex().size_full().child(view.clone()),
            Some(NavEntry::ArtistTracks { view, .. } | NavEntry::GenreTracks { view, .. }) => {
                v_flex().size_full().child(view.clone())
            }
            Some(NavEntry::PlaylistTracks { view, .. }) => v_flex().size_full().child(view.clone()),
            None => v_flex().size_full(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    const MAIN: i64 = 1;
    const FEATURED: i64 = 2;
    const GUEST: i64 = 3;
    const VARIOUS: i64 = 4;

    fn summary(id: i64, grouping: ArtistGrouping) -> Option<music_library::ArtistSummary> {
        let listed = match grouping {
            ArtistGrouping::AlbumArtist => [MAIN, VARIOUS].contains(&id),
            ArtistGrouping::TrackArtist => [MAIN, FEATURED, GUEST].contains(&id),
        };
        listed.then(|| music_library::ArtistSummary {
            id,
            name: id.to_string(),
            sort_name: id.to_string(),
            track_count: 1,
        })
    }

    fn resolve(
        artist_id: i64,
        preferred: ArtistGrouping,
        listed: Option<i64>,
    ) -> (Option<(i64, ArtistGrouping)>, bool) {
        let asked = Cell::new(false);
        let found = resolve_artist_page(
            artist_id,
            preferred,
            || {
                asked.set(true);
                listed
            },
            summary,
        );
        (
            found.map(|(artist, grouping)| (artist.id, grouping)),
            asked.get(),
        )
    }

    #[rstest::rstest]
    #[case::listed_artist_opens_directly(MAIN, ArtistGrouping::AlbumArtist, Some(MAIN), Some((MAIN, ArtistGrouping::AlbumArtist)))]
    #[case::featured_credit_opens_its_listed_artist(FEATURED, ArtistGrouping::AlbumArtist, Some(MAIN), Some((MAIN, ArtistGrouping::AlbumArtist)))]
    #[case::compilation_guest_keeps_their_own_page(GUEST, ArtistGrouping::AlbumArtist, None, Some((GUEST, ArtistGrouping::TrackArtist)))]
    #[case::album_artist_link_in_track_grouping(VARIOUS, ArtistGrouping::TrackArtist, None, Some((VARIOUS, ArtistGrouping::AlbumArtist)))]
    #[case::listed_same_as_clicked_falls_through(FEATURED, ArtistGrouping::AlbumArtist, Some(FEATURED), Some((FEATURED, ArtistGrouping::TrackArtist)))]
    #[case::unknown_artist_goes_nowhere(99, ArtistGrouping::AlbumArtist, None, None)]
    fn artist_navigation_picks_the_first_page_that_exists(
        #[case] clicked: i64,
        #[case] preferred: ArtistGrouping,
        #[case] listed: Option<i64>,
        #[case] expected: Option<(i64, ArtistGrouping)>,
    ) {
        assert_eq!(resolve(clicked, preferred, listed).0, expected);
    }

    #[test]
    fn the_credit_lookup_runs_only_when_the_clicked_artist_is_not_listed() {
        assert!(!resolve(MAIN, ArtistGrouping::AlbumArtist, Some(MAIN)).1);
        assert!(resolve(FEATURED, ArtistGrouping::AlbumArtist, Some(MAIN)).1);
    }
}

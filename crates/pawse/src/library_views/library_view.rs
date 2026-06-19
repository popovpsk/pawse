use gpui::{
    AppContext, Context, Entity, EventEmitter, IntoElement, ParentElement, Render, Styled,
    Subscription, Window, div,
};
use gpui_component::v_flex;

use crate::library_views::albums_view::{AddMusicFolderRequested, AlbumSelectedEvent, AlbumsView};
use crate::library_views::artist_tracks_view::ArtistTracksView;
use crate::library_views::artists_view::{ArtistSelectedEvent, ArtistsView};
use crate::library_views::liked_view::LikedView;
use crate::library_views::playlist_tracks_view::PlaylistTracksView;
use crate::library_views::playlists_view::{
    AllTracksSelectedEvent, PlaylistSelectedEvent, PlaylistsView,
};
use crate::library_views::tracks_view::TracksView;
use crate::localization::tr;
use crate::now_playing::{NavigateToAlbumRequested, NavigateToArtistRequested};
use crate::playback_queue::QueueSource;
use crate::services::Services;
use crate::settings_store::SettingsStore;

#[derive(Clone, Debug)]
pub enum LibraryViewEvent {
    StateChanged,
    AddMusicFolderRequested,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LibraryRootTab {
    Albums,
    Artists,
    Liked,
    Playlists,
}

enum NavEntry {
    Root(LibraryRootTab),
    AlbumTracks {
        view: Entity<TracksView>,
        _sub: Subscription,
    },
    ArtistTracks {
        view: Entity<ArtistTracksView>,
        _sub: Subscription,
    },
    PlaylistTracks(Entity<PlaylistTracksView>),
}

pub struct LibraryView {
    stack: Vec<NavEntry>,
    albums_view: Entity<AlbumsView>,
    artists_view: Entity<ArtistsView>,
    liked_view: Entity<LikedView>,
    playlists_view: Entity<PlaylistsView>,
    _album_subscription: Subscription,
    _artist_subscription: Subscription,
    _playlist_subscription: Subscription,
    _all_tracks_subscription: Subscription,
    _settings_subscription: Subscription,
    _settings_observer: Subscription,
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
                this.show_artist_tracks(event.artist.clone(), cx);
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

        let settings_subscription =
            cx.subscribe(&albums_view, |_, _, _: &AddMusicFolderRequested, cx| {
                cx.emit(LibraryViewEvent::AddMusicFolderRequested);
            });

        let settings_observer = cx.observe_global::<SettingsStore>(|this, cx| {
            let store = cx.global::<SettingsStore>();
            let liked = store.liked_enabled();
            let playlists = store.playlists_enabled();
            let before = this.stack.len();
            this.stack.retain(|entry| match entry {
                NavEntry::Root(LibraryRootTab::Liked) => liked,
                NavEntry::Root(LibraryRootTab::Playlists) => playlists,
                NavEntry::PlaylistTracks(_) => playlists,
                _ => true,
            });
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
            liked_view,
            playlists_view,
            _album_subscription: album_subscription,
            _artist_subscription: artist_subscription,
            _playlist_subscription: playlist_subscription,
            _all_tracks_subscription: all_tracks_subscription,
            _settings_subscription: settings_subscription,
            _settings_observer: settings_observer,
        }
    }

    pub fn is_drilled_in(&self) -> bool {
        self.stack.len() > 1
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
            Some(NavEntry::ArtistTracks { view, .. }) => {
                view.update(cx, |v, cx| v.set_filter(query, cx));
            }
            Some(NavEntry::PlaylistTracks(view)) => {
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

    pub fn navigate_to_artist(&mut self, artist_id: i64, cx: &mut Context<Self>) {
        let services = cx.global::<Services>();
        if let Some(artist) = services
            .library
            .artists()
            .into_iter()
            .find(|a| a.id == artist_id)
        {
            self.show_artist_tracks(artist, cx);
        }
    }

    fn show_album_tracks(&mut self, album: music_library::AlbumSummary, cx: &mut Context<Self>) {
        let view = cx.new(|cx| TracksView::new(&album, cx));
        let sub = cx.subscribe(&view, |this, _, event: &NavigateToArtistRequested, cx| {
            this.navigate_to_artist(event.artist_id, cx);
        });
        self.stack.push(NavEntry::AlbumTracks { view, _sub: sub });
        cx.emit(LibraryViewEvent::StateChanged);
        cx.notify();
    }

    fn show_artist_tracks(&mut self, artist: music_library::ArtistSummary, cx: &mut Context<Self>) {
        let view = cx.new(|cx| ArtistTracksView::new(&artist, cx));
        let sub = cx.subscribe(&view, |this, _, event: &NavigateToAlbumRequested, cx| {
            this.navigate_to_album(event.album_id, cx);
        });
        self.stack.push(NavEntry::ArtistTracks { view, _sub: sub });
        cx.emit(LibraryViewEvent::StateChanged);
        cx.notify();
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
        self.stack.push(NavEntry::PlaylistTracks(view));
        cx.emit(LibraryViewEvent::StateChanged);
        cx.notify();
    }

    fn show_all_tracks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.new(|cx| {
            PlaylistTracksView::new(tr().all_tracks.clone(), QueueSource::AllTracks, window, cx)
        });
        self.stack.push(NavEntry::PlaylistTracks(view));
        cx.emit(LibraryViewEvent::StateChanged);
        cx.notify();
    }
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
            Some(NavEntry::Root(LibraryRootTab::Liked)) => {
                v_flex().size_full().child(self.liked_view.clone())
            }
            Some(NavEntry::Root(LibraryRootTab::Playlists)) => {
                v_flex().size_full().child(self.playlists_view.clone())
            }
            Some(NavEntry::AlbumTracks { view, .. }) => v_flex().size_full().child(view.clone()),
            Some(NavEntry::ArtistTracks { view, .. }) => v_flex().size_full().child(view.clone()),
            Some(NavEntry::PlaylistTracks(view)) => v_flex().size_full().child(view.clone()),
            None => v_flex().size_full(),
        })
    }
}

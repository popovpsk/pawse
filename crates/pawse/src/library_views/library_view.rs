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
use crate::now_playing::NavigateToArtistRequested;
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

enum LibraryViewState {
    Root(LibraryRootTab),
    AlbumTracks,
    ArtistTracks,
    PlaylistTracks,
}

pub struct LibraryView {
    state: LibraryViewState,
    albums_view: Entity<AlbumsView>,
    artists_view: Entity<ArtistsView>,
    liked_view: Entity<LikedView>,
    playlists_view: Entity<PlaylistsView>,
    tracks_view: Option<Entity<TracksView>>,
    artist_tracks_view: Option<Entity<ArtistTracksView>>,
    playlist_tracks_view: Option<Entity<PlaylistTracksView>>,
    _album_subscription: Subscription,
    _artist_subscription: Subscription,
    _playlist_subscription: Subscription,
    _all_tracks_subscription: Subscription,
    _settings_subscription: Subscription,
    _settings_observer: Subscription,
    _tracks_artist_subscription: Option<Subscription>,
}

impl LibraryView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let albums_view = cx.new(|cx| AlbumsView::new(window, cx));
        let artists_view = cx.new(|cx| ArtistsView::new(window, cx));
        let liked_view = cx.new(|cx| LikedView::new(window, cx));
        let playlists_view = cx.new(|cx| PlaylistsView::new(window, cx));

        let album_subscription = cx.subscribe_in(
            &albums_view,
            window,
            |this, _, event: &AlbumSelectedEvent, window, cx| {
                this.show_album_tracks(event.album.clone(), window, cx);
            },
        );

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

        // Pop back to a still-visible tab if the user disables the one we're on.
        let settings_observer = cx.observe_global::<SettingsStore>(|this, cx| {
            let store = cx.global::<SettingsStore>();
            let liked = store.liked_enabled();
            let playlists = store.playlists_enabled();
            let needs_pop = match this.state {
                LibraryViewState::Root(LibraryRootTab::Liked) if !liked => true,
                LibraryViewState::Root(LibraryRootTab::Playlists) if !playlists => true,
                LibraryViewState::PlaylistTracks if !playlists => true,
                _ => false,
            };
            if needs_pop {
                this.state = LibraryViewState::Root(LibraryRootTab::Albums);
                this.tracks_view = None;
                this.artist_tracks_view = None;
                this.playlist_tracks_view = None;
                this._tracks_artist_subscription = None;
                cx.emit(LibraryViewEvent::StateChanged);
                cx.notify();
            }
        });

        Self {
            state: LibraryViewState::Root(LibraryRootTab::Albums),
            albums_view,
            artists_view,
            liked_view,
            playlists_view,
            tracks_view: None,
            artist_tracks_view: None,
            playlist_tracks_view: None,
            _album_subscription: album_subscription,
            _artist_subscription: artist_subscription,
            _playlist_subscription: playlist_subscription,
            _all_tracks_subscription: all_tracks_subscription,
            _settings_subscription: settings_subscription,
            _settings_observer: settings_observer,
            _tracks_artist_subscription: None,
        }
    }

    pub fn is_drilled_in(&self) -> bool {
        matches!(
            self.state,
            LibraryViewState::AlbumTracks
                | LibraryViewState::ArtistTracks
                | LibraryViewState::PlaylistTracks
        )
    }

    pub fn current_tab(&self) -> Option<LibraryRootTab> {
        match self.state {
            LibraryViewState::Root(t) => Some(t),
            _ => None,
        }
    }

    pub fn select_tab(&mut self, tab: LibraryRootTab, cx: &mut Context<Self>) {
        let same_root = matches!(self.state, LibraryViewState::Root(t) if t == tab);
        if same_root {
            return;
        }
        self.state = LibraryViewState::Root(tab);
        self.tracks_view = None;
        self.artist_tracks_view = None;
        self.playlist_tracks_view = None;
        self._tracks_artist_subscription = None;
        cx.emit(LibraryViewEvent::StateChanged);
        cx.notify();
    }

    pub fn apply_search(&mut self, query: &str, cx: &mut Context<Self>) {
        match self.state {
            LibraryViewState::Root(LibraryRootTab::Albums) => {
                self.albums_view.update(cx, |v, cx| v.set_filter(query, cx));
            }
            LibraryViewState::Root(LibraryRootTab::Artists) => {
                self.artists_view
                    .update(cx, |v, cx| v.set_filter(query, cx));
            }
            LibraryViewState::Root(LibraryRootTab::Liked) => {
                self.liked_view.update(cx, |v, cx| v.set_filter(query, cx));
            }
            LibraryViewState::Root(LibraryRootTab::Playlists) => {
                self.playlists_view
                    .update(cx, |v, cx| v.set_filter(query, cx));
            }
            LibraryViewState::AlbumTracks => {
                if let Some(tv) = &self.tracks_view {
                    tv.update(cx, |v, cx| v.set_filter(query, cx));
                }
            }
            LibraryViewState::ArtistTracks => {
                if let Some(av) = &self.artist_tracks_view {
                    av.update(cx, |v, cx| v.set_filter(query, cx));
                }
            }
            LibraryViewState::PlaylistTracks => {
                if let Some(pv) = &self.playlist_tracks_view {
                    pv.update(cx, |v, cx| v.set_filter(query, cx));
                }
            }
        }
    }

    pub fn go_back(&mut self, cx: &mut Context<Self>) {
        let tab = match self.state {
            LibraryViewState::AlbumTracks => LibraryRootTab::Albums,
            LibraryViewState::ArtistTracks => LibraryRootTab::Artists,
            LibraryViewState::PlaylistTracks => LibraryRootTab::Playlists,
            LibraryViewState::Root(t) => t,
        };
        self.state = LibraryViewState::Root(tab);
        self.tracks_view = None;
        self.artist_tracks_view = None;
        self.playlist_tracks_view = None;
        self._tracks_artist_subscription = None;
        cx.emit(LibraryViewEvent::StateChanged);
        cx.notify();
    }

    pub fn navigate_to_album(
        &mut self,
        album_id: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let services = cx.global::<Services>();
        if let Some(album) = services
            .library
            .albums()
            .into_iter()
            .find(|a| a.id == album_id)
        {
            self.show_album_tracks(album, window, cx);
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

    fn show_album_tracks(
        &mut self,
        album: music_library::AlbumSummary,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state = LibraryViewState::AlbumTracks;
        let tracks_view = cx.new(|cx| TracksView::new(&album, cx));
        self._tracks_artist_subscription = Some(cx.subscribe(
            &tracks_view,
            |this, _, event: &NavigateToArtistRequested, cx| {
                this.navigate_to_artist(event.artist_id, cx);
            },
        ));
        self.tracks_view = Some(tracks_view);
        self.artist_tracks_view = None;
        self.playlist_tracks_view = None;
        cx.emit(LibraryViewEvent::StateChanged);
        cx.notify();
    }

    fn show_artist_tracks(&mut self, artist: music_library::ArtistSummary, cx: &mut Context<Self>) {
        self.state = LibraryViewState::ArtistTracks;
        let artist_tracks_view = cx.new(|cx| ArtistTracksView::new(&artist, cx));
        self.artist_tracks_view = Some(artist_tracks_view);
        self.tracks_view = None;
        self.playlist_tracks_view = None;
        self._tracks_artist_subscription = None;
        cx.emit(LibraryViewEvent::StateChanged);
        cx.notify();
    }

    fn show_playlist_tracks(
        &mut self,
        playlist: music_library::PlaylistSummary,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state = LibraryViewState::PlaylistTracks;
        let view = cx.new(|cx| {
            PlaylistTracksView::new(
                playlist.name.clone().into(),
                QueueSource::Playlist(playlist.id),
                window,
                cx,
            )
        });
        self.playlist_tracks_view = Some(view);
        self.tracks_view = None;
        self.artist_tracks_view = None;
        self._tracks_artist_subscription = None;
        cx.emit(LibraryViewEvent::StateChanged);
        cx.notify();
    }

    fn show_all_tracks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.state = LibraryViewState::PlaylistTracks;
        let view = cx.new(|cx| {
            PlaylistTracksView::new(tr().all_tracks.clone(), QueueSource::AllTracks, window, cx)
        });
        self.playlist_tracks_view = Some(view);
        self.tracks_view = None;
        self.artist_tracks_view = None;
        self._tracks_artist_subscription = None;
        cx.emit(LibraryViewEvent::StateChanged);
        cx.notify();
    }
}

impl EventEmitter<LibraryViewEvent> for LibraryView {}

impl Render for LibraryView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().relative().size_full().child(match self.state {
            LibraryViewState::Root(LibraryRootTab::Albums) => {
                v_flex().size_full().child(self.albums_view.clone())
            }
            LibraryViewState::Root(LibraryRootTab::Artists) => {
                v_flex().size_full().child(self.artists_view.clone())
            }
            LibraryViewState::Root(LibraryRootTab::Liked) => {
                v_flex().size_full().child(self.liked_view.clone())
            }
            LibraryViewState::Root(LibraryRootTab::Playlists) => {
                v_flex().size_full().child(self.playlists_view.clone())
            }
            LibraryViewState::AlbumTracks => {
                if let Some(ref tracks_view) = self.tracks_view {
                    v_flex().size_full().child(tracks_view.clone())
                } else {
                    v_flex().size_full()
                }
            }
            LibraryViewState::ArtistTracks => {
                if let Some(ref artist_tracks_view) = self.artist_tracks_view {
                    v_flex().size_full().child(artist_tracks_view.clone())
                } else {
                    v_flex().size_full()
                }
            }
            LibraryViewState::PlaylistTracks => {
                if let Some(ref playlist_tracks_view) = self.playlist_tracks_view {
                    v_flex().size_full().child(playlist_tracks_view.clone())
                } else {
                    v_flex().size_full()
                }
            }
        })
    }
}

use gpui::{AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Window, div};
use gpui_component::v_flex;

use crate::audio_settings::AudioSettings;
use crate::library_views::albums_view::{AlbumSelectedEvent, AlbumsView};
use crate::library_views::tracks_view::{BackEvent, TracksView};

enum LibraryViewState {
    Albums,
    Tracks,
}

pub struct LibraryView {
    state: LibraryViewState,
    albums_view: Entity<AlbumsView>,
    tracks_view: Option<Entity<TracksView>>,
    tracks_subscription: Option<Subscription>,
    _album_subscription: Subscription,
    audio_settings: Entity<AudioSettings>,
}

impl LibraryView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let albums_view = cx.new(|cx| AlbumsView::new(window, cx));
        let audio_settings = cx.new(|cx| AudioSettings::new(window, cx));

        let album_subscription = cx.subscribe(
            &albums_view,
            |this, _, event: &AlbumSelectedEvent, cx| {
                this.show_tracks(event.album.clone(), cx);
            },
        );

        Self {
            state: LibraryViewState::Albums,
            albums_view,
            tracks_view: None,
            tracks_subscription: None,
            _album_subscription: album_subscription,
            audio_settings,
        }
    }

    fn show_tracks(&mut self, album: music_library::AlbumSummary, cx: &mut Context<Self>) {
        self.state = LibraryViewState::Tracks;
        let tracks_view = cx.new(|cx| TracksView::new(&album, cx));
        let back_subscription = cx.subscribe(
            &tracks_view,
            |this, _, _: &BackEvent, cx| {
                this.state = LibraryViewState::Albums;
                this.tracks_view = None;
                this.tracks_subscription = None;
                cx.notify();
            },
        );
        self.tracks_view = Some(tracks_view);
        self.tracks_subscription = Some(back_subscription);
        cx.notify();
    }
}

impl Render for LibraryView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .relative()
            .size_full()
            .child(self.audio_settings.clone())
            .child(match self.state {
                LibraryViewState::Albums => v_flex()
                    .size_full()
                    .child(self.albums_view.clone()),
                LibraryViewState::Tracks => {
                    if let Some(ref tracks_view) = self.tracks_view {
                        v_flex().size_full().child(tracks_view.clone())
                    } else {
                        v_flex().size_full()
                    }
                }
            })
    }
}

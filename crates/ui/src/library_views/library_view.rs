use gpui::{AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Window};
use gpui_component::v_flex;

use crate::library_views::albums_view::{AlbumSelectedEvent, AlbumsView};
use crate::library_views::tracks_view::{BackEvent, TracksView};
use crate::services::Services;

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
}

impl LibraryView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let albums_view = cx.new(|cx| AlbumsView::new(window, cx));

        let album_subscription = cx.subscribe(
            &albums_view,
            |this, _, event: &AlbumSelectedEvent, cx| {
                this.show_tracks(event.album_id, cx);
            },
        );

        let services = cx.global::<Services>();
        if services.library.has_tracks() {
            // Trigger background rescan on startup if we already have tracks
            // We don't block UI; just start it in background
            // Note: user needs to pick folder for rescan, so we skip auto-rescan
            // unless they configured a default folder. For now, manual only after first scan.
        }

        Self {
            state: LibraryViewState::Albums,
            albums_view,
            tracks_view: None,
            tracks_subscription: None,
            _album_subscription: album_subscription,
        }
    }

    fn show_tracks(&mut self, album_id: i64, cx: &mut Context<Self>) {
        self.state = LibraryViewState::Tracks;
        let tracks_view = cx.new(|cx| TracksView::new(album_id, cx));
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
        match self.state {
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
        }
    }
}

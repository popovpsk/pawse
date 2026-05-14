use std::rc::Rc;

use std::path::PathBuf;

use gpui::{Context, ElementId, EventEmitter, Hsla, InteractiveElement, IntoElement, ParentElement, Render, StatefulInteractiveElement, Styled, StyledImage, Subscription, Window, div, img, px, size, Size, Pixels};
use gpui_component::{h_flex, v_flex, v_virtual_list, ActiveTheme, VirtualListScrollHandle};

use crate::library_service::LibraryEvent;
use crate::services::Services;

#[derive(Clone, Debug)]
pub struct AlbumSelectedEvent {
    pub album: music_library::AlbumSummary,
}

const ALBUM_ROW_HEIGHT: f32 = 48.;

pub struct AlbumsView {
    albums: Vec<music_library::AlbumSummary>,
    is_scanning: bool,
    item_sizes: Rc<Vec<Size<Pixels>>>,
    scroll_handle: VirtualListScrollHandle,
    _subscription: Subscription,
}

impl AlbumsView {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let services = cx.global::<Services>();
        let library_event_bus = services.library_event_bus.clone();
        let library = services.library.clone();

        let albums = library.albums();
        let item_sizes = Self::make_item_sizes(&albums);
        let is_scanning = false;

        let subscription = cx.subscribe(
            &library_event_bus,
            |this, _, event: &LibraryEvent, cx| match event {
                LibraryEvent::ScanStarted => {
                    this.is_scanning = true;
                    cx.notify();
                }
                LibraryEvent::ScanComplete => {
                    this.is_scanning = false;
                    this.albums = cx.global::<Services>().library.albums();
                    this.item_sizes = Self::make_item_sizes(&this.albums);
                    cx.notify();
                }
                _ => {}
            },
        );

        Self {
            albums,
            is_scanning,
            item_sizes,
            scroll_handle: VirtualListScrollHandle::new(),
            _subscription: subscription,
        }
    }

    fn make_item_sizes(albums: &[music_library::AlbumSummary]) -> Rc<Vec<Size<Pixels>>> {
        Rc::new(vec![size(px(300.), px(ALBUM_ROW_HEIGHT + 1.)); albums.len()])
    }

}

impl EventEmitter<AlbumSelectedEvent> for AlbumsView {}

impl Render for AlbumsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = h_flex()
            .gap_2()
            .px_4()
            .py_2();

        if self.is_scanning && self.albums.is_empty() {
            return v_flex()
                .size_full()
                .child(header)
                .child(div().px_4().child("Scanning..."));
        }

        if self.albums.is_empty() {
            return v_flex()
                .size_full()
                .child(header)
                .child(div().px_4().child("No albums found. Add a music folder to get started."));
        }

        let item_sizes = self.item_sizes.clone();
        v_flex()
            .size_full()
            .child(header)
            .child(
                v_virtual_list(
                    cx.entity().clone(),
                    "albums_list",
                    item_sizes,
                    |view, visible_range, _window, cx| {
                        visible_range
                            .map(|ix| {
                                let album = &view.albums[ix];
                                let album = album.clone();
                                let year_str = album
                                    .year
                                    .map(|y| format!(" ({})", y))
                                    .unwrap_or_default();

                                div()
                                    .w_full()
                                    .h(px(ALBUM_ROW_HEIGHT))
                                    .px_4()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .border_b(px(1.))
                                    .border_color(Hsla {
                                        h: 0.,
                                        s: 0.,
                                        l: 1.,
                                        a: 0.1,
                                    })
                                    .cursor(gpui::CursorStyle::PointingHand)
                                    .hover(|style| style.bg(cx.theme().secondary))
                                    .child({
                                        let fallback_bg = cx.theme().secondary;
                                        let cover: gpui::AnyElement = if let Some(ref path) = album.cover_art_path {
                                            img(PathBuf::from(path))
                                                .w(px(32.))
                                                .h(px(32.))
                                                .rounded(px(4.))
                                                .object_fit(gpui::ObjectFit::Cover)
                                                .with_fallback(move || {
                                                    div()
                                                        .w(px(32.))
                                                        .h(px(32.))
                                                        .rounded(px(4.))
                                                        .bg(fallback_bg)
                                                        .into_any_element()
                                                })
                                                .into_any_element()
                                        } else {
                                            div()
                                                .w(px(32.))
                                                .h(px(32.))
                                                .rounded(px(4.))
                                                .bg(cx.theme().secondary)
                                                .into_any_element()
                                        };
                                        cover
                                    })
                                    .child(
                                        div()
                                            .flex_1()
                                            .child(format!(
                                                "{}{} - {}",
                                                album.artist_name, year_str, album.title
                                            ))
                                    )
                                    .id(ElementId::Integer(album.id as u64))
                                    .on_click(cx.listener(move |_this, _, _, _cx| {
                                        _cx.emit(AlbumSelectedEvent { album: album.clone() });
                                    }))
                            })
                            .collect::<Vec<_>>()
                    },
                )
                .track_scroll(&self.scroll_handle)
                .flex_1(),
            )
    }
}

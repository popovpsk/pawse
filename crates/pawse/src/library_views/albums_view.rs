use std::rc::Rc;

use gpui::{
    Context, ElementId, EventEmitter, InteractiveElement, IntoElement, ParentElement, Pixels,
    Render, Size, StatefulInteractiveElement, Styled, StyledImage, Subscription, Window, div, img,
    px, size,
};
use gpui_component::{ActiveTheme, VirtualListScrollHandle, h_flex, v_flex, v_virtual_list};
use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};

use crate::library_service::LibraryEvent;
use crate::services::Services;

#[derive(Clone, Debug)]
pub struct AlbumSelectedEvent {
    pub album: music_library::AlbumSummary,
}

const ALBUM_ROW_HEIGHT: f32 = 48.;

pub struct AlbumsView {
    albums_all: Vec<music_library::AlbumSummary>,
    search_entries: Vec<music_library::AlbumSearchEntry>,
    albums: Vec<music_library::AlbumSummary>,
    filter: String,
    matcher: Matcher,
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

        let albums_all = library.albums();
        let search_entries = library.album_search_entries();
        let albums = albums_all.clone();
        let item_sizes = Self::make_item_sizes(&albums);
        let is_scanning = false;

        let subscription =
            cx.subscribe(
                &library_event_bus,
                |this, _, event: &LibraryEvent, cx| match event {
                    LibraryEvent::ScanStarted => {
                        this.is_scanning = true;
                        cx.notify();
                    }
                    LibraryEvent::ScanComplete => {
                        this.is_scanning = false;
                        let services = cx.global::<Services>();
                        services.cover_art_cache.borrow_mut().clear();
                        this.albums_all = services.library.albums();
                        this.search_entries = services.library.album_search_entries();
                        {
                            let mut cache = services.cover_art_cache.borrow_mut();
                            for album in &this.albums_all {
                                cache.get_small(album.cover_art_id, &services.library);
                            }
                        }
                        this.recompute_visible();
                        cx.notify();
                    }
                    _ => {}
                },
            );

        Self {
            albums_all,
            search_entries,
            albums,
            filter: String::new(),
            matcher: Matcher::new(Config::DEFAULT),
            is_scanning,
            item_sizes,
            scroll_handle: VirtualListScrollHandle::new(),
            _subscription: subscription,
        }
    }

    fn make_item_sizes(albums: &[music_library::AlbumSummary]) -> Rc<Vec<Size<Pixels>>> {
        Rc::new(vec![
            size(px(300.), px(ALBUM_ROW_HEIGHT + 1.));
            albums.len()
        ])
    }

    pub fn set_filter(&mut self, query: &str, cx: &mut Context<Self>) {
        let trimmed = query.trim().to_string();
        if trimmed == self.filter {
            return;
        }
        self.filter = trimmed;
        self.recompute_visible();
        cx.notify();
    }

    fn recompute_visible(&mut self) {
        if self.filter.is_empty() {
            self.albums = self.albums_all.clone();
        } else {
            let pattern = Pattern::parse(&self.filter, CaseMatching::Ignore, Normalization::Smart);
            let mut buf: Vec<char> = Vec::new();
            let mut scored: Vec<(i64, u32)> = self
                .search_entries
                .iter()
                .filter_map(|entry| {
                    let haystack = Utf32Str::new(&entry.haystack, &mut buf);
                    pattern
                        .score(haystack, &mut self.matcher)
                        .map(|s| (entry.album_id, s))
                })
                .collect();
            scored.sort_by_key(|(_, score)| std::cmp::Reverse(*score));

            let by_id: std::collections::HashMap<i64, &music_library::AlbumSummary> =
                self.albums_all.iter().map(|a| (a.id, a)).collect();
            self.albums = scored
                .into_iter()
                .filter_map(|(id, _)| by_id.get(&id).map(|a| (*a).clone()))
                .collect();
        }
        self.item_sizes = Self::make_item_sizes(&self.albums);
    }
}

impl EventEmitter<AlbumSelectedEvent> for AlbumsView {}

impl Render for AlbumsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = h_flex().gap_2().px_4().py_2();

        if self.is_scanning && self.albums_all.is_empty() {
            return v_flex()
                .size_full()
                .child(header)
                .child(div().px_4().child("Scanning..."));
        }

        if self.albums.is_empty() {
            let message = if self.albums_all.is_empty() {
                "No albums found. Add a music folder to get started."
            } else {
                "No albums match your search."
            };
            return v_flex()
                .size_full()
                .child(header)
                .child(div().px_4().child(message));
        }

        let item_sizes = self.item_sizes.clone();
        v_flex().size_full().child(header).child(
            v_virtual_list(
                cx.entity().clone(),
                "albums_list",
                item_sizes,
                |view, visible_range, _window, cx| {
                    visible_range
                        .map(|ix| {
                            let album = &view.albums[ix];
                            let album = album.clone();
                            let year_str =
                                album.year.map(|y| format!(" ({})", y)).unwrap_or_default();

                            div()
                                .w_full()
                                .h(px(ALBUM_ROW_HEIGHT))
                                .px_4()
                                .flex()
                                .items_center()
                                .gap_2()
                                .border_b(px(1.))
                                .border_color(cx.theme().border)
                                .cursor(gpui::CursorStyle::PointingHand)
                                .hover(|style| style.bg(cx.theme().secondary))
                                .child({
                                    let fallback_bg = cx.theme().secondary;
                                    let cover: gpui::AnyElement = {
                                        let services = cx.global::<Services>();
                                        let cover_img = services
                                            .cover_art_cache
                                            .borrow_mut()
                                            .get_small(album.cover_art_id, &services.library);
                                        if let Some(cover_img) = cover_img {
                                            img(cover_img)
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
                                                .bg(fallback_bg)
                                                .into_any_element()
                                        }
                                    };
                                    cover
                                })
                                .child(div().flex_1().overflow_hidden().truncate().child(format!(
                                    "{}{} - {}",
                                    album.artist_name, year_str, album.title
                                )))
                                .id(ElementId::Integer(album.id as u64))
                                .on_click(cx.listener(move |_this, _, _, _cx| {
                                    _cx.emit(AlbumSelectedEvent {
                                        album: album.clone(),
                                    });
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

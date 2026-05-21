use std::rc::Rc;

use gpui::{
    Context, ElementId, EventEmitter, InteractiveElement, IntoElement, ParentElement, Pixels,
    Render, Size, StatefulInteractiveElement, Styled, Subscription, Window, div, px, size,
};
use gpui_component::{ActiveTheme, VirtualListScrollHandle, h_flex, v_flex, v_virtual_list};
use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};

use crate::library_service::LibraryEvent;
use crate::services::Services;

#[derive(Clone, Debug)]
pub struct ArtistSelectedEvent {
    pub artist: music_library::ArtistSummary,
}

const TOP_PADDING: f32 = 12.;
const ARTIST_ROW_HEIGHT: f32 = 44.;
const MIN_FUZZY_SCORE_PER_CHAR: u32 = 14;

pub struct ArtistsView {
    artists_all: Vec<music_library::ArtistSummary>,
    artists: Vec<music_library::ArtistSummary>,
    filter: String,
    matcher: Matcher,
    is_scanning: bool,
    item_sizes: Rc<Vec<Size<Pixels>>>,
    scroll_handle: VirtualListScrollHandle,
    _subscription: Subscription,
}

impl ArtistsView {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let services = cx.global::<Services>();
        let library_event_bus = services.library_event_bus.clone();
        let library = services.library.clone();

        let artists_all = library.artists();
        let artists = artists_all.clone();
        let item_sizes = Self::make_item_sizes(&artists);

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
                        this.artists_all = services.library.artists();
                        this.recompute_visible();
                        cx.notify();
                    }
                    _ => {}
                },
            );

        Self {
            artists_all,
            artists,
            filter: String::new(),
            matcher: Matcher::new(Config::DEFAULT),
            is_scanning: false,
            item_sizes,
            scroll_handle: VirtualListScrollHandle::new(),
            _subscription: subscription,
        }
    }

    fn make_item_sizes(artists: &[music_library::ArtistSummary]) -> Rc<Vec<Size<Pixels>>> {
        let mut sizes = vec![size(px(300.), px(TOP_PADDING))];
        sizes.extend(vec![
            size(px(300.), px(ARTIST_ROW_HEIGHT + 1.));
            artists.len()
        ]);
        Rc::new(sizes)
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
            self.artists = self.artists_all.clone();
        } else {
            let pattern = Pattern::parse(&self.filter, CaseMatching::Ignore, Normalization::Smart);
            let threshold = self.filter.chars().count() as u32 * MIN_FUZZY_SCORE_PER_CHAR;
            let mut buf: Vec<char> = Vec::new();
            let mut scored: Vec<(music_library::ArtistSummary, u32)> = self
                .artists_all
                .iter()
                .filter_map(|a| {
                    let haystack = Utf32Str::new(&a.name, &mut buf);
                    pattern
                        .score(haystack, &mut self.matcher)
                        .filter(|s| *s >= threshold)
                        .map(|s| (a.clone(), s))
                })
                .collect();
            scored.sort_by_key(|(_, score)| std::cmp::Reverse(*score));
            self.artists = scored.into_iter().map(|(a, _)| a).collect();
        }
        self.item_sizes = Self::make_item_sizes(&self.artists);
    }
}

impl EventEmitter<ArtistSelectedEvent> for ArtistsView {}

impl Render for ArtistsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.is_scanning && self.artists_all.is_empty() {
            return v_flex()
                .size_full()
                .child(div().px_4().child("Scanning..."));
        }

        if self.artists.is_empty() {
            let message = if self.artists_all.is_empty() {
                "No artists found."
            } else {
                "No artists match your search."
            };
            return v_flex()
                .size_full()
                .gap_3()
                .child(div().px_4().child(message));
        }

        let item_sizes = self.item_sizes.clone();
        v_flex().size_full().child(
            v_virtual_list(
                cx.entity().clone(),
                "artists_list",
                item_sizes,
                |view, visible_range, _window, cx| {
                    visible_range
                        .map(|ix| {
                            if ix == 0 {
                                return div().w_full().h(px(TOP_PADDING)).into_any_element();
                            }
                            let artist = view.artists[ix - 1].clone();
                            let count_label = if artist.track_count == 1 {
                                "1 track".to_string()
                            } else {
                                format!("{} tracks", artist.track_count)
                            };

                            h_flex()
                                .w_full()
                                .h(px(ARTIST_ROW_HEIGHT))
                                .px_4()
                                .items_center()
                                .gap_2()
                                .border_b(px(1.))
                                .border_color(cx.theme().border)
                                .cursor(gpui::CursorStyle::PointingHand)
                                .hover(|style| style.bg(cx.theme().secondary))
                                .child(
                                    div()
                                        .flex_1()
                                        .overflow_hidden()
                                        .truncate()
                                        .child(artist.name.clone()),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(count_label),
                                )
                                .id(ElementId::Integer(artist.id as u64))
                                .on_click(cx.listener(move |_this, _, _, cx| {
                                    cx.emit(ArtistSelectedEvent {
                                        artist: artist.clone(),
                                    });
                                }))
                                .into_any_element()
                        })
                        .collect::<Vec<_>>()
                },
            )
            .track_scroll(&self.scroll_handle)
            .flex_1(),
        )
    }
}

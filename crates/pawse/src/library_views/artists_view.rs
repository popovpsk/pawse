use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    Context, ElementId, EventEmitter, Image, InteractiveElement, IntoElement, ParentElement,
    Pixels, Render, SharedString, Size, StatefulInteractiveElement, Styled, Subscription, Window,
    div, px, size,
};
use gpui_component::{VirtualListScrollHandle, h_flex, v_flex, v_virtual_list};

use crate::theme_colors::Colors;
use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};
use ui_components::artist_avatar::artist_avatar;

use crate::library_service::LibraryEvent;
use crate::localization::tr;
use crate::services::Services;

#[derive(Clone, Debug)]
pub struct ArtistSelectedEvent {
    pub artist: music_library::ArtistSummary,
}

/// A display-ready artist row. All per-row work (the localized track-count
/// label, the name as a `SharedString`, and the resolved cover thumbnails) is
/// done here, off the render hot path, so the `v_virtual_list` closure only
/// clones cheap handles.
struct ArtistRow {
    summary: music_library::ArtistSummary,
    name: SharedString,
    count_label: SharedString,
    covers: Vec<Arc<Image>>,
}

impl ArtistRow {
    fn build(
        summary: music_library::ArtistSummary,
        cover_ids: &HashMap<i64, Vec<i64>>,
        cache: &mut crate::cover_art_cache::CoverArtCache,
        library: &crate::library_service::LibraryService,
    ) -> Self {
        let covers = cover_ids
            .get(&summary.id)
            .into_iter()
            .flat_map(|ids| ids.iter())
            .filter_map(|&id| cache.get_small(Some(id), library))
            .collect();
        let count_label = tr().n_tracks(summary.track_count).into();
        let name = summary.name.clone().into();
        Self {
            summary,
            name,
            count_label,
            covers,
        }
    }
}

const TOP_PADDING: f32 = 12.;
const ARTIST_ROW_HEIGHT: f32 = 56.;
const AVATAR_SIZE: f32 = 40.;
const MIN_FUZZY_SCORE_PER_CHAR: u32 = 14;

pub struct ArtistsView {
    artists_all: Vec<music_library::ArtistSummary>,
    rows: Vec<ArtistRow>,
    cover_ids: HashMap<i64, Vec<i64>>,
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
        let cover_ids = library.artist_album_covers();
        let rows = {
            let mut cache = services.cover_art_cache.borrow_mut();
            Self::build_rows(&artists_all, &cover_ids, &mut cache, &library)
        };
        let item_sizes = Self::make_item_sizes(rows.len());

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
                        {
                            let services = cx.global::<Services>();
                            this.artists_all = services.library.artists();
                            this.cover_ids = services.library.artist_album_covers();
                        }
                        this.recompute_visible(cx);
                        cx.notify();
                    }
                    _ => {}
                },
            );

        Self {
            artists_all,
            rows,
            cover_ids,
            filter: String::new(),
            matcher: Matcher::new(Config::DEFAULT),
            is_scanning: false,
            item_sizes,
            scroll_handle: VirtualListScrollHandle::new(),
            _subscription: subscription,
        }
    }

    fn build_rows(
        artists: &[music_library::ArtistSummary],
        cover_ids: &HashMap<i64, Vec<i64>>,
        cache: &mut crate::cover_art_cache::CoverArtCache,
        library: &crate::library_service::LibraryService,
    ) -> Vec<ArtistRow> {
        artists
            .iter()
            .map(|a| ArtistRow::build(a.clone(), cover_ids, cache, library))
            .collect()
    }

    fn make_item_sizes(row_count: usize) -> Rc<Vec<Size<Pixels>>> {
        let mut sizes = vec![size(px(300.), px(TOP_PADDING))];
        sizes.extend(vec![
            size(px(300.), px(ARTIST_ROW_HEIGHT + 1.));
            row_count
        ]);
        Rc::new(sizes)
    }

    pub fn set_filter(&mut self, query: &str, cx: &mut Context<Self>) {
        let trimmed = query.trim().to_string();
        if trimmed == self.filter {
            return;
        }
        self.filter = trimmed;
        self.recompute_visible(cx);
        cx.notify();
    }

    fn recompute_visible(&mut self, cx: &mut Context<Self>) {
        let filtered: Vec<music_library::ArtistSummary> = if self.filter.is_empty() {
            self.artists_all.clone()
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
            scored.into_iter().map(|(a, _)| a).collect()
        };

        let services = cx.global::<Services>();
        let library = services.library.clone();
        let mut cache = services.cover_art_cache.borrow_mut();
        self.rows = Self::build_rows(&filtered, &self.cover_ids, &mut cache, &library);
        drop(cache);
        self.item_sizes = Self::make_item_sizes(self.rows.len());
    }
}

impl EventEmitter<ArtistSelectedEvent> for ArtistsView {}

impl Render for ArtistsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = Colors::panel_border(cx);
        let secondary = Colors::cover_fallback_bg(cx);
        let list_hover = Colors::list_row_hover_bg(cx);
        let muted_fg = Colors::text_secondary(cx);

        if self.is_scanning && self.artists_all.is_empty() {
            return v_flex()
                .size_full()
                .child(div().px_4().child(tr().scanning.clone()));
        }

        if self.rows.is_empty() {
            let message = if self.artists_all.is_empty() {
                tr().no_artists_found.clone()
            } else {
                tr().no_artists_match.clone()
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
                move |view, visible_range, _window, cx| {
                    visible_range
                        .map(|ix| {
                            if ix == 0 {
                                return div().w_full().h(px(TOP_PADDING)).into_any_element();
                            }
                            let row_ix = ix - 1;
                            let row = &view.rows[row_ix];

                            h_flex()
                                .w_full()
                                .h(px(ARTIST_ROW_HEIGHT))
                                .px_4()
                                .items_center()
                                .gap_3()
                                .border_b(px(1.))
                                .border_color(border)
                                .hover(|style| style.bg(list_hover))
                                .child(artist_avatar(&row.covers, AVATAR_SIZE, secondary, muted_fg))
                                .child(
                                    div()
                                        .flex_1()
                                        .overflow_hidden()
                                        .truncate()
                                        .child(row.name.clone()),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(muted_fg)
                                        .child(row.count_label.clone()),
                                )
                                .id(ElementId::Integer(row.summary.id as u64))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(row) = this.rows.get(row_ix) {
                                        cx.emit(ArtistSelectedEvent {
                                            artist: row.summary.clone(),
                                        });
                                    }
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

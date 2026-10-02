use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    Context, ElementId, EventEmitter, Image, InteractiveElement, IntoElement, ParentElement,
    Pixels, Render, SharedString, Size, StatefulInteractiveElement, Styled, Subscription, Window,
    div, px, size,
};
use gpui_component::{
    VirtualListScrollHandle, h_flex,
    scroll::{ScrollableElement, ScrollbarAxis},
    v_flex, v_virtual_list,
};

use crate::theme_colors::Colors;
use nucleo_matcher::{Config, Matcher};
use ui_components::artist_avatar::artist_avatar;

use crate::library_service::LibraryEvent;
use crate::library_views::albums_view::{OpenLibrarySettings, empty_library, has_music_sources};
use crate::library_views::fuzzy::fuzzy_sorted;
use crate::localization::{LangChanged, tr};
use crate::services::Services;

#[derive(Clone, Debug)]
pub struct GenreSelectedEvent {
    pub genre: music_library::GenreSummary,
}

struct GenreRow {
    summary: music_library::GenreSummary,
    name: SharedString,
    count_label: SharedString,
    covers: Vec<Arc<Image>>,
}

impl GenreRow {
    fn build(
        summary: music_library::GenreSummary,
        cover_ids: &HashMap<String, Vec<i64>>,
        cache: &mut crate::cover_art_cache::CoverArtCache,
        library: &crate::library_service::LibraryService,
    ) -> Self {
        let covers = cover_ids
            .get(&summary.key)
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
const GENRE_ROW_HEIGHT: f32 = 56.;
const AVATAR_SIZE: f32 = 40.;

pub struct GenresView {
    genres_all: Vec<music_library::GenreSummary>,
    rows: Vec<GenreRow>,
    cover_ids: HashMap<String, Vec<i64>>,
    filter: String,
    matcher: Matcher,
    is_scanning: bool,
    item_sizes: Rc<Vec<Size<Pixels>>>,
    scroll_handle: VirtualListScrollHandle,
    _subscription: Subscription,
    _lang_subscription: Subscription,
}

impl GenresView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let services = cx.global::<Services>();
        let library_event_bus = services.library_event_bus.clone();
        let lang_event_bus = services.lang_event_bus.clone();
        let library = services.library.clone();
        let is_scanning = library.is_scanning();

        let genres_all = library.genres();
        let cover_ids = library.genre_album_covers();
        let rows = {
            let mut cache = services.cover_art_cache.borrow_mut();
            Self::build_rows(&genres_all, &cover_ids, &mut cache, &library)
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
                        cx.notify();
                    }
                    LibraryEvent::CatalogChanged => {
                        this.reload_source(cx);
                        cx.notify();
                    }
                    _ => {}
                },
            );

        let lang_subscription = cx.subscribe(&lang_event_bus, |this, _, _: &LangChanged, cx| {
            this.recompute_visible(cx);
            cx.notify();
        });

        Self {
            genres_all,
            rows,
            cover_ids,
            filter: String::new(),
            matcher: Matcher::new(Config::DEFAULT),
            is_scanning,
            item_sizes,
            scroll_handle: VirtualListScrollHandle::new(),
            _subscription: subscription,
            _lang_subscription: lang_subscription,
        }
    }

    fn reload_source(&mut self, cx: &mut Context<Self>) {
        {
            let library = &cx.global::<Services>().library;
            self.genres_all = library.genres();
            self.cover_ids = library.genre_album_covers();
        }
        self.recompute_visible(cx);
    }

    fn build_rows(
        genres: &[music_library::GenreSummary],
        cover_ids: &HashMap<String, Vec<i64>>,
        cache: &mut crate::cover_art_cache::CoverArtCache,
        library: &crate::library_service::LibraryService,
    ) -> Vec<GenreRow> {
        genres
            .iter()
            .map(|g| GenreRow::build(g.clone(), cover_ids, cache, library))
            .collect()
    }

    fn make_item_sizes(row_count: usize) -> Rc<Vec<Size<Pixels>>> {
        let mut sizes = vec![size(px(300.), px(TOP_PADDING))];
        sizes.extend(vec![size(px(300.), px(GENRE_ROW_HEIGHT + 1.)); row_count]);
        Rc::new(sizes)
    }

    pub fn set_filter(&mut self, query: &str, cx: &mut Context<Self>) {
        let trimmed = query.trim().to_string();
        if trimmed == self.filter {
            return;
        }
        self.filter = trimmed;
        self.recompute_visible(cx);
        self.scroll_handle
            .scroll_to_item(0, gpui::ScrollStrategy::Top);
        cx.notify();
    }

    fn recompute_visible(&mut self, cx: &mut Context<Self>) {
        let filtered: Vec<music_library::GenreSummary> = if self.filter.is_empty() {
            self.genres_all.clone()
        } else {
            fuzzy_sorted(
                &mut self.matcher,
                &self.filter,
                self.genres_all
                    .iter()
                    .enumerate()
                    .map(|(ix, g)| (ix, g.name.as_str())),
            )
            .into_iter()
            .map(|ix| self.genres_all[ix].clone())
            .collect()
        };

        let services = cx.global::<Services>();
        let library = services.library.clone();
        let mut cache = services.cover_art_cache.borrow_mut();
        self.rows = Self::build_rows(&filtered, &self.cover_ids, &mut cache, &library);
        drop(cache);
        self.item_sizes = Self::make_item_sizes(self.rows.len());
    }
}

impl EventEmitter<GenreSelectedEvent> for GenresView {}
impl EventEmitter<OpenLibrarySettings> for GenresView {}

impl Render for GenresView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = Colors::border(cx);
        let secondary = Colors::secondary(cx);
        let list_hover = Colors::list_hover(cx);
        let muted_fg = Colors::muted_foreground(cx);

        if self.is_scanning && self.genres_all.is_empty() {
            return v_flex()
                .size_full()
                .child(div().px_4().child(tr().scanning.clone()));
        }

        if self.rows.is_empty() {
            if self.genres_all.is_empty() {
                if !has_music_sources(cx) {
                    return empty_library(tr().no_music_sources.clone(), cx);
                }
                return v_flex().size_full().px_4().child(
                    div()
                        .text_color(muted_fg)
                        .child(tr().no_genres_found.clone()),
                );
            }
            return v_flex()
                .size_full()
                .gap_3()
                .child(div().px_4().child(tr().no_genres_match.clone()));
        }

        let item_sizes = self.item_sizes.clone();
        v_flex()
            .size_full()
            .relative()
            .child(
                v_virtual_list(
                    cx.entity().clone(),
                    "genres_list",
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
                                    .h(px(GENRE_ROW_HEIGHT))
                                    .px_4()
                                    .items_center()
                                    .gap_3()
                                    .border_b(px(1.))
                                    .border_color(border)
                                    .hover(|style| style.bg(list_hover))
                                    .child(artist_avatar(
                                        &row.covers,
                                        AVATAR_SIZE,
                                        secondary,
                                        muted_fg,
                                    ))
                                    .child(
                                        div()
                                            .flex_1()
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .child(row.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(muted_fg)
                                            .child(row.count_label.clone()),
                                    )
                                    .id(ElementId::NamedInteger("genre-row".into(), row_ix as u64))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(row) = this.rows.get(row_ix) {
                                            cx.emit(GenreSelectedEvent {
                                                genre: row.summary.clone(),
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
            .scrollbar(&self.scroll_handle, ScrollbarAxis::Vertical)
    }
}

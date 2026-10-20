use std::collections::HashMap;
use std::sync::Arc;

use gpui::{
    AnyElement, AppContext, Context, ElementId, Entity, EventEmitter, Hsla, Image,
    InteractiveElement, IntoElement, ParentElement, Pixels, Render, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Task, Window, div, prelude::FluentBuilder,
    px,
};
use gpui_component::{
    VirtualListScrollHandle, h_flex,
    scroll::{ScrollableElement, ScrollbarAxis},
    v_flex, v_virtual_list,
};

use crate::theme_colors::Colors;
use nucleo_matcher::{Config, Matcher};
use ui_components::artist_avatar::artist_avatar;
use ui_components::cover_thumb::artist_tile;

use crate::artist_card::ArtistCards;
use crate::library_service::LibraryEvent;
use crate::library_views::albums_view::{OpenLibrarySettings, empty_library, no_music_message};
use crate::library_views::artists_grid::{self, TileParams};
use crate::library_views::cover_grid::{
    self, GridCovers, ItemLayout, LIST_PAD_X, LibraryItem, MeasuredGrid,
};
use crate::library_views::fuzzy::fuzzy_sorted;
use crate::library_views::view_order::{self, ArtistKey, Section};
use crate::localization::{LangChanged, tr};
use crate::services::Services;
use crate::settings_store::{ArtistsSort, LibraryLayout, SettingsStore};
use music_library::ArtistGrouping;

#[derive(Clone, Debug)]
pub struct ArtistSelectedEvent {
    pub artist: music_library::ArtistSummary,
}

/// A display-ready artist row. All per-row work (the localized track-count
/// label, the name as a `SharedString`, and the resolved cover thumbnails) is
/// done here, off the render hot path, so the `v_virtual_list` closure only
/// clones cheap handles.
pub(super) struct ArtistRow {
    pub(super) summary: music_library::ArtistSummary,
    pub(super) name: SharedString,
    pub(super) count_label: SharedString,
    covers: Vec<Arc<Image>>,
    pub(super) tile_covers: Vec<i64>,
}

impl ArtistRow {
    fn build(
        summary: music_library::ArtistSummary,
        cover_ids: &HashMap<i64, Vec<i64>>,
        cache: &mut crate::cover_art_cache::CoverArtCache,
        library: &crate::library_service::LibraryService,
        grid: bool,
        photos: bool,
    ) -> Self {
        let no_metadata = summary.id == music_library::NO_METADATA_ARTIST_ID;
        let ids = cover_ids.get(&summary.id).map_or(&[][..], Vec::as_slice);
        let covers = if grid || photos {
            Vec::new()
        } else {
            ids.iter()
                .take(STACK_COVERS)
                .filter_map(|&id| cache.get_small(Some(id), library))
                .collect()
        };
        let tile_covers = if grid && !photos {
            ids.iter()
                .take(artists_grid::FLOW_COVERS)
                .copied()
                .collect()
        } else {
            Vec::new()
        };
        let count_label = tr().n_tracks(summary.track_count).into();
        let name = if no_metadata {
            tr().no_metadata.clone()
        } else {
            summary.name.clone().into()
        };
        Self {
            summary,
            name,
            count_label,
            covers,
            tile_covers,
        }
    }

    pub(super) fn is_no_metadata(&self) -> bool {
        self.summary.id == music_library::NO_METADATA_ARTIST_ID
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct ArtistsPrefs {
    layout: LibraryLayout,
    sort: ArtistsSort,
    desc: bool,
    grouped: bool,
}

impl ArtistsPrefs {
    fn read(settings: &SettingsStore) -> Self {
        let (sort, desc) = settings.artists_sort();
        Self {
            layout: settings.artists_layout(),
            sort,
            desc,
            grouped: settings.artists_grouped(),
        }
    }

    fn grid(&self) -> bool {
        self.layout == LibraryLayout::Grid
    }
}

const ARTIST_ROW_HEIGHT: f32 = 56.;
const AVATAR_SIZE: f32 = 40.;
const PHOTO_RADIUS: f32 = 4.;
const STACK_COVERS: usize = 3;

pub struct ArtistsView {
    artists_all: Vec<music_library::ArtistSummary>,
    keys: Vec<ArtistKey>,
    pub(super) rows: Vec<ArtistRow>,
    cover_ids: HashMap<i64, Vec<i64>>,
    search_haystacks: HashMap<i64, String>,
    grouping: ArtistGrouping,
    sections: Vec<Section>,
    section_labels: Vec<SharedString>,
    layout_items: ItemLayout,
    filter: String,
    matcher: Matcher,
    is_scanning: bool,
    prefs: ArtistsPrefs,
    pub(super) show_photos: bool,
    pub(super) cards: Entity<ArtistCards>,
    measured_width: Pixels,
    columns: usize,
    tile_width: f32,
    rem_size: f32,
    covers: GridCovers,
    scroll_handle: VirtualListScrollHandle,
    source_task: Option<Task<()>>,
    _subscription: Subscription,
    _lang_subscription: Subscription,
    _settings_observer: Subscription,
    _cards_observer: Subscription,
}

impl ArtistsView {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let services = cx.global::<Services>();
        let library_event_bus = services.library_event_bus.clone();
        let lang_event_bus = services.lang_event_bus.clone();
        let library = services.library.clone();
        let cards = services.artist_cards.clone();
        let (grouping, prefs, show_photos, rem_size) = {
            let settings = cx.global::<SettingsStore>();
            (
                settings.artists_grouping(),
                ArtistsPrefs::read(settings),
                settings.artists_show_photos(),
                f32::from(settings.font_scale().px()),
            )
        };

        let library_access = library.library_access();
        let artists_all = library.artists(grouping);
        let keys: Vec<ArtistKey> = artists_all.iter().map(ArtistKey::of).collect();
        let cover_ids = library.artist_album_covers(grouping);
        let search_haystacks = library.artist_search_haystacks(grouping);

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
                        this.covers.reset();
                        this.reload_source(cx);
                        cx.notify();
                    }
                    _ => {}
                },
            );

        // The track-count labels are precomputed into `rows`, so a language
        // change must rebuild them — `refresh_windows` alone only repaints the
        // stale `SharedString`s.
        let lang_subscription = cx.subscribe(&lang_event_bus, |this, _, _: &LangChanged, cx| {
            this.recompute_visible(cx);
            cx.notify();
        });

        let settings_observer = cx.observe_global::<SettingsStore>(|this, cx| {
            let (grouping, prefs, show_photos, rem_size) = {
                let settings = cx.global::<SettingsStore>();
                (
                    settings.artists_grouping(),
                    ArtistsPrefs::read(settings),
                    settings.artists_show_photos(),
                    f32::from(settings.font_scale().px()),
                )
            };
            let grouping_changed = this.grouping != grouping;
            let order_changed = grouping_changed || this.prefs != prefs;
            let photos_changed = this.show_photos != show_photos;
            let rem_changed = this.rem_size != rem_size;
            if !order_changed && !photos_changed && !rem_changed {
                return;
            }
            this.grouping = grouping;
            this.prefs = prefs;
            this.show_photos = show_photos;
            this.rem_size = rem_size;
            if grouping_changed {
                this.reload_source(cx);
            } else if order_changed || photos_changed {
                this.recompute_visible(cx);
            } else {
                this.rebuild_items();
            }
            if order_changed {
                cover_grid::scroll_to_top(&this.scroll_handle);
            }
            cx.notify();
        });

        let cards_observer = cx.observe(&cards, |this, _, cx| {
            if this.show_photos {
                cx.notify();
            }
        });

        let mut this = Self {
            artists_all,
            keys,
            rows: Vec::new(),
            cover_ids,
            search_haystacks,
            grouping,
            sections: Vec::new(),
            section_labels: Vec::new(),
            layout_items: ItemLayout::empty(),
            filter: String::new(),
            matcher: Matcher::new(Config::DEFAULT),
            is_scanning: false,
            prefs,
            show_photos,
            cards,
            measured_width: px(0.),
            columns: 1,
            tile_width: 0.,
            rem_size,
            covers: GridCovers::new(library_access),
            scroll_handle: VirtualListScrollHandle::new(),
            source_task: None,
            _subscription: subscription,
            _lang_subscription: lang_subscription,
            _settings_observer: settings_observer,
            _cards_observer: cards_observer,
        };
        this.recompute_visible(cx);
        this
    }

    fn reload_source(&mut self, cx: &mut Context<Self>) {
        let library = cx.global::<Services>().library.clone();
        let grouping = self.grouping;
        let load = cx.background_spawn(async move {
            (
                library.artists(grouping),
                library.artist_album_covers(grouping),
                library.artist_search_haystacks(grouping),
            )
        });
        self.source_task = Some(cx.spawn(async move |this, cx| {
            let (artists, cover_ids, search_haystacks) = load.await;
            this.update(cx, |this, cx| {
                this.artists_all = artists;
                this.cover_ids = cover_ids;
                this.search_haystacks = search_haystacks;
                this.keys = this.artists_all.iter().map(ArtistKey::of).collect();
                this.recompute_visible(cx);
                cx.notify();
            })
            .ok();
        }));
    }

    pub fn set_filter(&mut self, query: &str, cx: &mut Context<Self>) {
        let trimmed = query.trim().to_string();
        if trimmed == self.filter {
            return;
        }
        self.filter = trimmed;
        self.recompute_visible(cx);
        cover_grid::scroll_to_top(&self.scroll_handle);
        cx.notify();
    }

    fn recompute_visible(&mut self, cx: &mut Context<Self>) {
        let prefs = self.prefs;
        let order: Vec<usize> = if self.filter.is_empty() {
            view_order::order_artists(&self.keys, prefs.sort, prefs.desc)
        } else {
            fuzzy_sorted(
                &mut self.matcher,
                &self.filter,
                self.artists_all.iter().enumerate().map(|(ix, a)| {
                    let hay = self
                        .search_haystacks
                        .get(&a.id)
                        .map(String::as_str)
                        .unwrap_or(a.name.as_str());
                    (ix, hay)
                }),
            )
        };

        self.sections =
            if self.filter.is_empty() && prefs.grouped && prefs.sort == ArtistsSort::Name {
                view_order::sections(&order, |ix| self.keys[ix].section())
            } else {
                Vec::new()
            };
        self.section_labels = self.sections.iter().map(|s| s.key.label()).collect();

        let services = cx.global::<Services>();
        let library = services.library.clone();
        let grid = prefs.grid();
        let photos = self.show_photos;
        let mut cache = services.cover_art_cache.borrow_mut();
        self.rows = order
            .into_iter()
            .map(|ix| {
                ArtistRow::build(
                    self.artists_all[ix].clone(),
                    &self.cover_ids,
                    &mut cache,
                    &library,
                    grid,
                    photos,
                )
            })
            .collect();
        drop(cache);
        self.rebuild_items();
    }

    fn rebuild_items(&mut self) {
        self.layout_items = if self.prefs.grid() {
            cover_grid::grid_layout(
                &self.sections,
                self.rows.len(),
                self.columns,
                artists_grid::row_height(self.tile_width, self.rem_size, self.show_photos),
            )
        } else {
            cover_grid::list_layout(&self.sections, self.rows.len(), ARTIST_ROW_HEIGHT + 1.)
        };
    }

    fn covers_mut(&mut self) -> &mut GridCovers {
        &mut self.covers
    }
}

impl MeasuredGrid for ArtistsView {
    fn measured_width(&self) -> Pixels {
        self.measured_width
    }

    fn set_grid_width(&mut self, width: Pixels) {
        self.measured_width = width;
        let (columns, tile_width) = cover_grid::grid_metrics(f32::from(width));
        self.columns = columns;
        self.tile_width = tile_width;
        if self.prefs.grid() {
            self.rebuild_items();
        }
    }
}

impl ArtistsView {
    fn ensure_thumbs(
        &mut self,
        visible: std::ops::Range<usize>,
        items: &[LibraryItem],
        cx: &mut Context<Self>,
    ) {
        let ahead = visible.len();
        let end = (visible.end + ahead).min(items.len());
        let shown = visible.start.min(end)..visible.end.min(end);
        let below = shown.end..end;
        let above = visible.start.saturating_sub(ahead).min(shown.start)..shown.start;
        let wanted = end - above.start;
        let rows = &self.rows;
        let names = [shown, below, above]
            .into_iter()
            .flat_map(|range| &items[range])
            .filter_map(|item| match *item {
                LibraryItem::Row(ix) => rows.get(ix),
                _ => None,
            })
            .filter(|row| !row.is_no_metadata())
            .map(|row| row.summary.name.as_str());
        self.cards
            .update(cx, |cards, cx| cards.ensure_thumbs(wanted, names, cx));
    }

    fn ensure_tiles(&mut self, span: std::ops::Range<usize>, cx: &mut Context<Self>) {
        let rows = &self.rows[span];
        if self.show_photos {
            let names = rows
                .iter()
                .filter(|row| !row.is_no_metadata())
                .map(|row| row.summary.name.as_str());
            self.cards
                .update(cx, |cards, cx| cards.ensure_photos(rows.len(), names, cx));
        } else {
            let count = rows.iter().map(|row| row.tile_covers.len()).sum();
            let ids = rows.iter().flat_map(|row| row.tile_covers.iter().copied());
            self.covers.ensure(count, ids, cx, Self::covers_mut);
        }
    }
}

impl EventEmitter<ArtistSelectedEvent> for ArtistsView {}
impl EventEmitter<OpenLibrarySettings> for ArtistsView {}

#[derive(Clone, Copy)]
struct ArtistRowParams {
    border: Hsla,
    secondary: Hsla,
    list_hover: Hsla,
    muted_fg: Hsla,
}

impl Render for ArtistsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = Colors::border(cx);

        if self.is_scanning && self.artists_all.is_empty() {
            return v_flex()
                .size_full()
                .child(div().px_4().child(tr().scanning.clone()));
        }

        if self.rows.is_empty() {
            if self.artists_all.is_empty() {
                return empty_library(no_music_message(&tr().no_artists_found, cx), cx);
            }
            return v_flex()
                .size_full()
                .gap_3()
                .child(div().px_4().child(tr().no_artists_match.clone()));
        }

        let grid = self.prefs.grid();
        let list_area = v_flex().flex_1().min_h(px(0.)).relative().overflow_hidden();
        let list_area = if grid {
            list_area.child(cover_grid::width_probe(cx))
        } else {
            list_area
        };
        if grid && self.measured_width <= px(0.) {
            return v_flex().size_full().child(list_area);
        }

        let (header_height, header_pad) = cover_grid::header_metrics(grid);
        let slot = cover_grid::section_slot(
            &self.layout_items,
            &self.section_labels,
            &self.scroll_handle,
            grid,
            cx,
        );

        let params = ArtistRowParams {
            border,
            secondary: Colors::secondary(cx),
            list_hover: Colors::list_hover(cx),
            muted_fg: Colors::muted_foreground(cx),
        };
        let tiles = TileParams::new(
            self.tile_width,
            self.rem_size,
            params.list_hover,
            params.secondary,
            params.muted_fg,
        );
        let columns = self.columns.max(1);
        let item_sizes = self.layout_items.sizes.clone();
        let items = self.layout_items.items.clone();

        let list_area = list_area
            .child(
                v_virtual_list(
                    cx.entity().clone(),
                    "artists_list",
                    item_sizes,
                    move |view, visible_range, _window, cx| {
                        if grid {
                            if let Some(span) = cover_grid::cover_span(
                                &items,
                                visible_range.clone(),
                                columns,
                                view.rows.len(),
                            ) {
                                view.ensure_tiles(span, cx);
                            }
                        } else if view.show_photos {
                            view.ensure_thumbs(visible_range.clone(), &items, cx);
                        }
                        visible_range
                            .map(|ix| match items[ix] {
                                LibraryItem::TopPadding => div().into_any_element(),
                                LibraryItem::Header(section) => cover_grid::section_header(
                                    view.section_labels[section].clone(),
                                    header_pad,
                                    header_height,
                                    params.border,
                                )
                                .into_any_element(),
                                LibraryItem::Row(row_ix) => {
                                    let ruled = !cover_grid::closes_section(&items, ix);
                                    artist_row(view, row_ix, ruled, &params, cx)
                                }
                                LibraryItem::Strip { start, end } => {
                                    artists_grid::grid_strip(view, start, end, &tiles, cx)
                                }
                            })
                            .collect::<Vec<_>>()
                    },
                )
                .track_scroll(&self.scroll_handle)
                .overflow_x_hidden()
                .flex_1(),
            )
            .scrollbar(&self.scroll_handle, ScrollbarAxis::Vertical);

        v_flex().size_full().children(slot).child(list_area)
    }
}

fn artist_row(
    view: &ArtistsView,
    row_ix: usize,
    ruled: bool,
    p: &ArtistRowParams,
    cx: &mut Context<ArtistsView>,
) -> AnyElement {
    let row = &view.rows[row_ix];
    let picture = if view.show_photos {
        let photo = if row.is_no_metadata() {
            None
        } else {
            view.cards.read(cx).thumb(&row.summary.name)
        };
        artist_tile(
            photo.as_ref(),
            AVATAR_SIZE,
            PHOTO_RADIUS,
            p.secondary,
            p.muted_fg,
        )
    } else {
        artist_avatar(&row.covers, AVATAR_SIZE, p.secondary, p.muted_fg)
    };
    h_flex()
        .w_full()
        .h(px(ARTIST_ROW_HEIGHT))
        .px(px(LIST_PAD_X))
        .items_center()
        .gap_3()
        .when(ruled, |row| row.border_b(px(1.)).border_color(p.border))
        .hover(|style| style.bg(p.list_hover))
        .child(picture)
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
                .text_color(p.muted_fg)
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
}

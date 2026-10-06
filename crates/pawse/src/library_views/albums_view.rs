use std::collections::HashMap;
use std::sync::Arc;

use gpui::{
    App, Context, Div, ElementId, EventEmitter, Hsla, Image, InteractiveElement, IntoElement,
    ParentElement, Pixels, Render, SharedString, StatefulInteractiveElement, Styled, Subscription,
    Window, div, prelude::FluentBuilder, px,
};
use gpui_component::{
    Icon, VirtualListScrollHandle,
    button::{Button, ButtonVariants},
    scroll::{ScrollableElement, ScrollbarAxis},
    tooltip::Tooltip,
    v_flex, v_virtual_list,
};

use crate::theme_colors::Colors;
use nucleo_matcher::{Config, Matcher};
use ui_components::cover_thumb::cover_thumb;

use crate::library_service::{LibraryAccess, LibraryEvent};
use crate::library_views::albums_grid::{self, TileParams, TileSubtitle};
use crate::library_views::cover_grid::{
    self, GridCovers, ItemLayout, LIST_PAD_X, LibraryItem, MeasuredGrid,
};
use crate::library_views::fuzzy::fuzzy_sorted;
use crate::library_views::view_order::{self, AlbumKey, Section};
use crate::localization::{LangChanged, tr};
use crate::services::Services;
use crate::settings_store::{AlbumsArtistDisplay, AlbumsSort, LibraryLayout, SettingsStore};

#[derive(Clone, Debug)]
pub struct AlbumSelectedEvent {
    pub album: music_library::AlbumSummary,
}

#[derive(Clone, Debug)]
pub struct OpenLibrarySettings;

pub(super) struct AlbumRowData {
    pub(super) albums_all_ix: usize,
    pub(super) id: i64,
    pub(super) cover_art_id: Option<i64>,
    pub(super) title: SharedString,
    pub(super) artist: SharedString,
    pub(super) display_inline: SharedString,
    pub(super) subtitle_year: SharedString,
    pub(super) genre_inline: SharedString,
    pub(super) genre_tooltip: Option<SharedString>,
    pub(super) year: SharedString,
    pub(super) cover: Option<Arc<Image>>,
}

impl AlbumRowData {
    fn from_album(
        album: &music_library::AlbumSummary,
        albums_all_ix: usize,
        cover_cache: &mut crate::cover_art_cache::CoverArtCache,
        library: &crate::library_service::LibraryService,
        genres_map: &HashMap<i64, Vec<String>>,
        layout: LibraryLayout,
    ) -> Self {
        let (title, artist, year): (SharedString, SharedString, SharedString) =
            if album.id == music_library::NO_METADATA_ALBUM_ID {
                (
                    tr().no_metadata.clone(),
                    SharedString::default(),
                    SharedString::default(),
                )
            } else {
                (
                    album.title.clone().into(),
                    album.artist_name.clone().into(),
                    album
                        .year
                        .filter(|y| *y > 0)
                        .map(|y| y.to_string())
                        .unwrap_or_default()
                        .into(),
                )
            };
        let display_inline: SharedString = if artist.is_empty() {
            title.clone()
        } else {
            format!("{} - {}", artist, title).into()
        };
        let subtitle_year: SharedString = if year.is_empty() {
            artist.clone()
        } else if artist.is_empty() {
            year.clone()
        } else {
            format!("{} \u{00b7} {}", artist, year).into()
        };
        let (genre_inline, genre_tooltip): (SharedString, Option<SharedString>) =
            match genres_map.get(&album.id) {
                Some(genres) if genres.len() > 1 => (
                    format!("{} …", genres[0]).into(),
                    Some(genres.join(" · ").into()),
                ),
                Some(genres) if !genres.is_empty() => (genres[0].clone().into(), None),
                _ => (SharedString::default(), None),
            };
        Self {
            albums_all_ix,
            id: album.id,
            cover_art_id: album.cover_art_id,
            title,
            artist,
            display_inline,
            subtitle_year,
            genre_inline,
            genre_tooltip,
            year,
            cover: match layout {
                LibraryLayout::Grid => None,
                LibraryLayout::List => cover_cache.get_small(album.cover_art_id, library),
            },
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct AlbumsPrefs {
    layout: LibraryLayout,
    sort: AlbumsSort,
    desc: bool,
    grouped: bool,
    show_artist: bool,
    show_year: bool,
}

impl AlbumsPrefs {
    fn read(settings: &SettingsStore) -> Self {
        let (sort, desc) = settings.albums_sort();
        Self {
            layout: settings.albums_layout(),
            sort,
            desc,
            grouped: settings.albums_grouped(),
            show_artist: settings.albums_show_artist(),
            show_year: settings.albums_show_year(),
        }
    }

    fn order_changed(&self, other: &Self) -> bool {
        self.layout != other.layout
            || self.sort != other.sort
            || self.desc != other.desc
            || self.grouped != other.grouped
    }
}

struct AlbumRowParams {
    border: Hsla,
    list_hover: Hsla,
    muted: Hsla,
    muted_fg: Hsla,
    show_artist: bool,
    show_year: bool,
    show_genre: bool,
    artist_display: AlbumsArtistDisplay,
}

const ALBUM_ROW_HEIGHT: f32 = 48.;
const COVER_SIZE: f32 = 32.;
const COVER_RADIUS: f32 = 4.;
const GENRE_COLUMN_WIDTH: f32 = 120.;
const YEAR_COLUMN_WIDTH: f32 = 40.;
const ARTIST_COLUMN_WIDTH: f32 = 160.;

pub struct AlbumsView {
    pub(super) albums_all: Vec<music_library::AlbumSummary>,
    keys: Vec<AlbumKey>,
    search_entries: Vec<music_library::AlbumSearchEntry>,
    id_to_ix: HashMap<i64, usize>,
    genres_map: HashMap<i64, Vec<String>>,
    pub(super) row_data: Vec<AlbumRowData>,
    sections: Vec<Section>,
    section_labels: Vec<SharedString>,
    layout_items: ItemLayout,
    filter: String,
    matcher: Matcher,
    is_scanning: bool,
    prefs: AlbumsPrefs,
    measured_width: Pixels,
    columns: usize,
    tile_width: f32,
    rem_size: f32,
    covers: GridCovers,
    scroll_handle: VirtualListScrollHandle,
    _subscription: Subscription,
    _settings_observer: Subscription,
    _lang_subscription: Subscription,
}

impl AlbumsView {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (prefs, rem_size) = {
            let settings = cx.global::<SettingsStore>();
            (
                AlbumsPrefs::read(settings),
                f32::from(settings.font_scale().px()),
            )
        };
        let services = cx.global::<Services>();
        let library_event_bus = services.library_event_bus.clone();
        let lang_event_bus = services.lang_event_bus.clone();
        let library = services.library.clone();

        let library_access: LibraryAccess = library.library_access();
        let albums_all = library.albums();
        let keys = Self::album_keys(&albums_all);
        let search_entries = library.album_search_entries();
        let genres_map = library.album_genres_map();
        let id_to_ix = Self::id_index(&albums_all);

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
                        let services = cx.global::<Services>();
                        let cache = services.cover_art_cache.clone();
                        this.albums_all = services.library.albums();
                        this.search_entries = services.library.album_search_entries();
                        this.genres_map = services.library.album_genres_map();
                        cache.borrow_mut().clear(cx);
                        this.keys = Self::album_keys(&this.albums_all);
                        this.id_to_ix = Self::id_index(&this.albums_all);
                        this.covers.reset();
                        this.recompute_visible(cx);
                        cx.notify();
                    }
                    _ => {}
                },
            );

        let settings_observer = cx.observe_global::<SettingsStore>(|this, cx| {
            let (prefs, rem_size) = {
                let settings = cx.global::<SettingsStore>();
                (
                    AlbumsPrefs::read(settings),
                    f32::from(settings.font_scale().px()),
                )
            };
            let old = this.prefs;
            let rem_changed = this.rem_size != rem_size;
            this.prefs = prefs;
            this.rem_size = rem_size;
            if prefs.order_changed(&old) {
                this.recompute_visible(cx);
                cover_grid::scroll_to_top(&this.scroll_handle);
            } else if rem_changed || prefs != old {
                this.rebuild_items();
            }
            cx.notify();
        });

        let lang_subscription = cx.subscribe(&lang_event_bus, |this, _, _: &LangChanged, cx| {
            this.recompute_visible(cx);
            cx.notify();
        });

        let mut this = Self {
            albums_all,
            keys,
            search_entries,
            genres_map,
            id_to_ix,
            row_data: Vec::new(),
            sections: Vec::new(),
            section_labels: Vec::new(),
            layout_items: ItemLayout::empty(),
            filter: String::new(),
            matcher: Matcher::new(Config::DEFAULT),
            is_scanning: false,
            prefs,
            measured_width: px(0.),
            columns: 1,
            tile_width: 0.,
            rem_size,
            covers: GridCovers::new(library_access),
            scroll_handle: VirtualListScrollHandle::new(),
            _subscription: subscription,
            _settings_observer: settings_observer,
            _lang_subscription: lang_subscription,
        };
        this.recompute_visible(cx);
        this
    }

    fn album_keys(albums: &[music_library::AlbumSummary]) -> Vec<AlbumKey> {
        albums
            .iter()
            .map(|album| {
                AlbumKey::new(
                    &album.artist_name,
                    &album.title,
                    album.year,
                    album.id == music_library::NO_METADATA_ALBUM_ID,
                )
            })
            .collect()
    }

    fn id_index(albums: &[music_library::AlbumSummary]) -> HashMap<i64, usize> {
        albums
            .iter()
            .enumerate()
            .map(|(ix, a)| (a.id, ix))
            .collect()
    }

    fn rebuild_items(&mut self) {
        self.layout_items = match self.prefs.layout {
            LibraryLayout::List => {
                cover_grid::list_layout(&self.sections, self.row_data.len(), ALBUM_ROW_HEIGHT + 1.)
            }
            LibraryLayout::Grid => cover_grid::grid_layout(
                &self.sections,
                self.row_data.len(),
                self.columns,
                cover_grid::row_height(
                    self.tile_width,
                    self.rem_size,
                    self.prefs.show_artist || self.prefs.show_year,
                ),
            ),
        };
    }

    fn covers_mut(&mut self) -> &mut GridCovers {
        &mut self.covers
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
            view_order::order_albums(&self.keys, prefs.sort, prefs.desc)
        } else {
            fuzzy_sorted(
                &mut self.matcher,
                &self.filter,
                self.search_entries
                    .iter()
                    .map(|e| (e.album_id, e.haystack.as_str())),
            )
            .into_iter()
            .filter_map(|id| self.id_to_ix.get(&id).copied())
            .collect()
        };

        self.sections = if self.filter.is_empty() && prefs.grouped {
            view_order::sections(&order, |ix| self.keys[ix].section(prefs.sort))
        } else {
            Vec::new()
        };
        self.section_labels = self.sections.iter().map(|s| s.key.label()).collect();

        let services = cx.global::<Services>();
        let mut cover_cache = services.cover_art_cache.borrow_mut();
        let library = &services.library;
        let genres_map = &self.genres_map;
        self.row_data = order
            .into_iter()
            .map(|ix| {
                AlbumRowData::from_album(
                    &self.albums_all[ix],
                    ix,
                    &mut cover_cache,
                    library,
                    genres_map,
                    prefs.layout,
                )
            })
            .collect();
        drop(cover_cache);
        self.rebuild_items();
    }
}

impl MeasuredGrid for AlbumsView {
    fn measured_width(&self) -> Pixels {
        self.measured_width
    }

    fn set_grid_width(&mut self, width: Pixels) {
        self.measured_width = width;
        let (columns, tile_width) = cover_grid::grid_metrics(f32::from(width));
        self.columns = columns;
        self.tile_width = tile_width;
        if self.prefs.layout == LibraryLayout::Grid {
            self.rebuild_items();
        }
    }
}

impl EventEmitter<AlbumSelectedEvent> for AlbumsView {}
impl EventEmitter<OpenLibrarySettings> for AlbumsView {}

pub fn has_music_sources(cx: &App) -> bool {
    let store = cx.global::<SettingsStore>();
    !store.music_folders().is_empty() || crate::remote_settings::has_servers(store)
}

pub fn no_music_message(nothing_found: &SharedString, cx: &App) -> SharedString {
    if has_music_sources(cx) {
        nothing_found.clone()
    } else {
        tr().no_music_sources.clone()
    }
}

pub fn empty_library<V: EventEmitter<OpenLibrarySettings>>(
    message: SharedString,
    cx: &mut Context<V>,
) -> Div {
    v_flex()
        .size_full()
        .gap_3()
        .px_4()
        .items_start()
        .child(
            div()
                .text_color(Colors::muted_foreground(cx))
                .child(message),
        )
        .child(
            Button::new("open-library-settings")
                .primary()
                .icon(Icon::default().path("icons/settings.svg"))
                .label(tr().open_library_settings.clone())
                .on_click(cx.listener(|_, _, _, cx| cx.emit(OpenLibrarySettings))),
        )
}

impl Render for AlbumsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = Colors::border(cx);
        let muted = Colors::secondary(cx);
        let list_hover = Colors::list_hover(cx);
        let muted_fg = Colors::muted_foreground(cx);

        if self.is_scanning && self.albums_all.is_empty() {
            return v_flex()
                .size_full()
                .child(div().px_4().child(tr().scanning.clone()));
        }

        if self.row_data.is_empty() {
            if self.albums_all.is_empty() {
                return empty_library(no_music_message(&tr().no_albums_found, cx), cx);
            }
            return v_flex()
                .size_full()
                .gap_3()
                .child(div().px_4().child(tr().no_albums_match.clone()));
        }

        let grid = self.prefs.layout == LibraryLayout::Grid;
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

        let settings = cx.global::<SettingsStore>();
        let params = AlbumRowParams {
            border,
            list_hover,
            muted,
            muted_fg,
            show_artist: self.prefs.show_artist,
            show_year: self.prefs.show_year,
            show_genre: settings.albums_show_genre(),
            artist_display: settings.albums_artist_display(),
        };
        let tiles = TileParams::new(
            self.tile_width,
            self.rem_size,
            TileSubtitle::new(self.prefs.show_artist, self.prefs.show_year),
            list_hover,
            muted,
            muted_fg,
        );
        let columns = self.columns.max(1);
        let item_sizes = self.layout_items.sizes.clone();
        let items = self.layout_items.items.clone();

        let list_area = list_area
            .child(
                v_virtual_list(
                    cx.entity().clone(),
                    "albums_list",
                    item_sizes,
                    move |view, visible_range, _window, cx| {
                        if grid
                            && let Some(span) = cover_grid::cover_span(
                                &items,
                                visible_range.clone(),
                                columns,
                                view.row_data.len(),
                            )
                        {
                            let ids = view.row_data[span.clone()]
                                .iter()
                                .filter_map(|row| row.cover_art_id);
                            view.covers.ensure(span.len(), ids, cx, Self::covers_mut);
                        }
                        visible_range
                            .map(|ix| match items[ix] {
                                LibraryItem::TopPadding => div().into_any_element(),
                                LibraryItem::Header(section) => cover_grid::section_header(
                                    view.section_labels[section].clone(),
                                    header_pad,
                                    header_height,
                                    border,
                                )
                                .into_any_element(),
                                LibraryItem::Row(row_ix) => {
                                    let ruled = !cover_grid::closes_section(&items, ix);
                                    album_row(view, row_ix, ruled, &params, cx)
                                }
                                LibraryItem::Strip { start, end } => {
                                    albums_grid::grid_strip(view, start, end, &tiles, cx)
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

fn album_row(
    view: &mut AlbumsView,
    row_ix: usize,
    ruled: bool,
    p: &AlbumRowParams,
    cx: &mut Context<AlbumsView>,
) -> gpui::AnyElement {
    let row = &view.row_data[row_ix];
    let albums_all_ix = row.albums_all_ix;

    let cover_el = cover_thumb(
        row.cover.as_ref(),
        COVER_SIZE,
        COVER_RADIUS,
        p.muted,
        p.muted_fg,
    );

    let artist_column = p.show_artist && p.artist_display == AlbumsArtistDisplay::Column;
    let title_line = if p.show_artist && !artist_column {
        row.display_inline.clone()
    } else {
        row.title.clone()
    };

    let mut container = div()
        .w_full()
        .h(px(ALBUM_ROW_HEIGHT))
        .px(px(LIST_PAD_X))
        .flex()
        .items_center()
        .gap_2()
        .when(ruled, |row| row.border_b(px(1.)).border_color(p.border))
        .hover(|style| style.bg(p.list_hover))
        .child(cover_el)
        .child(
            div()
                .flex_1()
                .overflow_hidden()
                .text_ellipsis()
                .text_sm()
                .child(title_line),
        );

    if artist_column {
        container = container.child(
            div()
                .w(px(ARTIST_COLUMN_WIDTH))
                .flex_shrink_0()
                .overflow_hidden()
                .text_ellipsis()
                .text_sm()
                .text_color(p.muted_fg)
                .child(row.artist.clone()),
        );
    }

    if p.show_year {
        container = container.child(
            div()
                .w(px(YEAR_COLUMN_WIDTH))
                .flex_shrink_0()
                .whitespace_nowrap()
                .text_sm()
                .text_color(p.muted_fg)
                .text_right()
                .child(row.year.clone()),
        );
    }

    if p.show_genre {
        let genre_base = div()
            .ml_2()
            .w(px(GENRE_COLUMN_WIDTH))
            .flex_shrink_0()
            .overflow_hidden()
            .text_ellipsis()
            .text_sm()
            .text_color(p.muted_fg)
            .text_right()
            .child(row.genre_inline.clone());
        let genre_el = match row.genre_tooltip.clone() {
            Some(tooltip) => genre_base
                .id(("album_genre", row.id as u64))
                .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                .into_any_element(),
            None => genre_base.into_any_element(),
        };
        container = container.child(genre_el);
    }

    container
        .id(ElementId::Integer(row.id as u64))
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.emit(AlbumSelectedEvent {
                album: this.albums_all[albums_all_ix].clone(),
            });
        }))
        .into_any_element()
}

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    Context, ElementId, EventEmitter, Hsla, Image, InteractiveElement, IntoElement, ParentElement,
    Pixels, Render, SharedString, Size, StatefulInteractiveElement, Styled, Subscription, Window,
    div, px, size,
};
use gpui_component::{VirtualListScrollHandle, button::Button, h_flex, v_flex, v_virtual_list};

use crate::cover_art_cache::CoverArtCache;
use crate::theme_colors::Colors;
use nucleo_matcher::{Config, Matcher};
use ui_components::cover_thumb::cover_thumb;

use crate::library_service::LibraryEvent;
use crate::library_views::fuzzy::fuzzy_sorted;
use crate::localization::tr;
use crate::services::Services;
use crate::settings_store::SettingsStore;

#[derive(Clone, Debug)]
pub struct AlbumSelectedEvent {
    pub album: music_library::AlbumSummary,
}

#[derive(Clone, Debug)]
pub struct AddMusicFolderRequested;

enum AlbumItem {
    TopPadding,
    Album(usize),
}

struct AlbumRowData {
    albums_all_ix: usize,
    id: i64,
    display_text: SharedString,
    cover: Option<Arc<Image>>,
}

impl AlbumRowData {
    fn from_album(
        album: &music_library::AlbumSummary,
        albums_all_ix: usize,
        cover_cache: &mut CoverArtCache,
        library: &crate::library_service::LibraryService,
    ) -> Self {
        let year_str = album.year.map(|y| format!(" ({})", y)).unwrap_or_default();
        let display_text: SharedString =
            format!("{}{} - {}", album.artist_name, year_str, album.title).into();
        Self {
            albums_all_ix,
            id: album.id,
            display_text,
            cover: cover_cache.get_small(album.cover_art_id, library),
        }
    }
}

struct AlbumRowParams {
    border: Hsla,
    list_hover: Hsla,
    muted: Hsla,
    muted_fg: Hsla,
}

const TOP_PADDING: f32 = 12.;
const ALBUM_ROW_HEIGHT: f32 = 48.;
const COVER_SIZE: f32 = 32.;
const COVER_RADIUS: f32 = 4.;

pub struct AlbumsView {
    albums_all: Vec<music_library::AlbumSummary>,
    search_entries: Vec<music_library::AlbumSearchEntry>,
    id_to_ix: HashMap<i64, usize>,
    row_data: Vec<AlbumRowData>,
    items: Vec<AlbumItem>,
    filter: String,
    matcher: Matcher,
    is_scanning: bool,
    item_sizes: Rc<Vec<Size<Pixels>>>,
    scroll_handle: VirtualListScrollHandle,
    _subscription: Subscription,
    _settings_observer: Subscription,
}

impl AlbumsView {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let services = cx.global::<Services>();
        let library_event_bus = services.library_event_bus.clone();
        let library = services.library.clone();

        let albums_all = library.albums();
        let search_entries = library.album_search_entries();
        let id_to_ix = Self::id_index(&albums_all);
        let (items, item_sizes) = Self::build_items(albums_all.len());
        let row_data = {
            let mut cover_cache = services.cover_art_cache.borrow_mut();
            albums_all
                .iter()
                .enumerate()
                .map(|(ix, album)| AlbumRowData::from_album(album, ix, &mut cover_cache, &library))
                .collect()
        };
        let is_scanning = false;

        let subscription =
            cx.subscribe(
                &library_event_bus,
                |this, _, event: &LibraryEvent, cx| match event {
                    LibraryEvent::ScanStarted => {
                        this.is_scanning = true;
                        cx.notify();
                    }
                    LibraryEvent::ScanComplete { changed } => {
                        this.is_scanning = false;
                        if *changed {
                            let services = cx.global::<Services>();
                            services.cover_art_cache.borrow_mut().clear();
                            this.albums_all = services.library.albums();
                            this.search_entries = services.library.album_search_entries();
                            this.id_to_ix = Self::id_index(&this.albums_all);
                            this.recompute_visible(cx);
                        }
                        cx.notify();
                    }
                    _ => {}
                },
            );

        let settings_observer = cx.observe_global::<SettingsStore>(|_, cx| {
            cx.notify();
        });

        Self {
            albums_all,
            search_entries,
            id_to_ix,
            row_data,
            items,
            filter: String::new(),
            matcher: Matcher::new(Config::DEFAULT),
            is_scanning,
            item_sizes: Rc::new(item_sizes),
            scroll_handle: VirtualListScrollHandle::new(),
            _subscription: subscription,
            _settings_observer: settings_observer,
        }
    }

    fn id_index(albums: &[music_library::AlbumSummary]) -> HashMap<i64, usize> {
        albums.iter().enumerate().map(|(ix, a)| (a.id, ix)).collect()
    }

    fn build_items(count: usize) -> (Vec<AlbumItem>, Vec<Size<Pixels>>) {
        let mut items = vec![AlbumItem::TopPadding];
        let mut sizes = vec![size(px(0.), px(TOP_PADDING))];
        for ix in 0..count {
            items.push(AlbumItem::Album(ix));
            sizes.push(size(px(0.), px(ALBUM_ROW_HEIGHT + 1.)));
        }
        (items, sizes)
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
        let services = cx.global::<Services>();
        let mut cover_cache = services.cover_art_cache.borrow_mut();
        let library = &services.library;

        if self.filter.is_empty() {
            self.row_data = self
                .albums_all
                .iter()
                .enumerate()
                .map(|(ix, album)| AlbumRowData::from_album(album, ix, &mut cover_cache, library))
                .collect();
        } else {
            let ids = fuzzy_sorted(
                &mut self.matcher,
                &self.filter,
                self.search_entries
                    .iter()
                    .map(|e| (e.album_id, e.haystack.as_str())),
            );

            self.row_data = ids
                .into_iter()
                .filter_map(|id| {
                    let ix = *self.id_to_ix.get(&id)?;
                    Some(AlbumRowData::from_album(
                        &self.albums_all[ix],
                        ix,
                        &mut cover_cache,
                        library,
                    ))
                })
                .collect();
        }
        let (items, sizes) = Self::build_items(self.row_data.len());
        self.items = items;
        self.item_sizes = Rc::new(sizes);
    }
}

impl EventEmitter<AlbumSelectedEvent> for AlbumsView {}
impl EventEmitter<AddMusicFolderRequested> for AlbumsView {}

impl Render for AlbumsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = Colors::panel_border(cx);
        let muted = Colors::cover_fallback_bg(cx);
        let list_hover = Colors::list_row_hover_bg(cx);
        let muted_fg = Colors::text_secondary(cx);

        if self.is_scanning && self.albums_all.is_empty() {
            return v_flex()
                .size_full()
                .child(div().px_4().child(tr().scanning.clone()));
        }

        if self.row_data.is_empty() {
            let no_folders = cx.global::<SettingsStore>().music_folders().is_empty();
            if self.albums_all.is_empty() && no_folders {
                return v_flex()
                    .size_full()
                    .gap_3()
                    .px_4()
                    .pt_4()
                    .child(
                        div()
                            .text_color(muted_fg)
                            .child(tr().no_music_folders_configured.clone()),
                    )
                    .child(
                        h_flex().child(
                            Button::new("add-music-folder")
                                .label(tr().add_music_folder.clone())
                                .on_click(cx.listener(|_, _, _, cx| {
                                    cx.emit(AddMusicFolderRequested);
                                })),
                        ),
                    );
            }
            let message = if self.albums_all.is_empty() {
                tr().no_albums_found.clone()
            } else {
                tr().no_albums_match.clone()
            };
            return v_flex()
                .size_full()
                .gap_3()
                .child(div().px_4().child(message));
        }

        let params = AlbumRowParams {
            border,
            list_hover,
            muted,
            muted_fg,
        };
        let item_sizes = self.item_sizes.clone();
        v_flex().size_full().child(
            v_virtual_list(
                cx.entity().clone(),
                "albums_list",
                item_sizes,
                move |view, visible_range, _window, cx| {
                    visible_range
                        .map(|ix| match view.items[ix] {
                            AlbumItem::TopPadding => {
                                div().w_full().h(px(TOP_PADDING)).into_any_element()
                            }
                            AlbumItem::Album(row_ix) => album_row(view, row_ix, &params, cx),
                        })
                        .collect::<Vec<_>>()
                },
            )
            .track_scroll(&self.scroll_handle)
            .flex_1(),
        )
    }
}

fn album_row(
    view: &mut AlbumsView,
    row_ix: usize,
    p: &AlbumRowParams,
    cx: &mut Context<AlbumsView>,
) -> gpui::AnyElement {
    let row = &view.row_data[row_ix];
    let albums_all_ix = row.albums_all_ix;

    let cover_el = cover_thumb(row.cover.as_ref(), COVER_SIZE, COVER_RADIUS, p.muted, p.muted_fg);

    div()
        .w_full()
        .h(px(ALBUM_ROW_HEIGHT))
        .px_4()
        .flex()
        .items_center()
        .gap_2()
        .border_b(px(1.))
        .border_color(p.border)
        .hover(|style| style.bg(p.list_hover))
        .child(cover_el)
        .child(
            div()
                .flex_1()
                .overflow_hidden()
                .truncate()
                .text_sm()
                .child(row.display_text.clone()),
        )
        .id(ElementId::Integer(row.id as u64))
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.emit(AlbumSelectedEvent {
                album: this.albums_all[albums_all_ix].clone(),
            });
        }))
        .into_any_element()
}

use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use audio_engine::EngineEvent;
use gpui::prelude::FluentBuilder;
use gpui::{
    App, ClickEvent, Context, Corner, Div, ElementId, EventEmitter, FontWeight, Hsla, Image,
    InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels, Point, Render,
    SharedString, Size, Stateful, StatefulInteractiveElement, Styled, Subscription, Window,
    anchored, deferred, div, point, px, size, svg,
};
use gpui_component::{
    VirtualListScrollHandle, h_flex,
    scroll::{ScrollableElement, ScrollbarAxis},
    tooltip::Tooltip,
    v_flex, v_virtual_list,
};

use crate::cover_art_cache::CoverArtCache;
use crate::theme_colors::Colors;
use crate::track_list::{
    LIKE_ROW_GROUP, RowButtonColors, TrackRowBase, add_to_playlist_button, add_to_queue_button,
    append_album_to_queue, append_tracks_to_queue, fmt_track_num, like_button, track_duration,
};
use nucleo_matcher::{Config, Matcher};
use ui_components::cover_thumb::cover_thumb;

use crate::library_service::LibraryEvent;
use crate::library_views::fuzzy::fuzzy_scored;
use crate::localization::{LangChanged, tr};
use crate::now_playing::NavigateToAlbumRequested;
use crate::services::Services;
use crate::settings_store::SettingsStore;

const TRACK_ROW_HEIGHT: f32 = 36.;
const ALBUM_COVER_SIZE: f32 = 60.;
const QUEUE_BTN_SIZE: f32 = 34.;
const QUEUE_ICON_SIZE: f32 = 20.;
const ALBUM_MENU_WIDTH: f32 = 240.;
const ARTIST_HEADER_HEIGHT: f32 = 48.;
const ALBUM_HEADER_HEIGHT: f32 = 84.;
const DISC_HEADER_HEIGHT: f32 = 32.;
const DISC_HEADER_GAP: f32 = 24.;

#[derive(Clone, Debug)]
struct TrackRow {
    base: TrackRowBase,
    track_num_str: SharedString,
    disc_number: i32,
}

impl TrackRow {
    fn from_track(track: &music_library::Track) -> Self {
        Self {
            base: TrackRowBase::from_track(track),
            track_num_str: fmt_track_num(track.track_number),
            disc_number: track.disc_number,
        }
    }
}

#[derive(Clone, Debug)]
struct AlbumGroup {
    album_id: Option<i64>,
    album_title: SharedString,
    year: Option<i32>,
    cover: Option<Arc<Image>>,
    tracks: Vec<TrackRow>,
    /// Indices of `tracks` in the flat artist-wide list (used as playback queue index).
    global_indices: Vec<usize>,
}

#[derive(Clone)]
struct AlbumMenu {
    album_id: i64,
    anchor: Point<Pixels>,
}

#[derive(Clone)]
enum ItemKind {
    ArtistHeader,
    AlbumHeader(usize),
    DiscHeader(SharedString, bool),
    Track(usize, usize),
}

pub struct ArtistTracksView {
    artist_id: i64,
    artist_name: SharedString,
    tracks_all: Vec<Rc<music_library::Track>>,
    groups: Vec<AlbumGroup>,
    items: Vec<ItemKind>,
    item_sizes: Rc<Vec<Size<Pixels>>>,
    scroll_handle: VirtualListScrollHandle,
    filter: String,
    matcher: Matcher,
    current_track_id: Option<i64>,
    is_playing: bool,
    album_menu: Option<AlbumMenu>,
    partial_albums: HashSet<i64>,
    show_full_albums: bool,
    _engine_subscription: Subscription,
    _library_subscription: Subscription,
    _lang_subscription: Subscription,
}

impl ArtistTracksView {
    pub fn new(artist: &music_library::ArtistSummary, cx: &mut Context<Self>) -> Self {
        let services = cx.global::<Services>();
        let engine_event_bus = services.engine_event_bus.clone();
        let library_event_bus = services.library_event_bus.clone();
        let lang_event_bus = services.lang_event_bus.clone();
        let tracks_all: Vec<Rc<_>> = services
            .library
            .tracks_by_artist(artist.id)
            .into_iter()
            .map(Rc::new)
            .collect();

        let groups = {
            let mut cache = services.cover_art_cache.borrow_mut();
            Self::group_by_album(&tracks_all, &services.library, &mut cache)
        };
        let partial_albums = Self::compute_partial_albums(&tracks_all, &services.library);
        let (items, sizes) = Self::build_items(&groups, tr());

        let current_track_id = services
            .playback_queue
            .borrow()
            .current_track()
            .map(|t| t.id);
        let is_playing = services
            .is_playing
            .load(std::sync::atomic::Ordering::Relaxed);

        let engine_subscription = cx.subscribe(
            &engine_event_bus,
            |this, _, event: &EngineEvent, cx| match event {
                EngineEvent::Loaded { .. } => {
                    let id = cx
                        .global::<Services>()
                        .playback_queue
                        .borrow()
                        .current_track()
                        .map(|t| t.id);
                    if this.current_track_id != id {
                        this.current_track_id = id;
                        cx.notify();
                    }
                }
                EngineEvent::Playing if !this.is_playing => {
                    this.is_playing = true;
                    cx.notify();
                }
                EngineEvent::Paused if this.is_playing => {
                    this.is_playing = false;
                    cx.notify();
                }
                EngineEvent::TrackEnded if this.is_playing => {
                    this.is_playing = false;
                    cx.notify();
                }
                _ => {}
            },
        );

        let library_subscription =
            cx.subscribe(&library_event_bus, |this, _, event: &LibraryEvent, cx| {
                if let LibraryEvent::TrackLikedChanged { track_id, liked } = event {
                    let mut changed = false;
                    for t in this.tracks_all.iter_mut() {
                        if t.id == *track_id && t.liked != *liked {
                            Rc::make_mut(t).liked = *liked;
                            changed = true;
                        }
                    }
                    for g in this.groups.iter_mut() {
                        for t in g.tracks.iter_mut() {
                            if t.base.id == *track_id && t.base.liked != *liked {
                                t.base.liked = *liked;
                                changed = true;
                            }
                        }
                    }
                    if changed {
                        cx.notify();
                    }
                }
            });

        let lang_subscription = cx.subscribe(&lang_event_bus, |this, _, _: &LangChanged, cx| {
            let (items, sizes) = Self::build_items(&this.groups, tr());
            this.items = items;
            this.item_sizes = Rc::new(sizes);
            cx.notify();
        });

        let scroll_handle = VirtualListScrollHandle::new();
        if let Some(track_id) = current_track_id
            && let Some(item_ix) = items.iter().position(|item| {
                matches!(item, ItemKind::Track(g_ix, t_ix)
                    if groups[*g_ix].tracks[*t_ix].base.id == track_id)
            })
        {
            scroll_handle.scroll_to_item(item_ix, gpui::ScrollStrategy::Center);
        }

        Self {
            artist_id: artist.id,
            artist_name: artist.name.clone().into(),
            tracks_all,
            groups,
            items,
            item_sizes: Rc::new(sizes),
            scroll_handle,
            filter: String::new(),
            matcher: Matcher::new(Config::DEFAULT),
            current_track_id,
            is_playing,
            album_menu: None,
            partial_albums,
            show_full_albums: false,
            _engine_subscription: engine_subscription,
            _library_subscription: library_subscription,
            _lang_subscription: lang_subscription,
        }
    }

    fn compute_partial_albums(
        tracks: &[Rc<music_library::Track>],
        library: &crate::library_service::LibraryService,
    ) -> HashSet<i64> {
        let album_totals = library.album_track_counts();
        let mut artist_counts: std::collections::HashMap<i64, i64> =
            std::collections::HashMap::new();
        for track in tracks {
            if let Some(album_id) = track.album_id {
                *artist_counts.entry(album_id).or_default() += 1;
            }
        }
        artist_counts
            .into_iter()
            .filter(|(album_id, mine)| album_totals.get(album_id).copied().unwrap_or(0) > *mine)
            .map(|(album_id, _)| album_id)
            .collect()
    }

    fn group_by_album(
        tracks: &[Rc<music_library::Track>],
        library: &crate::library_service::LibraryService,
        cover_cache: &mut CoverArtCache,
    ) -> Vec<AlbumGroup> {
        let mut groups: Vec<AlbumGroup> = Vec::new();
        for (ix, track) in tracks.iter().enumerate() {
            let album_id = track.album_id;
            if let Some(last) = groups.last_mut()
                && last.album_id == album_id
            {
                last.tracks.push(TrackRow::from_track(track));
                last.global_indices.push(ix);
                continue;
            }
            let album_title = album_id
                .and_then(|id| library.album_title(id))
                .unwrap_or_else(|| "Unknown".to_string());
            let cover = cover_cache.get_small(track.cover_art_id, library);
            groups.push(AlbumGroup {
                album_id,
                album_title: album_title.into(),
                year: track.year,
                cover,
                tracks: vec![TrackRow::from_track(track)],
                global_indices: vec![ix],
            });
        }
        groups
    }

    fn build_items(
        groups: &[AlbumGroup],
        strings: &ui_resources::i18n::Strings,
    ) -> (Vec<ItemKind>, Vec<Size<Pixels>>) {
        let mut items = vec![ItemKind::ArtistHeader];
        let mut sizes = vec![size(px(300.), px(ARTIST_HEADER_HEIGHT))];
        for (g_ix, g) in groups.iter().enumerate() {
            items.push(ItemKind::AlbumHeader(g_ix));
            sizes.push(size(px(300.), px(ALBUM_HEADER_HEIGHT + 1.)));
            let max_disc = g.tracks.iter().map(|t| t.disc_number).max().unwrap_or(1);
            let multi_disc = max_disc > 1;
            let mut current_disc = 0i32;
            for (t_ix, track) in g.tracks.iter().enumerate() {
                if multi_disc && track.disc_number != current_disc {
                    let gap = current_disc != 0;
                    current_disc = track.disc_number;
                    items.push(ItemKind::DiscHeader(
                        strings.disc(current_disc as u32).into(),
                        gap,
                    ));
                    let extra = if gap { DISC_HEADER_GAP } else { 0. };
                    sizes.push(size(px(300.), px(DISC_HEADER_HEIGHT + extra + 1.)));
                }
                items.push(ItemKind::Track(g_ix, t_ix));
                sizes.push(size(px(300.), px(TRACK_ROW_HEIGHT + 1.)));
            }
        }
        (items, sizes)
    }

    pub fn set_filter(&mut self, query: &str, cx: &mut Context<Self>) {
        let trimmed = query.trim().to_string();
        if trimmed == self.filter {
            return;
        }
        self.filter = trimmed;
        self.album_menu = None;
        self.recompute_groups(cx);
        self.scroll_handle
            .scroll_to_item(0, gpui::ScrollStrategy::Top);
        cx.notify();
    }

    fn recompute_groups(&mut self, cx: &mut Context<Self>) {
        let services = cx.global::<Services>();
        let library = services.library.clone();
        let mut cover_cache = services.cover_art_cache.borrow_mut();
        if self.filter.is_empty() {
            self.groups = Self::group_by_album(&self.tracks_all, &library, &mut cover_cache);
        } else {
            let matches = fuzzy_scored(
                &mut self.matcher,
                &self.filter,
                self.tracks_all
                    .iter()
                    .enumerate()
                    .map(|(ix, t)| (ix, t.title.as_str())),
            );

            let mut groups: Vec<AlbumGroup> = Vec::new();
            for (global_ix, _) in matches {
                let track = &self.tracks_all[global_ix];
                let album_id = track.album_id;
                if let Some(last) = groups.last_mut()
                    && last.album_id == album_id
                {
                    last.tracks.push(TrackRow::from_track(track));
                    last.global_indices.push(global_ix);
                    continue;
                }
                let album_title = album_id
                    .and_then(|id| library.album_title(id))
                    .unwrap_or_else(|| "Unknown".to_string());
                let cover = cover_cache.get_small(track.cover_art_id, &library);
                groups.push(AlbumGroup {
                    album_id,
                    album_title: album_title.into(),
                    year: track.year,
                    cover,
                    tracks: vec![TrackRow::from_track(track)],
                    global_indices: vec![global_ix],
                });
            }
            self.groups = groups;
        }
        let strings = tr();
        let (items, sizes) = Self::build_items(&self.groups, strings);
        self.items = items;
        self.item_sizes = Rc::new(sizes);
    }

    fn rebuild_source(&mut self, cx: &mut Context<Self>) {
        let library = cx.global::<Services>().library.clone();
        let artist_tracks: Vec<Rc<music_library::Track>> = library
            .tracks_by_artist(self.artist_id)
            .into_iter()
            .map(Rc::new)
            .collect();
        if self.show_full_albums && !self.partial_albums.is_empty() {
            let mut combined: Vec<Rc<music_library::Track>> = Vec::new();
            let mut i = 0;
            while i < artist_tracks.len() {
                let album_id = artist_tracks[i].album_id;
                let mut j = i;
                while j < artist_tracks.len() && artist_tracks[j].album_id == album_id {
                    j += 1;
                }
                match album_id {
                    Some(aid) if self.partial_albums.contains(&aid) => {
                        combined.extend(library.tracks_for_album(aid).into_iter().map(Rc::new))
                    }
                    _ => combined.extend_from_slice(&artist_tracks[i..j]),
                }
                i = j;
            }
            self.tracks_all = combined;
        } else {
            self.tracks_all = artist_tracks;
        }
        self.album_menu = None;
        self.recompute_groups(cx);
    }

    fn header_name(&self) -> SharedString {
        if self.artist_id == music_library::NO_METADATA_ARTIST_ID {
            tr().no_metadata.clone()
        } else {
            self.artist_name.clone()
        }
    }
}

impl EventEmitter<NavigateToAlbumRequested> for ArtistTracksView {}

impl Render for ArtistTracksView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = Colors::border(cx);
        let list_hover = Colors::list_hover(cx);
        let muted_fg = Colors::muted_foreground(cx);
        let foreground = Colors::foreground(cx);
        let fallback_bg = Colors::secondary(cx);
        let fallback_fg = Colors::muted_foreground(cx);
        let popover_bg = Colors::popover(cx);
        let viewport = window.viewport_size();
        let liked_enabled = cx.global::<SettingsStore>().liked_enabled();
        let playlists_enabled = cx.global::<SettingsStore>().playlists_enabled();

        if self.tracks_all.is_empty() {
            return v_flex()
                .size_full()
                .child(artist_header_static(self.header_name()))
                .child(div().px_4().child(tr().no_tracks_for_artist.clone()));
        }

        if self.groups.is_empty() {
            return v_flex()
                .size_full()
                .child(artist_header_static(self.header_name()))
                .child(div().px_4().child(tr().no_tracks_match.clone()));
        }

        let p = ArtistTrackRowParams {
            border,
            list_hover,
            muted_fg,
            foreground,
            liked_enabled,
            playlists_enabled,
            buttons: RowButtonColors::from_cx(cx),
        };
        let item_sizes = self.item_sizes.clone();
        v_flex()
            .size_full()
            .relative()
            .child(
                v_virtual_list(
                    cx.entity().clone(),
                    "artist_tracks_list",
                    item_sizes,
                    move |view, visible_range, _window, cx| {
                        visible_range
                            .map(|ix| match &view.items[ix] {
                                ItemKind::ArtistHeader => artist_header(view, muted_fg, cx),
                                ItemKind::DiscHeader(disc, gap) => {
                                    artist_disc_header(disc.clone(), *gap, border, muted_fg)
                                }
                                ItemKind::AlbumHeader(g_ix) => artist_album_header(
                                    view,
                                    *g_ix,
                                    border,
                                    fallback_bg,
                                    fallback_fg,
                                    muted_fg,
                                    cx,
                                ),
                                ItemKind::Track(g_ix, t_ix) => {
                                    artist_track_row(view, *g_ix, *t_ix, &p, cx)
                                }
                            })
                            .collect::<Vec<_>>()
                    },
                )
                .track_scroll(&self.scroll_handle)
                .flex_1(),
            )
            .scrollbar(&self.scroll_handle, ScrollbarAxis::Vertical)
            .when_some(self.album_menu.clone(), |el, menu| {
                el.child(album_menu_overlay(
                    &menu,
                    viewport,
                    popover_bg,
                    border,
                    foreground,
                    fallback_bg,
                    cx,
                ))
            })
    }
}

fn artist_disc_header(
    disc: SharedString,
    gap: bool,
    border: gpui::Hsla,
    muted_fg: gpui::Hsla,
) -> gpui::AnyElement {
    let extra = if gap { DISC_HEADER_GAP } else { 0. };
    h_flex()
        .w_full()
        .h(px(DISC_HEADER_HEIGHT + extra))
        .px_4()
        .pb_2()
        .items_end()
        .border_b(px(1.))
        .border_color(border)
        .child(
            div()
                .text_sm()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(muted_fg)
                .child(disc),
        )
        .into_any_element()
}

fn artist_album_header(
    view: &mut ArtistTracksView,
    g_ix: usize,
    border: gpui::Hsla,
    fallback_bg: gpui::Hsla,
    fallback_fg: gpui::Hsla,
    muted_fg: gpui::Hsla,
    cx: &mut Context<ArtistTracksView>,
) -> gpui::AnyElement {
    let group = &view.groups[g_ix];
    let cover_el = cover_thumb(
        group.cover.as_ref(),
        ALBUM_COVER_SIZE,
        4.,
        fallback_bg,
        fallback_fg,
    );
    let album_id = group.album_id;
    let year_str = group.year.map(|y| format!(" · {}", y)).unwrap_or_default();
    let label = match album_id {
        None => format!("{}{}", tr().no_metadata, year_str),
        Some(_) => format!("{}{}", group.album_title, year_str),
    };
    let title_el = match album_id {
        Some(aid) => div()
            .id(("artist_album_link", aid as u64))
            .font_weight(FontWeight::SEMIBOLD)
            .cursor_pointer()
            .border_b(px(1.))
            .hover(|s| s.border_color(muted_fg))
            .on_click(cx.listener(move |_, _, _, cx| {
                cx.emit(NavigateToAlbumRequested { album_id: aid });
            }))
            .child(label)
            .into_any_element(),
        None => div()
            .font_weight(FontWeight::SEMIBOLD)
            .child(label)
            .into_any_element(),
    };
    let trigger_hover = Colors::muted(cx);
    let menu_album = if view.show_full_albums {
        None
    } else {
        album_id.filter(|aid| view.partial_albums.contains(aid))
    };
    h_flex()
        .w_full()
        .h(px(ALBUM_HEADER_HEIGHT))
        .px_4()
        .gap_3()
        .items_center()
        .border_b(px(1.))
        .border_color(border)
        .child(cover_el)
        .child(h_flex().flex_1().overflow_hidden().child(title_el))
        .child(album_queue_trigger(
            g_ix,
            menu_album,
            muted_fg,
            trigger_hover,
            cx,
        ))
        .into_any_element()
}

fn album_queue_trigger(
    g_ix: usize,
    menu_album: Option<i64>,
    icon_color: Hsla,
    hover_bg: Hsla,
    cx: &mut Context<ArtistTracksView>,
) -> Stateful<Div> {
    let base = div()
        .id(("artist-album-queue", g_ix))
        .size(px(QUEUE_BTN_SIZE))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor_pointer()
        .hover(move |s| s.bg(hover_bg))
        .child(
            svg()
                .path("icons/add-queue.svg")
                .size(px(QUEUE_ICON_SIZE))
                .text_color(icon_color),
        )
        .tooltip(|window, cx| Tooltip::new(tr().add_to_queue.clone()).build(window, cx));
    match menu_album {
        Some(aid) => base.on_click(cx.listener(move |this, ev: &ClickEvent, _, cx| {
            this.album_menu = Some(AlbumMenu {
                album_id: aid,
                anchor: ev.position(),
            });
            cx.notify();
        })),
        None => base.on_click(cx.listener(move |this, _, _, cx| {
            let tracks = group_tracks(this, g_ix);
            append_tracks_to_queue(tracks, cx);
        })),
    }
}

fn album_menu_overlay(
    menu: &AlbumMenu,
    viewport: Size<Pixels>,
    popover_bg: Hsla,
    border: Hsla,
    foreground: Hsla,
    hover_bg: Hsla,
    cx: &mut Context<ArtistTracksView>,
) -> gpui::AnyElement {
    let album_id = menu.album_id;
    let backdrop = div()
        .absolute()
        .left(px(0.))
        .top(px(0.))
        .w(viewport.width)
        .h(viewport.height)
        .occlude()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.album_menu = None;
                cx.notify();
            }),
        );
    let content = v_flex()
        .min_w(px(ALBUM_MENU_WIDTH))
        .bg(popover_bg)
        .border_1()
        .border_color(border)
        .rounded(px(8.))
        .shadow_md()
        .p_1()
        .occlude()
        .child(
            queue_menu_row(
                "artist-album-queue-artist".into(),
                "icons/s1-artists.svg",
                tr().add_artist_tracks_to_queue.clone(),
                foreground,
                hover_bg,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.album_menu = None;
                let tracks = artist_tracks_for_album(this, album_id);
                append_tracks_to_queue(tracks, cx);
                cx.notify();
            })),
        )
        .child(
            queue_menu_row(
                "artist-album-queue-album".into(),
                "icons/s1-albums.svg",
                tr().add_album_to_queue.clone(),
                foreground,
                hover_bg,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.album_menu = None;
                append_album_to_queue(album_id, cx);
                cx.notify();
            })),
        );
    let anchor = point(menu.anchor.x, menu.anchor.y + px(8.));
    let menu_layer = deferred(
        anchored()
            .anchor(Corner::TopRight)
            .snap_to_window_with_margin(px(8.))
            .position(anchor)
            .child(div().occlude().child(content)),
    )
    .with_priority(2);
    let backdrop_layer =
        deferred(anchored().position(point(px(0.), px(0.))).child(backdrop)).with_priority(1);
    div()
        .absolute()
        .left(px(0.))
        .top(px(0.))
        .size_full()
        .child(backdrop_layer)
        .child(menu_layer)
        .into_any_element()
}

fn group_tracks(view: &ArtistTracksView, g_ix: usize) -> Vec<Rc<music_library::Track>> {
    let Some(group) = view.groups.get(g_ix) else {
        return Vec::new();
    };
    group
        .global_indices
        .iter()
        .map(|&ix| view.tracks_all[ix].clone())
        .collect()
}

fn artist_tracks_for_album(
    view: &ArtistTracksView,
    album_id: i64,
) -> Vec<Rc<music_library::Track>> {
    view.tracks_all
        .iter()
        .filter(|t| t.album_id == Some(album_id))
        .cloned()
        .collect()
}

fn queue_menu_row(
    id: ElementId,
    icon: &'static str,
    label: SharedString,
    foreground: Hsla,
    hover_bg: Hsla,
) -> Stateful<Div> {
    h_flex()
        .id(id)
        .w_full()
        .gap_2()
        .px_2()
        .py_1p5()
        .rounded(px(4.))
        .cursor_pointer()
        .text_sm()
        .text_color(foreground)
        .hover(move |s| s.bg(hover_bg))
        .child(svg().path(icon).size(px(14.)).flex_shrink_0().text_color(foreground))
        .child(div().whitespace_nowrap().child(label))
}

struct ArtistTrackRowParams {
    border: gpui::Hsla,
    list_hover: gpui::Hsla,
    muted_fg: gpui::Hsla,
    foreground: gpui::Hsla,
    liked_enabled: bool,
    playlists_enabled: bool,
    buttons: RowButtonColors,
}

fn artist_track_row(
    view: &mut ArtistTracksView,
    g_ix: usize,
    t_ix: usize,
    p: &ArtistTrackRowParams,
    cx: &mut Context<ArtistTracksView>,
) -> gpui::AnyElement {
    let group = &view.groups[g_ix];
    let track = &group.tracks[t_ix];
    let global_ix = group.global_indices[t_ix];
    let track_id = track.base.id;
    let is_current = Some(track_id) == view.current_track_id;
    let is_playing = view.is_playing;
    let track_for_queue = view.tracks_all[global_ix].clone();

    let leading = if is_current {
        let icon = if is_playing {
            "icons/play.svg"
        } else {
            "icons/pause.svg"
        };
        div()
            .w_8()
            .flex()
            .items_center()
            .child(svg().path(icon).size(px(12.)).text_color(p.foreground))
    } else {
        div()
            .w_8()
            .text_color(p.muted_fg)
            .child(track.track_num_str.clone())
    };

    h_flex()
        .group(LIKE_ROW_GROUP)
        .w_full()
        .h(px(TRACK_ROW_HEIGHT))
        .pl_4()
        .pr_2()
        .gap_2()
        .items_center()
        .border_b(px(1.))
        .border_color(p.border)
        .when(is_current, |s| crate::track_list::current_row(s, cx))
        .hover(|s| s.bg(p.list_hover))
        .child(leading)
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .overflow_hidden()
                .text_ellipsis()
                .when(is_current, |d| d.font_weight(FontWeight::SEMIBOLD))
                .child(track.base.title.clone()),
        )
        .when(p.playlists_enabled, |el| {
            el.child(add_to_playlist_button(track_id, &p.buttons))
        })
        .when(p.liked_enabled, |el| {
            el.child(like_button(track_id, track.base.liked, &p.buttons))
        })
        .child(track_duration(cx, track.base.duration.clone()))
        .child(add_to_queue_button(track_for_queue, 26., 16., &p.buttons))
        .id(ElementId::Integer(track_id as u64))
        .on_click(cx.listener(move |this, _, window, cx| {
            crate::track_list::play_replacing_queue(
                this.tracks_all.clone(),
                global_ix,
                crate::playback_queue::QueueSource::Unknown,
                window,
                cx,
            );
        }))
        .into_any_element()
}

fn artist_header(
    view: &ArtistTracksView,
    muted_fg: Hsla,
    cx: &mut Context<ArtistTracksView>,
) -> gpui::AnyElement {
    let title = div()
        .flex_1()
        .min_w(px(0.))
        .overflow_hidden()
        .text_ellipsis()
        .text_xl()
        .font_weight(FontWeight::SEMIBOLD)
        .child(view.header_name());
    h_flex()
        .w_full()
        .h(px(ARTIST_HEADER_HEIGHT))
        .pl_4()
        .pr_6()
        .gap_3()
        .items_center()
        .child(title)
        .when(!view.partial_albums.is_empty(), |el| {
            let on = view.show_full_albums;
            let primary = Colors::primary(cx);
            let primary_fg = Colors::primary_foreground(cx);
            let accent = Colors::accent(cx);
            el.child(full_albums_icon(
                on, primary, primary_fg, muted_fg, accent, cx,
            ))
        })
        .into_any_element()
}

fn toggle_full_albums(this: &mut ArtistTracksView, cx: &mut Context<ArtistTracksView>) {
    this.show_full_albums = !this.show_full_albums;
    this.rebuild_source(cx);
    cx.notify();
}

fn full_albums_tooltip(window: &mut Window, cx: &mut App) -> gpui::AnyView {
    Tooltip::element(|_, _| div().w(px(260.)).child(tr().full_albums_tooltip.clone()))
        .build(window, cx)
}

fn full_albums_icon(
    on: bool,
    primary: Hsla,
    primary_fg: Hsla,
    muted_fg: Hsla,
    accent: Hsla,
    cx: &mut Context<ArtistTracksView>,
) -> Stateful<Div> {
    let icon_color = if on { primary_fg } else { muted_fg };
    div()
        .id("artist-full-albums-icon")
        .size(px(QUEUE_BTN_SIZE))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor_pointer()
        .map(|el| {
            if on {
                el.bg(primary)
            } else {
                el.hover(move |s| s.bg(accent))
            }
        })
        .child(
            svg()
                .path("icons/s1-albums.svg")
                .size(px(QUEUE_ICON_SIZE))
                .text_color(icon_color),
        )
        .on_click(cx.listener(|this, _, _, cx| toggle_full_albums(this, cx)))
        .tooltip(full_albums_tooltip)
}

fn artist_header_static(name: SharedString) -> gpui::Div {
    div().px_4().pt_3().pb_2().child(
        div()
            .text_xl()
            .font_weight(FontWeight::SEMIBOLD)
            .child(name),
    )
}

use std::rc::Rc;
use std::sync::Arc;

use audio_engine::EngineEvent;
use gpui::prelude::FluentBuilder;
use gpui::{
    Context, ElementId, FontWeight, Image, InteractiveElement, IntoElement, ParentElement, Pixels,
    Render, SharedString, Size, StatefulInteractiveElement, Styled, Subscription, Window, div, px,
    size, svg,
};
use gpui_component::{VirtualListScrollHandle, h_flex, v_flex, v_virtual_list};

use crate::cover_art_cache::CoverArtCache;
use crate::theme_colors::Colors;
use crate::track_list::{
    LIKE_ROW_GROUP, RowButtonColors, TrackRowBase, add_to_playlist_button, add_to_queue_button,
    fmt_track_num, like_button, track_duration,
};
use nucleo_matcher::{Config, Matcher};
use ui_components::cover_thumb::cover_thumb;

use crate::library_service::LibraryEvent;
use crate::library_views::fuzzy::fuzzy_scored;
use crate::localization::{LangChanged, tr};
use crate::services::Services;
use crate::settings_store::SettingsStore;

const TRACK_ROW_HEIGHT: f32 = 36.;
const ALBUM_COVER_SIZE: f32 = 60.;
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
enum ItemKind {
    ArtistHeader,
    AlbumHeader(usize),
    DiscHeader(SharedString, bool),
    Track(usize, usize),
}

pub struct ArtistTracksView {
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
            _engine_subscription: engine_subscription,
            _library_subscription: library_subscription,
            _lang_subscription: lang_subscription,
        }
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
}

impl Render for ArtistTracksView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = Colors::border(cx);
        let list_hover = Colors::list_hover(cx);
        let muted_fg = Colors::muted_foreground(cx);
        let foreground = Colors::foreground(cx);
        let fallback_bg = Colors::secondary(cx);
        let fallback_fg = Colors::muted_foreground(cx);
        let liked_enabled = cx.global::<SettingsStore>().liked_enabled();
        let playlists_enabled = cx.global::<SettingsStore>().playlists_enabled();

        if self.tracks_all.is_empty() {
            return v_flex()
                .size_full()
                .child(artist_header_static(self.artist_name.clone()))
                .child(div().px_4().child(tr().no_tracks_for_artist.clone()));
        }

        if self.groups.is_empty() {
            return v_flex()
                .size_full()
                .child(artist_header_static(self.artist_name.clone()))
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
        v_flex().size_full().child(
            v_virtual_list(
                cx.entity().clone(),
                "artist_tracks_list",
                item_sizes,
                move |view, visible_range, _window, cx| {
                    visible_range
                        .map(|ix| match &view.items[ix] {
                            ItemKind::ArtistHeader => {
                                artist_header_static(view.artist_name.clone()).into_any_element()
                            }
                            ItemKind::DiscHeader(disc, gap) => {
                                artist_disc_header(disc.clone(), *gap, border, muted_fg)
                            }
                            ItemKind::AlbumHeader(g_ix) => {
                                artist_album_header(view, *g_ix, border, fallback_bg, fallback_fg)
                            }
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
) -> gpui::AnyElement {
    let group = &view.groups[g_ix];
    let cover_el = cover_thumb(
        group.cover.as_ref(),
        ALBUM_COVER_SIZE,
        4.,
        fallback_bg,
        fallback_fg,
    );
    let year_str = group.year.map(|y| format!(" · {}", y)).unwrap_or_default();
    h_flex()
        .w_full()
        .h(px(ALBUM_HEADER_HEIGHT))
        .px_4()
        .gap_3()
        .items_center()
        .border_b(px(1.))
        .border_color(border)
        .child(cover_el)
        .child(
            div().flex_1().overflow_hidden().child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(format!("{}{}", group.album_title, year_str)),
            ),
        )
        .into_any_element()
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
                .truncate()
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
        .on_click(cx.listener(move |this, _, _, cx| {
            let services = cx.global::<Services>();
            let mut queue = services.playback_queue.borrow_mut();
            let played = queue
                .set_tracks_and_play_at(
                    this.tracks_all.clone(),
                    global_ix,
                    crate::playback_queue::QueueSource::Unknown,
                )
                .cloned();
            drop(queue);
            if let Some(track) = played {
                services.play_track(&track);
                crate::services::save_playback(cx);
            }
        }))
        .into_any_element()
}

fn artist_header_static(name: SharedString) -> gpui::Div {
    div().px_4().pt_3().pb_2().child(
        div()
            .text_xl()
            .font_weight(FontWeight::SEMIBOLD)
            .child(name),
    )
}

use std::collections::HashMap;
use std::rc::Rc;

use audio_engine::EngineEvent;
use gpui::prelude::FluentBuilder;
use gpui::{
    Context, ElementId, FontWeight, InteractiveElement, IntoElement, ParentElement, Pixels, Render,
    SharedString, Size, StatefulInteractiveElement, Styled, Subscription, Window, div, px, size,
};
use gpui_component::{VirtualListScrollHandle, h_flex, v_flex, v_virtual_list};

use crate::theme_colors::Colors;
use crate::track_list::{
    LIKE_ROW_GROUP, RowButtonColors, add_to_playlist_button, add_to_queue_button, like_button,
    track_duration,
};
use nucleo_matcher::{Config, Matcher};
use ui_components::cover_thumb::cover_thumb;

use crate::library_service::LibraryEvent;
use crate::library_views::fuzzy::fuzzy_sorted;
use crate::library_views::track_row::{CoverTrackRow, build_artist_map, build_haystacks};
use crate::localization::tr;
use crate::services::Services;
use crate::settings_store::SettingsStore;

const TOP_PADDING: f32 = 12.;
const TRACK_ROW_HEIGHT: f32 = 44.;
const COVER_SIZE: f32 = 32.;

#[derive(Clone, Copy)]
enum LikedItem {
    TopPadding,
    Track(usize),
}

pub struct LikedView {
    tracks_all: Vec<Rc<music_library::Track>>,
    row_data: Vec<CoverTrackRow>,
    artist_by_track: HashMap<i64, SharedString>,
    haystacks: Vec<String>,
    items: Vec<LikedItem>,
    item_sizes: Rc<Vec<Size<Pixels>>>,
    filter: String,
    matcher: Matcher,
    current_track_id: Option<i64>,
    is_playing: bool,
    scroll_handle: VirtualListScrollHandle,
    _library_subscription: Subscription,
    _engine_subscription: Subscription,
}

impl LikedView {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let services = cx.global::<Services>();
        let library = services.library.clone();
        let library_event_bus = services.library_event_bus.clone();
        let engine_event_bus = services.engine_event_bus.clone();

        let tracks_all: Vec<Rc<_>> = library.liked_tracks().into_iter().map(Rc::new).collect();
        let artist_by_track = build_artist_map(&library, &tracks_all);
        let haystacks = build_haystacks(&tracks_all, &artist_by_track);
        let (items, item_sizes) = Self::build_items(tracks_all.len());
        let row_data = {
            let mut cover_cache = services.cover_art_cache.borrow_mut();
            tracks_all
                .iter()
                .enumerate()
                .map(|(ix, t)| {
                    CoverTrackRow::from_track(t, ix, &artist_by_track, &mut cover_cache, &library)
                })
                .collect()
        };

        let current_track_id = services
            .playback_queue
            .borrow()
            .current_track()
            .map(|t| t.id);
        let is_playing = services
            .is_playing
            .load(std::sync::atomic::Ordering::Relaxed);

        let library_subscription = cx.subscribe(
            &library_event_bus,
            |this, _, event: &LibraryEvent, cx| match event {
                LibraryEvent::ScanComplete { changed } if *changed => {
                    let services = cx.global::<Services>();
                    this.tracks_all = services
                        .library
                        .liked_tracks()
                        .into_iter()
                        .map(Rc::new)
                        .collect();
                    this.artist_by_track = build_artist_map(&services.library, &this.tracks_all);
                    this.haystacks = build_haystacks(&this.tracks_all, &this.artist_by_track);
                    this.recompute_visible(cx);
                    cx.notify();
                }
                LibraryEvent::TrackLikedChanged { track_id, liked } => {
                    if *liked {
                        if !this.tracks_all.iter().any(|t| t.id == *track_id) {
                            let services = cx.global::<Services>();
                            this.tracks_all = services
                                .library
                                .liked_tracks()
                                .into_iter()
                                .map(Rc::new)
                                .collect();
                            this.artist_by_track =
                                build_artist_map(&services.library, &this.tracks_all);
                            this.haystacks =
                                build_haystacks(&this.tracks_all, &this.artist_by_track);
                            this.recompute_visible(cx);
                            cx.notify();
                        }
                    } else {
                        let before = this.tracks_all.len();
                        this.tracks_all.retain(|t| t.id != *track_id);
                        if this.tracks_all.len() != before {
                            this.haystacks =
                                build_haystacks(&this.tracks_all, &this.artist_by_track);
                            this.recompute_visible(cx);
                            cx.notify();
                        }
                    }
                }
                _ => {}
            },
        );

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

        Self {
            tracks_all,
            row_data,
            artist_by_track,
            haystacks,
            items,
            item_sizes: Rc::new(item_sizes),
            filter: String::new(),
            matcher: Matcher::new(Config::DEFAULT),
            current_track_id,
            is_playing,
            scroll_handle: VirtualListScrollHandle::new(),
            _library_subscription: library_subscription,
            _engine_subscription: engine_subscription,
        }
    }

    fn build_items(count: usize) -> (Vec<LikedItem>, Vec<Size<Pixels>>) {
        let mut items = vec![LikedItem::TopPadding];
        let mut sizes = vec![size(px(300.), px(TOP_PADDING))];
        for ix in 0..count {
            items.push(LikedItem::Track(ix));
            sizes.push(size(px(300.), px(TRACK_ROW_HEIGHT + 1.)));
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
                .tracks_all
                .iter()
                .enumerate()
                .map(|(ix, t)| {
                    CoverTrackRow::from_track(
                        t,
                        ix,
                        &self.artist_by_track,
                        &mut cover_cache,
                        library,
                    )
                })
                .collect();
        } else {
            let indices = fuzzy_sorted(
                &mut self.matcher,
                &self.filter,
                self.haystacks
                    .iter()
                    .enumerate()
                    .map(|(ix, h)| (ix, h.as_str())),
            );
            self.row_data = indices
                .into_iter()
                .map(|ix| {
                    CoverTrackRow::from_track(
                        &self.tracks_all[ix],
                        ix,
                        &self.artist_by_track,
                        &mut cover_cache,
                        library,
                    )
                })
                .collect();
        }
        let (items, sizes) = Self::build_items(self.row_data.len());
        self.items = items;
        self.item_sizes = Rc::new(sizes);
    }
}

impl Render for LikedView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = Colors::border(cx);
        let list_hover = Colors::list_hover(cx);
        let muted = Colors::muted(cx);
        let muted_fg = Colors::muted_foreground(cx);
        let liked_enabled = cx.global::<SettingsStore>().liked_enabled();
        let playlists_enabled = cx.global::<SettingsStore>().playlists_enabled();

        if self.row_data.is_empty() {
            let message = if self.tracks_all.is_empty() {
                tr().no_liked_tracks.clone()
            } else {
                tr().no_liked_match.clone()
            };
            return v_flex()
                .size_full()
                .gap_3()
                .pt_2()
                .child(div().px_4().text_color(muted_fg).child(message));
        }

        let p = LikedTrackRowParams {
            border,
            list_hover,
            muted,
            muted_fg,
            liked_enabled,
            playlists_enabled,
            buttons: RowButtonColors::from_cx(cx),
        };
        let item_sizes = self.item_sizes.clone();
        v_flex().size_full().child(
            v_virtual_list(
                cx.entity().clone(),
                "liked_list",
                item_sizes,
                move |view, visible_range, _window, cx| {
                    visible_range
                        .map(|ix| match view.items[ix] {
                            LikedItem::TopPadding => {
                                div().w_full().h(px(TOP_PADDING)).into_any_element()
                            }
                            LikedItem::Track(track_ix) => liked_track_row(view, track_ix, &p, cx),
                        })
                        .collect::<Vec<_>>()
                },
            )
            .track_scroll(&self.scroll_handle)
            .flex_1(),
        )
    }
}

struct LikedTrackRowParams {
    border: gpui::Hsla,
    list_hover: gpui::Hsla,
    muted: gpui::Hsla,
    muted_fg: gpui::Hsla,
    liked_enabled: bool,
    playlists_enabled: bool,
    buttons: RowButtonColors,
}

fn liked_track_row(
    view: &mut LikedView,
    track_ix: usize,
    p: &LikedTrackRowParams,
    cx: &mut Context<LikedView>,
) -> gpui::AnyElement {
    let row = &view.row_data[track_ix];
    let track_id = row.base.id;
    let track_all_ix = row.track_all_ix;
    let is_current = Some(track_id) == view.current_track_id;
    let track_for_queue = view.tracks_all[row.track_all_ix].clone();

    let cover_el = cover_thumb(row.cover.as_ref(), COVER_SIZE, 3., p.muted, p.muted_fg);

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
        .child(cover_el)
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .overflow_hidden()
                .truncate()
                .text_sm()
                .when(is_current, |d| d.font_weight(FontWeight::SEMIBOLD))
                .child(row.base.title.clone()),
        )
        .child(
            div()
                .w(px(140.))
                .min_w(px(0.))
                .overflow_hidden()
                .truncate()
                .text_sm()
                .text_color(p.muted_fg)
                .child(row.artist.clone()),
        )
        .when(p.playlists_enabled, |el| {
            el.child(add_to_playlist_button(track_id, &p.buttons))
        })
        .when(p.liked_enabled, |el| {
            el.child(like_button(track_id, row.base.liked, &p.buttons))
        })
        .child(track_duration(cx, row.base.duration.clone()))
        .child(add_to_queue_button(track_for_queue, 26., 16., &p.buttons))
        .id(ElementId::Integer(track_id as u64))
        .on_click(cx.listener(move |this, _, _, cx| {
            let services = cx.global::<Services>();
            let mut queue = services.playback_queue.borrow_mut();
            let track = queue
                .set_tracks_and_play_at(
                    this.tracks_all.clone(),
                    track_all_ix,
                    crate::playback_queue::QueueSource::Unknown,
                )
                .cloned();
            drop(queue);
            if let Some(track) = track {
                services.play_track(&track);
                crate::services::save_playback(cx);
            }
        }))
        .into_any_element()
}

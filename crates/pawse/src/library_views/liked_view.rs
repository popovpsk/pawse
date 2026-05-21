use std::collections::HashMap;
use std::rc::Rc;

use audio_engine::EngineEvent;
use gpui::prelude::FluentBuilder;
use gpui::{
    Context, ElementId, FontWeight, InteractiveElement, IntoElement, ObjectFit, ParentElement,
    Pixels, Render, Size, StatefulInteractiveElement, Styled, StyledImage, Subscription, Window,
    div, img, px, size,
};
use gpui_component::{ActiveTheme, VirtualListScrollHandle, h_flex, v_flex, v_virtual_list};
use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};
use ui_components::cover_placeholder::cover_placeholder;

use crate::library_service::LibraryEvent;
use crate::like_button::{LIKE_ROW_GROUP, like_button};
use crate::services::Services;

const TOP_PADDING: f32 = 12.;
const TRACK_ROW_HEIGHT: f32 = 44.;
const COVER_SIZE: f32 = 32.;
const MIN_FUZZY_SCORE_PER_CHAR: u32 = 14;

#[derive(Clone, Copy)]
enum LikedItem {
    TopPadding,
    Track(usize),
}

pub struct LikedView {
    tracks_all: Vec<music_library::Track>,
    tracks: Vec<music_library::Track>,
    artist_by_track: HashMap<i64, String>,
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

        let tracks_all = library.liked_tracks();
        let artist_by_track = build_artist_map(&library, &tracks_all);
        Self::prewarm_covers(services, &tracks_all);
        let tracks = tracks_all.clone();
        let (items, item_sizes) = Self::build_items(&tracks);

        let current_track_id = services
            .playback_queue
            .borrow()
            .current_track()
            .map(|t| t.id);

        let library_subscription = cx.subscribe(
            &library_event_bus,
            |this, _, event: &LibraryEvent, cx| match event {
                LibraryEvent::ScanComplete => {
                    let services = cx.global::<Services>();
                    this.tracks_all = services.library.liked_tracks();
                    this.artist_by_track = build_artist_map(&services.library, &this.tracks_all);
                    Self::prewarm_covers(services, &this.tracks_all);
                    this.recompute_visible();
                    cx.notify();
                }
                LibraryEvent::TrackLikedChanged { track_id, liked } => {
                    if *liked {
                        if !this.tracks_all.iter().any(|t| t.id == *track_id) {
                            let services = cx.global::<Services>();
                            this.tracks_all = services.library.liked_tracks();
                            this.artist_by_track =
                                build_artist_map(&services.library, &this.tracks_all);
                            Self::prewarm_covers(services, &this.tracks_all);
                            this.recompute_visible();
                            cx.notify();
                        }
                    } else {
                        let before = this.tracks_all.len();
                        this.tracks_all.retain(|t| t.id != *track_id);
                        if this.tracks_all.len() != before {
                            this.recompute_visible();
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
                    if this.current_track_id != id || !this.is_playing {
                        this.current_track_id = id;
                        this.is_playing = true;
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
            tracks,
            artist_by_track,
            items,
            item_sizes: Rc::new(item_sizes),
            filter: String::new(),
            matcher: Matcher::new(Config::DEFAULT),
            current_track_id,
            is_playing: current_track_id.is_some(),
            scroll_handle: VirtualListScrollHandle::new(),
            _library_subscription: library_subscription,
            _engine_subscription: engine_subscription,
        }
    }

    fn build_items(tracks: &[music_library::Track]) -> (Vec<LikedItem>, Vec<Size<Pixels>>) {
        let mut items = vec![LikedItem::TopPadding];
        let mut sizes = vec![size(px(300.), px(TOP_PADDING))];
        for ix in 0..tracks.len() {
            items.push(LikedItem::Track(ix));
            sizes.push(size(px(300.), px(TRACK_ROW_HEIGHT + 1.)));
        }
        (items, sizes)
    }

    fn prewarm_covers(services: &Services, tracks: &[music_library::Track]) {
        let mut cache = services.cover_art_cache.borrow_mut();
        for track in tracks {
            cache.get_small(track.cover_art_id, &services.library);
        }
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
            self.tracks = self.tracks_all.clone();
        } else {
            let pattern = Pattern::parse(&self.filter, CaseMatching::Ignore, Normalization::Smart);
            let threshold = self.filter.chars().count() as u32 * MIN_FUZZY_SCORE_PER_CHAR;
            let mut buf: Vec<char> = Vec::new();
            let mut scored: Vec<(music_library::Track, u32)> = self
                .tracks_all
                .iter()
                .filter_map(|track| {
                    let artist = self
                        .artist_by_track
                        .get(&track.id)
                        .map(String::as_str)
                        .unwrap_or("");
                    let haystack_str = format!("{} {}", track.title, artist);
                    let haystack = Utf32Str::new(&haystack_str, &mut buf);
                    pattern
                        .score(haystack, &mut self.matcher)
                        .filter(|s| *s >= threshold)
                        .map(|s| (track.clone(), s))
                })
                .collect();
            scored.sort_by_key(|(_, score)| std::cmp::Reverse(*score));
            self.tracks = scored.into_iter().map(|(t, _)| t).collect();
        }
        let (items, sizes) = Self::build_items(&self.tracks);
        self.items = items;
        self.item_sizes = Rc::new(sizes);
    }
}

fn build_artist_map(
    library: &crate::library_service::LibraryService,
    tracks: &[music_library::Track],
) -> HashMap<i64, String> {
    let ids: Vec<i64> = tracks.iter().map(|t| t.id).collect();
    library
        .track_artists_map(&ids)
        .into_iter()
        .map(|(id, names)| (id, names.join(", ")))
        .collect()
}

impl Render for LikedView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let border = theme.border;
        let secondary = theme.secondary;
        let muted = theme.muted;
        let muted_fg = theme.muted_foreground;

        if self.tracks.is_empty() {
            let message = if self.tracks_all.is_empty() {
                "No liked tracks yet. Hover a track and tap the heart to like it."
            } else {
                "No liked tracks match your search."
            };
            return v_flex()
                .size_full()
                .gap_3()
                .pt_2()
                .child(div().px_4().text_color(muted_fg).child(message));
        }

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
                            LikedItem::Track(track_ix) => {
                                let track = &view.tracks[track_ix];
                                let track_id = track.id;
                                let cover_art_id = track.cover_art_id;
                                let title = track.title.clone();
                                let artist = view
                                    .artist_by_track
                                    .get(&track_id)
                                    .cloned()
                                    .unwrap_or_default();
                                let duration_str = track
                                    .duration_ms
                                    .map(|ms| {
                                        let secs = (ms / 1000) as u32;
                                        format!("{:02}:{:02}", secs / 60, secs % 60)
                                    })
                                    .unwrap_or_default();
                                let liked = track.liked;
                                let is_current = Some(track_id) == view.current_track_id;

                                let services = cx.global::<Services>();
                                let cover_img = services
                                    .cover_art_cache
                                    .borrow_mut()
                                    .get_small(cover_art_id, &services.library);
                                let fallback_bg = muted;
                                let fallback_fg = muted_fg;
                                let cover_el: gpui::AnyElement = if let Some(cover_img) = cover_img
                                {
                                    img(cover_img)
                                        .w(px(COVER_SIZE))
                                        .h(px(COVER_SIZE))
                                        .rounded(px(3.))
                                        .object_fit(ObjectFit::Cover)
                                        .with_fallback({
                                            let bg = fallback_bg;
                                            let fg = fallback_fg;
                                            move || {
                                                cover_placeholder(COVER_SIZE, 3., bg, fg)
                                                    .into_any_element()
                                            }
                                        })
                                        .into_any_element()
                                } else {
                                    cover_placeholder(COVER_SIZE, 3., fallback_bg, fallback_fg)
                                        .into_any_element()
                                };

                                h_flex()
                                    .group(LIKE_ROW_GROUP)
                                    .w_full()
                                    .h(px(TRACK_ROW_HEIGHT))
                                    .px_4()
                                    .gap_2()
                                    .items_center()
                                    .cursor(gpui::CursorStyle::PointingHand)
                                    .border_b(px(1.))
                                    .border_color(border)
                                    .when(is_current, |s| s.bg(secondary))
                                    .hover(|s| s.bg(secondary))
                                    .child(cover_el)
                                    .child(
                                        div()
                                            .flex_1()
                                            .overflow_hidden()
                                            .truncate()
                                            .text_sm()
                                            .when(is_current, |d| {
                                                d.font_weight(FontWeight::SEMIBOLD)
                                            })
                                            .child(title),
                                    )
                                    .child(
                                        div()
                                            .w(px(140.))
                                            .overflow_hidden()
                                            .truncate()
                                            .text_sm()
                                            .text_color(muted_fg)
                                            .child(artist),
                                    )
                                    .child(like_button(track_id, liked, cx))
                                    .child(
                                        div()
                                            .w_16()
                                            .text_sm()
                                            .text_color(muted_fg)
                                            .child(duration_str),
                                    )
                                    .id(ElementId::Integer(track_id as u64))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        let services = cx.global::<Services>();
                                        let mut queue = services.playback_queue.borrow_mut();
                                        queue.set_tracks(this.tracks.clone());
                                        let track = queue.play_track_at(track_ix).cloned();
                                        drop(queue);
                                        if let Some(track) = track {
                                            services.play_track(&track);
                                            crate::services::save_playback(cx);
                                        }
                                    }))
                                    .into_any_element()
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

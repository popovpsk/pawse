use std::rc::Rc;

use audio_engine::EngineEvent;
use gpui::prelude::FluentBuilder;
use gpui::{
    Context, ElementId, FontWeight, InteractiveElement, IntoElement, ObjectFit, ParentElement,
    Pixels, Render, Size, StatefulInteractiveElement, Styled, StyledImage, Subscription, Window,
    div, img, px, size, svg,
};
use gpui_component::{ActiveTheme, VirtualListScrollHandle, h_flex, v_flex, v_virtual_list};

use crate::services::Services;

const TOP_PADDING: f32 = 12.;
const TRACK_ROW_HEIGHT: f32 = 36.;
const HEADER_HEIGHT: f32 = 40.;
const COVER_SIZE: f32 = 28.;

#[derive(Clone, Copy)]
enum QueueItem {
    TopPadding,
    Track(usize),
}

pub struct QueueView {
    tracks: Vec<music_library::Track>,
    items: Vec<QueueItem>,
    current_index: Option<usize>,
    is_playing: bool,
    item_sizes: Rc<Vec<Size<Pixels>>>,
    scroll_handle: VirtualListScrollHandle,
    _subscription: Subscription,
}

impl QueueView {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let services = cx.global::<Services>();
        let engine_event_bus = services.engine_event_bus.clone();

        let queue = services.playback_queue.borrow();
        let tracks = queue.tracks_vec();
        let current_index = queue.current_index();
        let is_playing = current_index.is_some();
        drop(queue);

        // Pre-warm cover art cache so render never hits the DB.
        {
            let mut cache = services.cover_art_cache.borrow_mut();
            for track in &tracks {
                cache.get_small(track.cover_art_id, &services.library);
            }
        }

        let (items, item_sizes) = Self::build_items(&tracks);

        let subscription =
            cx.subscribe(
                &engine_event_bus,
                |this, _, event: &EngineEvent, cx| match event {
                    EngineEvent::Loaded { .. } => {
                        let services = cx.global::<Services>();
                        let queue = services.playback_queue.borrow();
                        let new_tracks = queue.tracks_vec();
                        let new_index = queue.current_index();
                        drop(queue);
                        // Pre-warm cover art for the new queue.
                        {
                            let mut cache = services.cover_art_cache.borrow_mut();
                            for track in &new_tracks {
                                cache.get_small(track.cover_art_id, &services.library);
                            }
                        }
                        let (new_items, new_sizes) = Self::build_items(&new_tracks);
                        this.items = new_items;
                        this.item_sizes = Rc::new(new_sizes);
                        this.tracks = new_tracks;
                        this.current_index = new_index;
                        this.is_playing = true;
                        cx.notify();
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
            tracks,
            items,
            current_index,
            is_playing,
            item_sizes: Rc::new(item_sizes),
            scroll_handle: VirtualListScrollHandle::new(),
            _subscription: subscription,
        }
    }

    pub fn refresh_tracks(&mut self, cx: &mut Context<Self>) {
        let services = cx.global::<Services>();
        let queue = services.playback_queue.borrow();
        let new_tracks = queue.tracks_vec();
        let new_index = queue.current_index();
        drop(queue);
        {
            let mut cache = services.cover_art_cache.borrow_mut();
            for track in &new_tracks {
                cache.get_small(track.cover_art_id, &services.library);
            }
        }
        let (new_items, new_sizes) = Self::build_items(&new_tracks);
        self.items = new_items;
        self.item_sizes = Rc::new(new_sizes);
        self.tracks = new_tracks;
        self.current_index = new_index;
        cx.notify();
    }

    fn build_items(tracks: &[music_library::Track]) -> (Vec<QueueItem>, Vec<Size<Pixels>>) {
        let mut items = vec![QueueItem::TopPadding];
        let mut sizes = vec![size(px(300.), px(TOP_PADDING))];
        for ix in 0..tracks.len() {
            items.push(QueueItem::Track(ix));
            sizes.push(size(px(300.), px(TRACK_ROW_HEIGHT + 1.)));
        }
        (items, sizes)
    }
}

impl Render for QueueView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = h_flex()
            .w_full()
            .h(px(HEADER_HEIGHT))
            .flex_shrink_0()
            .px_4()
            .items_center()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(cx.theme().foreground)
                    .child("Queue"),
            );

        if self.tracks.is_empty() {
            return v_flex().size_full().child(header).child(
                div()
                    .px_4()
                    .pt_2()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("Queue is empty."),
            );
        }

        let item_sizes = self.item_sizes.clone();
        v_flex().size_full().child(header).child(
            v_virtual_list(
                cx.entity().clone(),
                "queue_list",
                item_sizes,
                |view, visible_range, _window, cx| {
                    visible_range
                        .map(|ix| match view.items[ix] {
                            QueueItem::TopPadding => {
                                div().w_full().h(px(TOP_PADDING)).into_any_element()
                            }
                            QueueItem::Track(track_ix) => {
                                let track = &view.tracks[track_ix];
                                let track_id = track.id;
                                let cover_art_id = track.cover_art_id;
                                let duration_str = track
                                    .duration_ms
                                    .map(|ms| {
                                        let secs = (ms / 1000) as u32;
                                        format!("{:02}:{:02}", secs / 60, secs % 60)
                                    })
                                    .unwrap_or_default();
                                let is_current = Some(track_ix) == view.current_index;

                                let left_cell: gpui::AnyElement = if is_current {
                                    let icon = if view.is_playing {
                                        "icons/s1-play.svg"
                                    } else {
                                        "icons/s1-pause.svg"
                                    };
                                    div()
                                        .w(px(COVER_SIZE))
                                        .h(px(COVER_SIZE))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            svg()
                                                .path(icon)
                                                .size(px(12.))
                                                .text_color(cx.theme().foreground),
                                        )
                                        .into_any_element()
                                } else {
                                    // Arc::clone from cache — O(1), no DB access.
                                    let services = cx.global::<Services>();
                                    let cover_img = services
                                        .cover_art_cache
                                        .borrow_mut()
                                        .get_small(cover_art_id, &services.library);
                                    let fallback_bg = cx.theme().muted;
                                    if let Some(cover_img) = cover_img {
                                        img(cover_img)
                                            .w(px(COVER_SIZE))
                                            .h(px(COVER_SIZE))
                                            .rounded(px(3.))
                                            .object_fit(ObjectFit::Cover)
                                            .with_fallback(move || {
                                                div()
                                                    .w(px(COVER_SIZE))
                                                    .h(px(COVER_SIZE))
                                                    .rounded(px(3.))
                                                    .bg(fallback_bg)
                                                    .into_any_element()
                                            })
                                            .into_any_element()
                                    } else {
                                        div()
                                            .w(px(COVER_SIZE))
                                            .h(px(COVER_SIZE))
                                            .rounded(px(3.))
                                            .bg(fallback_bg)
                                            .into_any_element()
                                    }
                                };

                                h_flex()
                                    .w_full()
                                    .h(px(TRACK_ROW_HEIGHT))
                                    .px_4()
                                    .gap_2()
                                    .items_center()
                                    .cursor(gpui::CursorStyle::PointingHand)
                                    .border_b(px(1.))
                                    .border_color(cx.theme().border)
                                    .when(is_current, |s| s.bg(cx.theme().secondary))
                                    .hover(|s| s.bg(cx.theme().secondary))
                                    .child(left_cell)
                                    .child(
                                        div()
                                            .flex_1()
                                            .overflow_hidden()
                                            .truncate()
                                            .text_sm()
                                            .when(is_current, |d| {
                                                d.font_weight(FontWeight::SEMIBOLD)
                                            })
                                            .child(track.title.clone()),
                                    )
                                    .child(
                                        div()
                                            .w_16()
                                            .text_sm()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(duration_str),
                                    )
                                    .id(ElementId::Integer(track_id as u64))
                                    .on_click(cx.listener(move |_this, _, _, cx| {
                                        let services = cx.global::<Services>();
                                        let mut queue = services.playback_queue.borrow_mut();
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

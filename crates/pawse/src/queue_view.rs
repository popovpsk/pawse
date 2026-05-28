use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use audio_engine::EngineEvent;
use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, AppContext, Context, Div, ElementId, FontWeight, Hsla, Image, InteractiveElement,
    IntoElement, ObjectFit, ParentElement, Pixels, Render, SharedString, Size,
    StatefulInteractiveElement, Styled, StyledImage, Subscription, Window, div, img, px,
};
use gpui_component::{VirtualListScrollHandle, h_flex, v_flex, v_virtual_list};

use crate::cover_art_cache::CoverArtCache;
use crate::theme_colors::Colors;
use crate::track_duration::track_duration;
use ui_components::cover_placeholder::cover_placeholder;

use crate::library_service::LibraryEvent;
use crate::like_button::{LIKE_ROW_GROUP, like_button};
use crate::playback_queue::RemoveOutcome;
use crate::playlist_buttons::add_to_playlist_button;
use crate::services::Services;
use crate::settings_store::SettingsStore;

#[derive(Clone)]
struct DraggedQueueTrack {
    from_ix: usize,
    title: SharedString,
}

impl Render for DraggedQueueTrack {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_3()
            .py_1()
            .rounded_md()
            .bg(Colors::popover_background(cx))
            .text_color(Colors::popover_text(cx))
            .border_1()
            .border_color(Colors::panel_border(cx))
            .text_sm()
            .opacity(0.9)
            .child(self.title.clone())
    }
}

#[derive(Clone, Copy)]
struct QueueRowParams {
    muted: Hsla,
    muted_fg: Hsla,
    border: Hsla,
    accent: Hsla,
    list_hover: Hsla,
    liked_enabled: bool,
    playlists_enabled: bool,
    show_track_duration: bool,
    show_queue_actions: bool,
    show_queue_artist: bool,
    item_height: f32,
}

impl QueueRowParams {
    fn from_cx(cx: &mut Context<QueueView>) -> Self {
        let settings = cx.global::<SettingsStore>();
        let show_queue_artist = settings.show_queue_artist();
        Self {
            muted: Colors::control_hover_bg(cx),
            muted_fg: Colors::text_secondary(cx),
            border: Colors::panel_border(cx),
            accent: Colors::icon_button_hover_bg(cx),
            list_hover: Colors::list_row_hover_bg(cx),
            liked_enabled: settings.liked_enabled(),
            playlists_enabled: settings.playlists_enabled(),
            show_track_duration: settings.show_track_duration(),
            show_queue_actions: settings.show_queue_actions(),
            show_queue_artist,
            item_height: if show_queue_artist { 48. } else { 36. } + 1.,
        }
    }
}

struct Track {
    id: i64,
    title: SharedString,
    duration: SharedString,
    artist: SharedString,
    cover: Option<Arc<Image>>,
    liked: bool,
}

impl Track {
    pub fn from_library_track(
        src: &music_library::Track,
        artist_by_track: &HashMap<i64, SharedString>,
        cover_art_cache: &mut CoverArtCache,
        library: &crate::library_service::LibraryService,
    ) -> Self {
        let duration = src
            .duration_ms
            .map(|ms| {
                let secs = (ms / 1000) as u32;
                format!("{:02}:{:02}", secs / 60, secs % 60)
            })
            .unwrap_or_default();

        let cover = cover_art_cache.get_small(src.cover_art_id, library);
        Self {
            id: src.id,
            title: src.title.clone().into(),
            artist: artist_by_track.get(&src.id).cloned().unwrap_or_default(),
            cover,
            liked: src.liked,
            duration: duration.into(),
        }
    }
}

pub struct QueueView {
    tracks: Vec<Track>,
    current_index: Option<usize>,
    is_playing: bool,
    item_sizes: Rc<Vec<Size<Pixels>>>,
    scroll_handle: VirtualListScrollHandle,
    _subscription: Subscription,
    _library_subscription: Subscription,
}

impl QueueView {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let services = cx.global::<Services>();
        let engine_event_bus = services.engine_event_bus.clone();
        let library_event_bus = services.library_event_bus.clone();
        let is_playing = services
            .is_playing
            .load(std::sync::atomic::Ordering::Relaxed);

        let subscription =
            cx.subscribe(
                &engine_event_bus,
                |this, _, event: &EngineEvent, cx| match event {
                    EngineEvent::Loaded { .. } => {
                        this.refresh_tracks(cx);
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
                    EngineEvent::Stopped => {
                        this.is_playing = false;
                        cx.notify();
                    }
                    _ => {}
                },
            );

        let library_subscription =
            cx.subscribe(&library_event_bus, |this, _, event: &LibraryEvent, cx| {
                match event {
                    LibraryEvent::TrackLikedChanged { track_id, liked } => {
                        let mut changed = false;
                        for t in this.tracks.iter_mut() {
                            if t.id == *track_id && t.liked != *liked {
                                t.liked = *liked;
                                changed = true;
                            }
                        }
                        if changed {
                            cx.notify();
                        }
                    }
                    LibraryEvent::PlaylistTracksChanged { .. } => {
                        // Services has already synced the queue if it was
                        // backed by this playlist; refresh our snapshot.
                        this.refresh_tracks(cx);
                    }
                    LibraryEvent::QueueChanged => {
                        this.refresh_tracks(cx);
                    }
                    _ => {}
                }
            });

        let mut result = Self {
            tracks: Vec::new(),
            current_index: None,
            is_playing,
            scroll_handle: VirtualListScrollHandle::new(),
            _subscription: subscription,
            _library_subscription: library_subscription,
            item_sizes: Rc::new(Vec::new()),
        };
        result.refresh_tracks(cx);
        result
    }

    pub fn refresh_tracks(&mut self, cx: &mut Context<Self>) {
        let services = cx.global::<Services>();
        let queue = services.playback_queue.borrow();
        let new_tracks = queue.tracks_vec();
        let new_index = queue.current_index();
        drop(queue);
        let is_same_queue = self.tracks.len() == new_tracks.len()
            && self
                .tracks
                .iter()
                .zip(new_tracks.iter())
                .all(|(x, y)| x.id == y.id);

        if !is_same_queue {
            let artist_by_track = build_artist_map(&services.library, &new_tracks);
            let mut art_cache = services.cover_art_cache.borrow_mut();

            self.tracks = new_tracks
                .iter()
                .map(|x| {
                    Track::from_library_track(
                        x,
                        &artist_by_track,
                        &mut art_cache,
                        &services.library,
                    )
                })
                .collect();
        };
        self.current_index = new_index;
        cx.notify();
    }

    fn virtual_list_item_sizes(&mut self, item_height: f32) -> Rc<Vec<Size<Pixels>>> {
        if self.tracks.len() == self.item_sizes.len()
            && self
                .item_sizes
                .first()
                .unwrap_or(&Size::new(px(0.), px(0.)))
                .height
                == px(item_height)
        {
            self.item_sizes.clone()
        } else {
            self.item_sizes = Rc::new(vec![Size::new(px(0.), px(item_height)); self.tracks.len()]);
            self.item_sizes.clone()
        }
    }
}

impl Render for QueueView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let foreground = Colors::text_primary(cx);
        let header = queue_header(foreground);

        if self.tracks.is_empty() {
            return queue_empty_state(cx, header);
        }

        let params = QueueRowParams::from_cx(cx);

        v_flex().size_full().child(header).child(
            v_virtual_list(
                cx.entity().clone(),
                "queue_list",
                self.virtual_list_item_sizes(params.item_height),
                move |view, visible_range, _window, cx| {
                    visible_range
                        .map(|track_ix| {
                            queue_visible_range_row(cx, view, &params, track_ix).into_any_element()
                        })
                        .collect::<Vec<_>>()
                },
            )
            .track_scroll(&self.scroll_handle),
        )
    }
}

fn queue_visible_range_row(
    cx: &mut Context<QueueView>,
    view: &mut QueueView,
    params: &QueueRowParams,
    track_ix: usize,
) -> gpui::Stateful<Div> {
    let track = &view.tracks[track_ix];
    let track_id = track.id;
    let is_current = Some(track_ix) == view.current_index;

    h_flex()
        .id(ElementId::Integer(track_ix as u64))
        .group(LIKE_ROW_GROUP)
        .w_full()
        .h(px(params.item_height))
        .pl_4()
        .pr_2()
        .gap_1()
        .items_center()
        .border_b(px(1.))
        .border_color(params.border)
        .when(is_current, |s| crate::row_style::current_row(s, cx))
        .hover(|s| s.bg(params.list_hover))
        .child(album_cover_cell(params, track.cover.clone()))
        .when_else(
            params.show_queue_artist,
            |row| {
                row.child(
                    v_flex()
                        .ml_2()
                        .flex_1()
                        .overflow_hidden()
                        .truncate()
                        .child(
                            div()
                                .text_sm()
                                .when(is_current, |d| d.font_weight(FontWeight::SEMIBOLD))
                                .child(track.title.clone()),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .text_xs()
                                .text_color(params.muted_fg)
                                .child(track.artist.clone()),
                        ),
                )
            },
            |row| {
                row.child(
                    div()
                        .ml_2()
                        .flex_1()
                        .overflow_hidden()
                        .truncate()
                        .text_sm()
                        .when(is_current, |d| d.font_weight(FontWeight::SEMIBOLD))
                        .child(track.title.clone()),
                )
            },
        )
        .when(
            params.show_queue_actions && params.playlists_enabled,
            |row| {
                row.child(
                    div()
                        .flex_shrink_0()
                        .id(ElementId::NamedInteger(
                            "queue-playlist".into(),
                            track_ix as u64,
                        ))
                        .child(add_to_playlist_button(track_id, cx)),
                )
            },
        )
        .when(params.show_queue_actions && params.liked_enabled, |row| {
            row.child(
                div()
                    .flex_shrink_0()
                    .id(ElementId::NamedInteger(
                        "queue-like".into(),
                        track_ix as u64,
                    ))
                    .child(like_button(track_id, track.liked, cx)),
            )
        })
        .when(params.show_track_duration, |row| {
            row.child(track_duration(cx, track.duration.clone()))
        })
        .child(
            div()
                .id(ElementId::NamedInteger(
                    "remove-from-queue".into(),
                    track_ix as u64,
                ))
                .flex_shrink_0()
                .size(px(26.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .cursor(gpui::CursorStyle::PointingHand)
                .opacity(0.)
                .group_hover(LIKE_ROW_GROUP, |s| s.opacity(1.))
                .hover(|s| s.bg(params.accent))
                .tooltip(|window, cx| {
                    gpui_component::tooltip::Tooltip::new("Remove from queue").build(window, cx)
                })
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    let services = cx.global::<Services>();
                    let outcome = services
                        .playback_queue
                        .borrow_mut()
                        .remove_track_at(track_ix);
                    match outcome {
                        RemoveOutcome::PlayNext(next) => {
                            // The successor starts from its beginning;
                            // reset the persisted position so a save
                            // racing the async engine reset is correct.
                            services
                                .current_position_ms
                                .store(0, std::sync::atomic::Ordering::Relaxed);
                            if this.is_playing {
                                services.play_track(&next);
                            } else {
                                // Load the successor so now-playing
                                // updates without resuming playback.
                                services.load_track(&next);
                            }
                        }
                        RemoveOutcome::Stopped => {
                            services
                                .current_position_ms
                                .store(0, std::sync::atomic::Ordering::Relaxed);
                            services.engine_manager.stop();
                        }
                        RemoveOutcome::Unaffected => {}
                    }
                    this.refresh_tracks(cx);
                    crate::services::save_playback(cx);
                }))
                .child(
                    gpui::svg()
                        .path("icons/s1-x.svg")
                        .size(px(14.))
                        .text_color(params.muted_fg),
                ),
        )
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
        .on_drag(
            DraggedQueueTrack {
                from_ix: track_ix,
                title: track.title.clone(),
            },
            |drag, _, _, cx| {
                cx.stop_propagation();
                cx.new(|_| drag.clone())
            },
        )
        .drag_over::<DraggedQueueTrack>(move |style, drag, _, cx| {
            // The dragged item lands below the target when moving
            // down (the target shifts up after removal) and above
            // it when moving up; match the indicator to that gap.
            // When indicating the top gap, drop the row's own bottom
            // divider so only the single indicator line shows.
            let style = if drag.from_ix < track_ix {
                style.border_b_2()
            } else {
                style.border_t_2().border_b(px(0.))
            };
            style.border_color(Colors::drag_over_border(cx))
        })
        .on_drop(cx.listener(move |this, drag: &DraggedQueueTrack, _, cx| {
            if drag.from_ix == track_ix {
                return;
            }
            let services = cx.global::<Services>();
            services
                .playback_queue
                .borrow_mut()
                .move_track(drag.from_ix, track_ix);
            this.refresh_tracks(cx);
            crate::services::save_playback(cx);
        }))
}

fn queue_empty_state(cx: &Context<QueueView>, header: Div) -> Div {
    v_flex().size_full().child(header).child(
        div()
            .px_4()
            .pt_2()
            .text_sm()
            .text_color(Colors::text_secondary(cx))
            .child("Queue is empty."),
    )
}

fn queue_header(foreground: Hsla) -> Div {
    h_flex()
        .w_full()
        .h(px(40.))
        .flex_shrink_0()
        .px_4()
        .items_center()
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(foreground)
                .child("Queue"),
        )
}

fn album_cover_cell(params: &QueueRowParams, cover_img: Option<Arc<Image>>) -> AnyElement {
    let cover_size = if params.show_queue_artist { 32. } else { 24. };

    if let Some(cover_img) = cover_img {
        img(cover_img)
            .flex_shrink_0()
            .w(px(cover_size))
            .h(px(cover_size))
            .rounded(px(3.))
            .object_fit(ObjectFit::Cover)
            .with_fallback({
                let bg = params.muted;
                let fg = params.muted_fg;
                move || cover_placeholder(cover_size, 3., bg, fg).into_any_element()
            })
            .into_any_element()
    } else {
        cover_placeholder(cover_size, 3., params.muted, params.muted_fg).into_any_element()
    }
}

fn build_artist_map(
    library: &crate::library_service::LibraryService,
    tracks: &[music_library::Track],
) -> HashMap<i64, SharedString> {
    let ids: Vec<i64> = tracks.iter().map(|t| t.id).collect();
    library
        .track_artists_map(&ids)
        .into_iter()
        .map(|(id, names)| (id, names.join(", ").into()))
        .collect()
}

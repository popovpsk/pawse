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
use ui_components::cover_placeholder::cover_placeholder;

use crate::library_service::LibraryEvent;
use crate::like_button::{LIKE_ROW_GROUP, like_button};
use crate::playback_queue::QueueSource;
use crate::playlist_buttons::{add_to_playlist_button, remove_from_playlist_button};
use crate::services::Services;
use crate::settings_store::SettingsStore;

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
    artist_by_track: HashMap<i64, String>,
    items: Vec<QueueItem>,
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

        let queue = services.playback_queue.borrow();
        let tracks = queue.tracks_vec();
        let current_index = queue.current_index();
        drop(queue);
        let is_playing = services
            .is_playing
            .load(std::sync::atomic::Ordering::Relaxed);

        // Pre-warm cover art cache so render never hits the DB.
        {
            let mut cache = services.cover_art_cache.borrow_mut();
            for track in &tracks {
                cache.get_small(track.cover_art_id, &services.library);
            }
        }

        let artist_by_track = build_artist_map(&services.library, &tracks);
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
                        this.artist_by_track = build_artist_map(&services.library, &new_tracks);
                        let (new_items, new_sizes) = Self::build_items(&new_tracks);
                        this.items = new_items;
                        this.item_sizes = Rc::new(new_sizes);
                        this.tracks = new_tracks;
                        this.current_index = new_index;
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
                    _ => {}
                }
            });

        Self {
            tracks,
            artist_by_track,
            items,
            current_index,
            is_playing,
            item_sizes: Rc::new(item_sizes),
            scroll_handle: VirtualListScrollHandle::new(),
            _subscription: subscription,
            _library_subscription: library_subscription,
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
        self.artist_by_track = build_artist_map(&services.library, &new_tracks);
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
        let theme = cx.theme();
        let foreground = theme.foreground;
        let muted = theme.muted;
        let muted_fg = theme.muted_foreground;
        let border = theme.border;
        let secondary = theme.secondary;
        let settings = cx.global::<SettingsStore>();
        let liked_enabled = settings.liked_enabled();
        let playlists_enabled = settings.playlists_enabled();
        let show_track_duration = settings.show_track_duration();
        // Only surface the "remove from playlist" X if both the queue is backed
        // by a playlist AND the playlists feature flag is on. If the user
        // disables the flag mid-playback the X disappears immediately.
        let playlist_source = if playlists_enabled {
            match cx.global::<Services>().playback_queue.borrow().source() {
                QueueSource::Playlist(id) => Some(id),
                _ => None,
            }
        } else {
            None
        };

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
                    .text_color(foreground)
                    .child("Queue"),
            );

        if self.tracks.is_empty() {
            return v_flex().size_full().child(header).child(
                div()
                    .px_4()
                    .pt_2()
                    .text_sm()
                    .text_color(muted_fg)
                    .child("Queue is empty."),
            );
        }

        let item_sizes = self.item_sizes.clone();
        v_flex().size_full().child(header).child(
            v_virtual_list(
                cx.entity().clone(),
                "queue_list",
                item_sizes,
                move |view, visible_range, _window, cx| {
                    visible_range
                        .map(|ix| match view.items[ix] {
                            QueueItem::TopPadding => {
                                div().w_full().h(px(TOP_PADDING)).into_any_element()
                            }
                            QueueItem::Track(track_ix) => {
                                let track = &view.tracks[track_ix];
                                let track_id = track.id;
                                let cover_art_id = track.cover_art_id;
                                let liked = track.liked;
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
                                let is_current = Some(track_ix) == view.current_index;

                                // Arc::clone from cache — O(1), no DB access.
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

                                let left_cell: gpui::AnyElement = cover_el;

                                h_flex()
                                    .group(LIKE_ROW_GROUP)
                                    .w_full()
                                    .h(px(TRACK_ROW_HEIGHT))
                                    .pl_4()
                                    .pr_2()
                                    .gap_2()
                                    .items_center()
                                    .cursor(gpui::CursorStyle::PointingHand)
                                    .border_b(px(1.))
                                    .border_color(border)
                                    .when(is_current, |s| s.bg(secondary))
                                    .hover(|s| s.bg(secondary))
                                    .child(left_cell)
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(80.))
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
                                            .max_w(px(110.))
                                            .overflow_hidden()
                                            .truncate()
                                            .text_sm()
                                            .text_color(muted_fg)
                                            .child(artist),
                                    )
                                    .when(playlists_enabled, |row| {
                                        row.child(add_to_playlist_button(track_id, cx))
                                    })
                                    .when(liked_enabled, |row| {
                                        row.child(like_button(track_id, liked, cx))
                                    })
                                    .when_some(playlist_source, |row, pid| {
                                        row.child(remove_from_playlist_button(track_id, pid, cx))
                                    })
                                    .when(show_track_duration, |row| {
                                        row.child(
                                            div()
                                                .text_sm()
                                                .text_color(muted_fg)
                                                .child(duration_str),
                                        )
                                    })
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

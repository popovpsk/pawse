use std::rc::Rc;

use audio_engine::EngineEvent;
use gpui::prelude::FluentBuilder;
use gpui::{
    AppContext, Context, ElementId, Entity, EventEmitter, FontWeight, InteractiveElement,
    IntoElement, ParentElement, Pixels, Render, Size, StatefulInteractiveElement, Styled,
    Subscription, Window, div, px, size, svg,
};
use gpui_component::{ActiveTheme, VirtualListScrollHandle, h_flex, v_flex, v_virtual_list};
use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};

use crate::library_service::LibraryEvent;
use crate::library_views::album_info::AlbumInfo;
use crate::like_button::{LIKE_ROW_GROUP, like_button};
use crate::now_playing::NavigateToArtistRequested;
use crate::playlist_buttons::add_to_playlist_button;
use crate::queue_button::add_to_queue_button;
use crate::services::Services;
use crate::settings_store::SettingsStore;

const TOP_PADDING: f32 = 12.;
const TRACK_ROW_HEIGHT: f32 = 36.;
const DISC_HEADER_HEIGHT: f32 = 32.;
const ALBUM_INFO_HEIGHT: f32 = 170.;
const MIN_FUZZY_SCORE_PER_CHAR: u32 = 14;

#[derive(Clone, Copy)]
enum TrackItem {
    TopPadding,
    AlbumInfo,
    DiscHeader(i32),
    Track(usize),
}

pub struct TracksView {
    tracks_all: Vec<music_library::Track>,
    tracks: Vec<music_library::Track>,
    filter: String,
    matcher: Matcher,
    items: Vec<TrackItem>,
    item_sizes: Rc<Vec<Size<Pixels>>>,
    scroll_handle: VirtualListScrollHandle,
    album_info: Entity<AlbumInfo>,
    current_track_id: Option<i64>,
    is_playing: bool,
    _subscription: Subscription,
    _library_subscription: Subscription,
    _album_info_subscription: Subscription,
}

impl TracksView {
    pub fn new(album: &music_library::AlbumSummary, cx: &mut Context<Self>) -> Self {
        let services = cx.global::<Services>();
        let tracks_all = services.library.tracks_for_album(album.id);
        let tracks = tracks_all.clone();
        let (items, item_sizes_vec) = Self::build_items(&tracks);

        let item_sizes = Rc::new(item_sizes_vec);
        let engine_event_bus = services.engine_event_bus.clone();
        let library_event_bus = services.library_event_bus.clone();
        let current_track_id = services
            .playback_queue
            .borrow()
            .current_track()
            .map(|t| t.id);
        let is_playing = services
            .is_playing
            .load(std::sync::atomic::Ordering::Relaxed);
        let album_info = cx.new(|_cx| AlbumInfo::new(album));

        let subscription =
            cx.subscribe(
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
                    EngineEvent::TrackEnded => {
                        if this.is_playing {
                            this.is_playing = false;
                            cx.notify();
                        }
                        let queue_empty = cx
                            .global::<Services>()
                            .playback_queue
                            .borrow()
                            .current_track()
                            .is_none();
                        if queue_empty && this.current_track_id.is_some() {
                            this.current_track_id = None;
                            cx.notify();
                        }
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
                            t.liked = *liked;
                            changed = true;
                        }
                    }
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
            });

        let album_info_subscription = cx.subscribe(
            &album_info,
            |_, _, event: &NavigateToArtistRequested, cx| {
                cx.emit(NavigateToArtistRequested {
                    artist_id: event.artist_id,
                });
            },
        );

        Self {
            tracks_all,
            tracks,
            filter: String::new(),
            matcher: Matcher::new(Config::DEFAULT),
            items,
            item_sizes,
            scroll_handle: VirtualListScrollHandle::new(),
            album_info,
            current_track_id,
            is_playing,
            _subscription: subscription,
            _library_subscription: library_subscription,
            _album_info_subscription: album_info_subscription,
        }
    }

    fn build_items(tracks: &[music_library::Track]) -> (Vec<TrackItem>, Vec<Size<Pixels>>) {
        let max_disc = tracks.iter().map(|t| t.disc_number).max().unwrap_or(1);
        let multi_disc = max_disc > 1;

        let mut items = vec![TrackItem::TopPadding, TrackItem::AlbumInfo];
        let mut item_sizes_vec = vec![
            size(px(300.), px(TOP_PADDING)),
            size(px(300.), px(ALBUM_INFO_HEIGHT + 1.)),
        ];

        if multi_disc {
            let mut current_disc = 0i32;
            for (ix, track) in tracks.iter().enumerate() {
                if track.disc_number != current_disc {
                    current_disc = track.disc_number;
                    items.push(TrackItem::DiscHeader(current_disc));
                    item_sizes_vec.push(size(px(300.), px(DISC_HEADER_HEIGHT + 1.)));
                }
                items.push(TrackItem::Track(ix));
                item_sizes_vec.push(size(px(300.), px(TRACK_ROW_HEIGHT + 1.)));
            }
        } else {
            for ix in 0..tracks.len() {
                items.push(TrackItem::Track(ix));
                item_sizes_vec.push(size(px(300.), px(TRACK_ROW_HEIGHT + 1.)));
            }
        }

        (items, item_sizes_vec)
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
                    let haystack = Utf32Str::new(&track.title, &mut buf);
                    pattern
                        .score(haystack, &mut self.matcher)
                        .filter(|s| *s >= threshold)
                        .map(|s| (track.clone(), s))
                })
                .collect();
            scored.sort_by_key(|(_, score)| std::cmp::Reverse(*score));
            self.tracks = scored.into_iter().map(|(t, _)| t).collect();
        }
        let (items, item_sizes_vec) = Self::build_items(&self.tracks);
        self.items = items;
        self.item_sizes = Rc::new(item_sizes_vec);
    }
}

impl EventEmitter<NavigateToArtistRequested> for TracksView {}

impl Render for TracksView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let border = theme.border;
        let secondary = theme.secondary;
        let muted_fg = theme.muted_foreground;
        let foreground = theme.foreground;
        let liked_enabled = cx.global::<SettingsStore>().liked_enabled();
        let playlists_enabled = cx.global::<SettingsStore>().playlists_enabled();

        if self.tracks.is_empty() {
            let message = if self.tracks_all.is_empty() {
                "No tracks found for this album."
            } else {
                "No tracks match your search."
            };
            return v_flex()
                .size_full()
                .child(self.album_info.clone())
                .child(div().px_4().child(message));
        }

        let item_sizes = self.item_sizes.clone();
        v_flex().size_full().child(
            v_virtual_list(
                cx.entity().clone(),
                "tracks_list",
                item_sizes,
                move |view, visible_range, _window, cx| {
                    visible_range
                        .map(|ix| match view.items[ix] {
                            TrackItem::TopPadding => {
                                div().w_full().h(px(TOP_PADDING)).into_any_element()
                            }
                            TrackItem::AlbumInfo => view.album_info.clone().into_any_element(),
                            TrackItem::DiscHeader(disc) => h_flex()
                                .w_full()
                                .h(px(DISC_HEADER_HEIGHT))
                                .px_4()
                                .items_center()
                                .border_b(px(1.))
                                .border_color(border)
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(muted_fg)
                                        .child(format!("Disc {}", disc)),
                                )
                                .into_any_element(),
                            TrackItem::Track(track_ix) => {
                                let track = &view.tracks[track_ix];
                                let track_id = track.id;
                                let track_num_str = track
                                    .track_number
                                    .map(|n| format!("{}.", n))
                                    .unwrap_or_default();
                                let duration_str = track
                                    .duration_ms
                                    .map(|ms| {
                                        let secs = (ms / 1000) as u32;
                                        format!("{:02}:{:02}", secs / 60, secs % 60)
                                    })
                                    .unwrap_or_default();

                                let is_current = Some(track.id) == view.current_track_id;
                                let liked = track.liked;
                                let track_for_queue = track.clone();

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
                                    .when(is_current, |style| style.bg(secondary))
                                    .hover(|style| style.bg(secondary))
                                    .child(if is_current {
                                        let icon = if view.is_playing {
                                            "icons/play.svg"
                                        } else {
                                            "icons/pause.svg"
                                        };
                                        div()
                                            .w_8()
                                            .flex()
                                            .items_center()
                                            .child(
                                                svg()
                                                    .path(icon)
                                                    .size(px(12.))
                                                    .text_color(foreground),
                                            )
                                            .into_any_element()
                                    } else {
                                        div().w_8().child(track_num_str).into_any_element()
                                    })
                                    .child(
                                        div()
                                            .flex_1()
                                            .overflow_hidden()
                                            .truncate()
                                            .when(is_current, |d| {
                                                d.font_weight(FontWeight::SEMIBOLD)
                                            })
                                            .child(track.title.clone()),
                                    )
                                    .when(playlists_enabled, |row| {
                                        row.child(add_to_playlist_button(track_id, cx))
                                    })
                                    .when(liked_enabled, |row| {
                                        row.child(like_button(track_id, liked, cx))
                                    })
                                    .child(div().child(duration_str))
                                    .child(add_to_queue_button(track_for_queue, 26., 16., cx))
                                    .id(ElementId::Integer(track_id as u64))
                                    .on_click(cx.listener(move |this, _, _, _cx| {
                                        let services = _cx.global::<Services>();
                                        let mut queue = services.playback_queue.borrow_mut();
                                        let track = queue
                                            .set_tracks_and_play_at(
                                                this.tracks.clone(),
                                                track_ix,
                                                crate::playback_queue::QueueSource::Unknown,
                                            )
                                            .cloned();
                                        drop(queue);
                                        if let Some(track) = track {
                                            services.play_track(&track);
                                            crate::services::save_playback(_cx);
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

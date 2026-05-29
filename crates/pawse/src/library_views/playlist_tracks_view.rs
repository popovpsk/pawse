use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use audio_engine::EngineEvent;
use gpui::prelude::FluentBuilder;
use gpui::{
    Context, ElementId, FontWeight, Image, InteractiveElement, IntoElement, ObjectFit,
    ParentElement, Pixels, Render, SharedString, Size, StatefulInteractiveElement, Styled,
    StyledImage, Subscription, Window, div, img, px, size, svg,
};
use gpui_component::{VirtualListScrollHandle, h_flex, v_flex, v_virtual_list};

use crate::cover_art_cache::CoverArtCache;
use crate::theme_colors::Colors;
use crate::track_list::{
    LIKE_ROW_GROUP, RowButtonColors, TrackRowBase, add_to_playlist_button, add_to_queue_button,
    like_button, remove_from_playlist_button, track_duration,
};
use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};
use ui_components::cover_placeholder::cover_placeholder;

use crate::library_service::LibraryEvent;
use crate::playback_queue::QueueSource;
use crate::services::Services;
use crate::settings_store::SettingsStore;

const TOP_PADDING: f32 = 12.;
const TRACK_ROW_HEIGHT: f32 = 44.;
const COVER_SIZE: f32 = 32.;
const HEADER_HEIGHT: f32 = 56.;
const MIN_FUZZY_SCORE_PER_CHAR: u32 = 14;

#[derive(Clone, Copy)]
enum Item {
    TopPadding,
    Header,
    Track(usize),
}

struct TrackRow {
    base: TrackRowBase,
    track_all_ix: usize,
    artist: SharedString,
    cover: Option<Arc<Image>>,
}

impl TrackRow {
    fn from_track(
        track: &music_library::Track,
        track_all_ix: usize,
        artist_by_track: &HashMap<i64, SharedString>,
        cover_cache: &mut CoverArtCache,
        library: &crate::library_service::LibraryService,
    ) -> Self {
        Self {
            base: TrackRowBase::from_track(track),
            track_all_ix,
            artist: artist_by_track.get(&track.id).cloned().unwrap_or_default(),
            cover: cover_cache.get_small(track.cover_art_id, library),
        }
    }
}

pub struct PlaylistTracksView {
    playlist: music_library::PlaylistSummary,
    tracks_all: Vec<Rc<music_library::Track>>,
    row_data: Vec<TrackRow>,
    artist_by_track: HashMap<i64, SharedString>,
    items: Vec<Item>,
    item_sizes: Rc<Vec<Size<Pixels>>>,
    filter: String,
    matcher: Matcher,
    current_track_id: Option<i64>,
    is_playing: bool,
    scroll_handle: VirtualListScrollHandle,
    _library_subscription: Subscription,
    _engine_subscription: Subscription,
}

impl PlaylistTracksView {
    pub fn new(
        playlist: &music_library::PlaylistSummary,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let services = cx.global::<Services>();
        let library = services.library.clone();
        let library_event_bus = services.library_event_bus.clone();
        let engine_event_bus = services.engine_event_bus.clone();

        let tracks_all: Vec<Rc<_>> = library
            .tracks_for_playlist(playlist.id)
            .into_iter()
            .map(Rc::new)
            .collect();
        let artist_by_track = build_artist_map(&library, &tracks_all);
        let (items, sizes) = Self::build_items(tracks_all.len());
        let row_data = {
            let mut cover_cache = services.cover_art_cache.borrow_mut();
            tracks_all
                .iter()
                .enumerate()
                .map(|(ix, t)| {
                    TrackRow::from_track(t, ix, &artist_by_track, &mut cover_cache, &library)
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

        let playlist_id = playlist.id;
        let library_subscription = cx.subscribe(
            &library_event_bus,
            move |this, _, event: &LibraryEvent, cx| match event {
                LibraryEvent::PlaylistTracksChanged { playlist_id: pid } if *pid == playlist_id => {
                    let services = cx.global::<Services>();
                    this.tracks_all = services
                        .library
                        .tracks_for_playlist(playlist_id)
                        .into_iter()
                        .map(Rc::new)
                        .collect();
                    this.artist_by_track = build_artist_map(&services.library, &this.tracks_all);
                    this.recompute_visible(cx);
                    cx.notify();
                }
                LibraryEvent::ScanComplete => {
                    let services = cx.global::<Services>();
                    this.tracks_all = services
                        .library
                        .tracks_for_playlist(playlist_id)
                        .into_iter()
                        .map(Rc::new)
                        .collect();
                    this.artist_by_track = build_artist_map(&services.library, &this.tracks_all);
                    this.recompute_visible(cx);
                    cx.notify();
                }
                LibraryEvent::TrackLikedChanged { track_id, liked } => {
                    let mut changed = false;
                    for t in this.tracks_all.iter_mut() {
                        if t.id == *track_id && t.liked != *liked {
                            Rc::make_mut(t).liked = *liked;
                            changed = true;
                        }
                    }
                    for r in this.row_data.iter_mut() {
                        if r.base.id == *track_id && r.base.liked != *liked {
                            r.base.liked = *liked;
                        }
                    }
                    if changed {
                        cx.notify();
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
            playlist: playlist.clone(),
            tracks_all,
            row_data,
            artist_by_track,
            items,
            item_sizes: Rc::new(sizes),
            filter: String::new(),
            matcher: Matcher::new(Config::DEFAULT),
            current_track_id,
            is_playing,
            scroll_handle: VirtualListScrollHandle::new(),
            _library_subscription: library_subscription,
            _engine_subscription: engine_subscription,
        }
    }

    fn build_items(count: usize) -> (Vec<Item>, Vec<Size<Pixels>>) {
        let mut items = vec![Item::TopPadding, Item::Header];
        let mut sizes = vec![
            size(px(300.), px(TOP_PADDING)),
            size(px(300.), px(HEADER_HEIGHT + 1.)),
        ];
        for ix in 0..count {
            items.push(Item::Track(ix));
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
                    TrackRow::from_track(t, ix, &self.artist_by_track, &mut cover_cache, library)
                })
                .collect();
        } else {
            let pattern = Pattern::parse(&self.filter, CaseMatching::Ignore, Normalization::Smart);
            let threshold = self.filter.chars().count() as u32 * MIN_FUZZY_SCORE_PER_CHAR;
            let mut buf: Vec<char> = Vec::new();
            let mut scored: Vec<(usize, u32)> = self
                .tracks_all
                .iter()
                .enumerate()
                .filter_map(|(ix, track)| {
                    let artist = self
                        .artist_by_track
                        .get(&track.id)
                        .map(SharedString::as_str)
                        .unwrap_or("");
                    let hay = format!("{} {}", track.title, artist);
                    let h = Utf32Str::new(&hay, &mut buf);
                    pattern
                        .score(h, &mut self.matcher)
                        .filter(|s| *s >= threshold)
                        .map(|s| (ix, s))
                })
                .collect();
            scored.sort_by_key(|(_, s)| std::cmp::Reverse(*s));
            self.row_data = scored
                .iter()
                .map(|(ix, _)| {
                    TrackRow::from_track(
                        &self.tracks_all[*ix],
                        *ix,
                        &self.artist_by_track,
                        &mut cover_cache,
                        library,
                    )
                })
                .collect()
        }
        let (items, sizes) = Self::build_items(self.row_data.len());
        self.items = items;
        self.item_sizes = Rc::new(sizes);
    }
}

fn build_artist_map(
    library: &crate::library_service::LibraryService,
    tracks: &[Rc<music_library::Track>],
) -> HashMap<i64, SharedString> {
    let ids: Vec<i64> = tracks.iter().map(|t| t.id).collect();
    library
        .track_artists_map(&ids)
        .into_iter()
        .map(|(id, names)| (id, names.join(", ").into()))
        .collect()
}

impl Render for PlaylistTracksView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = Colors::panel_border(cx);
        let list_hover = Colors::list_row_hover_bg(cx);
        let muted = Colors::control_hover_bg(cx);
        let muted_fg = Colors::text_secondary(cx);
        let foreground = Colors::text_primary(cx);
        let liked_enabled = cx.global::<SettingsStore>().liked_enabled();
        let playlists_enabled = cx.global::<SettingsStore>().playlists_enabled();

        let header = div().px_4().pt_3().pb_2().child(
            div()
                .text_xl()
                .font_weight(FontWeight::SEMIBOLD)
                .child(self.playlist.name.clone()),
        );

        if self.row_data.is_empty() {
            let message = if self.tracks_all.is_empty() {
                crate::localization::tr(cx).playlist_is_empty.clone()
            } else {
                crate::localization::tr(cx).no_tracks_match.clone()
            };
            return v_flex()
                .size_full()
                .child(header)
                .child(div().px_4().text_color(muted_fg).child(message));
        }

        let p = PlaylistTrackRowParams {
            playlist_id: self.playlist.id,
            border,
            list_hover,
            muted,
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
                "playlist_tracks_list",
                item_sizes,
                move |view, visible_range, _window, cx| {
                    visible_range
                        .map(|ix| match view.items[ix] {
                            Item::TopPadding => {
                                div().w_full().h(px(TOP_PADDING)).into_any_element()
                            }
                            Item::Header => div()
                                .w_full()
                                .h(px(HEADER_HEIGHT))
                                .px_4()
                                .flex()
                                .items_center()
                                .child(
                                    div()
                                        .text_xl()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(view.playlist.name.clone()),
                                )
                                .into_any_element(),
                            Item::Track(track_ix) => playlist_track_row(view, track_ix, &p, cx),
                        })
                        .collect::<Vec<_>>()
                },
            )
            .track_scroll(&self.scroll_handle)
            .flex_1(),
        )
    }
}

struct PlaylistTrackRowParams {
    playlist_id: i64,
    border: gpui::Hsla,
    list_hover: gpui::Hsla,
    muted: gpui::Hsla,
    muted_fg: gpui::Hsla,
    foreground: gpui::Hsla,
    liked_enabled: bool,
    playlists_enabled: bool,
    buttons: RowButtonColors,
}

fn playlist_track_row(
    view: &mut PlaylistTracksView,
    track_ix: usize,
    p: &PlaylistTrackRowParams,
    cx: &mut Context<PlaylistTracksView>,
) -> gpui::AnyElement {
    let row = &view.row_data[track_ix];
    let track_id = row.base.id;
    let track_all_ix = row.track_all_ix;
    let is_current = Some(track_id) == view.current_track_id;
    let is_playing = view.is_playing;
    let track_for_queue = view.tracks_all[row.track_all_ix].clone();
    let playlist_id = p.playlist_id;

    let cover_el: gpui::AnyElement = if let Some(ref cover_img) = row.cover {
        img(cover_img.clone())
            .w(px(COVER_SIZE))
            .h(px(COVER_SIZE))
            .rounded(px(3.))
            .object_fit(ObjectFit::Cover)
            .with_fallback({
                let bg = p.muted;
                let fg = p.muted_fg;
                move || cover_placeholder(COVER_SIZE, 3., bg, fg).into_any_element()
            })
            .into_any_element()
    } else {
        cover_placeholder(COVER_SIZE, 3., p.muted, p.muted_fg).into_any_element()
    };

    let leading: gpui::AnyElement = if is_current {
        let icon = if is_playing {
            "icons/play.svg"
        } else {
            "icons/pause.svg"
        };
        div()
            .size(px(COVER_SIZE))
            .flex()
            .items_center()
            .justify_center()
            .child(svg().path(icon).size(px(12.)).text_color(p.foreground))
            .into_any_element()
    } else {
        cover_el
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
        .child(remove_from_playlist_button(
            track_id,
            playlist_id,
            &p.buttons,
        ))
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
                    QueueSource::Playlist(playlist_id),
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

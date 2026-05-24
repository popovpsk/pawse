use std::rc::Rc;

use audio_engine::EngineEvent;
use gpui::prelude::FluentBuilder;
use gpui::{
    Context, ElementId, FontWeight, InteractiveElement, IntoElement, ObjectFit, ParentElement,
    Pixels, Render, Size, StatefulInteractiveElement, Styled, StyledImage, Subscription, Window,
    div, img, px, size, svg,
};
use gpui_component::{ActiveTheme, VirtualListScrollHandle, h_flex, v_flex, v_virtual_list};
use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};
use ui_components::cover_placeholder::cover_placeholder;

use crate::library_service::LibraryEvent;
use crate::like_button::{LIKE_ROW_GROUP, like_button};
use crate::playlist_buttons::add_to_playlist_button;
use crate::queue_button::add_to_queue_button;
use crate::services::Services;
use crate::settings_store::SettingsStore;

const TRACK_ROW_HEIGHT: f32 = 36.;
const ALBUM_COVER_SIZE: f32 = 60.;
const ARTIST_HEADER_HEIGHT: f32 = 48.;
const ALBUM_HEADER_HEIGHT: f32 = 84.;
const DISC_HEADER_HEIGHT: f32 = 32.;
const MIN_FUZZY_SCORE_PER_CHAR: u32 = 14;

#[derive(Clone, Debug)]
struct AlbumGroup {
    album_id: Option<i64>,
    album_title: String,
    year: Option<i32>,
    cover_art_id: Option<i64>,
    tracks: Vec<music_library::Track>,
    /// Indices of `tracks` in the flat artist-wide list (used as playback queue index).
    global_indices: Vec<usize>,
}

#[derive(Clone, Copy)]
enum ItemKind {
    ArtistHeader,
    AlbumHeader(usize),
    DiscHeader(i32),
    Track(usize, usize),
}

pub struct ArtistTracksView {
    artist: music_library::ArtistSummary,
    tracks_all: Vec<music_library::Track>,
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
}

impl ArtistTracksView {
    pub fn new(artist: &music_library::ArtistSummary, cx: &mut Context<Self>) -> Self {
        let services = cx.global::<Services>();
        let engine_event_bus = services.engine_event_bus.clone();
        let library_event_bus = services.library_event_bus.clone();
        let tracks_all = services.library.tracks_by_artist(artist.id);

        // Pre-warm cover cache for album headers.
        {
            let mut cache = services.cover_art_cache.borrow_mut();
            for t in &tracks_all {
                cache.get_small(t.cover_art_id, &services.library);
            }
        }

        let groups = Self::group_by_album(&tracks_all, &services.library);
        let (items, sizes) = Self::build_items(&groups);

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
                            t.liked = *liked;
                            changed = true;
                        }
                    }
                    for g in this.groups.iter_mut() {
                        for t in g.tracks.iter_mut() {
                            if t.id == *track_id && t.liked != *liked {
                                t.liked = *liked;
                                changed = true;
                            }
                        }
                    }
                    if changed {
                        cx.notify();
                    }
                }
            });

        Self {
            artist: artist.clone(),
            tracks_all,
            groups,
            items,
            item_sizes: Rc::new(sizes),
            scroll_handle: VirtualListScrollHandle::new(),
            filter: String::new(),
            matcher: Matcher::new(Config::DEFAULT),
            current_track_id,
            is_playing,
            _engine_subscription: engine_subscription,
            _library_subscription: library_subscription,
        }
    }

    fn group_by_album(
        tracks: &[music_library::Track],
        library: &crate::library_service::LibraryService,
    ) -> Vec<AlbumGroup> {
        let mut groups: Vec<AlbumGroup> = Vec::new();
        for (ix, track) in tracks.iter().enumerate() {
            let album_id = track.album_id;
            if let Some(last) = groups.last_mut()
                && last.album_id == album_id
            {
                last.tracks.push(track.clone());
                last.global_indices.push(ix);
                continue;
            }
            let album_title = album_id
                .and_then(|id| library.album_title(id))
                .unwrap_or_else(|| "Unknown".to_string());
            groups.push(AlbumGroup {
                album_id,
                album_title,
                year: track.year,
                cover_art_id: track.cover_art_id,
                tracks: vec![track.clone()],
                global_indices: vec![ix],
            });
        }
        groups
    }

    fn build_items(groups: &[AlbumGroup]) -> (Vec<ItemKind>, Vec<Size<Pixels>>) {
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
                    current_disc = track.disc_number;
                    items.push(ItemKind::DiscHeader(current_disc));
                    sizes.push(size(px(300.), px(DISC_HEADER_HEIGHT + 1.)));
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
        cx.notify();
    }

    fn recompute_groups(&mut self, cx: &mut Context<Self>) {
        let services = cx.global::<Services>();
        let library = services.library.clone();
        if self.filter.is_empty() {
            self.groups = Self::group_by_album(&self.tracks_all, &library);
        } else {
            let pattern = Pattern::parse(&self.filter, CaseMatching::Ignore, Normalization::Smart);
            let threshold = self.filter.chars().count() as u32 * MIN_FUZZY_SCORE_PER_CHAR;
            let mut buf: Vec<char> = Vec::new();
            let kept: Vec<(usize, music_library::Track)> = self
                .tracks_all
                .iter()
                .enumerate()
                .filter(|(_, t)| {
                    let haystack = Utf32Str::new(&t.title, &mut buf);
                    pattern
                        .score(haystack, &mut self.matcher)
                        .is_some_and(|s| s >= threshold)
                })
                .map(|(ix, t)| (ix, t.clone()))
                .collect();

            let mut groups: Vec<AlbumGroup> = Vec::new();
            for (global_ix, track) in kept {
                let album_id = track.album_id;
                if let Some(last) = groups.last_mut()
                    && last.album_id == album_id
                {
                    last.tracks.push(track);
                    last.global_indices.push(global_ix);
                    continue;
                }
                let album_title = album_id
                    .and_then(|id| library.album_title(id))
                    .unwrap_or_else(|| "Unknown".to_string());
                groups.push(AlbumGroup {
                    album_id,
                    album_title,
                    year: track.year,
                    cover_art_id: track.cover_art_id,
                    tracks: vec![track],
                    global_indices: vec![global_ix],
                });
            }
            self.groups = groups;
        }
        let (items, sizes) = Self::build_items(&self.groups);
        self.items = items;
        self.item_sizes = Rc::new(sizes);
    }
}

impl Render for ArtistTracksView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let border = theme.border;
        let secondary = theme.secondary;
        let muted_fg = theme.muted_foreground;
        let foreground = theme.foreground;
        let fallback_bg = theme.secondary;
        let fallback_fg = theme.muted_foreground;
        let liked_enabled = cx.global::<SettingsStore>().liked_enabled();
        let playlists_enabled = cx.global::<SettingsStore>().playlists_enabled();

        if self.tracks_all.is_empty() {
            return v_flex()
                .size_full()
                .child(artist_header_static(self.artist.name.clone()))
                .child(div().px_4().child("No tracks for this artist."));
        }

        if self.groups.is_empty() {
            return v_flex()
                .size_full()
                .child(artist_header_static(self.artist.name.clone()))
                .child(div().px_4().child("No tracks match your search."));
        }

        let item_sizes = self.item_sizes.clone();
        v_flex().size_full().child(
            v_virtual_list(
                cx.entity().clone(),
                "artist_tracks_list",
                item_sizes,
                move |view, visible_range, _window, cx| {
                    visible_range
                        .map(|ix| match view.items[ix] {
                            ItemKind::ArtistHeader => {
                                artist_header_static(view.artist.name.clone()).into_any_element()
                            }
                            ItemKind::DiscHeader(disc) => h_flex()
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
                            ItemKind::AlbumHeader(g_ix) => {
                                let group = &view.groups[g_ix];
                                let services = cx.global::<Services>();
                                let cover_img = services
                                    .cover_art_cache
                                    .borrow_mut()
                                    .get_small(group.cover_art_id, &services.library);
                                let cover_el: gpui::AnyElement = if let Some(cover_img) = cover_img
                                {
                                    img(cover_img)
                                        .w(px(ALBUM_COVER_SIZE))
                                        .h(px(ALBUM_COVER_SIZE))
                                        .rounded(px(4.))
                                        .object_fit(ObjectFit::Cover)
                                        .with_fallback(move || {
                                            cover_placeholder(
                                                ALBUM_COVER_SIZE,
                                                4.,
                                                fallback_bg,
                                                fallback_fg,
                                            )
                                            .into_any_element()
                                        })
                                        .into_any_element()
                                } else {
                                    cover_placeholder(
                                        ALBUM_COVER_SIZE,
                                        4.,
                                        fallback_bg,
                                        fallback_fg,
                                    )
                                    .into_any_element()
                                };
                                let year_str =
                                    group.year.map(|y| format!(" · {}", y)).unwrap_or_default();
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
                                            div().font_weight(FontWeight::SEMIBOLD).child(format!(
                                                "{}{}",
                                                group.album_title, year_str
                                            )),
                                        ),
                                    )
                                    .into_any_element()
                            }
                            ItemKind::Track(g_ix, t_ix) => {
                                let group = &view.groups[g_ix];
                                let track = &group.tracks[t_ix];
                                let track_id = track.id;
                                let is_current = Some(track_id) == view.current_track_id;
                                let is_playing = view.is_playing;
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
                                let title = track.title.clone();
                                let liked = track.liked;
                                let global_ix = group.global_indices[t_ix];
                                let track_for_queue = track.clone();

                                let leading: gpui::AnyElement = if is_current {
                                    let icon = if is_playing {
                                        "icons/play.svg"
                                    } else {
                                        "icons/pause.svg"
                                    };
                                    div()
                                        .w_8()
                                        .flex()
                                        .items_center()
                                        .child(
                                            svg().path(icon).size(px(12.)).text_color(foreground),
                                        )
                                        .into_any_element()
                                } else {
                                    div()
                                        .w_8()
                                        .text_color(muted_fg)
                                        .child(track_num_str)
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
                                    .child(leading)
                                    .child(
                                        div()
                                            .flex_1()
                                            .overflow_hidden()
                                            .truncate()
                                            .when(is_current, |d| {
                                                d.font_weight(FontWeight::SEMIBOLD)
                                            })
                                            .child(title),
                                    )
                                    .when(playlists_enabled, |row| {
                                        row.child(add_to_playlist_button(track_id, cx))
                                    })
                                    .when(liked_enabled, |row| {
                                        row.child(like_button(track_id, liked, cx))
                                    })
                                    .child(
                                        div()
                                            .w_16()
                                            .text_sm()
                                            .text_color(muted_fg)
                                            .child(duration_str),
                                    )
                                    .child(add_to_queue_button(track_for_queue, 30., 18., cx))
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
                        })
                        .collect::<Vec<_>>()
                },
            )
            .track_scroll(&self.scroll_handle)
            .flex_1(),
        )
    }
}

fn artist_header_static(name: String) -> gpui::Div {
    div().px_4().pt_3().pb_2().child(
        div()
            .text_xl()
            .font_weight(FontWeight::SEMIBOLD)
            .child(name),
    )
}

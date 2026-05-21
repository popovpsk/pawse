use audio_engine::EngineEvent;
use gpui::prelude::FluentBuilder;
use gpui::{
    Context, ElementId, FontWeight, InteractiveElement, IntoElement, ObjectFit, ParentElement,
    Render, StatefulInteractiveElement, Styled, StyledImage, Subscription, Window, div, img, px,
    svg,
};
use gpui_component::scroll::ScrollableElement;
use gpui_component::{ActiveTheme, h_flex, v_flex};
use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};
use ui_components::cover_placeholder::cover_placeholder;

use crate::library_service::LibraryEvent;
use crate::like_button::{LIKE_ROW_GROUP, like_button};
use crate::services::Services;

const TRACK_ROW_HEIGHT: f32 = 36.;
const ALBUM_COVER_SIZE: f32 = 60.;
const MIN_FUZZY_SCORE_PER_CHAR: u32 = 14;

#[derive(Clone)]
struct AlbumGroup {
    album_id: Option<i64>,
    album_title: String,
    year: Option<i32>,
    cover_art_id: Option<i64>,
    tracks: Vec<music_library::Track>,
    /// Indices of `tracks` in the flat artist-wide list (used as playback queue index).
    global_indices: Vec<usize>,
}

pub struct ArtistTracksView {
    artist: music_library::ArtistSummary,
    tracks_all: Vec<music_library::Track>,
    groups: Vec<AlbumGroup>,
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

        let current_track_id = services
            .playback_queue
            .borrow()
            .current_track()
            .map(|t| t.id);

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
            filter: String::new(),
            matcher: Matcher::new(Config::DEFAULT),
            current_track_id,
            is_playing: current_track_id.is_some(),
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
            return;
        }
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
}

impl Render for ArtistTracksView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = div().px_4().pt_3().pb_2().child(
            div()
                .text_xl()
                .font_weight(FontWeight::SEMIBOLD)
                .child(self.artist.name.clone()),
        );

        if self.tracks_all.is_empty() {
            return v_flex()
                .size_full()
                .child(header)
                .child(div().px_4().child("No tracks for this artist."));
        }

        if self.groups.is_empty() {
            return v_flex()
                .size_full()
                .child(header)
                .child(div().px_4().child("No tracks match your search."));
        }

        let mut body = v_flex().w_full();
        for group in self.groups.clone() {
            body = body.child(self.render_group(group, cx));
        }

        v_flex()
            .size_full()
            .child(header)
            .child(div().flex_1().overflow_y_scrollbar().child(body))
    }
}

impl ArtistTracksView {
    fn render_group(&self, group: AlbumGroup, cx: &mut Context<Self>) -> impl IntoElement {
        let services = cx.global::<Services>();
        let cover_img = services
            .cover_art_cache
            .borrow_mut()
            .get_small(group.cover_art_id, &services.library);
        let fallback_bg = cx.theme().secondary;
        let fallback_fg = cx.theme().muted_foreground;
        let cover_el: gpui::AnyElement = if let Some(cover_img) = cover_img {
            img(cover_img)
                .w(px(ALBUM_COVER_SIZE))
                .h(px(ALBUM_COVER_SIZE))
                .rounded(px(4.))
                .object_fit(ObjectFit::Cover)
                .with_fallback({
                    let bg = fallback_bg;
                    let fg = fallback_fg;
                    move || cover_placeholder(ALBUM_COVER_SIZE, 4., bg, fg).into_any_element()
                })
                .into_any_element()
        } else {
            cover_placeholder(ALBUM_COVER_SIZE, 4., fallback_bg, fallback_fg).into_any_element()
        };

        let year_str = group.year.map(|y| format!(" · {}", y)).unwrap_or_default();
        let album_header = h_flex()
            .w_full()
            .px_4()
            .pt_4()
            .pb_2()
            .gap_3()
            .items_center()
            .child(cover_el)
            .child(
                div().flex_1().overflow_hidden().child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format!("{}{}", group.album_title, year_str)),
                ),
            );

        let mut list = v_flex()
            .w_full()
            .border_t(px(1.))
            .border_color(cx.theme().border);
        for (local_ix, track) in group.tracks.iter().enumerate() {
            let track = track.clone();
            let global_ix = group.global_indices[local_ix];
            let is_current = Some(track.id) == self.current_track_id;
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
            let track_id = track.id;
            let is_playing = self.is_playing;

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
                        svg()
                            .path(icon)
                            .size(px(12.))
                            .text_color(cx.theme().foreground),
                    )
                    .into_any_element()
            } else {
                div()
                    .w_8()
                    .text_color(cx.theme().muted_foreground)
                    .child(track_num_str)
                    .into_any_element()
            };

            let row = h_flex()
                .group(LIKE_ROW_GROUP)
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
                .child(leading)
                .child(
                    div()
                        .flex_1()
                        .overflow_hidden()
                        .truncate()
                        .when(is_current, |d| d.font_weight(FontWeight::SEMIBOLD))
                        .child(title),
                )
                .child(like_button(track_id, liked, cx))
                .child(
                    div()
                        .w_16()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(duration_str),
                )
                .id(ElementId::Integer(track_id as u64))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let services = cx.global::<Services>();
                    let mut queue = services.playback_queue.borrow_mut();
                    queue.set_tracks(this.tracks_all.clone());
                    let played = queue.play_track_at(global_ix).cloned();
                    drop(queue);
                    if let Some(track) = played {
                        services.play_track(&track);
                        crate::services::save_playback(cx);
                    }
                }));
            list = list.child(row);
        }

        v_flex().w_full().child(album_header).child(list)
    }
}

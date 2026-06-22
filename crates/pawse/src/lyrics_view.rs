use std::path::PathBuf;
use std::time::Duration;

use audio_engine::EngineEvent;
use gpui::prelude::FluentBuilder;
use gpui::{
    Animation, AnimationExt, AppContext, Context, FontWeight, Hsla, InteractiveElement,
    IntoElement, ParentElement, Pixels, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Task, Window, div, ease_out_quint, px, svg,
};
use gpui_component::{h_flex, tooltip::Tooltip, v_flex};

use crate::library_service::{LibraryEvent, LyricsAccess};
use crate::localization::tr;
use crate::services::Services;
use crate::settings_store::SettingsStore;
use crate::theme_colors::Colors;

const ACTIVE_TOLERANCE_MS: u32 = 50;
const SCROLL_ANIM: Duration = Duration::from_millis(360);
const CENTER_BIAS: f32 = 0.4;
const SCROLL_EPS: Pixels = px(1.);

#[derive(PartialEq)]
struct LyricRow {
    text: SharedString,
    time_ms: Option<u32>,
    label: Option<SharedString>,
}

#[derive(Clone)]
struct TrackContext {
    id: i64,
    path: String,
    start_offset_ms: i32,
    album_id: Option<i64>,
    title: String,
    duration_secs: Option<u64>,
}

enum LoadOutcome {
    Lyrics {
        text: String,
        source: String,
        is_cue: bool,
    },
    NotFound {
        is_cue: bool,
    },
    Absent {
        is_cue: bool,
    },
}

pub struct LyricsView {
    current_track_id: Option<i64>,
    rows: Vec<LyricRow>,
    synced: bool,
    source: String,
    track_duration_ms: Option<u64>,
    active_ix: Option<usize>,
    hovered_ix: Option<usize>,
    can_export: bool,
    is_cue: bool,
    fetching: bool,
    loading: bool,
    not_found: bool,
    current_raw: Option<String>,
    visible: bool,
    scroll_handle: ScrollHandle,
    autoscroll: bool,
    scroll_seq: usize,
    scroll_anim: Option<(Pixels, Pixels)>,
    access: LyricsAccess,
    _scroll_task: Option<Task<()>>,
    _load_task: Option<Task<()>>,
    _subscription: Subscription,
    _library_subscription: Subscription,
}

impl LyricsView {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let services = cx.global::<Services>();
        let engine_event_bus = services.engine_event_bus.clone();
        let library_event_bus = services.library_event_bus.clone();
        let access = services.library.lyrics_access();

        let subscription =
            cx.subscribe(
                &engine_event_bus,
                |this, _, event: &EngineEvent, cx| match event {
                    EngineEvent::Loaded { duration, .. } => {
                        this.track_duration_ms = Some(duration.as_millis() as u64);
                        this.load(cx);
                    }
                    EngineEvent::PositionChanged(pos) => this.update_active(*pos, cx),
                    EngineEvent::Stopped => this.clear(cx),
                    _ => {}
                },
            );

        let library_subscription =
            cx.subscribe(&library_event_bus, |this, _, event: &LibraryEvent, cx| {
                if let LibraryEvent::LyricsChanged { track_id } = event
                    && this.current_track_id == Some(*track_id)
                {
                    this.load(cx);
                }
            });

        let mut result = Self {
            current_track_id: None,
            rows: Vec::new(),
            synced: false,
            source: String::new(),
            track_duration_ms: None,
            active_ix: None,
            hovered_ix: None,
            can_export: false,
            is_cue: false,
            fetching: false,
            loading: false,
            not_found: false,
            current_raw: None,
            visible: false,
            scroll_handle: ScrollHandle::new(),
            autoscroll: true,
            scroll_seq: 0,
            scroll_anim: None,
            access,
            _scroll_task: None,
            _load_task: None,
            _subscription: subscription,
            _library_subscription: library_subscription,
        };
        result.load(cx);
        result
    }

    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.visible == visible {
            return;
        }
        self.visible = visible;
        // why: opening the panel on a track we haven't resolved yet kicks the (maybe network) load; a known not-found stays cached
        if visible && self.rows.is_empty() && !self.fetching && !self.loading && !self.not_found {
            self.load(cx);
        }
    }

    fn current_context(cx: &mut Context<Self>) -> Option<TrackContext> {
        let track = cx
            .global::<Services>()
            .playback_queue
            .borrow()
            .current_track()
            .cloned()?;
        Some(TrackContext {
            id: track.id,
            path: track.path,
            start_offset_ms: track.start_offset_ms,
            album_id: track.album_id,
            title: track.title,
            duration_secs: track.duration_ms.map(|ms| (ms / 1000) as u64),
        })
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let Some(ctx) = Self::current_context(cx) else {
            self.clear(cx);
            return;
        };
        let changed = self.current_track_id != Some(ctx.id);
        self.current_track_id = Some(ctx.id);
        if changed {
            // why: drop the previous track's lines at once so they don't linger under the new track during the background read
            self.reset_display();
        }
        // why: show a blank rather than a flash of "No lyrics" while the background read runs, whenever nothing is on screen yet
        if self.rows.is_empty() && !self.fetching {
            self.loading = true;
        }
        let want_fetch = self.visible && cx.global::<SettingsStore>().lyrics_from_internet();
        self.spawn_load(ctx, want_fetch, cx);
        cx.notify();
    }

    fn spawn_load(&mut self, ctx: TrackContext, want_fetch: bool, cx: &mut Context<Self>) {
        let access = self.access.clone();
        self._load_task = Some(cx.spawn(async move |this, cx| {
            let bg = ctx.clone();
            let outcome = cx
                .background_spawn(async move {
                    let is_cue = bg.start_offset_ms != 0 || access.is_multitrack_file(&bg.path);
                    match access.stored(bg.id) {
                        Some(s) if s.not_found => LoadOutcome::NotFound { is_cue },
                        Some(s) => LoadOutcome::Lyrics {
                            text: s.text,
                            source: s.source,
                            is_cue,
                        },
                        None => LoadOutcome::Absent { is_cue },
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                this.apply_load_outcome(ctx, want_fetch, outcome, cx)
            })
            .ok();
        }));
    }

    fn apply_load_outcome(
        &mut self,
        ctx: TrackContext,
        want_fetch: bool,
        outcome: LoadOutcome,
        cx: &mut Context<Self>,
    ) {
        if self.current_track_id != Some(ctx.id) {
            return;
        }
        self.loading = false;
        match outcome {
            LoadOutcome::Lyrics {
                text,
                source,
                is_cue,
            } => {
                self.is_cue = is_cue;
                self.apply_text(&text, &source, cx);
            }
            LoadOutcome::NotFound { is_cue } => {
                self.is_cue = is_cue;
                self.set_not_found(cx);
            }
            LoadOutcome::Absent { is_cue } => {
                self.is_cue = is_cue;
                if want_fetch {
                    self.kick_fetch(ctx, cx);
                } else {
                    self.set_empty(cx);
                }
            }
        }
    }

    fn kick_fetch(&mut self, ctx: TrackContext, cx: &mut Context<Self>) {
        self.fetching = true;
        self.not_found = false;
        cx.notify();
        let access = self.access.clone();
        self._load_task = Some(cx.spawn(async move |this, cx| {
            let id = ctx.id;
            // why: fetched lyrics render via the LyricsChanged reload, so the background task only writes; `emitted` tells us whether a render is coming or we must clear the spinner ourselves
            let emitted = cx
                .background_spawn(async move {
                    let artist = access.first_artist(id).unwrap_or_default();
                    let album = ctx.album_id.and_then(|aid| access.album_title(aid));
                    let query = lyrics::LyricsQuery {
                        artist,
                        title: ctx.title,
                        album,
                        duration_secs: ctx.duration_secs,
                    };
                    match lyrics::fetch(&query) {
                        Ok(Some(remote)) => match pick_remote(remote) {
                            Some(raw) => {
                                access.save(id, &raw, music_library::lyrics_source::LRCLIB)
                            }
                            None => access.mark_not_found(id),
                        },
                        Ok(None) => access.mark_not_found(id),
                        Err(e) => {
                            log::warn!("lyrics fetch failed for track {}: {}", id, e);
                            false
                        }
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                if this.current_track_id == Some(id) && !emitted && this.fetching {
                    this.fetching = false;
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    fn apply_text(&mut self, raw: &str, source: &str, cx: &mut Context<Self>) {
        let parsed = lyrics::parse_lrc(raw);
        let rows: Vec<LyricRow> = parsed
            .lines
            .iter()
            .map(|l| LyricRow {
                text: SharedString::from(l.text.clone()),
                time_ms: l.time_ms,
                label: l.time_ms.map(format_ms),
            })
            .collect();
        let rows_changed = rows != self.rows;
        self.synced = parsed.synced;
        self.rows = rows;
        self.source = source.to_string();
        self.current_raw = Some(raw.to_string());
        self.can_export =
            !self.rows.is_empty() && source != music_library::lyrics_source::LRC && !self.is_cue;
        self.fetching = false;
        self.loading = false;
        self.not_found = false;
        if rows_changed {
            self.active_ix = None;
            self.hovered_ix = None;
            self.autoscroll = true;
            self.scroll_anim = None;
            self.scroll_handle.scroll_to_item(0);
        }
        cx.notify();
    }

    fn clear_content(&mut self) {
        self.rows.clear();
        self.synced = false;
        self.source.clear();
        self.active_ix = None;
        self.hovered_ix = None;
        self.can_export = false;
        self.current_raw = None;
        self.scroll_anim = None;
    }

    fn reset_display(&mut self) {
        self.clear_content();
        self.not_found = false;
        self.fetching = false;
    }

    fn set_empty(&mut self, cx: &mut Context<Self>) {
        self.clear_content();
        self.not_found = false;
        self.fetching = false;
        self.loading = false;
        cx.notify();
    }

    fn set_not_found(&mut self, cx: &mut Context<Self>) {
        self.clear_content();
        self.not_found = true;
        self.fetching = false;
        self.loading = false;
        cx.notify();
    }

    fn clear(&mut self, cx: &mut Context<Self>) {
        self.current_track_id = None;
        self.track_duration_ms = None;
        self.is_cue = false;
        self._load_task = None;
        self.reset_display();
        self.loading = false;
        cx.notify();
    }

    fn seek_to_line(&mut self, ix: usize, time_ms: u32, cx: &mut Context<Self>) {
        let Some(total) = self.track_duration_ms.filter(|&d| d > 0) else {
            return;
        };
        // why: snap highlight to the clicked line so a position report rounding just below time_ms can't flash the previous line
        self.active_ix = Some(ix);
        cx.notify();
        let frac = (time_ms as f64 / total as f64).clamp(0.0, 1.0) as f32;
        cx.global::<Services>().engine_manager.seek(frac);
    }

    fn set_hovered(&mut self, ix: usize, hovered: bool, cx: &mut Context<Self>) {
        let next = if hovered {
            Some(ix)
        } else if self.hovered_ix == Some(ix) {
            None
        } else {
            return;
        };
        if self.hovered_ix == next {
            return;
        }
        self.hovered_ix = next;
        cx.notify();
    }

    fn update_active(&mut self, pos: Duration, cx: &mut Context<Self>) {
        if !self.synced || self.rows.is_empty() {
            return;
        }
        let pos_ms = pos.as_millis() as u32 + ACTIVE_TOLERANCE_MS;
        let mut new_active: Option<usize> = None;
        let mut lo = 0usize;
        let mut hi = self.rows.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            match self.rows[mid].time_ms {
                Some(t) if t <= pos_ms => {
                    new_active = Some(mid);
                    lo = mid + 1;
                }
                _ => hi = mid,
            }
        }
        if new_active == self.active_ix {
            return;
        }
        self.active_ix = new_active;
        if self.autoscroll
            && let Some(ix) = new_active
        {
            self.start_autoscroll(ix, cx);
        }
        cx.notify();
    }

    fn centered_offset(&self, ix: usize) -> Option<Pixels> {
        let item = self.scroll_handle.bounds_for_item(ix)?;
        let vp = self.scroll_handle.bounds();
        if vp.size.height <= px(0.) {
            return None;
        }
        let max = self.scroll_handle.max_offset().height;
        let target = vp.top() + vp.size.height * CENTER_BIAS - item.size.height * 0.5 - item.top();
        Some(target.clamp(-max, px(0.)))
    }

    fn start_autoscroll(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(to) = self.centered_offset(ix) else {
            return;
        };
        let from = self.scroll_handle.offset().y;
        if (from - to).abs() < SCROLL_EPS {
            self.scroll_anim = None;
            return;
        }
        self.scroll_seq = self.scroll_seq.wrapping_add(1);
        let seq = self.scroll_seq;
        self.scroll_anim = Some((from, to));
        self._scroll_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SCROLL_ANIM).await;
            this.update(cx, |this, cx| {
                // why: drop the animator once it has rested the offset at `to`, but only for the latest run so an interrupting scroll isn't clobbered
                if this.scroll_seq == seq {
                    this.scroll_anim = None;
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    fn disengage(&mut self, cx: &mut Context<Self>) {
        if !self.autoscroll && self.scroll_anim.is_none() {
            return;
        }
        self.autoscroll = false;
        self.scroll_anim = None;
        self._scroll_task = None;
        cx.notify();
    }

    fn resync(&mut self, cx: &mut Context<Self>) {
        self.autoscroll = true;
        if let Some(ix) = self.active_ix {
            self.start_autoscroll(ix, cx);
        }
        cx.notify();
    }

    fn export(&mut self, cx: &mut Context<Self>) {
        if !self.can_export {
            return;
        }
        let Some(ctx) = Self::current_context(cx) else {
            return;
        };
        let Some(raw) = self.current_raw.clone() else {
            return;
        };
        // why: don't fake success — can_export clears only when the write lands and source flips to "lrc" via LyricsChanged, so a failed write keeps the button for retry
        cx.global::<Services>()
            .library
            .save_lyrics_file(ctx.id, PathBuf::from(ctx.path), raw);
    }
}

fn pick_remote(remote: lyrics::RemoteLyrics) -> Option<String> {
    remote
        .synced
        .filter(|s| !s.trim().is_empty())
        .or_else(|| remote.plain.filter(|s| !s.trim().is_empty()))
}

impl Render for LyricsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let foreground = Colors::foreground(cx);
        let muted_foreground = Colors::muted_foreground(cx);
        let primary = Colors::primary(cx);
        let muted = Colors::muted(cx);
        let lyrics_font_size = cx.global::<SettingsStore>().lyrics_font_size();
        let synced = self.synced;
        let active_ix = self.active_ix;
        let hovered_ix = self.hovered_ix;
        let show_sync = synced && active_ix.is_some() && !self.autoscroll;

        let header = h_flex()
            .w_full()
            .h(px(40.))
            .flex_shrink_0()
            .px_4()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(foreground)
                    .child(tr().lyrics.clone()),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap_1()
                    .when(show_sync, |d| {
                        d.child(
                            div()
                                .id("lyrics_sync")
                                .size(px(28.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_full()
                                .cursor_pointer()
                                .hover(|s| s.bg(muted))
                                .tooltip(|window, cx| {
                                    Tooltip::new(tr().lyrics_follow.clone()).build(window, cx)
                                })
                                .on_click(cx.listener(|this, _, _, cx| this.resync(cx)))
                                .child(
                                    svg()
                                        .path("icons/locate.svg")
                                        .size(px(18.))
                                        .text_color(foreground),
                                ),
                        )
                    })
                    .when(self.can_export, |d| {
                        d.child(
                            div()
                                .id("lyrics_save")
                                .size(px(28.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_full()
                                .cursor_pointer()
                                .hover(|s| s.bg(muted))
                                .tooltip(|window, cx| {
                                    Tooltip::new(tr().lyrics_save.clone()).build(window, cx)
                                })
                                .on_click(cx.listener(|this, _, _, cx| this.export(cx)))
                                .child(
                                    svg()
                                        .path("icons/save.svg")
                                        .size(px(18.))
                                        .text_color(foreground),
                                ),
                        )
                    }),
            );

        let body = if self.fetching {
            centered_message(tr().lyrics_fetching.clone(), muted_foreground).into_any_element()
        } else if self.loading {
            div().flex_1().into_any_element()
        } else if !self.rows.is_empty() {
            let list = v_flex()
                .id("lyrics_list")
                .size_full()
                .overflow_y_scroll()
                .track_scroll(&self.scroll_handle)
                .on_scroll_wheel(cx.listener(|this, _, _, cx| this.disengage(cx)))
                .py_2()
                .children(self.rows.iter().enumerate().map(|(ix, row)| {
                    let is_active = Some(ix) == active_ix;
                    let color = if synced && is_active {
                        primary
                    } else {
                        foreground
                    };
                    let line = div()
                        .w_full()
                        .px_4()
                        .py_1()
                        .text_size(px(lyrics_font_size))
                        .text_color(color)
                        .when(is_active, |d| d.font_weight(FontWeight::SEMIBOLD));
                    match (synced, row.time_ms, row.label.clone()) {
                        // why: keep the click/hover/tooltip target on the text run, not the
                        // full-width row, so hovering the blank space past a short line is inert
                        (true, Some(time_ms), Some(label)) => line
                            .child(
                                div()
                                    .id(("lyrics_line", ix))
                                    .cursor_pointer()
                                    .when(Some(ix) == hovered_ix, |d| d.underline())
                                    .tooltip(move |window, cx| {
                                        Tooltip::new(label.clone()).build(window, cx)
                                    })
                                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                                        this.set_hovered(ix, *hovered, cx)
                                    }))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.seek_to_line(ix, time_ms, cx)
                                    }))
                                    .child(row.text.clone()),
                            )
                            .into_any_element(),
                        _ => line.child(row.text.clone()).into_any_element(),
                    }
                }));

            let list = if let Some((from, to)) = self.scroll_anim {
                let handle = self.scroll_handle.clone();
                list.with_animation(
                    ("lyrics-autoscroll", self.scroll_seq),
                    Animation::new(SCROLL_ANIM).with_easing(ease_out_quint()),
                    move |el, delta| {
                        let mut offset = handle.offset();
                        offset.y = from + (to - from) * delta;
                        handle.set_offset(offset);
                        el
                    },
                )
                .into_any_element()
            } else {
                list.into_any_element()
            };

            v_flex()
                .flex_1()
                .min_h(px(0.))
                .child(list)
                .into_any_element()
        } else {
            let message = if self.not_found {
                tr().lyrics_not_found.clone()
            } else {
                tr().lyrics_empty.clone()
            };
            centered_message(message, muted_foreground).into_any_element()
        };

        v_flex().size_full().child(header).child(body)
    }
}

fn format_ms(ms: u32) -> SharedString {
    let total_secs = ms / 1000;
    SharedString::from(format!("{:02}:{:02}", total_secs / 60, total_secs % 60))
}

fn centered_message(message: SharedString, color: Hsla) -> gpui::Div {
    v_flex()
        .flex_1()
        .w_full()
        .items_center()
        .justify_center()
        .child(div().px_4().text_sm().text_color(color).child(message))
}

use std::path::PathBuf;
use std::time::{Duration, Instant};

use audio_engine::EngineEvent;
use gpui::prelude::FluentBuilder;
use gpui::{
    Animation, AnimationExt, AppContext, Context, Entity, FontWeight, Hsla, InteractiveElement,
    IntoElement, ParentElement, Pixels, Render, ScrollHandle, SharedString, Size,
    StatefulInteractiveElement, Styled, Subscription, Task, Window, canvas, div, ease_out_quint,
    px, svg,
};
use gpui_component::{h_flex, tooltip::Tooltip, v_flex};

use crate::library_service::{LibraryEvent, LyricsAccess};
use crate::localization::tr;
use crate::lyrics_fill::{self, FillPlan, LineShape};
use crate::services::Services;
use crate::settings_store::SettingsStore;
use crate::theme_colors::Colors;

const SCROLL_ANIM: Duration = Duration::from_millis(360);
const FRAME_MIN_MS: f32 = 30.;
const CENTER_BIAS: f32 = 0.4;
const SCROLL_EPS: Pixels = px(1.);

#[derive(Clone)]
struct TrackContext {
    id: i64,
    path: String,
    is_cue: bool,
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
    rows: Vec<lyrics_fill::LyricRow>,
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
    pos_base_ms: u64,
    pos_base_at: Instant,
    playing: bool,
    measured: Size<Pixels>,
    measured_ix: Option<usize>,
    shape: Option<LineShape>,
    shape_key: Option<(SharedString, Pixels, f32)>,
    fill_step_ms: f32,
    viewport: Size<Pixels>,
    recenter: bool,
    access: LyricsAccess,
    _scroll_task: Option<Task<()>>,
    _frame_task: Option<Task<()>>,
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
        let playing = services
            .is_playing
            .load(std::sync::atomic::Ordering::Relaxed);
        let pos_base_ms = services
            .current_position_ms
            .load(std::sync::atomic::Ordering::Relaxed);

        let subscription =
            cx.subscribe(
                &engine_event_bus,
                |this, _, event: &EngineEvent, cx| match event {
                    EngineEvent::Loaded { duration, .. } => {
                        this.track_duration_ms = Some(duration.as_millis() as u64);
                        this.load(cx);
                    }
                    EngineEvent::PositionChanged(pos) => this.update_active(*pos, cx),
                    EngineEvent::Playing => this.set_playing(true, cx),
                    EngineEvent::Paused | EngineEvent::TrackEnded | EngineEvent::Error(_) => {
                        this.set_playing(false, cx)
                    }
                    EngineEvent::Stopped => {
                        this.set_playing(false, cx);
                        this.clear(cx)
                    }
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
            pos_base_ms,
            pos_base_at: Instant::now(),
            playing,
            measured: Size::default(),
            measured_ix: None,
            shape: None,
            shape_key: None,
            fill_step_ms: 0.,
            viewport: Size::default(),
            recenter: false,
            access,
            _scroll_task: None,
            _frame_task: None,
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
        if visible {
            self.recenter = true;
        }
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
        let is_cue = track.is_cue || music_library::remote::is_remote(&track.path);
        Some(TrackContext {
            id: track.id,
            path: track.path,
            is_cue,
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
            self.reset_display();
        }
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
                    let is_cue = bg.is_cue;
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
        let rows = lyrics_fill::build_rows(&parsed, self.track_duration_ms);
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
            self.invalidate_fill();
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
        self.pos_base_ms = 0;
        self.pos_base_at = Instant::now();
        self.invalidate_fill();
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
        self.active_ix = Some(ix);
        self.pos_base_ms = time_ms as u64;
        self.pos_base_at = Instant::now();
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

    fn invalidate_fill(&mut self) {
        self.measured = Size::default();
        self.measured_ix = None;
        self.shape = None;
        self.shape_key = None;
    }

    fn now_ms(&self) -> u64 {
        if self.playing {
            self.pos_base_ms + self.pos_base_at.elapsed().as_millis() as u64
        } else {
            self.pos_base_ms
        }
    }

    fn display_ms(&self) -> u64 {
        self.now_ms() + lyrics_fill::ACTIVE_TOLERANCE_MS as u64
    }

    fn set_playing(&mut self, playing: bool, cx: &mut Context<Self>) {
        if self.playing == playing {
            return;
        }
        self.pos_base_ms = self.now_ms();
        self.pos_base_at = Instant::now();
        self.playing = playing;
        cx.notify();
    }

    fn active_for(&self, pos_ms: u64) -> Option<usize> {
        if !self.synced {
            return None;
        }
        lyrics_fill::active_row(&self.rows, pos_ms)
    }

    fn set_active(&mut self, ix: Option<usize>, cx: &mut Context<Self>) -> bool {
        if ix == self.active_ix {
            return false;
        }
        self.active_ix = ix;
        if self.autoscroll
            && let Some(ix) = ix
        {
            self.start_autoscroll(ix, cx);
        }
        true
    }

    fn update_active(&mut self, pos: Duration, cx: &mut Context<Self>) {
        self.pos_base_ms = pos.as_millis() as u64;
        self.pos_base_at = Instant::now();
        let next = self.active_for(self.pos_base_ms);
        let changed = self.set_active(next, cx);
        if self.visible
            && (changed || (self.synced && cx.global::<SettingsStore>().lyrics_karaoke_fill()))
        {
            cx.notify();
        }
    }

    fn active_plan(
        &mut self,
        window: &mut Window,
        karaoke: bool,
        font_size: f32,
    ) -> Option<FillPlan> {
        let ix = self.active_ix;
        if self.measured_ix != ix {
            self.measured_ix = ix;
            self.measured = Size::default();
            self.shape = None;
            self.shape_key = None;
        }
        if !karaoke || !self.synced {
            return None;
        }
        let ix = ix?;
        let text = self.rows.get(ix)?.text.clone();
        if text.is_empty() {
            return None;
        }
        let Size { width, height } = self.measured;
        if width <= px(0.) || height <= px(0.) {
            return None;
        }
        let key = (text.clone(), width, font_size);
        if self.shape_key.as_ref() != Some(&key) {
            self.shape = lyrics_fill::shape_line(window, &text, width, px(font_size));
            self.shape_key = Some(key);
        }
        let (start, span) = lyrics_fill::fill_span(&self.rows, ix, self.track_duration_ms)?;
        let t = lyrics_fill::progress(self.display_ms(), start, span);
        let shape = self.shape.as_ref()?;
        if shape.rows.is_empty() {
            return None;
        }
        let pitch = height / shape.rows.len() as f32;
        if pitch < px(font_size * 0.9) || pitch > px(font_size * 2.2) {
            return None;
        }
        self.fill_step_ms = span as f32 / f32::from(shape.total).max(1.);
        Some(lyrics_fill::fill_plan(shape, width, pitch, t))
    }

    fn active_fills(&self) -> bool {
        let Some(ix) = self.active_ix else {
            return false;
        };
        self.rows.get(ix).is_some_and(|row| !row.text.is_empty())
            && lyrics_fill::fill_span(&self.rows, ix, self.track_duration_ms).is_some()
    }

    fn centered_offset(&self, ix: usize) -> Option<Pixels> {
        let item = self.scroll_handle.bounds_for_item(ix)?;
        let vp = self.scroll_handle.bounds();
        if vp.size.height <= px(0.) {
            return None;
        }
        let max = self.scroll_handle.max_offset().y;
        let target = vp.top() + vp.size.height * CENTER_BIAS - item.size.height * 0.5 - item.top();
        Some(target.clamp(-max, px(0.)))
    }

    fn recenter_to(&mut self, ix: usize) {
        let Some(to) = self.centered_offset(ix) else {
            return;
        };
        let mut offset = self.scroll_handle.offset();
        if (offset.y - to).abs() < SCROLL_EPS {
            return;
        }
        offset.y = to;
        self.scroll_handle.set_offset(offset);
        self.scroll_anim = None;
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
        if music_library::remote::is_remote(&ctx.path) {
            return;
        }
        let Some(raw) = self.current_raw.clone() else {
            return;
        };
        let folders = cx.global::<SettingsStore>().music_folders().to_vec();
        cx.global::<Services>().library.save_lyrics_file(
            ctx.id,
            PathBuf::from(ctx.path),
            raw,
            folders,
        );
    }
}

fn pick_remote(remote: lyrics::RemoteLyrics) -> Option<String> {
    remote
        .synced
        .filter(|s| !s.trim().is_empty())
        .or_else(|| remote.plain.filter(|s| !s.trim().is_empty()))
}

impl Render for LyricsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let foreground = Colors::foreground(cx);
        let muted_foreground = Colors::muted_foreground(cx);
        let primary = Colors::primary(cx);
        let muted = Colors::muted(cx);
        let settings = cx.global::<SettingsStore>();
        let lyrics_font_size = settings.lyrics_font_size();
        let karaoke = settings.lyrics_karaoke_fill();
        let dim_inactive = settings.lyrics_dim_inactive();
        let synced = self.synced;

        if synced && self.playing {
            let next = self.active_for(self.now_ms());
            self.set_active(next, cx);
        }

        let viewport = self.scroll_handle.bounds().size;
        if self.viewport != viewport {
            self.viewport = viewport;
            self.recenter = true;
        }
        if self.recenter && viewport.height > px(0.) {
            self.recenter = false;
            if self.autoscroll
                && let Some(ix) = self.active_ix
            {
                self.recenter_to(ix);
            }
        }

        let entity = cx.entity();
        let plan = self.active_plan(window, karaoke, lyrics_font_size);
        let karaoke_active = karaoke && self.active_fills();
        if self.playing && plan.as_ref().is_some_and(|p| p.t < 1.) {
            let wait = Duration::from_millis(self.fill_step_ms.max(FRAME_MIN_MS) as u64);
            self._frame_task = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(wait).await;
                this.update(cx, |_, cx| cx.notify()).ok();
            }));
        } else {
            self._frame_task = None;
        }

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
                    let color = if !synced {
                        foreground
                    } else if is_active {
                        if karaoke_active { foreground } else { primary }
                    } else if dim_inactive {
                        muted_foreground
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
                        (true, Some(time_ms), Some(label)) => line
                            .flex()
                            .child(
                                div()
                                    .id(("lyrics_line", ix))
                                    .max_w_full()
                                    .relative()
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
                                    .child(row.text.clone())
                                    .when(is_active && karaoke, |d| {
                                        d.child(measure_canvas(entity.clone()))
                                    })
                                    .children(match (is_active, plan.as_ref()) {
                                        (true, Some(plan)) => lyrics_fill::fill_children(
                                            plan,
                                            &row.text,
                                            px(lyrics_font_size),
                                            primary,
                                        ),
                                        _ => Vec::new(),
                                    }),
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

fn measure_canvas(entity: Entity<LyricsView>) -> impl IntoElement {
    canvas(
        move |bounds, window, cx| {
            if entity.read(cx).measured != bounds.size {
                entity.update(cx, |this, _| this.measured = bounds.size);
                window.on_next_frame(move |_, cx| {
                    entity.update(cx, |_, cx| cx.notify());
                });
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full()
}

fn centered_message(message: SharedString, color: Hsla) -> gpui::Div {
    v_flex()
        .flex_1()
        .w_full()
        .items_center()
        .justify_center()
        .child(div().px_4().text_sm().text_color(color).child(message))
}

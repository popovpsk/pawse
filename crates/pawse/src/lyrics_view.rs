use std::path::PathBuf;
use std::time::{Duration, Instant};

use audio_engine::EngineEvent;
use gpui::prelude::FluentBuilder;
use gpui::{
    Animation, AnimationExt, AppContext, Context, Entity, FontWeight, Hsla, InteractiveElement,
    IntoElement, ParentElement, Pixels, Render, ScrollHandle, SharedString, Size,
    StatefulInteractiveElement, Styled, Subscription, Task, Window, canvas, div, ease_out_quint,
    px,
};
use gpui_component::{tooltip::Tooltip, v_flex};
use music_library::{StoredLyrics, lyrics_source};

use crate::library_service::{LibraryEvent, LyricsAccess};
use crate::localization::tr;
use crate::lyrics_fill::{self, FillPlan, LineShape};
use crate::panel_header::{
    panel_header, panel_header_actions, panel_header_button, panel_header_segment,
    panel_header_segments,
};
use crate::playback_status::{Phase, StatusChanged};
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
    own_file: Option<PathBuf>,
    is_cue: bool,
    album_id: Option<i64>,
    title: String,
    duration_secs: Option<u64>,
}

struct LoadOutcome {
    variants: Vec<StoredLyrics>,
    is_cue: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Lrclib {
    Unknown,
    NotFound,
    Found,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct SourceSegment {
    source: &'static str,
    available: bool,
}

const SEGMENT_ORDER: [&str; 3] = [
    lyrics_source::LRC,
    lyrics_source::EMBEDDED,
    lyrics_source::LRCLIB,
];

pub struct LyricsView {
    current_track_id: Option<i64>,
    rows: Vec<lyrics_fill::LyricRow>,
    synced: bool,
    source: String,
    variants: Vec<StoredLyrics>,
    lrclib: Lrclib,
    choice: Option<&'static str>,
    segments: Vec<SourceSegment>,
    prefer_lrclib: bool,
    online: bool,
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
    _fetch_task: Option<Task<()>>,
    _subscription: Subscription,
    _status_subscription: Subscription,
    _library_subscription: Subscription,
    _settings_subscription: Subscription,
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
                    EngineEvent::PositionChanged(pos) => this.update_active(*pos, cx),
                    EngineEvent::Playing => this.set_playing(true, cx),
                    EngineEvent::Paused | EngineEvent::TrackEnded | EngineEvent::Error(_) => {
                        this.set_playing(false, cx)
                    }
                    EngineEvent::Stopped => {
                        this.set_playing(false, cx);
                        this.clear(cx)
                    }
                    EngineEvent::Buffering(buffering) => {
                        let playing = cx
                            .global::<Services>()
                            .is_playing
                            .load(std::sync::atomic::Ordering::Relaxed);
                        this.set_playing(playing && !buffering, cx)
                    }
                    EngineEvent::Preparing { .. } | EngineEvent::Loaded { .. } => {}
                },
            );

        let playback_status = cx.global::<Services>().playback_status.clone();
        let status_subscription =
            cx.subscribe(&playback_status, |this, status, _: &StatusChanged, cx| {
                let (track_id, phase, duration) = {
                    let status = status.read(cx);
                    (status.track_id(), status.phase(), status.duration())
                };
                if phase == Phase::Idle || track_id.is_none() {
                    return;
                }
                this.track_duration_ms = duration.map(|d| d.as_millis() as u64);
                if phase == Phase::Preparing {
                    this.set_playing(false, cx);
                }
                if this.current_track_id != track_id {
                    this.load(cx);
                }
            });

        let library_subscription =
            cx.subscribe(&library_event_bus, |this, _, event: &LibraryEvent, cx| {
                if let LibraryEvent::LyricsChanged { track_id } = event
                    && this.current_track_id == Some(*track_id)
                {
                    this.load(cx);
                }
            });

        let settings_subscription =
            cx.observe_global::<SettingsStore>(|this, cx| this.settings_changed(cx));
        let settings = cx.global::<SettingsStore>();
        let prefer_lrclib = settings.lyrics_prefer_lrclib();
        let online = settings.lyrics_from_internet();

        let mut result = Self {
            current_track_id: None,
            rows: Vec::new(),
            synced: false,
            source: String::new(),
            variants: Vec::new(),
            lrclib: Lrclib::Unknown,
            choice: None,
            segments: Vec::new(),
            prefer_lrclib,
            online,
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
            _fetch_task: None,
            _subscription: subscription,
            _status_subscription: status_subscription,
            _library_subscription: library_subscription,
            _settings_subscription: settings_subscription,
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
        } else if visible {
            self.maybe_fetch(cx);
        }
    }

    fn settings_changed(&mut self, cx: &mut Context<Self>) {
        let settings = cx.global::<SettingsStore>();
        let prefer_lrclib = settings.lyrics_prefer_lrclib();
        let online = settings.lyrics_from_internet();
        if prefer_lrclib == self.prefer_lrclib && online == self.online {
            return;
        }
        self.prefer_lrclib = prefer_lrclib;
        self.online = online;
        if self.current_track_id.is_some() && !self.loading {
            self.show_best(cx);
            self.maybe_fetch(cx);
        }
    }

    fn current_context(cx: &mut Context<Self>) -> Option<TrackContext> {
        let track = cx
            .global::<Services>()
            .playback_queue
            .borrow()
            .current_track()
            .cloned()?;
        let own_file = track.own_file().map(PathBuf::from);
        Some(TrackContext {
            id: track.id,
            is_cue: own_file.is_none(),
            own_file,
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
        self.spawn_load(ctx, cx);
        cx.notify();
    }

    fn spawn_load(&mut self, ctx: TrackContext, cx: &mut Context<Self>) {
        let access = self.access.clone();
        self._load_task = Some(cx.spawn(async move |this, cx| {
            let id = ctx.id;
            let is_cue = ctx.is_cue;
            let variants = cx
                .background_spawn(async move { access.variants(id) })
                .await;
            this.update(cx, |this, cx| {
                this.apply_load_outcome(id, LoadOutcome { variants, is_cue }, cx)
            })
            .ok();
        }));
    }

    fn apply_load_outcome(&mut self, track_id: i64, outcome: LoadOutcome, cx: &mut Context<Self>) {
        if self.current_track_id != Some(track_id) {
            return;
        }
        self.loading = false;
        self.is_cue = outcome.is_cue;
        self.set_variants(outcome.variants);
        self.show_best(cx);
        self.maybe_fetch(cx);
    }

    fn set_variants(&mut self, variants: Vec<StoredLyrics>) {
        self.lrclib = match variants.iter().find(|v| v.source == lyrics_source::LRCLIB) {
            Some(v) if v.not_found => Lrclib::NotFound,
            Some(_) => Lrclib::Found,
            None => Lrclib::Unknown,
        };
        self.variants = variants;
        if self.choice.is_some_and(|choice| {
            choice == lyrics_source::LRCLIB && self.lrclib == Lrclib::NotFound
        }) {
            self.choice = None;
        }
    }

    fn show_best(&mut self, cx: &mut Context<Self>) {
        let choices = lyrics_source::choices(&self.variants, self.prefer_lrclib);
        let can_search = self.online && self.lrclib == Lrclib::Unknown;
        self.segments = SEGMENT_ORDER
            .iter()
            .filter_map(|&source| {
                let available = choices.iter().any(|v| v.source == source);
                let searchable = source == lyrics_source::LRCLIB && can_search;
                (available || searchable).then_some(SourceSegment { source, available })
            })
            .collect();
        let picked = lyrics_source::pick(&self.variants, self.prefer_lrclib, self.choice)
            .map(|v| (v.text.clone(), v.source.clone()));
        match picked {
            Some((text, source)) => self.apply_text(&text, &source, cx),
            None if self.lrclib == Lrclib::NotFound && !self.fetching => self.set_not_found(cx),
            None => self.set_empty(cx),
        }
    }

    fn maybe_fetch(&mut self, cx: &mut Context<Self>) {
        if !self.visible || !self.online || self.fetching || self.loading {
            return;
        }
        if self.lrclib != Lrclib::Unknown {
            return;
        }
        let has_local = !lyrics_source::choices(&self.variants, self.prefer_lrclib).is_empty();
        if has_local && !self.prefer_lrclib {
            return;
        }
        if let Some(ctx) = Self::current_context(cx)
            && self.current_track_id == Some(ctx.id)
        {
            self.kick_fetch(ctx, cx);
        }
    }

    fn select_source(&mut self, source: &'static str, cx: &mut Context<Self>) {
        let Some(segment) = self.segments.iter().find(|s| s.source == source).copied() else {
            return;
        };
        self.choice = Some(source);
        if segment.available {
            self.show_best(cx);
            return;
        }
        if self.fetching {
            return;
        }
        if let Some(ctx) = Self::current_context(cx)
            && self.current_track_id == Some(ctx.id)
        {
            self.kick_fetch(ctx, cx);
        }
    }

    fn kick_fetch(&mut self, ctx: TrackContext, cx: &mut Context<Self>) {
        self.fetching = true;
        cx.notify();
        let access = self.access.clone();
        self._fetch_task = Some(cx.spawn(async move |this, cx| {
            let id = ctx.id;
            let refreshed = cx
                .background_spawn(async move {
                    let artist = access.first_artist(id).unwrap_or_default();
                    let album = ctx.album_id.and_then(|aid| access.album_title(aid));
                    let query = lyrics::LyricsQuery {
                        artist,
                        title: ctx.title,
                        album,
                        duration_secs: ctx.duration_secs,
                    };
                    let written = match lyrics::fetch(&query) {
                        Ok(Some(remote)) => match pick_remote(remote) {
                            Some(raw) => access.save(id, &raw, lyrics_source::LRCLIB),
                            None => access.mark_not_found(id),
                        },
                        Ok(None) => access.mark_not_found(id),
                        Err(e) => {
                            log::warn!("lyrics fetch failed for track {}: {}", id, e);
                            false
                        }
                    };
                    written.then(|| access.variants(id))
                })
                .await;
            this.update(cx, |this, cx| {
                if this.current_track_id != Some(id) {
                    return;
                }
                this.fetching = false;
                match refreshed {
                    Some(variants) => this.set_variants(variants),
                    None => {
                        if this.choice == Some(lyrics_source::LRCLIB) {
                            this.choice = None;
                        }
                    }
                }
                this.show_best(cx);
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
        let has_sidecar = self.variants.iter().any(|v| v.source == lyrics_source::LRC);
        self.can_export = !self.rows.is_empty()
            && source == lyrics_source::LRCLIB
            && !self.is_cue
            && !has_sidecar;
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
        self._fetch_task = None;
        self.variants.clear();
        self.lrclib = Lrclib::Unknown;
        self.choice = None;
        self.segments.clear();
    }

    fn set_empty(&mut self, cx: &mut Context<Self>) {
        self.clear_content();
        self.not_found = false;
        self.loading = false;
        cx.notify();
    }

    fn set_not_found(&mut self, cx: &mut Context<Self>) {
        self.clear_content();
        self.not_found = true;
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
        let Some(file) = ctx.own_file else {
            return;
        };
        let Some(raw) = self.current_raw.clone() else {
            return;
        };
        let folders = cx.global::<SettingsStore>().music_folders().to_vec();
        cx.global::<Services>()
            .library
            .save_lyrics_file(ctx.id, file, raw, folders);
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

        let segments = (self.segments.len() > 1).then(|| {
            let shown = self.source.as_str();
            let fetching = self.fetching;
            panel_header_segments(cx).children(self.segments.iter().enumerate().map(
                |(ix, segment)| {
                    let source = segment.source;
                    let (icon, tooltip) = match source {
                        lyrics_source::LRC => {
                            ("icons/lyrics-lrc.svg", tr().lyrics_source_lrc.clone())
                        }
                        lyrics_source::EMBEDDED => {
                            ("icons/lyrics-tag.svg", tr().lyrics_source_tags.clone())
                        }
                        _ if fetching => (
                            "icons/loader-circle.svg",
                            tr().lyrics_searching_lrclib.clone(),
                        ),
                        _ if segment.available => {
                            ("icons/lyrics-web.svg", tr().lyrics_source_lrclib.clone())
                        }
                        _ => ("icons/lyrics-web.svg", tr().lyrics_search_lrclib.clone()),
                    };
                    let spinning = fetching && source == lyrics_source::LRCLIB;
                    panel_header_segment(
                        ("lyrics_source", ix),
                        icon,
                        tooltip,
                        segment.available && source == shown,
                        spinning,
                        cx,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.select_source(source, cx)))
                },
            ))
        });

        let actions = panel_header_actions()
            .children(segments)
            .when(show_sync, |d| {
                d.child(
                    panel_header_button(
                        "lyrics_sync",
                        "icons/locate.svg",
                        tr().lyrics_follow.clone(),
                        cx,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.resync(cx))),
                )
            })
            .when(self.can_export, |d| {
                d.child(
                    panel_header_button(
                        "lyrics_save",
                        "icons/save.svg",
                        tr().lyrics_save.clone(),
                        cx,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.export(cx))),
                )
            });
        let header = panel_header(tr().lyrics.clone(), actions, cx);

        let body = if self.fetching && self.rows.is_empty() {
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

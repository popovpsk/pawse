use std::time::Duration;

use audio_engine::EngineEvent;
use gpui::prelude::FluentBuilder;
use gpui::{
    AppContext, Context, FontWeight, Hsla, InteractiveElement, IntoElement, ParentElement, Render,
    ScrollHandle, SharedString, StatefulInteractiveElement, Styled, Subscription, Window, div, px,
    svg,
};
use gpui_component::scroll::{ScrollableElement, ScrollbarAxis};
use gpui_component::{h_flex, tooltip::Tooltip, v_flex};

use crate::library_service::LibraryEvent;
use crate::localization::tr;
use crate::services::Services;
use crate::settings_store::SettingsStore;
use crate::theme_colors::Colors;

const LYRICS_SOURCE: &str = "lrclib";
const LYRICS_LOOKAHEAD: usize = 2;
const ACTIVE_TOLERANCE_MS: u32 = 50;

pub struct LyricsView {
    current_track_id: Option<i64>,
    lines: Vec<SharedString>,
    times: Vec<Option<u32>>,
    time_labels: Vec<Option<SharedString>>,
    track_duration_ms: Option<u64>,
    synced: bool,
    active_ix: Option<usize>,
    hovered_ix: Option<usize>,
    source_label: SharedString,
    can_export: bool,
    is_cue: bool,
    fetching: bool,
    searched: bool,
    current_raw: Option<(String, bool)>,
    visible: bool,
    scroll_handle: ScrollHandle,
    _subscription: Subscription,
    _library_subscription: Subscription,
}

impl LyricsView {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let services = cx.global::<Services>();
        let engine_event_bus = services.engine_event_bus.clone();
        let library_event_bus = services.library_event_bus.clone();

        let subscription =
            cx.subscribe(
                &engine_event_bus,
                |this, _, event: &EngineEvent, cx| match event {
                    EngineEvent::Loaded { duration, .. } => {
                        this.track_duration_ms = Some(duration.as_millis() as u64);
                        this.load_for_current(cx);
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
                    this.refresh_from_db(cx);
                }
            });

        let mut result = Self {
            current_track_id: None,
            lines: Vec::new(),
            times: Vec::new(),
            time_labels: Vec::new(),
            track_duration_ms: None,
            synced: false,
            active_ix: None,
            hovered_ix: None,
            source_label: tr().lyrics.clone(),
            can_export: false,
            is_cue: false,
            fetching: false,
            searched: false,
            current_raw: None,
            visible: false,
            scroll_handle: ScrollHandle::new(),
            _subscription: subscription,
            _library_subscription: library_subscription,
        };
        result.refresh_from_db(cx);
        result
    }

    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.visible == visible {
            return;
        }
        self.visible = visible;
        if visible && self.lines.is_empty() && !self.fetching && !self.searched {
            self.maybe_fetch(cx);
        }
    }

    fn current_track(cx: &mut Context<Self>) -> Option<music_library::Track> {
        cx.global::<Services>()
            .playback_queue
            .borrow()
            .current_track()
            .cloned()
    }

    fn is_cue_track(track: &music_library::Track, cx: &mut Context<Self>) -> bool {
        track.start_offset_ms != 0
            || cx
                .global::<Services>()
                .library
                .is_multitrack_file(&track.path)
    }

    fn refresh_from_db(&mut self, cx: &mut Context<Self>) {
        let Some(track) = Self::current_track(cx) else {
            self.clear(cx);
            return;
        };
        self.current_track_id = Some(track.id);
        self.is_cue = Self::is_cue_track(&track, cx);
        let stored = cx.global::<Services>().library.lyrics_for_track(track.id);
        match stored {
            Some(stored) => self.apply_text(&stored.text, &stored.source, cx),
            None => {
                self.set_empty(cx);
                if self.visible {
                    self.maybe_fetch(cx);
                }
            }
        }
    }

    fn load_for_current(&mut self, cx: &mut Context<Self>) {
        let Some(track) = Self::current_track(cx) else {
            self.clear(cx);
            return;
        };
        let changed = self.current_track_id != Some(track.id);
        if !changed {
            return;
        }
        self.current_track_id = Some(track.id);
        self.is_cue = Self::is_cue_track(&track, cx);
        self.searched = false;
        let stored = cx.global::<Services>().library.lyrics_for_track(track.id);
        match stored {
            Some(stored) => self.apply_text(&stored.text, &stored.source, cx),
            None => {
                self.set_empty(cx);
                if self.visible {
                    self.maybe_fetch(cx);
                }
            }
        }
    }

    fn maybe_fetch(&mut self, cx: &mut Context<Self>) {
        if !cx.global::<SettingsStore>().lyrics_from_internet() {
            return;
        }
        let Some(track) = Self::current_track(cx) else {
            return;
        };
        let req_id = track.id;
        let artist = cx
            .global::<Services>()
            .library
            .track_artists(track.id)
            .into_iter()
            .next()
            .unwrap_or_default();
        let query = lyrics::LyricsQuery {
            artist,
            title: track.title.clone(),
            album: None,
            duration_secs: track.duration_ms.map(|ms| (ms / 1000) as u64),
        };

        self.fetching = true;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let res = cx
                .background_spawn(async move { lyrics::fetch(&query) })
                .await;
            this.update(cx, |this, cx| this.apply_fetch_result(req_id, res, cx))
                .ok();
        })
        .detach();
    }

    fn apply_fetch_result(
        &mut self,
        req_id: i64,
        res: anyhow::Result<Option<lyrics::RemoteLyrics>>,
        cx: &mut Context<Self>,
    ) {
        if self.current_track_id != Some(req_id) {
            return;
        }
        self.fetching = false;
        self.searched = true;
        let remote = match res {
            Ok(remote) => remote,
            Err(e) => {
                log::warn!("lyrics fetch failed for track {}: {}", req_id, e);
                None
            }
        };
        let text: Option<(String, bool)> = remote.and_then(|r| {
            r.synced
                .filter(|s| !s.trim().is_empty())
                .map(|s| (s, true))
                .or_else(|| r.plain.filter(|s| !s.trim().is_empty()).map(|s| (s, false)))
        });
        match text {
            Some((raw, synced)) => {
                cx.global::<Services>().library.save_lyrics(
                    req_id,
                    raw.clone(),
                    synced,
                    LYRICS_SOURCE,
                );
                self.apply_text(&raw, LYRICS_SOURCE, cx)
            }
            None => self.set_empty(cx),
        }
    }

    fn apply_text(&mut self, raw: &str, source: &str, cx: &mut Context<Self>) {
        let parsed = lyrics::parse_lrc(raw);
        let lines: Vec<SharedString> = parsed
            .lines
            .iter()
            .map(|l| SharedString::from(l.text.clone()))
            .collect();
        let lines_changed = lines != self.lines;
        self.synced = parsed.synced;
        self.lines = lines;
        self.times = parsed.lines.iter().map(|l| l.time_ms).collect();
        self.time_labels = parsed
            .lines
            .iter()
            .map(|l| l.time_ms.map(format_ms))
            .collect();
        self.current_raw = Some((raw.to_string(), parsed.synced));
        self.can_export = !self.lines.is_empty() && source != "lrc" && !self.is_cue;
        self.fetching = false;
        if lines_changed {
            self.active_ix = None;
            self.hovered_ix = None;
            self.scroll_handle.scroll_to_item(0);
        }
        cx.notify();
    }

    fn set_empty(&mut self, cx: &mut Context<Self>) {
        self.lines.clear();
        self.times.clear();
        self.time_labels.clear();
        self.synced = false;
        self.active_ix = None;
        self.hovered_ix = None;
        self.can_export = false;
        self.current_raw = None;
        self.fetching = false;
        cx.notify();
    }

    fn clear(&mut self, cx: &mut Context<Self>) {
        self.current_track_id = None;
        self.track_duration_ms = None;
        self.is_cue = false;
        self.searched = false;
        self.set_empty(cx);
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
        if !self.synced || self.times.is_empty() {
            return;
        }
        let pos_ms = pos.as_millis() as u32 + ACTIVE_TOLERANCE_MS;
        let mut new_active: Option<usize> = None;
        let mut lo = 0usize;
        let mut hi = self.times.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            match self.times[mid] {
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
        if let Some(ix) = new_active {
            self.scroll_handle
                .scroll_to_top_of_item(ix.saturating_sub(LYRICS_LOOKAHEAD));
        }
        cx.notify();
    }

    fn export(&mut self, cx: &mut Context<Self>) {
        if !self.can_export {
            return;
        }
        let Some(track) = Self::current_track(cx) else {
            return;
        };
        let Some((raw, synced)) = self.current_raw.clone() else {
            return;
        };
        // why: don't fake success — can_export clears only when the write lands and source flips to "lrc" via LyricsChanged, so a failed write keeps the button for retry
        cx.global::<Services>().library.save_lyrics_file(
            track.id,
            std::path::PathBuf::from(track.path),
            raw,
            synced,
        );
    }
}

impl Render for LyricsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let foreground = Colors::foreground(cx);
        let muted_foreground = Colors::muted_foreground(cx);
        let primary = Colors::primary(cx);
        let muted = Colors::muted(cx);
        let synced = self.synced;
        let active_ix = self.active_ix;
        let hovered_ix = self.hovered_ix;

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
                    .child(self.source_label.clone()),
            )
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
                                .text_color(muted_foreground),
                        ),
                )
            });

        let body = if self.fetching {
            centered_message(tr().lyrics_fetching.clone(), muted_foreground).into_any_element()
        } else if self.lines.is_empty() {
            let message = if self.searched {
                tr().lyrics_not_found.clone()
            } else {
                tr().lyrics_empty.clone()
            };
            centered_message(message, muted_foreground).into_any_element()
        } else {
            v_flex()
                .relative()
                .flex_1()
                .min_h(px(0.))
                .child(
                    v_flex()
                        .id("lyrics_list")
                        .size_full()
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll_handle)
                        .py_2()
                        .children(self.lines.iter().enumerate().map(|(ix, line)| {
                            let is_active = Some(ix) == active_ix;
                            let color = if synced {
                                if is_active { primary } else { muted_foreground }
                            } else {
                                foreground
                            };
                            let row = div()
                                .w_full()
                                .px_4()
                                .py_1()
                                .text_sm()
                                .text_color(color)
                                .when(is_active, |d| d.font_weight(FontWeight::SEMIBOLD));
                            match (synced, self.times[ix], self.time_labels[ix].clone()) {
                                (true, Some(time_ms), Some(label)) => row
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
                                    .child(line.clone())
                                    .into_any_element(),
                                _ => row.child(line.clone()).into_any_element(),
                            }
                        })),
                )
                .scrollbar(&self.scroll_handle, ScrollbarAxis::Vertical)
                .into_any_element()
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

mod paint;
pub mod settings;

use std::sync::Arc;
use std::time::{Duration, Instant};

use audio_output::SignalTap;
use gpui::{
    AppContext, Bounds, Context, Font, InteractiveElement, IntoElement, MouseMoveEvent,
    ParentElement, Pixels, Render, SharedString, StatefulInteractiveElement, Styled, Subscription,
    Task, Window, canvas, div,
};
use gpui_component::ActiveTheme;
use spectrum::{Analyzer, Settings, ZONES};

use crate::frame_grid;
use crate::services::Services;
use crate::settings_store::{SettingsStore, SpectrumFrameRate, SpectrumStyle};
use crate::theme_colors::Colors;
use paint::{Frame, Palette};

pub const SILHOUETTE_LABELS: f32 = 16.;
pub const WIDE_MAX_WIDTH: f32 = 1_200.;
pub const COVER_FRACTION: f32 = 0.3;

const IDLE_POLL: Duration = Duration::from_millis(250);
const LATENCY_REFRESH: Duration = Duration::from_secs(3);
const ZONE_TEXT_REFRESH: Duration = Duration::from_millis(125);
const HIDDEN_AFTER: Duration = Duration::from_secs(1);
const DEFAULT_POINTS: usize = 256;
const MAX_POINTS: usize = 1_024;

pub struct Block {
    pub gap: f32,
    min: f32,
    share: f32,
    max: f32,
}

impl Block {
    pub fn fit(&self, stack: f32, keep: f32) -> Option<f32> {
        let height = (stack * self.share)
            .clamp(self.min, self.max)
            .min(stack - keep - self.gap);
        (height >= self.min).then_some(height)
    }
}

pub fn block(style: SpectrumStyle) -> Option<Block> {
    match style {
        SpectrumStyle::Cover => None,
        SpectrumStyle::Silhouette => Some(Block {
            gap: 18.,
            min: 40. + SILHOUETTE_LABELS,
            share: 0.11,
            max: 112. + SILHOUETTE_LABELS,
        }),
        SpectrumStyle::Zones => Some(Block {
            gap: 22.,
            min: 110.,
            share: 0.22,
            max: 260.,
        }),
        SpectrumStyle::Analyzer => Some(Block {
            gap: 26.,
            min: 210.,
            share: 0.38,
            max: 420.,
        }),
    }
}

fn release_seconds(style: SpectrumStyle) -> f32 {
    match style {
        SpectrumStyle::Cover => 0.34,
        SpectrumStyle::Silhouette => 0.3,
        SpectrumStyle::Zones => 0.22,
        SpectrumStyle::Analyzer => 0.16,
    }
}

pub struct SpectrumView {
    analyzer: Analyzer,
    style: SpectrumStyle,
    settings: Settings,
    period: Duration,
    rate: u32,
    legend: SharedString,
    tap: Option<Arc<SignalTap>>,
    wanted: bool,
    cover_luma: f32,
    bounds: Bounds<Pixels>,
    hover: Option<f32>,
    readout: Option<(usize, SharedString)>,
    readout_at: Instant,
    zone_text: [SharedString; ZONES.len()],
    zone_text_at: Instant,
    last_tick: Instant,
    painted_at: Instant,
    idle: bool,
    latency_at: Option<Instant>,
    frames: Option<Task<()>>,
    _settings_subscription: Subscription,
}

impl SpectrumView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let settings_subscription =
            cx.observe_global::<SettingsStore>(|this: &mut Self, cx| this.apply_settings(cx));
        let mut view = Self {
            analyzer: Analyzer::new(),
            style: SpectrumStyle::default(),
            settings: Settings::default(),
            period: frame_grid::FAST,
            rate: 0,
            legend: SharedString::default(),
            tap: None,
            wanted: false,
            cover_luma: 1.,
            bounds: Bounds::default(),
            hover: None,
            readout: None,
            readout_at: Instant::now(),
            zone_text: std::array::from_fn(|_| SharedString::new_static("—")),
            zone_text_at: Instant::now(),
            last_tick: Instant::now(),
            painted_at: Instant::now(),
            idle: false,
            latency_at: None,
            frames: None,
            _settings_subscription: settings_subscription,
        };
        view.analyzer.set_points(DEFAULT_POINTS);
        view.apply_settings(cx);
        view
    }

    pub fn set_cover_luma(&mut self, luma: f32) {
        self.cover_luma = luma;
    }

    pub fn set_wanted(&mut self, wanted: bool, cx: &mut Context<Self>) {
        if self.wanted == wanted {
            return;
        }
        self.wanted = wanted;
        if wanted {
            self.last_tick = Instant::now();
            self.painted_at = self.last_tick;
            self.idle = false;
            self.frames = Some(cx.spawn(async move |this, cx| {
                loop {
                    let Ok(wait) = this.update(cx, |view, cx| view.tick(cx)) else {
                        return;
                    };
                    cx.background_executor().timer(wait).await;
                }
            }));
        } else {
            self.frames = None;
            if let Some(tap) = self.tap.take() {
                tap.set_active(false);
            }
            self.analyzer.reset();
            self.hover = None;
            self.readout = None;
        }
    }

    fn refresh_readout(&mut self, now: Instant) {
        let count = self.analyzer.frequencies().len();
        let index = self
            .hover
            .filter(|_| paint::shows_detail(self.style))
            .and_then(|x| {
                paint::hover_index(
                    self.style,
                    f32::from(self.bounds.size.width),
                    f32::from(self.bounds.size.height),
                    x,
                    count,
                )
            });
        let Some(index) = index else {
            self.readout = None;
            return;
        };
        let same = self.readout.as_ref().is_some_and(|(i, _)| *i == index);
        if same && now.duration_since(self.readout_at) < ZONE_TEXT_REFRESH {
            return;
        }
        self.readout_at = now;
        let hz = self.analyzer.frequencies()[index];
        let db = self.analyzer.levels()[index];
        self.readout = Some((index, paint::readout_text(hz, db)));
    }

    fn apply_settings(&mut self, cx: &mut Context<Self>) {
        let store = cx.global::<SettingsStore>();
        let style = store.spectrum_style();
        let settings = Settings {
            smoothing_octaves: store.spectrum_smoothing().octaves(style),
            tilt_db_per_octave: store.spectrum_tilt().db_per_octave(),
            release_seconds: release_seconds(style),
        };
        self.period = match store.spectrum_frame_rate() {
            SpectrumFrameRate::Smooth => frame_grid::FAST,
            SpectrumFrameRate::Efficient => frame_grid::SLOW,
        };
        if style == self.style && settings == self.settings && !self.legend.is_empty() {
            return;
        }
        self.style = style;
        self.settings = settings;
        self.analyzer.set_settings(settings);
        self.refresh_legend();
        cx.notify();
    }

    fn refresh_legend(&mut self) {
        let fft = spectrum::window_len(if self.rate > 0 { self.rate } else { 44_100 });
        let fraction = (1. / self.settings.smoothing_octaves).round() as u32;
        let tilt = self.settings.tilt_db_per_octave;
        let tilt = if tilt > 0. {
            format!("tilt +{tilt} dB/oct")
        } else {
            "tilt off".to_string()
        };
        self.legend = format!("FFT {fft} · 1/{fraction} oct · {tilt} · zones dBFS RMS").into();
    }

    fn sync_tap(&mut self, cx: &mut Context<Self>) {
        let tap = cx.global::<Services>().player.signal_tap();
        let same = match (&tap, &self.tap) {
            (Some(new), Some(old)) => Arc::ptr_eq(new, old),
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        if let Some(old) = self.tap.take() {
            old.set_active(false);
        }
        if let Some(new) = &tap {
            new.set_active(true);
        }
        self.tap = tap;
        self.latency_at = None;
    }

    fn refresh_latency(&mut self, cx: &mut Context<Self>) {
        let Some(tap) = &self.tap else {
            return;
        };
        if self
            .latency_at
            .is_some_and(|at| at.elapsed() < LATENCY_REFRESH)
        {
            return;
        }
        let output = cx.global::<Services>().output.clone();
        if !Arc::ptr_eq(tap, &output.signal_tap()) {
            return;
        }
        self.latency_at = Some(Instant::now());
        cx.background_spawn(async move { output.refresh_tap_latency() })
            .detach();
    }

    fn tick(&mut self, cx: &mut Context<Self>) -> Duration {
        self.sync_tap(cx);
        let width = paint::plot_width(self.style, f32::from(self.bounds.size.width));
        if width > 1. {
            let count = (width / paint::point_step(self.style)) as usize + 1;
            self.analyzer.set_points(count.clamp(2, MAX_POINTS));
        }
        let now = Instant::now();
        let dt = now.duration_since(self.last_tick).as_secs_f32();
        self.last_tick = now;
        let rate = self.tap.as_ref().and_then(|tap| {
            let rate = tap.sample_rate();
            tap.read(self.analyzer.signal_tail_mut(rate))
        });
        if rate.is_none() && self.analyzer.is_settled(paint::shows_detail(self.style)) {
            self.idle = true;
            return IDLE_POLL;
        }
        if std::mem::take(&mut self.idle) {
            self.painted_at = now;
        }
        if now.duration_since(self.painted_at) > HIDDEN_AFTER {
            cx.notify();
            return IDLE_POLL;
        }
        if rate.is_some() {
            self.refresh_latency(cx);
        }
        if let Some(rate) = rate
            && rate != self.rate
        {
            self.rate = rate;
            self.refresh_legend();
        }
        self.analyzer.update(rate, dt);
        if self.style == SpectrumStyle::Analyzer
            && now.duration_since(self.zone_text_at) >= ZONE_TEXT_REFRESH
        {
            self.zone_text_at = now;
            for (z, text) in self.zone_text.iter_mut().enumerate() {
                let level = self.analyzer.zone_level(z);
                *text = if level < -90. {
                    SharedString::new_static("—")
                } else {
                    paint::db_value(level).into()
                };
            }
        }
        self.refresh_readout(now);
        cx.notify();
        let period = if cx.reduce_motion() {
            frame_grid::SLOWEST
        } else {
            self.period
        };
        frame_grid::wait(period, Duration::ZERO)
    }
}

impl Drop for SpectrumView {
    fn drop(&mut self) {
        if let Some(tap) = self.tap.take() {
            tap.set_active(false);
        }
    }
}

impl Render for SpectrumView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette {
            accent: Colors::primary(cx),
            fg: Colors::foreground(cx),
            muted: Colors::muted_foreground(cx),
        };
        let ui_font = window.text_style().font();
        let mono_font = Font {
            family: cx.theme().mono_font_family.clone(),
            ..ui_font.clone()
        };
        let name_font = paint::semibold(&ui_font);
        let measure = cx.entity();
        let entity = cx.entity();
        div()
            .id("spectrum")
            .size_full()
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if !*hovered && this.hover.take().is_some() {
                    this.readout = None;
                    cx.notify();
                }
            }))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                this.hover = Some(f32::from(event.position.x - this.bounds.origin.x));
                this.refresh_readout(Instant::now());
                cx.notify();
            }))
            .child(
                canvas(
                    move |bounds, _, cx| {
                        measure.update(cx, |this, _| {
                            this.bounds = bounds;
                            this.painted_at = Instant::now();
                        });
                    },
                    move |bounds, (), window, cx| {
                        entity.update(cx, |this, cx| {
                            let frame = Frame {
                                analyzer: &this.analyzer,
                                style: this.style,
                                palette,
                                cover_luma: this.cover_luma,
                                hovered: this.hover.is_some(),
                                readout: this.readout.as_ref().map(|(i, text)| (*i, text)),
                                zone_text: &this.zone_text,
                                legend: &this.legend,
                                ui_font: &ui_font,
                                name_font: &name_font,
                                mono_font: &mono_font,
                            };
                            paint::draw(&frame, bounds, window, cx);
                        });
                    },
                )
                .size_full(),
            )
    }
}

use std::time::{Duration, Instant};

use audio_engine::EngineEvent;
use gpui::SharedString;
use gpui::{
    AppContext, Context, Entity, ParentElement, Render, Styled, Subscription, Window, div,
    prelude::FluentBuilder, px,
};
use gpui_component::h_flex;

use crate::theme_colors::Colors;
use ui_components::slider::{Slider, SliderEvent};

use crate::services::Services;
use crate::settings_store::SettingsStore;

const SEEK_RESET: Duration = Duration::from_secs(3);

const SLIDER_MIN_W: f32 = 250.0;
const SLIDER_MAX_W: f32 = 400.0;
// Fixed-width elements outside slider in footer layout:
// now_playing(200) + queue+vol(200) + footer px_4(32) + gaps(32) + slider row px_4(32)
const FOOTER_FIXED_W: f32 = 496.0;
// Time labels (40px each) + gap_3 (12px) on both sides when visible
const LABELS_W: f32 = 104.0;

pub struct TrackProgressSlider {
    duration_secs: f32,
    duration_str: SharedString,
    current_position_secs: f32,
    position_str: SharedString,
    has_track: bool,
    show_labels: bool,
    slider: Entity<Slider>,
    _engine_subscription: Subscription,
    _slider_subscription: Subscription,
    _settings_subscription: Subscription,
    seek_count: u32,
    last_seek_press: Option<Instant>,
    last_seek_dir: i32,
    seek_target_secs: f32,
}

impl Render for TrackProgressSlider {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        let show_labels = self.show_labels;
        let viewport_w = f32::from(window.viewport_size().width);
        let labels_w = if show_labels { LABELS_W } else { 0.0 };
        let slider_w = (viewport_w - FOOTER_FIXED_W - labels_w).clamp(SLIDER_MIN_W, SLIDER_MAX_W);
        let text_secondary = Colors::muted_foreground(cx);
        h_flex()
            .gap_3()
            .items_center()
            .justify_center()
            .w_full()
            .when(show_labels, |b| {
                b.child(
                    div()
                        .w(px(40.))
                        .text_xs()
                        .text_color(text_secondary)
                        .text_right()
                        .child(self.position_str.clone()),
                )
            })
            .child(div().w(px(slider_w)).child(self.slider.clone()))
            .when(show_labels, |b| {
                b.child(
                    div()
                        .w(px(40.))
                        .text_xs()
                        .text_color(text_secondary)
                        .child(self.duration_str.clone()),
                )
            })
    }
}

impl TrackProgressSlider {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let engine_event_bus = cx.global::<Services>().engine_event_bus.clone();
        let slider = cx.new(|cx| {
            Slider::new(cx)
                .default_value(0.0)
                .min(0.0)
                .max(1.0)
                .step(0.001)
                .live_update(false)
                .disabled(true)
        });

        let slider_subscription =
            cx.subscribe(&slider, |this, _, event: &SliderEvent, cx| match event {
                SliderEvent::Change(value) => {
                    let pos = *value * this.duration_secs;
                    this.set_position(pos);
                    let services = cx.global::<Services>();
                    services.engine_manager.seek(*value);
                    cx.notify();
                }
            });

        let subscription =
            cx.subscribe(
                &engine_event_bus,
                |this, _, event: &EngineEvent, cx| match event {
                    EngineEvent::Loaded { duration, .. } => {
                        this.duration_secs = duration.as_secs_f32();
                        this.duration_str = Self::format_time(this.duration_secs).into();
                        this.set_position(0.0);
                        this.has_track = true;
                        this.seek_count = 0;
                        this.seek_target_secs = 0.0;
                        this.last_seek_press = None;
                        this.last_seek_dir = 0;
                        let duration_secs = this.duration_secs;
                        this.slider.update(cx, |slider, cx| {
                            slider.set_value_silent(0.0, cx);
                            slider.set_disabled(false, cx);
                            slider.set_tooltip_formatter(
                                Some(Box::new(move |value| {
                                    Self::format_time(value * duration_secs)
                                })),
                                cx,
                            );
                        });
                        cx.notify();
                    }
                    EngineEvent::PositionChanged(position) => {
                        if !this.has_track || this.slider.read(cx).is_interacting() {
                            return;
                        }
                        let new_position = position.as_secs_f32();
                        this.set_position(new_position);
                        let value = if this.duration_secs > 0.0 {
                            new_position / this.duration_secs
                        } else {
                            0.0
                        };
                        this.slider.update(cx, |slider, cx| {
                            slider.set_value_silent(value, cx);
                        });
                        cx.notify();
                    }
                    EngineEvent::TrackEnded | EngineEvent::Stopped | EngineEvent::Error(_) => {
                        this.has_track = false;
                        this.set_position(0.0);
                        this.duration_secs = 0.0;
                        this.duration_str = "".into();
                        this.slider.update(cx, |slider, cx| {
                            slider.set_value_silent(0.0, cx);
                            slider.set_disabled(true, cx);
                            slider.set_tooltip_formatter(None, cx);
                        });
                        cx.notify();
                    }
                    _ => {}
                },
            );

        let show_labels = cx.global::<SettingsStore>().show_time_labels();
        let settings_subscription = cx.observe_global::<SettingsStore>(|this: &mut Self, cx| {
            let new_val = cx.global::<SettingsStore>().show_time_labels();
            if new_val != this.show_labels {
                this.show_labels = new_val;
                cx.notify();
            }
        });

        let (duration_secs, duration_str, current_position_secs, has_track): (
            f32,
            SharedString,
            f32,
            bool,
        ) = {
            let services = cx.global::<Services>();
            let has_current = services.playback_queue.borrow().current_track().is_some();
            let dur_ms = services
                .current_duration_ms
                .load(std::sync::atomic::Ordering::Relaxed);
            if has_current && dur_ms > 0 {
                let d = dur_ms as f32 / 1000.0;
                let pos = services
                    .current_position_ms
                    .load(std::sync::atomic::Ordering::Relaxed) as f32
                    / 1000.0;
                (d, Self::format_time(d).into(), pos, true)
            } else {
                (0.0, "".into(), 0.0, false)
            }
        };
        if has_track {
            let value = if duration_secs > 0.0 {
                current_position_secs / duration_secs
            } else {
                0.0
            };
            slider.update(cx, |slider, cx| {
                slider.set_value_silent(value, cx);
                slider.set_disabled(false, cx);
                slider.set_tooltip_formatter(
                    Some(Box::new(move |v| Self::format_time(v * duration_secs))),
                    cx,
                );
            });
        }

        Self {
            duration_secs,
            duration_str,
            current_position_secs,
            position_str: Self::format_time(current_position_secs).into(),
            has_track,
            show_labels,
            slider,
            _engine_subscription: subscription,
            _slider_subscription: slider_subscription,
            _settings_subscription: settings_subscription,
            seek_count: 0,
            last_seek_press: None,
            last_seek_dir: 0,
            seek_target_secs: 0.0,
        }
    }

    fn format_time(secs: f32) -> String {
        let mins = (secs / 60.0) as u32;
        let secs = (secs % 60.0) as u32;
        format!("{:02}:{:02}", mins, secs)
    }

    fn set_position(&mut self, secs: f32) {
        self.current_position_secs = secs;
        self.position_str = Self::format_time(secs).into();
    }

    pub fn seek_step(&mut self, dir: i32, cx: &mut Context<Self>) {
        if !self.has_track || self.duration_secs <= 0.0 {
            return;
        }
        let now = Instant::now();
        let within = self
            .last_seek_press
            .is_some_and(|t| now.duration_since(t) < SEEK_RESET);
        // base position: accumulated target while in-window, else live position
        let base = if within {
            self.seek_target_secs
        } else {
            self.current_position_secs
        };
        if within && dir == self.last_seek_dir {
            self.seek_count += 1;
        } else {
            self.seek_count = 1;
        }
        let step = match self.seek_count {
            1..=3 => 5.0,
            4 => 10.0,
            _ => 15.0,
        };
        let target = (base + dir as f32 * step).clamp(0.0, self.duration_secs);
        self.seek_target_secs = target;
        self.last_seek_press = Some(now);
        self.last_seek_dir = dir;
        let frac = (target / self.duration_secs).clamp(0.0, 1.0);
        self.set_position(target);
        self.slider.update(cx, |s, cx| s.set_value_silent(frac, cx));
        cx.global::<Services>().engine_manager.seek(frac);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::TrackProgressSlider;

    #[test]
    fn format_time_minutes_and_seconds() {
        assert_eq!(TrackProgressSlider::format_time(0.0), "00:00");
        assert_eq!(TrackProgressSlider::format_time(59.0), "00:59");
        assert_eq!(TrackProgressSlider::format_time(60.0), "01:00");
        assert_eq!(TrackProgressSlider::format_time(90.0), "01:30");
    }

    #[test]
    fn format_time_truncates_subsecond() {
        assert_eq!(TrackProgressSlider::format_time(59.9), "00:59");
    }

    #[test]
    fn format_time_minutes_exceed_sixty_without_hours() {
        assert_eq!(TrackProgressSlider::format_time(3599.0), "59:59");
        assert_eq!(TrackProgressSlider::format_time(3661.0), "61:01");
    }
}

use audio_engine::EngineEvent;
use gpui::{
    AppContext, Context, Entity, ParentElement, Render, Styled, Subscription, Window, div,
    prelude::FluentBuilder, px,
};
use gpui_component::{ActiveTheme, h_flex};
use ui_components::slider::{Slider, SliderEvent};

use crate::services::Services;
use crate::settings_store::SettingsStore;

const SLIDER_MIN_W: f32 = 250.0;
const SLIDER_MAX_W: f32 = 400.0;
// Fixed-width elements outside slider in footer layout:
// now_playing(200) + queue+vol(200) + footer px_4(32) + gaps(32) + slider row px_4(32)
const FOOTER_FIXED_W: f32 = 496.0;
// Time labels (40px each) + gap_3 (12px) on both sides when visible
const LABELS_W: f32 = 104.0;

pub struct TrackProgressSlider {
    duration_secs: f32,
    current_position_secs: f32,
    has_track: bool,
    show_labels: bool,
    slider: Entity<Slider>,
    _engine_subscription: Subscription,
    _slider_subscription: Subscription,
    _settings_subscription: Subscription,
}

impl Render for TrackProgressSlider {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        let show_labels = self.show_labels;
        let viewport_w = f32::from(window.viewport_size().width);
        let labels_w = if show_labels { LABELS_W } else { 0.0 };
        let slider_w = (viewport_w - FOOTER_FIXED_W - labels_w).clamp(SLIDER_MIN_W, SLIDER_MAX_W);
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
                        .text_color(cx.theme().muted_foreground)
                        .text_right()
                        .child(Self::format_time(self.current_position_secs)),
                )
            })
            .child(div().w(px(slider_w)).child(self.slider.clone()))
            .when(show_labels, |b| {
                b.child(
                    div()
                        .w(px(40.))
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(Self::format_time(self.duration_secs)),
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
                    this.current_position_secs = *value * this.duration_secs;
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
                        this.current_position_secs = 0.0;
                        this.has_track = true;
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
                        this.current_position_secs = new_position;
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
                    EngineEvent::TrackEnded | EngineEvent::Error(_) => {
                        this.has_track = false;
                        this.current_position_secs = 0.0;
                        this.duration_secs = 0.0;
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

        Self {
            duration_secs: 0.0,
            current_position_secs: 0.0,
            has_track: false,
            show_labels,
            slider,
            _engine_subscription: subscription,
            _slider_subscription: slider_subscription,
            _settings_subscription: settings_subscription,
        }
    }

    fn format_time(secs: f32) -> String {
        let mins = (secs / 60.0) as u32;
        let secs = (secs % 60.0) as u32;
        format!("{:02}:{:02}", mins, secs)
    }
}

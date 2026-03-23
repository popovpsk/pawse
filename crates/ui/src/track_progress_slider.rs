use audio_engine::EngineEvent;
use gpui::{
    AppContext, Context, Entity, EventEmitter, ParentElement, Render, Styled, Subscription, Window,
    div,
};
use gpui_component::{
    h_flex,
    slider::{Slider, SliderEvent, SliderState},
};

use crate::services::Services;

pub struct TrackProgressSlider {
    slider_state: Entity<SliderState>,
    duration_secs: f32,
    current_position_secs: f32,
    _subscription: Subscription,
}

impl TrackProgressSlider {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let engine_event_bus = cx.global::<Services>().engine_event_bus.clone();

        let slider_state = cx.new(|_| {
            SliderState::new()
                .min(0.0)
                .max(100.0)
                .default_value(0.0)
                .step(0.1)
        });

        let subscription =
            cx.subscribe(
                &engine_event_bus,
                |this, _, event: &EngineEvent, cx| match event {
                    EngineEvent::Loaded { duration, .. } => {
                        this.duration_secs = duration.as_secs_f32();
                        this.slider_state.update(cx, |state, _cx| {
                            *state = SliderState::new()
                                .min(0.0)
                                .max(this.duration_secs)
                                .step(0.1)
                                .default_value(0.0);
                        });
                        cx.notify();
                    }
                    EngineEvent::PositionChanged(position) => {
                        let new_position = position.as_secs_f32();
                        if new_position != this.current_position_secs {
                            this.current_position_secs = new_position;
                            cx.notify();
                        }
                    }
                    _ => {}
                },
            );

        cx.subscribe(
            &slider_state,
            |this, _, event: &SliderEvent, cx| match event {
                SliderEvent::Change(value) => {
                    let position = value.start();
                    this.current_position_secs = position;

                    let services = cx.global::<Services>();
                    let normalized_position = if this.duration_secs > 0.0 {
                        position / this.duration_secs
                    } else {
                        0.0
                    };

                    services.engine_manager.seek(normalized_position);
                }
            },
        )
        .detach();

        Self {
            slider_state,
            duration_secs: 0.0,
            current_position_secs: 0.0,
            _subscription: subscription,
        }
    }

    fn format_time(secs: f32) -> String {
        let mins = (secs / 60.0) as u32;
        let secs = (secs % 60.0) as u32;
        format!("{:02}:{:02}", mins, secs)
    }
}

impl EventEmitter<SliderEvent> for TrackProgressSlider {}

impl Render for TrackProgressSlider {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        let slider_value = self.slider_state.read(cx).value().start();
        if (slider_value - self.current_position_secs).abs() > 0.01 {
            self.slider_state.update(cx, |state, cx| {
                state.set_value(self.current_position_secs, window, cx);
            });
        }

        h_flex()
            .gap_3()
            .items_center()
            .w_full()
            .child(
                div()
                    .w_20()
                    .child(Self::format_time(self.current_position_secs)),
            )
            .child(h_flex().w_full().child(Slider::new(&self.slider_state)))
            .child(div().w_20().child(Self::format_time(self.duration_secs)))
    }
}

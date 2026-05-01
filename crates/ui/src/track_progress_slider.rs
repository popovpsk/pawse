use audio_engine::EngineEvent;
use gpui::{Context, ParentElement, Render, Styled, Subscription, Window, div, px, relative};
use gpui_component::{ActiveTheme, h_flex};

use crate::services::Services;

pub struct TrackProgressSlider {
    duration_secs: f32,
    current_position_secs: f32,
    _subscription: Subscription,
}

impl TrackProgressSlider {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let engine_event_bus = cx.global::<Services>().engine_event_bus.clone();

        let subscription = cx.subscribe(
            &engine_event_bus,
            |this, _, event: &EngineEvent, cx| match event {
                EngineEvent::Loaded { duration, .. } => {
                    this.duration_secs = duration.as_secs_f32();
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

        Self {
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

impl Render for TrackProgressSlider {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        let pct = if self.duration_secs > 0.0 {
            self.current_position_secs / self.duration_secs
        } else {
            0.0
        };

        h_flex()
            .gap_3()
            .items_center()
            .w_full()
            .child(
                div()
                    .w_20()
                    .child(Self::format_time(self.current_position_secs)),
            )
            .child(
                div()
                    .flex_1()
                    .h(px(4.))
                    .rounded_full()
                    .bg(cx.theme().muted)
                    .child(
                        div()
                            .h_full()
                            .w(relative(pct))
                            .rounded_full()
                            .bg(cx.theme().foreground),
                    ),
            )
            .child(div().w_20().child(Self::format_time(self.duration_secs)))
    }
}

use std::path::PathBuf;

use audio_engine::EngineEvent;
use gpui::{ClickEvent, Context, IntoElement, Render, Styled, Subscription, Window};
use gpui_component::button::Button;

use crate::services::Services;

pub struct PrevButton {
    current_position_secs: f32,
    _subscription: Subscription,
}

impl PrevButton {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let engine_event_bus = cx.global::<Services>().engine_event_bus.clone();

        let subscription = cx.subscribe(
            &engine_event_bus,
            |this, _, event: &EngineEvent, _cx| match event {
                EngineEvent::PositionChanged(position) => {
                    this.current_position_secs = position.as_secs_f32();
                }
                EngineEvent::Loaded { .. } => {
                    this.current_position_secs = 0.0;
                }
                _ => {}
            },
        );

        Self {
            current_position_secs: 0.0,
            _subscription: subscription,
        }
    }

    fn on_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let services = cx.global::<Services>();
        let mut queue = services.playback_queue.borrow_mut();
        match queue.previous(self.current_position_secs) {
            crate::playback_queue::PreviousAction::SeekToStart => {
                services.engine_manager.seek(0.0);
                services.engine_manager.play();
            }
            crate::playback_queue::PreviousAction::PreviousTrack(track) => {
                services.engine_manager.set_track(PathBuf::from(&track.path));
                services.engine_manager.play();
            }
        }
    }
}

impl Render for PrevButton {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        Button::new("prev_button")
            .label("⏮")
            .tooltip("previous")
            .w_9()
            .h_9()
            .rounded_full()
            .on_click(cx.listener(PrevButton::on_click))
    }
}

use audio_engine::EngineEvent;
use gpui::{ClickEvent, Context, IntoElement, Render, Styled, Subscription, Window};
use gpui_component::button::{Button, ButtonVariants};

use crate::services::Services;

struct PlayButtonState {
    is_playing: bool,
}

pub struct PlayButton {
    state: PlayButtonState,
    _subscription: Subscription,
}

impl PlayButton {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let engine_event_bus = cx.global::<Services>().engine_event_bus.clone();

        let subscription =
            cx.subscribe(
                &engine_event_bus,
                |this, _, event: &EngineEvent, cx| match event {
                    EngineEvent::Playing => {
                        this.state.is_playing = true;
                        cx.notify();
                    }
                    EngineEvent::Paused => {
                        this.state.is_playing = false;
                        cx.notify();
                    }
                    EngineEvent::TrackEnded => {
                        this.state.is_playing = false;
                        cx.notify();
                    }
                    _ => {}
                },
            );

        Self {
            state: PlayButtonState { is_playing: false },
            _subscription: subscription,
        }
    }

    fn on_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let services = cx.global::<Services>();
        if self.state.is_playing {
            services.engine_manager.pause();
        } else {
            services.engine_manager.play();
        }
    }
}

impl Render for PlayButton {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let label = if !self.state.is_playing { "▶" } else { "⏸" };

        Button::new("play_button")
            .primary()
            .label(label)
            .w_9()
            .h_9()
            .rounded_full()
            .on_click(cx.listener(PlayButton::on_click))
    }
}

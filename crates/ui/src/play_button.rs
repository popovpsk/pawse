use gpui::{ClickEvent, Context, IntoElement, Render, Styled, Window};
use gpui_component::button::{Button, ButtonVariants};

use crate::services::Services;

struct PlayButtonState {
    is_playing: bool,
}

pub struct PlayButton {
    state: PlayButtonState,
}

impl PlayButton {
    fn on_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let services = cx.global::<Services>();
        if self.state.is_playing {
            services.engine_manager.pause();
        } else {
            services.engine_manager.play();
        }
        self.state.is_playing = !self.state.is_playing;
        cx.notify();
    }
}

impl Render for PlayButton {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let label = {
            if !self.state.is_playing {
                "▶"
            } else {
                "⏸"
            }
        };

        Button::new("play_button")
            .primary()
            .label(label)
            .tooltip("play")
            .w_9()
            .h_9()
            .rounded_full()
            .on_click(cx.listener(PlayButton::on_click))
    }
}

impl PlayButton {
    pub fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self {
            state: PlayButtonState { is_playing: false },
        }
    }
}

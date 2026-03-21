use gpui::{
    AppContext, ClickEvent, Context, Entity, IntoElement, ParentElement, Render, Styled, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex,
};

use crate::services::Services;

pub struct Footer {
    play_button: Entity<PlayButton>,
}

impl Footer {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            play_button: cx.new(|cx| PlayButton::new(window, cx)),
        }
    }
}

impl Render for Footer {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        h_flex().pb_3().child(self.play_button.clone())
    }
}

struct PlayButtonState {
    is_playing: bool,
}

pub struct PlayButton {
    state: PlayButtonState,
}

impl PlayButton {
    fn on_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.state.is_playing = !self.state.is_playing;
        let services = cx.global::<Services>();
        services.audio_engine.play();
        cx.notify();
    }
}

impl Render for PlayButton {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        Button::new("play")
            .primary()
            .label("▶")
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

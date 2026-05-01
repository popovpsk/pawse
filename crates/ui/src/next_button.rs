use std::path::PathBuf;

use gpui::{ClickEvent, Context, IntoElement, Render, Styled, Window};
use gpui_component::button::Button;

use crate::services::Services;

pub struct NextButton;

impl NextButton {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self
    }

    fn on_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let services = cx.global::<Services>();
        let mut queue = services.playback_queue.borrow_mut();
        if let Some(track) = queue.next_track() {
            services.engine_manager.set_track(PathBuf::from(&track.path));
            services.engine_manager.play();
        }
    }
}

impl Render for NextButton {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        Button::new("next_button")
            .label("⏭")
            .tooltip("next")
            .w_9()
            .h_9()
            .rounded_full()
            .on_click(cx.listener(NextButton::on_click))
    }
}

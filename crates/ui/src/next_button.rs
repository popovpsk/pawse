use std::path::PathBuf;

use gpui::{
    ClickEvent, Context, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Window, div, px, svg,
};
use gpui_component::ActiveTheme;

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
            services
                .engine_manager
                .set_track(PathBuf::from(&track.path));
            services.engine_manager.play();
        }
    }
}

impl Render for NextButton {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("next_button")
            .cursor_pointer()
            .size(px(36.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .hover(|style| style.bg(cx.theme().muted))
            .on_click(cx.listener(NextButton::on_click))
            .child(
                svg()
                    .path("icons/next.svg")
                    .size(px(22.))
                    .text_color(cx.theme().foreground),
            )
    }
}

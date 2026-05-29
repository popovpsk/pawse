use gpui::{
    ClickEvent, Context, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Window, div, px, svg,
};
use gpui_component::tooltip::Tooltip;

use crate::localization::tr;
use crate::services::Services;
use crate::theme_colors::Colors;

pub struct NextButton;

impl NextButton {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self
    }

    fn on_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let services = cx.global::<Services>();
        let mut queue = services.playback_queue.borrow_mut();
        if let Some(track) = queue.next_track().cloned() {
            drop(queue);
            services.play_track(&track);
            crate::services::save_playback(cx);
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
            .hover(|style| style.bg(Colors::control_hover_bg(cx)))
            .tooltip(|window, cx| Tooltip::new(tr().next.clone()).build(window, cx))
            .on_click(cx.listener(NextButton::on_click))
            .child(
                svg()
                    .path("icons/next.svg")
                    .size(px(22.))
                    .text_color(Colors::text_primary(cx)),
            )
    }
}

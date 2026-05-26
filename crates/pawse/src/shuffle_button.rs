use gpui::{
    ClickEvent, Context, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Window, div, px, svg,
};
use gpui_component::tooltip::Tooltip;

use crate::services::Services;
use crate::theme_colors::Colors;

pub struct ShuffleButton;

impl ShuffleButton {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self
    }

    fn on_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        {
            let services = cx.global::<Services>();
            let mut queue = services.playback_queue.borrow_mut();
            let new = !queue.shuffle();
            queue.set_shuffle(new);
        }
        crate::services::save_playback(cx);
        cx.notify();
    }
}

impl Render for ShuffleButton {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let enabled = cx.global::<Services>().playback_queue.borrow().shuffle();
        let color = if enabled {
            Colors::text_accent(cx)
        } else {
            Colors::text_secondary(cx)
        };
        div()
            .id("shuffle_button")
            .cursor_pointer()
            .size(px(36.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .hover(|style| style.bg(Colors::control_hover_bg(cx)))
            .tooltip(|window, cx| Tooltip::new("Shuffle").build(window, cx))
            .on_click(cx.listener(ShuffleButton::on_click))
            .child(
                svg()
                    .path("icons/shuffle.svg")
                    .size(px(18.))
                    .text_color(color),
            )
    }
}

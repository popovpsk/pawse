use gpui::{
    ClickEvent, Context, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Transformation, Window, div, px, size, svg,
};
use gpui_component::tooltip::Tooltip;

use crate::localization::tr;
use crate::theme_colors::Colors;

pub struct PrevButton;

impl PrevButton {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self
    }

    fn on_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        crate::services::play_previous(cx);
    }
}

impl Render for PrevButton {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("prev_button")
            .cursor_pointer()
            .size(px(36.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .hover(|style| style.bg(Colors::muted(cx)))
            .tooltip(|window, cx| Tooltip::new(tr().previous.clone()).build(window, cx))
            .on_click(cx.listener(PrevButton::on_click))
            .child(
                svg()
                    .path("icons/next.svg")
                    .size(px(22.))
                    .with_transformation(Transformation::scale(size(-1.0, 1.0)))
                    .text_color(Colors::foreground(cx)),
            )
    }
}

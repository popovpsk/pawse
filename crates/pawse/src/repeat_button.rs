use gpui::{
    ClickEvent, Context, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Window, div, px, svg,
};
use gpui_component::ActiveTheme;

use crate::playback_queue::RepeatMode;
use crate::services::Services;

pub struct RepeatButton;

impl RepeatButton {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self
    }

    fn on_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        {
            let services = cx.global::<Services>();
            let mut queue = services.playback_queue.borrow_mut();
            let next = queue.repeat().cycle();
            queue.set_repeat(next);
        }
        crate::services::save_playback(cx);
        cx.notify();
    }
}

impl Render for RepeatButton {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mode = cx.global::<Services>().playback_queue.borrow().repeat();
        let (icon, color) = match mode {
            RepeatMode::Off => ("icons/repeat.svg", cx.theme().muted_foreground),
            RepeatMode::All => ("icons/repeat.svg", cx.theme().primary),
            RepeatMode::One => ("icons/repeat-one.svg", cx.theme().primary),
        };
        div()
            .id("repeat_button")
            .cursor_pointer()
            .size(px(36.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .hover(|style| style.bg(cx.theme().muted))
            .on_click(cx.listener(RepeatButton::on_click))
            .child(svg().path(icon).size(px(18.)).text_color(color))
    }
}

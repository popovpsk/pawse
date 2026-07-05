use gpui::{
    ClickEvent, Context, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Subscription, Window, div, px, svg,
};
use gpui_component::tooltip::Tooltip;

use crate::library_service::LibraryEvent;
use crate::localization::tr;
use crate::playback_queue::RepeatMode;
use crate::services::Services;
use crate::theme_colors::Colors;

pub struct RepeatButton {
    _subscription: Subscription,
}

impl RepeatButton {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let bus = cx.global::<Services>().library_event_bus.clone();
        let subscription = cx.subscribe(&bus, |_, _, event: &LibraryEvent, cx| {
            if matches!(event, LibraryEvent::PlaybackModeChanged) {
                cx.notify();
            }
        });
        Self {
            _subscription: subscription,
        }
    }

    fn on_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        crate::services::cycle_repeat(cx);
        cx.notify();
    }
}

impl Render for RepeatButton {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mode = cx.global::<Services>().playback_queue.borrow().repeat();
        let (icon, color) = match mode {
            RepeatMode::Off => ("icons/repeat.svg", Colors::muted_foreground(cx)),
            RepeatMode::All => ("icons/repeat.svg", Colors::primary(cx)),
            RepeatMode::One => ("icons/repeat-one.svg", Colors::primary(cx)),
        };
        let tooltip_text = tr().repeat_mode.clone();

        div()
            .id("repeat_button")
            .cursor_pointer()
            .size(px(36.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .hover(|style| style.bg(Colors::muted(cx)))
            .tooltip(move |window, cx| Tooltip::new(tooltip_text.clone()).build(window, cx))
            .on_click(cx.listener(RepeatButton::on_click))
            .child(svg().path(icon).size(px(18.)).text_color(color))
    }
}

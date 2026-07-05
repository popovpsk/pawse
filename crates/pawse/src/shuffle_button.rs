use gpui::{
    ClickEvent, Context, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Subscription, Window, div, px, svg,
};
use gpui_component::tooltip::Tooltip;

use crate::library_service::LibraryEvent;
use crate::localization::tr;
use crate::services::Services;
use crate::theme_colors::Colors;

pub struct ShuffleButton {
    _subscription: Subscription,
}

impl ShuffleButton {
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
        crate::services::toggle_shuffle(cx);
        cx.notify();
    }
}

impl Render for ShuffleButton {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let enabled = cx.global::<Services>().playback_queue.borrow().shuffle();
        let color = if enabled {
            Colors::primary(cx)
        } else {
            Colors::muted_foreground(cx)
        };
        div()
            .id("shuffle_button")
            .cursor_pointer()
            .size(px(36.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .hover(|style| style.bg(Colors::muted(cx)))
            .tooltip(|window, cx| Tooltip::new(tr().shuffle.clone()).build(window, cx))
            .on_click(cx.listener(ShuffleButton::on_click))
            .child(
                svg()
                    .path("icons/shuffle.svg")
                    .size(px(18.))
                    .text_color(color),
            )
    }
}

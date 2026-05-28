use gpui::{App, Div, ParentElement, SharedString, Styled, div, px};

use crate::theme_colors::Colors;

pub fn track_duration(cx: &App, duration: SharedString) -> Div {
    div()
        .flex_shrink_0()
        .size(px(40.))
        .flex()
        .items_center()
        .justify_center()
        .text_sm()
        .text_color(Colors::text_secondary(cx))
        .child(duration.clone())
}

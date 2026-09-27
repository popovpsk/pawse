use gpui::{
    App, Div, FontWeight, InteractiveElement, ParentElement, SharedString, Stateful,
    StatefulInteractiveElement, Styled, div, px, rems, svg,
};
use gpui_component::{h_flex, tooltip::Tooltip};

use crate::theme_colors::Colors;

const HEADER_HEIGHT: f32 = 40.;
const BUTTON_SIZE: f32 = 28.;
const ICON_SIZE: f32 = 16.;

pub fn panel_header(title: SharedString, actions: Div, cx: &App) -> Div {
    h_flex()
        .w_full()
        .h(px(HEADER_HEIGHT))
        .flex_shrink_0()
        .px_4()
        .items_center()
        .justify_between()
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(Colors::foreground(cx))
                .child(title),
        )
        .child(actions)
}

pub fn panel_header_actions() -> Div {
    h_flex().items_center().gap_1()
}

pub fn panel_header_button(
    id: &'static str,
    icon: &'static str,
    tooltip: SharedString,
    cx: &App,
) -> Stateful<Div> {
    let hover_bg = Colors::muted(cx);
    div()
        .id(id)
        .flex_shrink_0()
        .size(rems(BUTTON_SIZE / 16.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor_pointer()
        .hover(move |s| s.bg(hover_bg))
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        .child(
            svg()
                .path(icon)
                .size(rems(ICON_SIZE / 16.))
                .text_color(Colors::muted_foreground(cx)),
        )
}

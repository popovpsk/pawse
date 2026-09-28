use std::time::Duration;

use gpui::prelude::FluentBuilder;
use gpui::{
    Animation, AnimationExt, App, Div, ElementId, FontWeight, InteractiveElement, ParentElement,
    SharedString, Stateful, StatefulInteractiveElement, Styled, Transformation, div, percentage,
    px, rems, svg,
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

pub fn panel_header_segments(cx: &App) -> Div {
    h_flex()
        .flex_shrink_0()
        .items_center()
        .mr_1()
        .rounded_full()
        .border_1()
        .border_color(Colors::border(cx))
}

pub fn panel_header_segment(
    id: impl Into<ElementId>,
    icon: &'static str,
    tooltip: SharedString,
    selected: bool,
    spinning: bool,
    cx: &App,
) -> Stateful<Div> {
    let hover_bg = Colors::muted(cx);
    let icon_color = if selected {
        Colors::foreground(cx)
    } else {
        Colors::muted_foreground(cx)
    };
    let icon = svg()
        .path(icon)
        .size(rems(ICON_SIZE / 16.))
        .text_color(icon_color);
    let id = id.into();
    div()
        .id(id.clone())
        .flex_shrink_0()
        .size(rems(BUTTON_SIZE / 16.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor_pointer()
        .when(selected, |d| d.bg(hover_bg))
        .hover(move |s| s.bg(hover_bg))
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        .map(|d| {
            if spinning {
                d.child(icon.with_animation(
                    id,
                    Animation::new(Duration::from_secs(1)).repeat(),
                    |icon, delta| {
                        icon.with_transformation(Transformation::rotate(percentage(delta)))
                    },
                ))
            } else {
                d.child(icon)
            }
        })
}

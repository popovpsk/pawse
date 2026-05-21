use gpui::prelude::FluentBuilder;
use gpui::{
    App, ElementId, InteractiveElement, IntoElement, MouseButton, ParentElement,
    StatefulInteractiveElement, Styled, div, px, svg,
};
use gpui_component::ActiveTheme;

use crate::services::Services;

/// Group name used by track rows so the like-button can reveal itself on row hover.
/// Apply `.group(LIKE_ROW_GROUP)` to the row container and the outline heart inside
/// the row will fade in via `.group_hover(...)`.
pub const LIKE_ROW_GROUP: &str = "pawse-track-row";

pub const LIKE_BUTTON_SIZE: f32 = 26.;

pub fn like_button(track_id: i64, liked: bool, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let muted_bg = theme.muted;
    let icon_color = if liked {
        theme.primary
    } else {
        theme.muted_foreground
    };
    let icon_path = if liked {
        "icons/s1-heart-fill.svg"
    } else {
        "icons/s1-heart.svg"
    };

    div()
        .id(ElementId::Name(format!("like-{}", track_id).into()))
        .size(px(LIKE_BUTTON_SIZE))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor_pointer()
        .when(!liked, |d| {
            d.opacity(0.).group_hover(LIKE_ROW_GROUP, |s| s.opacity(1.))
        })
        .hover(|s| s.bg(muted_bg))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            let services = cx.global::<Services>();
            services.library.set_liked(track_id, !liked);
        })
        .child(svg().path(icon_path).size(px(15.)).text_color(icon_color))
}

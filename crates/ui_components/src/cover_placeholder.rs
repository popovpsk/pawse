use gpui::{Hsla, IntoElement, ParentElement, Styled, div, px, svg};

pub fn cover_placeholder(size: f32, radius: f32, bg: Hsla, fg: Hsla) -> impl IntoElement {
    div()
        .w(px(size))
        .h(px(size))
        .rounded(px(radius))
        .bg(bg)
        .flex()
        .items_center()
        .justify_center()
        .child(
            svg()
                .path("icons/placeholder-notes.svg")
                .size(px(size * 0.52))
                .text_color(fg),
        )
}

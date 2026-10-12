use gpui::{Hsla, IntoElement, ParentElement, Styled, div, px, svg};

pub fn cover_placeholder(size: f32, radius: f32, bg: Hsla, fg: Hsla) -> impl IntoElement {
    placeholder("icons/placeholder-notes.svg", 0.52, size, radius, bg, fg)
}

pub fn artist_placeholder(size: f32, radius: f32, bg: Hsla, fg: Hsla) -> impl IntoElement {
    placeholder("icons/s1-artists.svg", 0.6, size, radius, bg, fg)
}

fn placeholder(
    icon: &'static str,
    scale: f32,
    size: f32,
    radius: f32,
    bg: Hsla,
    fg: Hsla,
) -> impl IntoElement {
    div()
        .flex_shrink_0()
        .w(px(size))
        .h(px(size))
        .rounded(px(radius))
        .bg(bg)
        .flex()
        .items_center()
        .justify_center()
        .child(svg().path(icon).size(px(size * scale)).text_color(fg))
}

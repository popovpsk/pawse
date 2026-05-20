use gpui::{Hsla, IntoElement, Styled, div, linear_color_stop, linear_gradient, px};

#[derive(Clone, Copy)]
pub enum FadeEdge {
    Top,
    Bottom,
}

/// Gradient overlay pinned to the top or bottom edge of a `relative`-positioned
/// container. Opaque at the edge, transparent on the inside — hides content
/// scrolling under the header or footer.
pub fn fade_overlay(
    edge: FadeEdge,
    color: Hsla,
    height_px: f32,
    offset_px: f32,
) -> impl IntoElement {
    let mut d = div()
        .absolute()
        .left(px(0.))
        .right(px(0.))
        .h(px(height_px))
        .bg(linear_gradient(
            match edge {
                FadeEdge::Top => 180.0,
                FadeEdge::Bottom => 0.0,
            },
            linear_color_stop(color, 0.0),
            linear_color_stop(color.opacity(0.0), 1.0),
        ));
    d = match edge {
        FadeEdge::Top => d.top(px(offset_px)),
        FadeEdge::Bottom => d.bottom(px(offset_px)),
    };
    d
}

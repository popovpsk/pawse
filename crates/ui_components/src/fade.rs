use gpui::{Hsla, IntoElement, Styled, div, linear_color_stop, linear_gradient, px};

#[derive(Clone, Copy)]
pub enum FadeEdge {
    Top,
    Bottom,
    Left,
    Right,
}

/// Gradient overlay pinned to one edge of a `relative`-positioned container.
/// Opaque at the edge, transparent on the inside — hides content scrolling
/// or overflowing under the edge. `size_px` is the band's perpendicular
/// dimension (height for Top/Bottom, width for Left/Right); `offset_px`
/// shifts it inward from the corresponding edge.
pub fn fade_overlay(edge: FadeEdge, color: Hsla, size_px: f32, offset_px: f32) -> impl IntoElement {
    let angle = match edge {
        FadeEdge::Top => 180.0,
        FadeEdge::Bottom => 0.0,
        FadeEdge::Left => 90.0,
        FadeEdge::Right => 270.0,
    };
    let base = div().absolute().bg(linear_gradient(
        angle,
        linear_color_stop(color, 0.0),
        linear_color_stop(color.opacity(0.0), 1.0),
    ));
    match edge {
        FadeEdge::Top => base
            .left(px(0.))
            .right(px(0.))
            .h(px(size_px))
            .top(px(offset_px)),
        FadeEdge::Bottom => base
            .left(px(0.))
            .right(px(0.))
            .h(px(size_px))
            .bottom(px(offset_px)),
        FadeEdge::Left => base
            .top(px(0.))
            .bottom(px(0.))
            .w(px(size_px))
            .left(px(offset_px)),
        FadeEdge::Right => base
            .top(px(0.))
            .bottom(px(0.))
            .w(px(size_px))
            .right(px(offset_px)),
    }
}

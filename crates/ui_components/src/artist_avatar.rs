use std::sync::Arc;

use gpui::{
    AnyElement, Hsla, Image, IntoElement, ObjectFit, ParentElement, Styled, StyledImage, div, img,
    px,
};

use crate::cover_placeholder::cover_placeholder;

const RADIUS: f32 = 4.;

// (left_offset, top_offset, size_scale) for layers front→back.
// Layer 0 = front (oldest album), layer 1 = middle, layer 2 = back (newest).
const LAYERS: [(f32, f32, f32); 3] = [(0., 0., 1.0), (14., 3., 0.88), (28., 6., 0.78)];

/// Stacked cover art composite for an artist row.
///
/// `covers` is oldest-first (up to 3). The oldest cover is painted on top.
/// The container is 56 px wide so back layers can peek to the right.
pub fn artist_avatar(covers: &[Arc<Image>], size: f32, bg: Hsla, fg: Hsla) -> AnyElement {
    if covers.is_empty() {
        return div()
            .w(px(64.))
            .h(px(size))
            .flex()
            .items_center()
            .child(cover_placeholder(size, RADIUS, bg, fg))
            .into_any_element();
    }

    let n = covers.len().min(3);

    // Build layers back-to-front so that earlier children (back) are painted
    // under later children (front). Covers are oldest-first, so we iterate in
    // reverse to add the back layer first.
    let layers: Vec<AnyElement> = (0..n)
        .rev()
        .map(|i| {
            let (left, top, scale) = LAYERS[i];
            let img_size = size * scale;
            img(covers[i].clone())
                .w(px(img_size))
                .h(px(img_size))
                .rounded(px(RADIUS))
                .object_fit(ObjectFit::Cover)
                .border_2()
                .border_color(bg)
                .absolute()
                .left(px(left))
                .top(px(top))
                .into_any_element()
        })
        .collect();

    div()
        .relative()
        .w(px(64.))
        .h(px(size))
        .children(layers)
        .into_any_element()
}

use std::sync::Arc;

use gpui::{
    AnyElement, Hsla, Image, IntoElement, ObjectFit, RenderImage, Styled, StyledImage, img, px,
};

use crate::cover_placeholder::cover_placeholder;

pub fn cover_thumb(
    cover: Option<&Arc<Image>>,
    size: f32,
    radius: f32,
    bg: Hsla,
    fg: Hsla,
) -> AnyElement {
    match cover {
        Some(c) => img(c.clone())
            .w(px(size))
            .h(px(size))
            .rounded(px(radius))
            .object_fit(ObjectFit::Cover)
            .with_fallback(move || cover_placeholder(size, radius, bg, fg).into_any_element())
            .into_any_element(),
        None => cover_placeholder(size, radius, bg, fg).into_any_element(),
    }
}

pub fn cover_tile(
    cover: Option<&Arc<RenderImage>>,
    size: f32,
    radius: f32,
    bg: Hsla,
    fg: Hsla,
) -> AnyElement {
    match cover {
        Some(c) => img(c.clone())
            .w(px(size))
            .h(px(size))
            .rounded(px(radius))
            .object_fit(ObjectFit::Cover)
            .into_any_element(),
        None => cover_placeholder(size, radius, bg, fg).into_any_element(),
    }
}

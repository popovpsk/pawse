use std::sync::Arc;

use gpui::{
    AnyElement, Context, ElementId, Hsla, InteractiveElement, IntoElement, ObjectFit,
    ParentElement, RenderImage, StatefulInteractiveElement, Styled, StyledImage, div, img,
    prelude::FluentBuilder, px,
};
use gpui_component::{h_flex, v_flex};
use ui_components::cover_thumb::{artist_tile, cover_tile};

use crate::artist_card::ArtistCards;
use crate::cover_art_cache::CoverArtCache;
use crate::library_views::artists_view::{ArtistSelectedEvent, ArtistsView};
use crate::library_views::cover_grid::{
    self, GRID_PAD_X, TILE_GAP, TILE_PAD, TILE_RADIUS, TILE_TEXT_GAP, caption_heights,
};
use crate::services::Services;

pub(super) const FLOW_COVERS: usize = 3;
const FLOW_FRONT: f32 = 0.68;
const FLOW_SIDE: f32 = 0.58;
const FLOW_SIDE_OPACITY: f32 = 0.55;
const FLOW_PAIR_PEEK: f32 = 0.24;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Flow {
    front: f32,
    front_left: f32,
    side: f32,
    side_top: f32,
    second_left: f32,
    third_left: f32,
}

pub(super) fn flow(cover_size: f32, covers: usize) -> Flow {
    let front = cover_size * FLOW_FRONT;
    let side = cover_size * FLOW_SIDE;
    let (front_left, second_left) = if covers == 2 {
        let group = front + cover_size * FLOW_PAIR_PEEK;
        let left = (cover_size - group) / 2.;
        (left, left + group - side)
    } else {
        ((cover_size - front) / 2., 0.)
    };
    Flow {
        front,
        front_left,
        side,
        side_top: (front - side) / 2.,
        second_left,
        third_left: cover_size - side,
    }
}

fn cover_size(tile_width: f32) -> f32 {
    (tile_width - 2. * TILE_PAD).max(1.)
}

fn picture_height(cover_size: f32, photos: bool) -> f32 {
    if photos {
        cover_size
    } else {
        cover_size * FLOW_FRONT
    }
}

pub(super) fn row_height(tile_width: f32, rem: f32, photos: bool) -> f32 {
    let size = cover_size(tile_width);
    cover_grid::row_height(tile_width, rem, true) - (size - picture_height(size, photos))
}

pub(super) struct TileParams {
    tile_width: f32,
    cover_size: f32,
    title_height: f32,
    subtitle_height: f32,
    list_hover: Hsla,
    muted: Hsla,
    muted_fg: Hsla,
}

impl TileParams {
    pub(super) fn new(
        tile_width: f32,
        rem: f32,
        list_hover: Hsla,
        muted: Hsla,
        muted_fg: Hsla,
    ) -> Self {
        let (title_height, subtitle_height) = caption_heights(rem);
        Self {
            tile_width,
            cover_size: cover_size(tile_width),
            title_height,
            subtitle_height,
            list_hover,
            muted,
            muted_fg,
        }
    }
}

pub(super) fn grid_strip(
    view: &ArtistsView,
    start: usize,
    end: usize,
    p: &TileParams,
    cx: &mut Context<ArtistsView>,
) -> AnyElement {
    let end = end.min(view.rows.len());
    let pictures: Vec<AnyElement> = {
        let mut cache = cx.global::<Services>().cover_art_cache.borrow_mut();
        let cards = view.cards.read(cx);
        (start..end)
            .map(|ix| tile_picture(view, ix, p, &mut cache, cards))
            .collect()
    };
    let mut strip = h_flex()
        .w_full()
        .px(px(GRID_PAD_X))
        .gap(px(TILE_GAP))
        .items_start();
    for (ix, picture) in (start..end).zip(pictures) {
        strip = strip.child(grid_tile(view, ix, picture, p, cx));
    }
    strip.into_any_element()
}

fn tile_picture(
    view: &ArtistsView,
    ix: usize,
    p: &TileParams,
    cache: &mut CoverArtCache,
    cards: &ArtistCards,
) -> AnyElement {
    let row = &view.rows[ix];
    if view.show_photos {
        let photo = if row.is_no_metadata() {
            None
        } else {
            cards.photo(&row.summary.name)
        };
        return artist_tile(
            photo.as_ref(),
            p.cover_size,
            TILE_RADIUS,
            p.muted,
            p.muted_fg,
        );
    }
    let covers = &row.tile_covers;
    let f = flow(p.cover_size, covers.len());
    let mut layer = |id: i64, left: f32, top: f32, size: f32, opacity: f32| {
        flow_layer(
            cache.peek_large(Some(id)),
            left,
            top,
            size,
            opacity,
            p.muted,
        )
    };
    let right = covers
        .get(2)
        .map(|&id| layer(id, f.third_left, f.side_top, f.side, FLOW_SIDE_OPACITY));
    let left = covers
        .get(1)
        .map(|&id| layer(id, f.second_left, f.side_top, f.side, FLOW_SIDE_OPACITY));
    let front = match covers.first() {
        Some(&id) => layer(id, f.front_left, 0., f.front, 1.),
        None => div()
            .absolute()
            .left(px(f.front_left))
            .child(cover_tile(None, f.front, TILE_RADIUS, p.muted, p.muted_fg))
            .into_any_element(),
    };
    div()
        .relative()
        .flex_shrink_0()
        .w(px(p.cover_size))
        .h(px(f.front))
        .children(right)
        .children(left)
        .child(front)
        .into_any_element()
}

fn flow_layer(
    image: Option<Arc<RenderImage>>,
    left: f32,
    top: f32,
    size: f32,
    opacity: f32,
    bg: Hsla,
) -> AnyElement {
    match image {
        Some(image) => place(
            img(image).object_fit(ObjectFit::Cover),
            left,
            top,
            size,
            opacity,
        )
        .into_any_element(),
        None => place(div().bg(bg), left, top, size, opacity).into_any_element(),
    }
}

fn place<E: Styled>(element: E, left: f32, top: f32, size: f32, opacity: f32) -> E {
    element
        .absolute()
        .left(px(left))
        .top(px(top))
        .w(px(size))
        .h(px(size))
        .rounded(px(TILE_RADIUS))
        .shadow_lg()
        .opacity(opacity)
}

fn grid_tile(
    view: &ArtistsView,
    ix: usize,
    picture: AnyElement,
    p: &TileParams,
    cx: &mut Context<ArtistsView>,
) -> AnyElement {
    let row = &view.rows[ix];
    v_flex()
        .w(px(p.tile_width))
        .flex_shrink_0()
        .p(px(TILE_PAD))
        .gap(px(TILE_TEXT_GAP))
        .rounded(px(TILE_RADIUS + TILE_PAD))
        .hover(|style| style.bg(p.list_hover))
        .when(!view.show_photos, |tile| tile.text_center())
        .child(picture)
        .child(
            div()
                .w_full()
                .h(px(p.title_height))
                .line_height(px(p.title_height))
                .overflow_hidden()
                .text_ellipsis()
                .text_sm()
                .child(row.name.clone()),
        )
        .child(
            div()
                .w_full()
                .h(px(p.subtitle_height))
                .line_height(px(p.subtitle_height))
                .overflow_hidden()
                .text_ellipsis()
                .text_xs()
                .text_color(p.muted_fg)
                .child(row.count_label.clone()),
        )
        .id(ElementId::Integer(row.summary.id as u64))
        .on_click(cx.listener(move |this, _, _, cx| {
            if let Some(row) = this.rows.get(ix) {
                cx.emit(ArtistSelectedEvent {
                    artist: row.summary.clone(),
                });
            }
        }))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_covers_flow_from_the_centre_and_show_both_sides() {
        for size in [60., 134., 150., 300.] {
            let f = flow(size, 3);
            assert!(
                (f.front_left - (size - f.front - f.front_left)).abs() < 1e-3,
                "{size}: the front is centred"
            );
            assert!(
                f.second_left >= 0. && f.second_left < f.front_left,
                "{size}"
            );
            assert!(f.third_left + f.side <= size + 1e-3, "{size}");
            assert!(
                f.third_left + f.side > f.front_left + f.front,
                "{size}: the right cover is hidden"
            );
            assert!(f.side_top >= 0. && f.side_top + f.side <= f.front, "{size}");
        }
    }

    #[test]
    fn a_pair_is_centred_as_a_group_with_the_second_cover_on_the_right() {
        for size in [60., 134., 150., 300.] {
            let f = flow(size, 2);
            let right_edge = f.second_left + f.side;
            assert!((f.front_left - (size - right_edge)).abs() < 1e-3, "{size}");
            assert!(f.front_left >= 0., "{size}");
            assert!(
                (right_edge - (f.front_left + f.front) - size * FLOW_PAIR_PEEK).abs() < 1e-3,
                "{size}: the second cover peeks out by the pair's own margin"
            );
        }
    }

    #[test]
    fn a_single_cover_is_centred() {
        let f = flow(150., 1);
        assert!((f.front_left * 2. + f.front - 150.).abs() < 1e-3);
    }

    #[test]
    fn a_flow_row_is_shorter_than_a_photo_row_by_the_trimmed_picture() {
        let tile = 166.;
        let size = cover_size(tile);
        let trimmed = row_height(tile, 16., true) - row_height(tile, 16., false);
        assert!(
            (trimmed - (size - flow(size, 3).front)).abs() < 1e-3,
            "{trimmed}"
        );
        assert!(
            (row_height(tile, 16., true) - cover_grid::row_height(tile, 16., true)).abs() < 1e-3
        );
    }
}

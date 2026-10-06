use std::sync::Arc;

use gpui::{
    AnyElement, Context, ElementId, Hsla, InteractiveElement, IntoElement, ParentElement,
    RenderImage, StatefulInteractiveElement, Styled, div, prelude::FluentBuilder, px,
};
use gpui_component::{h_flex, v_flex};
use ui_components::cover_thumb::cover_tile;

use crate::library_views::albums_view::{AlbumSelectedEvent, AlbumsView};
use crate::library_views::cover_grid::{
    GRID_PAD_X, TILE_GAP, TILE_PAD, TILE_RADIUS, TILE_TEXT_GAP, caption_heights,
};
use crate::services::Services;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum TileSubtitle {
    ArtistYear,
    Artist,
    Year,
    None,
}

impl TileSubtitle {
    pub(super) fn new(show_artist: bool, show_year: bool) -> Self {
        match (show_artist, show_year) {
            (true, true) => TileSubtitle::ArtistYear,
            (true, false) => TileSubtitle::Artist,
            (false, true) => TileSubtitle::Year,
            (false, false) => TileSubtitle::None,
        }
    }
}

pub(super) struct TileParams {
    tile_width: f32,
    cover_size: f32,
    title_height: f32,
    subtitle_height: f32,
    list_hover: Hsla,
    muted: Hsla,
    muted_fg: Hsla,
    subtitle: TileSubtitle,
}

impl TileParams {
    pub(super) fn new(
        tile_width: f32,
        rem: f32,
        subtitle: TileSubtitle,
        list_hover: Hsla,
        muted: Hsla,
        muted_fg: Hsla,
    ) -> Self {
        let (title_height, subtitle_height) = caption_heights(rem);
        Self {
            tile_width,
            cover_size: (tile_width - 2. * TILE_PAD).max(1.),
            title_height,
            subtitle_height,
            list_hover,
            muted,
            muted_fg,
            subtitle,
        }
    }
}

pub(super) fn grid_strip(
    view: &mut AlbumsView,
    start: usize,
    end: usize,
    p: &TileParams,
    cx: &mut Context<AlbumsView>,
) -> AnyElement {
    let end = end.min(view.row_data.len());
    let cache = cx.global::<Services>().cover_art_cache.clone();
    let mut cache = cache.borrow_mut();
    let mut row = h_flex()
        .w_full()
        .px(px(GRID_PAD_X))
        .gap(px(TILE_GAP))
        .items_start();
    for ix in start..end {
        let cover = cache.peek_large(view.row_data[ix].cover_art_id);
        row = row.child(grid_tile(view, ix, cover.as_ref(), p, cx));
    }
    row.into_any_element()
}

fn grid_tile(
    view: &AlbumsView,
    ix: usize,
    cover: Option<&Arc<RenderImage>>,
    p: &TileParams,
    cx: &mut Context<AlbumsView>,
) -> AnyElement {
    let row = &view.row_data[ix];
    let albums_all_ix = row.albums_all_ix;

    let tile = v_flex()
        .w(px(p.tile_width))
        .flex_shrink_0()
        .p(px(TILE_PAD))
        .gap(px(TILE_TEXT_GAP))
        .rounded(px(TILE_RADIUS + TILE_PAD))
        .hover(|style| style.bg(p.list_hover))
        .child(cover_tile(
            cover,
            p.cover_size,
            TILE_RADIUS,
            p.muted,
            p.muted_fg,
        ))
        .child(
            div()
                .w_full()
                .h(px(p.title_height))
                .line_height(px(p.title_height))
                .overflow_hidden()
                .text_ellipsis()
                .text_sm()
                .child(row.title.clone()),
        );

    let subtitle = match p.subtitle {
        TileSubtitle::ArtistYear => Some(row.subtitle_year.clone()),
        TileSubtitle::Artist => Some(row.artist.clone()),
        TileSubtitle::Year => Some(row.year.clone()),
        TileSubtitle::None => None,
    };
    tile.when_some(subtitle, |tile, subtitle| {
        tile.child(
            div()
                .w_full()
                .h(px(p.subtitle_height))
                .line_height(px(p.subtitle_height))
                .overflow_hidden()
                .text_ellipsis()
                .text_xs()
                .text_color(p.muted_fg)
                .child(subtitle),
        )
    })
    .id(ElementId::Integer(row.id as u64))
    .on_click(cx.listener(move |this, _, _, cx| {
        cx.emit(AlbumSelectedEvent {
            album: this.albums_all[albums_all_ix].clone(),
        });
    }))
    .into_any_element()
}

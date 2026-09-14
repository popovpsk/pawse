use std::sync::Arc;

use gpui::{
    AnyElement, Context, Div, ElementId, Hsla, InteractiveElement, IntoElement, ParentElement,
    RenderImage, StatefulInteractiveElement, Styled, canvas, div, px,
};
use gpui_component::{
    h_flex,
    scroll::{ScrollableElement, ScrollbarAxis},
    v_flex, v_virtual_list,
};
use ui_components::cover_thumb::cover_tile;

use crate::library_views::albums_view::{AlbumSelectedEvent, AlbumsView, TOP_PADDING};
use crate::services::Services;
use crate::settings_store::SettingsStore;
use crate::theme_colors::Colors;

const GRID_PAD_X: f32 = 16.;
const TILE_MIN_WIDTH: f32 = 150.;
const TILE_GAP: f32 = 16.;
const TILE_PAD: f32 = 8.;
const TILE_TEXT_GAP: f32 = 4.;
const TILE_RADIUS: f32 = 6.;
const ROW_GAP: f32 = 8.;
const TEXT_SM_REMS: f32 = 0.875;
const TEXT_XS_REMS: f32 = 0.75;
const CAPTION_LINE_HEIGHT: f32 = 1.5;

struct TileParams {
    columns: usize,
    tile_width: f32,
    cover_size: f32,
    title_height: f32,
    subtitle_height: f32,
    list_hover: Hsla,
    muted: Hsla,
    muted_fg: Hsla,
    show_year: bool,
}

pub(super) fn grid_metrics(width: f32) -> (usize, f32) {
    let avail = (width - 2. * GRID_PAD_X).max(1.);
    let columns = (((avail + TILE_GAP) / (TILE_MIN_WIDTH + TILE_GAP)).floor() as usize).max(1);
    let tile_width = ((avail - (columns - 1) as f32 * TILE_GAP) / columns as f32).max(1.);
    (columns, tile_width)
}

pub(super) fn caption_heights(rem: f32) -> (f32, f32) {
    (
        (rem * TEXT_SM_REMS * CAPTION_LINE_HEIGHT).round(),
        (rem * TEXT_XS_REMS * CAPTION_LINE_HEIGHT).round(),
    )
}

pub(super) fn row_height(tile_width: f32, rem: f32) -> f32 {
    let (title, subtitle) = caption_heights(rem);
    tile_width + 2. * TILE_TEXT_GAP + title + subtitle + ROW_GAP
}

pub(super) fn render_grid(view: &mut AlbumsView, cx: &mut Context<AlbumsView>) -> Div {
    let measure = {
        let entity = cx.entity();
        canvas(
            move |bounds, window, cx| {
                let width = bounds.size.width;
                if entity.read(cx).measured_width != width {
                    entity.update(cx, |this, _| this.set_grid_width(width));
                    window.on_next_frame(move |_, cx| {
                        entity.update(cx, |_, cx| cx.notify());
                    });
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full()
    };

    let container = v_flex().size_full().relative().child(measure);
    if view.measured_width <= px(0.) {
        return container;
    }

    let params = {
        let settings = cx.global::<SettingsStore>();
        let (title_height, subtitle_height) = caption_heights(view.rem_size);
        TileParams {
            columns: view.columns.max(1),
            tile_width: view.tile_width,
            cover_size: (view.tile_width - 2. * TILE_PAD).max(1.),
            title_height,
            subtitle_height,
            list_hover: Colors::list_hover(cx),
            muted: Colors::secondary(cx),
            muted_fg: Colors::muted_foreground(cx),
            show_year: settings.albums_show_year(),
        }
    };
    let item_sizes = view.item_sizes.clone();

    container
        .child(
            v_virtual_list(
                cx.entity().clone(),
                "albums_grid",
                item_sizes,
                move |view, visible_range, _window, cx| {
                    if visible_range.end > 1 {
                        let margin = params.columns;
                        let first = visible_range.start.saturating_sub(1) * params.columns;
                        let last = visible_range.end.saturating_sub(1) * params.columns;
                        view.ensure_grid_covers(
                            first.saturating_sub(margin)..(last + margin).min(view.row_data.len()),
                            cx,
                        );
                    }
                    visible_range
                        .map(|ix| match ix {
                            0 => div().w_full().h(px(TOP_PADDING)).into_any_element(),
                            _ => grid_row(view, ix - 1, &params, cx),
                        })
                        .collect::<Vec<_>>()
                },
            )
            .track_scroll(&view.scroll_handle)
            .overflow_x_hidden()
            .flex_1(),
        )
        .scrollbar(&view.scroll_handle, ScrollbarAxis::Vertical)
}

fn grid_row(
    view: &mut AlbumsView,
    row_ix: usize,
    p: &TileParams,
    cx: &mut Context<AlbumsView>,
) -> AnyElement {
    let start = row_ix * p.columns;
    let end = (start + p.columns).min(view.row_data.len());
    let cache = cx.global::<Services>().cover_art_cache.clone();
    let covers: Vec<Option<Arc<RenderImage>>> = {
        let mut cache = cache.borrow_mut();
        (start..end)
            .map(|ix| cache.peek_large(view.row_data[ix].cover_art_id))
            .collect()
    };
    let mut row = h_flex()
        .w_full()
        .px(px(GRID_PAD_X))
        .gap(px(TILE_GAP))
        .items_start();
    for (slot, ix) in (start..end).enumerate() {
        row = row.child(grid_tile(view, ix, covers[slot].as_ref(), p, cx));
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

    let subtitle = if p.show_year {
        row.subtitle_year.clone()
    } else {
        row.artist.clone()
    };
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
    .id(ElementId::Integer(row.id as u64))
    .on_click(cx.listener(move |this, _, _, cx| {
        cx.emit(AlbumSelectedEvent {
            album: this.albums_all[albums_all_ix].clone(),
        });
    }))
    .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_narrow_panel_keeps_the_tile_inside_the_panel() {
        for w in [40., 80., 120., 181.] {
            let (columns, tile_width) = grid_metrics(w);
            assert_eq!(columns, 1, "{w}");
            assert!(tile_width > 0., "{w}: {tile_width}");
            assert!(
                tile_width <= (w - 2. * GRID_PAD_X).max(1.),
                "{w}: tile {tile_width} overflows the content box"
            );
        }
    }

    #[test]
    fn captions_scale_with_the_rem_size() {
        let (small_title, small_subtitle) = caption_heights(16.);
        assert_eq!((small_title, small_subtitle), (21., 18.));

        let (large_title, large_subtitle) = caption_heights(22.);
        assert!(large_title > small_title);
        assert!(large_subtitle > small_subtitle);
    }

    #[test]
    fn a_caption_box_is_never_shorter_than_its_glyphs() {
        for rem in [16., 19., 22.] {
            let (title, subtitle) = caption_heights(rem);
            assert!(title >= (rem * TEXT_SM_REMS).ceil(), "{rem}: {title}");
            assert!(subtitle >= (rem * TEXT_XS_REMS).ceil(), "{rem}: {subtitle}");
        }
    }

    #[test]
    fn row_height_tracks_the_tile_and_the_rem_size() {
        assert!(row_height(150., 22.) > row_height(150., 16.));
        assert!(row_height(200., 16.) > row_height(150., 16.));
    }

    #[test]
    fn columns_grow_with_width_and_never_shrink_below_the_minimum() {
        let mut last = 0;
        for w in (200..2000).step_by(10) {
            let (columns, tile_width) = grid_metrics(w as f32);
            assert!(columns >= last, "{w}: {columns} < {last}");
            assert!(tile_width >= TILE_MIN_WIDTH - 0.01, "{w}: {tile_width}");
            last = columns;
        }
        assert_eq!(last, grid_metrics(1990.).0);
        assert!(last > 1);
    }

    #[test]
    fn tiles_and_gaps_fill_the_available_width_exactly() {
        for w in [400., 768., 1024., 1600.] {
            let (columns, tile_width) = grid_metrics(w);
            let used = columns as f32 * tile_width + (columns - 1) as f32 * TILE_GAP;
            assert!((used - (w - 2. * GRID_PAD_X)).abs() < 0.01);
        }
    }
}

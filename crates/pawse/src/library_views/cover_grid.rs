use std::collections::HashSet;
use std::rc::Rc;

use gpui::{
    Context, Div, FontWeight, Hsla, InteractiveElement, IntoElement, ParentElement, Pixels, Point,
    SharedString, Size, Styled, canvas, div, px, size,
};
use gpui_component::{VirtualListScrollHandle, h_flex};

use crate::cover_art_cache::decode_cover_tile;
use crate::library_service::LibraryAccess;
use crate::library_views::view_order::Section;
use crate::services::Services;
use crate::theme_colors::Colors;

const TOP_PADDING: f32 = 12.;
pub(super) const GRID_PAD_X: f32 = 16.;
const TILE_MIN_WIDTH: f32 = 150.;
pub(super) const TILE_GAP: f32 = 16.;
pub(super) const TILE_PAD: f32 = 8.;
pub(super) const TILE_TEXT_GAP: f32 = 4.;
pub(super) const TILE_RADIUS: f32 = 6.;
const ROW_GAP: f32 = 8.;
const TEXT_SM_REMS: f32 = 0.875;
const TEXT_XS_REMS: f32 = 0.75;
const CAPTION_LINE_HEIGHT: f32 = 1.5;
const LIST_HEADER_HEIGHT: f32 = 36.;
const GRID_HEADER_HEIGHT: f32 = 44.;
pub(super) const LIST_PAD_X: f32 = 16.;

pub(super) fn header_metrics(grid: bool) -> (f32, f32) {
    if grid {
        (GRID_HEADER_HEIGHT, GRID_PAD_X + TILE_PAD)
    } else {
        (LIST_HEADER_HEIGHT, LIST_PAD_X)
    }
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

pub(super) fn row_height(tile_width: f32, rem: f32, subtitle: bool) -> f32 {
    let (title, subtitle_height) = caption_heights(rem);
    let caption = if subtitle {
        TILE_TEXT_GAP + title + TILE_TEXT_GAP + subtitle_height
    } else {
        TILE_TEXT_GAP + title
    };
    tile_width + caption + ROW_GAP
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LibraryItem {
    TopPadding,
    Header(usize),
    Row(usize),
    Strip { start: usize, end: usize },
}

pub(super) struct ItemLayout {
    pub(super) items: Rc<Vec<LibraryItem>>,
    pub(super) sizes: Rc<Vec<Size<Pixels>>>,
    headers: Vec<(f32, usize)>,
    height: f32,
}

impl ItemLayout {
    pub(super) fn empty() -> Self {
        Self {
            items: Rc::new(Vec::new()),
            sizes: Rc::new(Vec::new()),
            headers: Vec::new(),
            height: 0.,
        }
    }
}

struct LayoutBuilder {
    items: Vec<LibraryItem>,
    sizes: Vec<Size<Pixels>>,
    headers: Vec<(f32, usize)>,
    top: f32,
}

impl LayoutBuilder {
    fn new(docked: bool) -> Self {
        let mut builder = Self {
            items: Vec::new(),
            sizes: Vec::new(),
            headers: Vec::new(),
            top: 0.,
        };
        builder.push(
            LibraryItem::TopPadding,
            if docked { 0. } else { TOP_PADDING },
        );
        builder
    }

    fn push(&mut self, item: LibraryItem, height: f32) {
        if let LibraryItem::Header(section) = item {
            self.headers.push((self.top, section));
        }
        self.items.push(item);
        self.sizes.push(size(px(0.), px(height)));
        self.top += height;
    }

    fn finish(self) -> ItemLayout {
        ItemLayout {
            items: Rc::new(self.items),
            sizes: Rc::new(self.sizes),
            headers: self.headers,
            height: self.top,
        }
    }
}

fn section_ranges(
    sections: &[Section],
    rows: usize,
) -> Vec<(Option<usize>, std::ops::Range<usize>)> {
    if sections.is_empty() {
        return vec![(None, 0..rows)];
    }
    sections
        .iter()
        .enumerate()
        .map(|(ix, section)| (Some(ix), section.rows.clone()))
        .collect()
}

pub(super) fn list_layout(sections: &[Section], rows: usize, row_height: f32) -> ItemLayout {
    let mut builder = LayoutBuilder::new(!sections.is_empty());
    for (header, range) in section_ranges(sections, rows) {
        if let Some(section) = header.filter(|&section| section > 0) {
            builder.push(LibraryItem::Header(section), LIST_HEADER_HEIGHT);
        }
        for row in range {
            builder.push(LibraryItem::Row(row), row_height);
        }
    }
    builder.finish()
}

pub(super) fn grid_layout(
    sections: &[Section],
    rows: usize,
    columns: usize,
    strip_height: f32,
) -> ItemLayout {
    let columns = columns.max(1);
    let mut builder = LayoutBuilder::new(!sections.is_empty());
    for (header, range) in section_ranges(sections, rows) {
        if let Some(section) = header.filter(|&section| section > 0) {
            builder.push(LibraryItem::Header(section), GRID_HEADER_HEIGHT);
        }
        let mut start = range.start;
        while start < range.end {
            let end = (start + columns).min(range.end);
            builder.push(LibraryItem::Strip { start, end }, strip_height);
            start = end;
        }
    }
    builder.finish()
}

pub(super) fn closes_section(items: &[LibraryItem], ix: usize) -> bool {
    matches!(items.get(ix + 1), Some(LibraryItem::Header(_)))
}

fn strip_span(items: &[LibraryItem], visible: std::ops::Range<usize>) -> Option<(usize, usize)> {
    let mut span: Option<(usize, usize)> = None;
    for item in &items[visible.start.min(items.len())..visible.end.min(items.len())] {
        if let LibraryItem::Strip { start, end } = *item {
            span = Some(match span {
                Some((first, _)) => (first, end),
                None => (start, end),
            });
        }
    }
    span
}

pub(super) fn cover_span(
    items: &[LibraryItem],
    visible: std::ops::Range<usize>,
    columns: usize,
    rows: usize,
) -> Option<std::ops::Range<usize>> {
    let (first, last) = strip_span(items, visible)?;
    Some(first.saturating_sub(columns)..(last + columns).min(rows))
}

#[derive(Debug, PartialEq)]
struct DockedLabels {
    current: (usize, f32),
    incoming: Option<(usize, f32)>,
}

fn docked_labels(headers: &[(f32, usize)], scroll_y: f32, header_height: f32) -> DockedLabels {
    let slot = TOP_PADDING + header_height;
    let crossed = headers.partition_point(|(top, _)| *top <= scroll_y - header_height);
    let current = crossed.checked_sub(1).map_or(0, |ix| headers[ix].1);
    let next = headers
        .get(crossed)
        .map(|&(top, section)| (section, top - scroll_y + slot));
    let current_top = next.map_or(TOP_PADDING, |(_, top)| {
        (top - header_height).min(TOP_PADDING)
    });
    DockedLabels {
        current: (current, current_top),
        incoming: next.filter(|&(_, top)| top < slot),
    }
}

pub(super) fn section_header(label: SharedString, pad_x: f32, height: f32, border: Hsla) -> Div {
    h_flex()
        .w_full()
        .h(px(height))
        .px(px(pad_x))
        .items_center()
        .gap_3()
        .child(
            div()
                .flex_shrink_0()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .child(label),
        )
        .child(div().flex_1().h(px(1.)).bg(border))
}

pub(super) fn scroll_to_top(scroll: &VirtualListScrollHandle) {
    scroll.set_offset(Point::default());
}

pub(super) fn section_slot<V: 'static>(
    layout: &ItemLayout,
    labels: &[SharedString],
    scroll: &VirtualListScrollHandle,
    grid: bool,
    cx: &Context<V>,
) -> Option<Div> {
    if labels.is_empty() {
        return None;
    }
    let (height, pad) = header_metrics(grid);
    let border = Colors::border(cx);
    let max_scroll = (layout.height - f32::from(scroll.bounds().size.height)).max(0.);
    let scroll_y = (-f32::from(scroll.offset().y)).clamp(0., max_scroll);
    let docked = docked_labels(&layout.headers, scroll_y, height);
    let label = |(section, top): (usize, f32)| {
        labels.get(section).map(|label| {
            div()
                .absolute()
                .top(px(top))
                .left_0()
                .right_0()
                .child(section_header(label.clone(), pad, height, border))
        })
    };
    let scroll = scroll.clone();
    let view = cx.entity_id();
    Some(
        div()
            .relative()
            .flex_shrink_0()
            .w_full()
            .h(px(TOP_PADDING + height))
            .overflow_hidden()
            .children(label(docked.current))
            .children(docked.incoming.and_then(label))
            .on_scroll_wheel(move |event, window, cx| {
                let delta = event.delta.pixel_delta(window.line_height());
                if delta.x.abs() > delta.y.abs() {
                    return;
                }
                let offset = scroll.offset();
                let y = (f32::from(offset.y) + f32::from(delta.y)).clamp(-max_scroll, 0.);
                if y != f32::from(offset.y) {
                    scroll.set_offset(Point::new(offset.x, px(y)));
                    cx.notify(view);
                }
            }),
    )
}

pub(super) trait MeasuredGrid: 'static {
    fn measured_width(&self) -> Pixels;
    fn set_grid_width(&mut self, width: Pixels);
}

pub(super) fn width_probe<V: MeasuredGrid>(cx: &mut Context<V>) -> impl IntoElement {
    let entity = cx.entity();
    canvas(
        move |bounds, window, cx| {
            let width = bounds.size.width;
            if entity.read(cx).measured_width() != width {
                entity.update(cx, |this, _| this.set_grid_width(width));
                let entity = entity.clone();
                window.on_next_frame(move |_, cx| {
                    entity.update(cx, |_, cx| cx.notify());
                });
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full()
}

pub(super) struct GridCovers {
    in_flight: HashSet<i64>,
    unavailable: HashSet<i64>,
    access: LibraryAccess,
}

impl GridCovers {
    pub(super) fn new(access: LibraryAccess) -> Self {
        Self {
            in_flight: HashSet::new(),
            unavailable: HashSet::new(),
            access,
        }
    }

    pub(super) fn reset(&mut self) {
        self.in_flight.clear();
        self.unavailable.clear();
    }

    pub(super) fn ensure<V: 'static>(
        &mut self,
        span: usize,
        ids: impl Iterator<Item = i64>,
        cx: &mut Context<V>,
        field: fn(&mut V) -> &mut GridCovers,
    ) {
        let cache = cx.global::<Services>().cover_art_cache.clone();
        cache.borrow_mut().fit_large_capacity(span, cx);
        let mut wanted: Vec<i64> = {
            let cache = cache.borrow();
            ids.filter(|id| {
                !cache.holds_large(*id)
                    && !self.in_flight.contains(id)
                    && !self.unavailable.contains(id)
            })
            .collect()
        };
        if wanted.is_empty() {
            return;
        }
        wanted.sort_unstable();
        wanted.dedup();
        for id in &wanted {
            self.in_flight.insert(*id);
        }

        let access = self.access.clone();
        cx.spawn(async move |this, cx| {
            for id in wanted {
                let access = access.clone();
                let decoded = cx
                    .background_executor()
                    .spawn(async move {
                        access
                            .cover_large(id)
                            .and_then(|bytes| decode_cover_tile(&bytes))
                    })
                    .await;
                let updated = this.update(cx, |view, cx| {
                    let covers = field(view);
                    covers.in_flight.remove(&id);
                    let Some(image) = decoded else {
                        covers.unavailable.insert(id);
                        return;
                    };
                    cx.global::<Services>()
                        .cover_art_cache
                        .clone()
                        .borrow_mut()
                        .insert_large(id, image, cx);
                    cx.notify();
                });
                if updated.is_err() {
                    break;
                }
            }
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library_views::view_order::SectionKey;

    fn section(rows: std::ops::Range<usize>) -> Section {
        Section {
            key: SectionKey::Other,
            rows,
        }
    }

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
    fn row_height_tracks_the_tile_the_rem_size_and_the_subtitle() {
        assert!(row_height(150., 22., true) > row_height(150., 16., true));
        assert!(row_height(200., 16., true) > row_height(150., 16., true));
        let (_, subtitle) = caption_heights(16.);
        assert_eq!(
            row_height(150., 16., true) - row_height(150., 16., false),
            TILE_TEXT_GAP + subtitle
        );
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

    #[test]
    fn an_ungrouped_grid_is_padding_then_full_strips() {
        let layout = grid_layout(&[], 7, 3, 100.);
        assert_eq!(
            *layout.items,
            vec![
                LibraryItem::TopPadding,
                LibraryItem::Strip { start: 0, end: 3 },
                LibraryItem::Strip { start: 3, end: 6 },
                LibraryItem::Strip { start: 6, end: 7 },
            ]
        );
        assert!(layout.headers.is_empty());
        assert_eq!(layout.sizes.len(), layout.items.len());
    }

    #[test]
    fn a_grouped_grid_docks_the_first_header_and_starts_every_section_on_a_fresh_strip() {
        let layout = grid_layout(&[section(0..2), section(2..7)], 7, 3, 100.);
        assert_eq!(
            *layout.items,
            vec![
                LibraryItem::TopPadding,
                LibraryItem::Strip { start: 0, end: 2 },
                LibraryItem::Header(1),
                LibraryItem::Strip { start: 2, end: 5 },
                LibraryItem::Strip { start: 5, end: 7 },
            ]
        );
        assert_eq!(layout.sizes[0].height, px(0.));
        assert_eq!(layout.headers, vec![(100., 1)]);
    }

    #[test]
    fn a_grouped_list_inlines_every_header_but_the_first() {
        let layout = list_layout(&[section(0..1), section(1..3)], 3, 49.);
        assert_eq!(
            *layout.items,
            vec![
                LibraryItem::TopPadding,
                LibraryItem::Row(0),
                LibraryItem::Header(1),
                LibraryItem::Row(1),
                LibraryItem::Row(2),
            ]
        );
        assert_eq!(layout.headers, vec![(49., 1)]);
    }

    #[test]
    fn only_the_row_right_before_a_header_closes_its_section() {
        let layout = list_layout(&[section(0..2), section(2..3)], 3, 49.);
        let closing: Vec<usize> = (0..layout.items.len())
            .filter(|&ix| closes_section(&layout.items, ix))
            .collect();
        assert_eq!(closing, vec![2]);
        assert_eq!(layout.items[2], LibraryItem::Row(1));
    }

    #[test]
    fn an_ungrouped_list_keeps_its_top_padding() {
        let layout = list_layout(&[], 2, 49.);
        assert_eq!(layout.sizes[0].height, px(TOP_PADDING));
        assert!(layout.headers.is_empty());
    }

    #[test]
    fn the_strip_span_covers_only_visible_strips() {
        let layout = grid_layout(&[section(0..2), section(2..7)], 7, 3, 100.);
        assert_eq!(strip_span(&layout.items, 0..1), None);
        assert_eq!(strip_span(&layout.items, 0..2), Some((0, 2)));
        assert_eq!(strip_span(&layout.items, 1..5), Some((0, 7)));
        assert_eq!(strip_span(&layout.items, 2..4), Some((2, 5)));
        assert_eq!(strip_span(&layout.items, 3..99), Some((2, 7)));
    }

    const H: f32 = 36.;
    const SLOT: f32 = TOP_PADDING + H;

    #[test]
    fn the_first_section_is_docked_from_the_start() {
        let docked = docked_labels(&[(200., 1)], 0., H);
        assert_eq!(docked.current, (0, TOP_PADDING));
        assert_eq!(docked.incoming, None);
        let docked = docked_labels(&[], 5000., H);
        assert_eq!(docked.current, (0, TOP_PADDING));
    }

    #[test]
    fn the_incoming_header_continues_its_path_and_pushes_the_docked_one_up() {
        let headers = [(200., 1), (600., 2)];
        assert_eq!(docked_labels(&headers, 200., H).incoming, None);

        let docked = docked_labels(&headers, 210., H);
        assert_eq!(docked.incoming, Some((1, SLOT - 10.)));
        assert_eq!(docked.current, (0, TOP_PADDING - 10.));
        let (_, incoming_top) = docked.incoming.unwrap();
        assert_eq!(docked.current.1 + H, incoming_top);

        let docked = docked_labels(&headers, 200. + H, H);
        assert_eq!(docked.current, (1, TOP_PADDING));
        assert_eq!(docked.incoming, None);
    }

    #[test]
    fn the_incoming_header_meets_the_docked_one_exactly_where_it_settles() {
        let headers = [(200., 1)];
        let before = docked_labels(&headers, 200. + H - 0.5, H);
        let after = docked_labels(&headers, 200. + H, H);
        assert_eq!(before.incoming, Some((1, TOP_PADDING + 0.5)));
        assert_eq!(after.current, (1, TOP_PADDING));
    }
}

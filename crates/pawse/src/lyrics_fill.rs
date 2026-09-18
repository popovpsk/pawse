use gpui::{Div, FontWeight, Hsla, ParentElement, Pixels, SharedString, Styled, Window, div, px};

pub const ACTIVE_TOLERANCE_MS: u32 = 50;
const FILL_LEAD: f32 = 0.92;
pub const INTERLUDE_MIN_MS: u32 = 3_000;
pub const INTERLUDE_TEXT: &str = "♪ ♪ ♪";

const FEATHER_PX: f32 = 12.;
const FEATHER_ALPHA: [f32; 4] = [0.85, 0.6, 0.35, 0.12];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Lyric,
    Interlude,
}

#[derive(Debug, PartialEq)]
pub struct LyricRow {
    pub text: SharedString,
    pub time_ms: Option<u32>,
    pub label: Option<SharedString>,
    pub kind: RowKind,
}

#[derive(Debug, PartialEq)]
pub struct LineShape {
    pub rows: Vec<Pixels>,
    pub total: Pixels,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FillRect {
    pub row: usize,
    pub left: Pixels,
    pub right: Pixels,
    pub alpha: f32,
}

pub struct FillPlan {
    pub t: f32,
    pub width: Pixels,
    pub pitch: Pixels,
    pub edge_row: usize,
    pub edge_x: Pixels,
    pub rows: Vec<Pixels>,
}

pub fn format_ms(ms: u32) -> SharedString {
    let total_secs = ms / 1000;
    SharedString::from(format!("{:02}:{:02}", total_secs / 60, total_secs % 60))
}

pub fn build_rows(parsed: &lyrics::Lyrics, track_duration_ms: Option<u64>) -> Vec<LyricRow> {
    let base: Vec<LyricRow> = parsed
        .lines
        .iter()
        .map(|l| LyricRow {
            text: SharedString::from(l.text.clone()),
            time_ms: l.time_ms,
            label: l.time_ms.map(format_ms),
            kind: RowKind::Lyric,
        })
        .collect();

    if !parsed.synced || base.is_empty() {
        return base;
    }

    let track_end = track_duration_ms.map(|ms| ms.min(u32::MAX as u64) as u32);
    let nexts: Vec<Option<u32>> = (0..base.len())
        .map(|i| base.get(i + 1).and_then(|r| r.time_ms).or(track_end))
        .collect();
    let mut out: Vec<LyricRow> = Vec::with_capacity(base.len());
    for (ix, mut row) in base.into_iter().enumerate() {
        let gap = nexts[ix].map(|next| next.saturating_sub(row.time_ms.unwrap_or(0)));

        if row.text.is_empty() {
            if gap.is_some_and(|g| g >= INTERLUDE_MIN_MS) {
                if out
                    .last()
                    .is_some_and(|prev| prev.kind == RowKind::Interlude)
                {
                    continue;
                }
                row.text = SharedString::new_static(INTERLUDE_TEXT);
                row.kind = RowKind::Interlude;
            }
            out.push(row);
            continue;
        }

        out.push(row);
    }

    out
}

pub fn fill_span(
    rows: &[LyricRow],
    ix: usize,
    track_duration_ms: Option<u64>,
) -> Option<(u32, u32)> {
    let row = rows.get(ix)?;
    let start = row.time_ms?;
    let end = rows
        .get(ix + 1)
        .and_then(|r| r.time_ms)
        .or_else(|| track_duration_ms.map(|ms| ms.min(u32::MAX as u64) as u32))?;
    let span = end.saturating_sub(start);
    if span == 0 {
        return None;
    }
    let span = match row.kind {
        RowKind::Interlude => span,
        RowKind::Lyric => ((span as f32 * FILL_LEAD) as u32).max(1),
    };
    Some((start, span))
}

pub fn active_row(rows: &[LyricRow], pos_ms: u64) -> Option<usize> {
    if rows.is_empty() {
        return None;
    }
    let pos_ms = (pos_ms + ACTIVE_TOLERANCE_MS as u64).min(u32::MAX as u64) as u32;
    let mut found: Option<usize> = None;
    let mut lo = 0usize;
    let mut hi = rows.len();
    while lo < hi {
        let mid = (lo + hi) / 2;
        match rows[mid].time_ms {
            Some(t) if t <= pos_ms => {
                found = Some(mid);
                lo = mid + 1;
            }
            _ => hi = mid,
        }
    }
    found
}

pub fn progress(now_ms: u64, start_ms: u32, span_ms: u32) -> f32 {
    if span_ms == 0 {
        return 1.;
    }
    let elapsed = now_ms.saturating_sub(start_ms as u64);
    (elapsed as f32 / span_ms as f32).clamp(0., 1.)
}

pub fn shape_line(
    window: &mut Window,
    text: &SharedString,
    width: Pixels,
    font_size: Pixels,
) -> Option<LineShape> {
    if text.is_empty() || width <= px(0.) {
        return None;
    }

    let mut style = window.text_style();
    style.font_size = font_size.into();
    style.font_weight = FontWeight::SEMIBOLD;
    let run = style.to_run(text.len());

    let lines = window
        .text_system()
        .shape_text(text.clone(), font_size, &[run], Some(width), None)
        .ok()?;
    let wrapped = lines.into_iter().next()?;
    let layout = wrapped.unwrapped_layout.clone();

    let mut starts: Vec<usize> = Vec::with_capacity(wrapped.wrap_boundaries.len() + 1);
    starts.push(0);
    for boundary in wrapped.wrap_boundaries.iter() {
        let run = layout.runs.get(boundary.run_ix)?;
        let glyph = run.glyphs.get(boundary.glyph_ix)?;
        starts.push(glyph.index);
    }

    let mut rows = Vec::with_capacity(starts.len());
    for (ix, start) in starts.iter().enumerate() {
        let start_x = layout.x_for_index(*start);
        let end_x = match starts.get(ix + 1) {
            Some(next) => layout.x_for_index(*next),
            None => layout.width,
        };
        rows.push((end_x - start_x).max(px(0.)));
    }

    let total = rows.iter().fold(px(0.), |acc, w| acc + *w);
    if total <= px(0.) {
        return None;
    }

    Some(LineShape { rows, total })
}

pub fn fill_plan(shape: &LineShape, width: Pixels, pitch: Pixels, t: f32) -> FillPlan {
    let t = t.clamp(0., 1.);
    let mut remaining = shape.total * t;
    let last = shape.rows.len().saturating_sub(1);
    let mut edge_row = last;
    let mut edge_x = shape.rows.get(last).copied().unwrap_or(px(0.));

    for (ix, row_width) in shape.rows.iter().enumerate() {
        if remaining <= *row_width || ix == last {
            edge_row = ix;
            edge_x = if remaining > *row_width {
                *row_width
            } else {
                remaining
            };
            break;
        }
        remaining -= *row_width;
    }

    FillPlan {
        t,
        width,
        pitch,
        edge_row,
        edge_x,
        rows: shape.rows.clone(),
    }
}

pub fn feather_span(end: Pixels, row_width: Pixels) -> (Pixels, Pixels) {
    let feather = px(FEATHER_PX).min(end).min((row_width - end).max(px(0.)));
    ((end - feather / 2.).max(px(0.)), feather)
}

pub fn fill_rects(plan: &FillPlan) -> Vec<FillRect> {
    let mut out: Vec<FillRect> = Vec::new();
    if plan.rows.is_empty() {
        return out;
    }

    for row in 0..=plan.edge_row.min(plan.rows.len() - 1) {
        let row_width = plan.rows[row];

        if row != plan.edge_row {
            if row_width > px(0.) {
                out.push(FillRect {
                    row,
                    left: px(0.),
                    right: row_width,
                    alpha: 1.,
                });
            }
            continue;
        }

        let (solid_end, feather) = feather_span(plan.edge_x, row_width);

        if solid_end > px(0.) {
            out.push(FillRect {
                row,
                left: px(0.),
                right: solid_end,
                alpha: 1.,
            });
        }

        if feather <= px(0.) {
            continue;
        }

        let band = feather / FEATHER_ALPHA.len() as f32;
        for (step, alpha) in FEATHER_ALPHA.iter().enumerate() {
            let left = solid_end + band * step as f32;
            out.push(FillRect {
                row,
                left,
                right: left + band,
                alpha: *alpha,
            });
        }
    }

    out
}

pub fn fill_children(
    plan: &FillPlan,
    text: &SharedString,
    font_size: Pixels,
    color: Hsla,
) -> Vec<Div> {
    fill_rects(plan)
        .into_iter()
        .map(|rect| {
            layer(
                plan,
                rect.row,
                rect.left,
                rect.right,
                text,
                font_size,
                color.opacity(rect.alpha),
            )
        })
        .collect()
}

fn layer(
    plan: &FillPlan,
    row: usize,
    left: Pixels,
    right: Pixels,
    text: &SharedString,
    font_size: Pixels,
    color: Hsla,
) -> Div {
    let top = plan.pitch * row as f32;
    div()
        .absolute()
        .left(left)
        .top(top)
        .w(right - left)
        .h(plan.pitch)
        .overflow_hidden()
        .child(
            div()
                .absolute()
                .left(-left)
                .top(-top)
                .w(plan.width)
                .text_size(font_size)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(color)
                .child(text.clone()),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn halo() -> lyrics::Lyrics {
        synced(&[
            (
                188_260,
                "How you're plannin' to go about makin' your amends",
            ),
            (194_830, ""),
            (196_980, "To the dead"),
            (201_660, ""),
            (204_460, "To the dead"),
            (209_170, ""),
            (212_850, "With your halo slippin' down"),
        ])
    }

    fn synced_rows_of(lines: &[(u32, &str)]) -> Vec<LyricRow> {
        build_rows(&synced(lines), None)
    }

    fn synced(lines: &[(u32, &str)]) -> lyrics::Lyrics {
        lyrics::Lyrics {
            synced: true,
            lines: lines
                .iter()
                .map(|(ms, text)| lyrics::LyricLine {
                    time_ms: Some(*ms),
                    text: (*text).to_string(),
                })
                .collect(),
        }
    }

    #[test]
    fn only_blank_lines_become_interludes() {
        let rows = build_rows(&halo(), Some(240_000));
        let kinds: Vec<RowKind> = rows.iter().map(|r| r.kind).collect();
        assert_eq!(
            kinds,
            vec![
                RowKind::Lyric,
                RowKind::Lyric,
                RowKind::Lyric,
                RowKind::Lyric,
                RowKind::Lyric,
                RowKind::Interlude,
                RowKind::Lyric,
            ]
        );
        assert_eq!(rows[5].time_ms, Some(209_170));
        assert_eq!(rows[5].text.as_ref(), INTERLUDE_TEXT);
        assert_eq!(rows.len(), 7);
    }

    #[test]
    fn a_long_gap_without_a_blank_line_is_left_alone() {
        let rows = build_rows(&synced(&[(0, "one"), (60_000, "two")]), Some(120_000));
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.kind == RowKind::Lyric));
    }

    #[test]
    fn a_blank_line_needs_at_least_the_minimum() {
        let rows = build_rows(
            &synced(&[
                (0, "one"),
                (1_000, ""),
                (1_000 + INTERLUDE_MIN_MS - 1, "two"),
            ]),
            None,
        );
        assert_eq!(rows[1].kind, RowKind::Lyric);
        assert!(rows[1].text.is_empty());

        let rows = build_rows(
            &synced(&[(0, "one"), (1_000, ""), (1_000 + INTERLUDE_MIN_MS, "two")]),
            None,
        );
        assert_eq!(rows[1].kind, RowKind::Interlude);
    }

    #[test]
    fn back_to_back_blank_lines_collapse_into_one_interlude() {
        let rows = build_rows(
            &synced(&[(0, "one"), (4_000, ""), (8_000, ""), (12_000, "two")]),
            None,
        );
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1].kind, RowKind::Interlude);
        assert_eq!(rows[1].time_ms, Some(4_000));
        assert_eq!(fill_span(&rows, 1, None), Some((4_000, 8_000)));
    }

    #[test]
    fn a_lyric_row_keeps_its_whole_interval_less_the_lead() {
        let rows = build_rows(&halo(), Some(240_000));
        let (start, span) = fill_span(&rows, 0, Some(240_000)).unwrap();
        assert_eq!(start, 188_260);
        assert_eq!(span, (6_570. * 0.92) as u32);

        let (_, long_span) =
            fill_span(&synced_rows_of(&[(0, "one"), (60_000, "two")]), 0, None).unwrap();
        assert_eq!(long_span, (60_000. * 0.92) as u32);
    }

    #[test]
    fn an_interlude_row_keeps_its_interval_untouched() {
        let rows = build_rows(&halo(), Some(240_000));
        assert_eq!(fill_span(&rows, 5, Some(240_000)), Some((209_170, 3_680)));
    }

    #[test]
    fn last_row_span_uses_track_duration() {
        let rows = build_rows(&synced(&[(0, "one")]), Some(4_000));
        assert_eq!(fill_span(&rows, 0, Some(4_000)), Some((0, 3_680)));
        assert_eq!(fill_span(&rows, 0, None), None);
    }

    #[test]
    fn a_span_needs_a_row_and_a_following_time() {
        let rows = build_rows(&synced(&[(0, "one"), (3_000, "two")]), Some(9_000));
        assert_eq!(fill_span(&rows, 9, Some(9_000)), None);
        assert_eq!(fill_span(&rows, 0, Some(9_000)), Some((0, 2_760)));
        assert_eq!(fill_span(&rows, 1, Some(9_000)), Some((3_000, 5_520)));
    }

    #[test]
    fn the_fill_starts_at_zero_and_ends_before_the_hand_over() {
        let rows = build_rows(&synced(&[(1_000, "one"), (3_000, "two")]), None);
        let (start, span) = fill_span(&rows, 0, None).unwrap();
        let lead = ACTIVE_TOLERANCE_MS as u64;

        let goes_active = 1_000 - lead;
        assert_eq!(active_row(&rows, goes_active), Some(0));
        assert_eq!(progress(goes_active + lead, start, span), 0.);

        let finishes = start as u64 + span as u64;
        assert!(progress(finishes - 1, start, span) < 1.);
        assert_eq!(progress(finishes, start, span), 1.);

        let hands_over = 3_000 - lead;
        assert_eq!(active_row(&rows, hands_over - 1), Some(0));
        assert_eq!(active_row(&rows, hands_over), Some(1));
        assert!(finishes < hands_over + lead);
    }

    #[test]
    fn a_trailing_blank_line_becomes_the_last_interlude() {
        let rows = build_rows(&synced(&[(0, "one"), (5_000, "")]), Some(20_000));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].kind, RowKind::Interlude);
        assert_eq!(fill_span(&rows, 1, Some(20_000)), Some((5_000, 15_000)));
    }

    #[test]
    fn a_trailing_blank_past_the_reported_duration_stays_blank() {
        let rows = build_rows(&synced(&[(0, "one"), (30_000, "")]), Some(20_000));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].kind, RowKind::Lyric);
        assert!(rows[1].text.is_empty());
    }

    #[test]
    fn collapsing_blank_lines_leaves_no_timing_hole() {
        let rows = build_rows(
            &synced(&[(0, "a"), (4_000, ""), (8_000, ""), (12_000, "b")]),
            None,
        );
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1].kind, RowKind::Interlude);
        assert_eq!(active_row(&rows, 9_000), Some(1));
        assert_eq!(fill_span(&rows, 1, None), Some((4_000, 8_000)));
    }

    #[test]
    fn a_zero_width_wrapped_row_draws_nothing() {
        let rects = fill_rects(&plan_of(vec![px(0.), px(60.)], 0.5));
        assert!(rects.iter().all(|r| r.row == 1));
        assert!(rects.iter().all(|r| r.right > r.left));

        let plan = plan_of(vec![px(0.), px(60.)], 1.);
        assert_eq!(plan.edge_row, 1);
        assert_eq!(plan.edge_x, px(60.));
    }

    #[test]
    fn timestamps_render_as_minutes_and_seconds() {
        assert_eq!(format_ms(0).as_ref(), "00:00");
        assert_eq!(format_ms(59_999).as_ref(), "00:59");
        assert_eq!(format_ms(61_000).as_ref(), "01:01");
        assert_eq!(format_ms(3_600_000).as_ref(), "60:00");
    }

    #[test]
    fn a_lone_lyric_line_stays_one_row() {
        let rows = build_rows(&synced(&[(0, "one")]), Some(60_000));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, RowKind::Lyric);
    }

    #[test]
    fn plain_lyrics_pass_through() {
        let parsed = lyrics::Lyrics {
            synced: false,
            lines: vec![lyrics::LyricLine {
                time_ms: None,
                text: "plain".to_string(),
            }],
        };
        let rows = build_rows(&parsed, Some(10_000));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].time_ms, None);
    }

    #[test]
    fn a_timestamp_past_the_reported_duration_has_no_span() {
        let rows = build_rows(&synced(&[(0, "one"), (100_000, "two")]), Some(50_000));
        assert_eq!(fill_span(&rows, rows.len() - 1, Some(50_000)), None);
    }

    #[test]
    fn a_single_row_shape_fills_straight_across() {
        assert!(fill_rects(&plan_of(vec![px(80.)], 0.)).is_empty());

        let rects = fill_rects(&plan_of(vec![px(80.)], 0.5));
        assert!(rects.iter().all(|r| r.row == 0));
        assert_eq!(rects[0].left, px(0.));
        assert_eq!(rects[0].right, px(40. - FEATHER_PX / 2.));
        assert_eq!(rects.last().unwrap().right, px(40. + FEATHER_PX / 2.));

        assert_eq!(
            fill_rects(&plan_of(vec![px(80.)], 1.)),
            vec![FillRect {
                row: 0,
                left: px(0.),
                right: px(80.),
                alpha: 1.,
            }]
        );
    }

    #[test]
    fn progress_is_clamped() {
        assert_eq!(progress(0, 1_000, 2_000), 0.);
        assert_eq!(progress(2_000, 1_000, 2_000), 0.5);
        assert_eq!(progress(9_000, 1_000, 2_000), 1.);
    }

    fn row(time_ms: u32, text: &str) -> LyricRow {
        LyricRow {
            text: SharedString::from(text.to_string()),
            time_ms: Some(time_ms),
            label: Some(format_ms(time_ms)),
            kind: RowKind::Lyric,
        }
    }

    fn plan_of(rows: Vec<Pixels>, t: f32) -> FillPlan {
        let total = rows.iter().fold(px(0.), |acc, w| acc + *w);
        fill_plan(&LineShape { rows, total }, px(300.), px(20.), t)
    }

    #[test]
    fn an_unknown_track_length_leaves_the_last_gap_alone() {
        let rows = build_rows(&synced(&[(0, "one"), (2_000, "")]), None);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].kind, RowKind::Lyric);
    }

    #[test]
    fn two_rows_sharing_a_timestamp_have_no_span() {
        let rows = build_rows(&synced(&[(4_000, "one"), (4_000, "two")]), Some(9_000));
        assert_eq!(fill_span(&rows, 0, Some(9_000)), None);
    }

    #[test]
    fn a_zero_span_reads_as_finished() {
        assert_eq!(progress(0, 0, 0), 1.);
    }

    #[test]
    fn no_row_is_active_before_the_first_timestamp() {
        let rows = vec![row(1_000, "one"), row(5_000, "two")];
        assert_eq!(active_row(&rows, 0), None);
        assert_eq!(active_row(&[], 10_000), None);
    }

    #[test]
    fn a_row_goes_active_a_touch_before_its_timestamp() {
        let rows = vec![row(1_000, "one"), row(5_000, "two")];
        let lead = ACTIVE_TOLERANCE_MS as u64;
        assert_eq!(active_row(&rows, 1_000 - lead), Some(0));
        assert_eq!(active_row(&rows, 1_000 - lead - 1), None);
        assert_eq!(active_row(&rows, 1_000), Some(0));
        assert_eq!(active_row(&rows, 4_000), Some(0));
        assert_eq!(active_row(&rows, 5_000 - lead), Some(1));
        assert_eq!(active_row(&rows, 900_000), Some(1));
    }

    #[test]
    fn untimed_rows_are_never_active() {
        let rows = vec![LyricRow {
            text: SharedString::new_static("plain"),
            time_ms: None,
            label: None,
            kind: RowKind::Lyric,
        }];
        assert_eq!(active_row(&rows, 10_000), None);
    }

    #[test]
    fn an_untouched_line_draws_nothing() {
        assert!(fill_rects(&plan_of(vec![px(100.), px(60.)], 0.)).is_empty());
    }

    #[test]
    fn an_empty_shape_draws_nothing() {
        assert!(fill_rects(&plan_of(Vec::new(), 1.)).is_empty());
    }

    #[test]
    fn a_finished_line_is_one_solid_rect_per_row() {
        let rects = fill_rects(&plan_of(vec![px(100.), px(60.)], 1.));
        assert_eq!(
            rects,
            vec![
                FillRect {
                    row: 0,
                    left: px(0.),
                    right: px(100.),
                    alpha: 1.
                },
                FillRect {
                    row: 1,
                    left: px(0.),
                    right: px(60.),
                    alpha: 1.
                },
            ]
        );
    }

    #[test]
    fn the_ramp_sits_astride_the_edge_and_stays_inside_the_row() {
        let rects = fill_rects(&plan_of(vec![px(100.), px(60.)], 0.75));
        assert_eq!(
            rects.first().map(|r| (r.row, r.right, r.alpha)),
            Some((0, px(100.), 1.))
        );

        let ramp: Vec<&FillRect> = rects.iter().filter(|r| r.row == 1).collect();
        assert_eq!(ramp.len(), FEATHER_ALPHA.len() + 1);
        assert_eq!(ramp[0].left, px(0.));
        assert_eq!(ramp[0].right, px(20. - FEATHER_PX / 2.));
        assert_eq!(ramp[0].alpha, 1.);
        assert_eq!(ramp.last().unwrap().right, px(20. + FEATHER_PX / 2.));

        for pair in ramp.windows(2) {
            assert_eq!(pair[0].right, pair[1].left);
            assert!(pair[0].alpha > pair[1].alpha);
        }
        let widths = [px(100.), px(60.)];
        for rect in &rects {
            assert!(rect.left >= px(0.));
            assert!(rect.right <= widths[rect.row]);
            assert!(rect.right >= rect.left);
        }
    }

    #[test]
    fn rows_behind_the_edge_are_solid_across_their_own_width() {
        let rects = fill_rects(&plan_of(vec![px(40.), px(90.), px(70.)], 0.8));
        let solid: Vec<(usize, Pixels)> = rects
            .iter()
            .filter(|r| r.alpha == 1.)
            .map(|r| (r.row, r.right))
            .collect();
        assert_eq!(solid[0], (0, px(40.)));
        assert_eq!(solid[1], (1, px(90.)));
        assert_eq!(solid[2].0, 2);
        assert!(solid[2].1 < px(70.));
        assert!(
            rects
                .iter()
                .all(|r| r.right <= [px(40.), px(90.), px(70.)][r.row])
        );
    }

    #[test]
    fn a_row_boundary_keeps_the_earlier_row_full() {
        let plan = plan_of(vec![px(100.), px(60.)], 100. / 160.);
        assert_eq!(plan.edge_row, 0);
        assert_eq!(plan.edge_x, px(100.));
    }

    #[test]
    fn feather_is_centred_on_the_edge() {
        let (solid_end, feather) = feather_span(px(120.), px(300.));
        assert_eq!(feather, px(FEATHER_PX));
        assert_eq!(solid_end, px(120. - FEATHER_PX / 2.));
        assert_eq!(solid_end + feather, px(120. + FEATHER_PX / 2.));
    }

    #[test]
    fn feather_collapses_at_the_row_ends() {
        assert_eq!(feather_span(px(0.), px(300.)), (px(0.), px(0.)));
        assert_eq!(feather_span(px(300.), px(300.)), (px(300.), px(0.)));
        let (solid_end, feather) = feather_span(px(4.), px(300.));
        assert_eq!(feather, px(4.));
        assert_eq!(solid_end, px(2.));
        let (solid_end, feather) = feather_span(px(297.), px(300.));
        assert_eq!(feather, px(3.));
        assert_eq!(solid_end + feather, px(298.5));
    }

    #[test]
    fn fill_plan_walks_wrapped_rows() {
        let shape = LineShape {
            rows: vec![px(100.), px(60.)],
            total: px(160.),
        };
        let start = fill_plan(&shape, px(100.), px(20.), 0.);
        assert_eq!(start.edge_row, 0);
        assert_eq!(start.edge_x, px(0.));

        let mid = fill_plan(&shape, px(100.), px(20.), 0.75);
        assert_eq!(mid.edge_row, 1);
        assert_eq!(mid.edge_x, px(20.));

        let end = fill_plan(&shape, px(100.), px(20.), 1.);
        assert_eq!(end.edge_row, 1);
        assert_eq!(end.edge_x, px(60.));
    }
}

use gpui::{
    App, Bounds, ContentMask, Font, FontWeight, Hsla, Path, Pixels, Point, ShapedLine,
    SharedString, TextAlign, TextRun, Window, fill, linear_color_stop, linear_gradient, point, px,
    size,
};
use spectrum::{Analyzer, ZONES, position, zone_of};

use super::SILHOUETTE_LABELS;
use crate::cover_mode_view::COVER_RADIUS;
use crate::cover_skin::color::{oklch, to_hsla, to_oklch};
use crate::settings_store::SpectrumStyle;

const COVER_STROKE: f32 = 1.6;
const COVER_LIGHT_LUMA: f32 = 0.5;
const COVER_INK_GAP: f32 = 0.3;
const COVER_INK_DARK_MAX_L: f32 = 0.4;
const COVER_INK_LIGHT_MIN_L: f32 = 0.72;
const FLATNESS: f32 = 0.1;
const MAX_STEPS: usize = 16;
const MITER_MIN: f32 = 0.5;
const SOLID: (Point<f32>, Point<f32>, Point<f32>) = (point(0., 1.), point(0., 1.), point(0., 1.));
const COVER_RANGE: (f32, f32) = (-20., -76.);
const SILHOUETTE_RANGE: (f32, f32) = (-22., -74.);
const ZONES_PLOT: (f32, f32) = (18., 20.);
const ZONES_RANGE: (f32, f32) = (-12., -84.);
const ANALYZER_PLOT: (f32, f32) = (34., 38.);
const LABEL_PAD: f32 = 4.;
const ANALYZER_GUTTER: f32 = 32.;
const ANALYZER_RANGE: (f32, f32) = (-12., -96.);
const NOTES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];
const TICKS: [(f32, &str); 8] = [
    (50., "50"),
    (100., "100"),
    (200., "200"),
    (500., "500"),
    (1_000., "1k"),
    (2_000., "2k"),
    (5_000., "5k"),
    (10_000., "10k"),
];

#[derive(Clone, Copy)]
pub struct Palette {
    pub accent: Hsla,
    pub fg: Hsla,
    pub muted: Hsla,
}

pub struct Frame<'a> {
    pub analyzer: &'a Analyzer,
    pub style: SpectrumStyle,
    pub palette: Palette,
    pub cover_luma: f32,
    pub hovered: bool,
    pub readout: Option<(usize, &'a SharedString)>,
    pub zone_text: &'a [SharedString],
    pub legend: &'a SharedString,
    pub ui_font: &'a Font,
    pub name_font: &'a Font,
    pub mono_font: &'a Font,
}

#[derive(Clone, Copy)]
struct Plot {
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    top_db: f32,
    bottom_db: f32,
    corner: f32,
}

impl Plot {
    fn x(&self, hz: f32) -> f32 {
        self.left + self.width * position(hz)
    }

    fn y(&self, db: f32) -> f32 {
        let t = (self.top_db - db) / (self.top_db - self.bottom_db);
        self.top + t.clamp(0., 1.) * self.height
    }

    fn bottom(&self) -> f32 {
        self.top + self.height
    }

    fn right(&self) -> f32 {
        self.left + self.width
    }

    fn floor(&self, x: f32) -> f32 {
        let r = self.corner;
        let edge = (x - self.left).min(self.right() - x).max(0.);
        if edge >= r {
            return self.bottom();
        }
        let dx = r - edge;
        self.bottom() - r + (r * r - dx * dx).sqrt()
    }

    fn bounds(&self, origin: Point<Pixels>) -> Bounds<Pixels> {
        Bounds::new(
            point(origin.x + px(self.left), origin.y + px(self.top)),
            size(px(self.width), px(self.height)),
        )
    }
}

#[derive(Clone, Copy)]
enum Align {
    Left,
    Center,
    Right,
}

pub fn plot_width(style: SpectrumStyle, width: f32) -> f32 {
    match style {
        SpectrumStyle::Cover => width - COVER_STROKE,
        SpectrumStyle::Analyzer => width - ANALYZER_GUTTER,
        SpectrumStyle::Silhouette | SpectrumStyle::Zones => width,
    }
}

pub fn shows_detail(style: SpectrumStyle) -> bool {
    matches!(style, SpectrumStyle::Zones | SpectrumStyle::Analyzer)
}

pub fn hover_index(
    style: SpectrumStyle,
    width: f32,
    height: f32,
    x: f32,
    count: usize,
) -> Option<usize> {
    let plot = plot(style, width, height);
    let t = (x - plot.left) / plot.width;
    if count == 0 || !(0. ..=1.).contains(&t) {
        return None;
    }
    Some(((t * (count - 1) as f32).round() as usize).min(count - 1))
}

pub fn readout_text(style: SpectrumStyle, hz: f32, db: f32) -> SharedString {
    let level = if db < range(style).1 {
        "—".into()
    } else {
        db_value(db)
    };
    format!(
        "{} · {} · {} dB · {}",
        hz_label(hz),
        note(hz),
        level,
        zone_of(hz).name
    )
    .into()
}

pub fn point_step(style: SpectrumStyle) -> f32 {
    match style {
        SpectrumStyle::Cover | SpectrumStyle::Silhouette => 3.,
        SpectrumStyle::Zones | SpectrumStyle::Analyzer => 2.,
    }
}

fn range(style: SpectrumStyle) -> (f32, f32) {
    match style {
        SpectrumStyle::Cover => COVER_RANGE,
        SpectrumStyle::Silhouette => SILHOUETTE_RANGE,
        SpectrumStyle::Zones => ZONES_RANGE,
        SpectrumStyle::Analyzer => ANALYZER_RANGE,
    }
}

fn plot(style: SpectrumStyle, width: f32, height: f32) -> Plot {
    let edge = COVER_STROKE / 2.;
    let (left, top, w, h) = match style {
        SpectrumStyle::Cover => (edge, 0., width - edge * 2., height - edge),
        SpectrumStyle::Silhouette => (0., 0., width, height - SILHOUETTE_LABELS),
        SpectrumStyle::Zones => (
            0.,
            ZONES_PLOT.0,
            width,
            height - ZONES_PLOT.0 - ZONES_PLOT.1,
        ),
        SpectrumStyle::Analyzer => (
            0.,
            ANALYZER_PLOT.0,
            width - ANALYZER_GUTTER,
            height - ANALYZER_PLOT.0 - ANALYZER_PLOT.1,
        ),
    };
    let (top_db, bottom_db) = range(style);
    Plot {
        left,
        top,
        width: w.max(1.),
        height: h.max(1.),
        top_db,
        bottom_db,
        corner: if style == SpectrumStyle::Cover {
            COVER_RADIUS - edge
        } else {
            0.
        },
    }
}

pub fn draw(frame: &Frame, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    let origin = bounds.origin;
    let width = f32::from(bounds.size.width);
    let height = f32::from(bounds.size.height);
    if width < 2. || height < 2. || frame.analyzer.frequencies().len() < 2 {
        return;
    }
    let plot = plot(frame.style, width, height);
    match frame.style {
        SpectrumStyle::Cover => cover(frame, origin, plot, height, window, cx),
        SpectrumStyle::Silhouette => silhouette(frame, origin, plot, width, window, cx),
        SpectrumStyle::Zones => zones(frame, origin, plot, window, cx),
        SpectrumStyle::Analyzer => analyzer(frame, origin, plot, height, window, cx),
    }
}

fn cover(
    frame: &Frame,
    origin: Point<Pixels>,
    plot: Plot,
    height: f32,
    window: &mut Window,
    cx: &mut App,
) {
    let dark = frame.cover_luma > COVER_LIGHT_LUMA;
    let ink = if dark { gpui::black() } else { gpui::white() };
    let (fill_top, fill_bottom, stroke) = if dark {
        (0.2, 0.04, 0.72)
    } else {
        (0.26, 0.06, 0.85)
    };
    curve(
        frame,
        origin,
        plot,
        frame.analyzer.levels(),
        cover_ink(frame.palette.accent, frame.cover_luma),
        Some((fill_top, fill_bottom)),
        Some((COVER_STROKE, stroke)),
        window,
    );
    if frame.hovered {
        dividers(
            origin,
            plot,
            height - 10.,
            height,
            ink.opacity(0.45),
            window,
        );
        zone_names(
            frame,
            origin,
            plot,
            height - 16.,
            ink.opacity(0.75),
            window,
            cx,
        );
    }
}

fn cover_ink(accent: Hsla, luma: f32) -> Hsla {
    let bottom = oklch(luma, luma, luma).l;
    let mut ink = to_oklch(accent);
    ink.l = if luma > COVER_LIGHT_LUMA {
        ink.l.min(COVER_INK_DARK_MAX_L).min(bottom - COVER_INK_GAP)
    } else {
        ink.l.max(COVER_INK_LIGHT_MIN_L).max(bottom + COVER_INK_GAP)
    }
    .clamp(0., 1.);
    to_hsla(ink, accent.a)
}

fn silhouette(
    frame: &Frame,
    origin: Point<Pixels>,
    plot: Plot,
    width: f32,
    window: &mut Window,
    cx: &mut App,
) {
    let palette = frame.palette;
    curve(
        frame,
        origin,
        plot,
        frame.analyzer.levels(),
        palette.accent,
        Some((0.32, 0.03)),
        Some((1.6, 1.)),
        window,
    );
    hline(
        origin,
        0.,
        width,
        plot.bottom(),
        palette.fg.opacity(0.14),
        window,
    );
    if frame.hovered {
        dividers(
            origin,
            plot,
            plot.bottom() - 4.,
            plot.bottom() + 5.,
            palette.fg.opacity(0.35),
            window,
        );
        zone_names(
            frame,
            origin,
            plot,
            plot.bottom() + 10.,
            palette.muted.opacity(0.7),
            window,
            cx,
        );
    }
}

fn zones(frame: &Frame, origin: Point<Pixels>, plot: Plot, window: &mut Window, cx: &mut App) {
    let palette = frame.palette;
    dividers(
        origin,
        plot,
        2.,
        plot.bottom(),
        palette.fg.opacity(0.11),
        window,
    );
    zone_names(
        frame,
        origin,
        plot,
        8.,
        palette.muted.opacity(0.85),
        window,
        cx,
    );
    for db in [-24., -48., -72.] {
        let y = plot.y(db);
        hline(
            origin,
            plot.left,
            plot.width,
            y,
            palette.fg.opacity(0.07),
            window,
        );
        let line = shape(
            window,
            db_label(db),
            frame.mono_font,
            9.,
            palette.muted.opacity(0.8),
        );
        put(
            &line,
            origin,
            plot.left + 3.,
            y - 7.,
            Align::Left,
            9.,
            window,
            cx,
        );
    }
    curve(
        frame,
        origin,
        plot,
        frame.analyzer.levels(),
        palette.accent,
        Some((0.3, 0.02)),
        Some((1.5, 1.)),
        window,
    );
    curve(
        frame,
        origin,
        plot,
        frame.analyzer.peaks(),
        palette.fg,
        None,
        Some((0.9, 0.3)),
        window,
    );
    hline(
        origin,
        plot.left,
        plot.width,
        plot.bottom(),
        palette.fg.opacity(0.16),
        window,
    );
    frequency_labels(
        frame,
        origin,
        plot,
        plot.bottom() + 10.,
        "20 Hz",
        "20k",
        window,
        cx,
    );
    readout(
        frame,
        origin,
        plot,
        plot.right() - 4.,
        plot.top + 9.,
        window,
        cx,
    );
}

fn analyzer(
    frame: &Frame,
    origin: Point<Pixels>,
    plot: Plot,
    height: f32,
    window: &mut Window,
    cx: &mut App,
) {
    let palette = frame.palette;
    for (i, zone) in ZONES.iter().enumerate() {
        if i % 2 == 1 {
            let x0 = plot.x(zone.low);
            rect(
                origin,
                x0,
                0.,
                plot.x(zone.high) - x0,
                plot.bottom(),
                palette.fg.opacity(0.028),
                window,
            );
        }
    }
    let mut decade = 10.;
    while decade <= 10_000. {
        for m in 1..=9 {
            let hz = decade * m as f32;
            if (spectrum::MIN_HZ..=spectrum::MAX_HZ).contains(&hz) {
                let alpha = if m == 1 { 0.1 } else { 0.04 };
                vline(
                    origin,
                    plot.x(hz),
                    plot.top,
                    plot.bottom(),
                    palette.fg.opacity(alpha),
                    window,
                );
            }
        }
        decade *= 10.;
    }
    let mut db = plot.top_db;
    while db >= plot.bottom_db {
        let alpha = if (db as i32) % 12 == 0 { 0.085 } else { 0.035 };
        hline(
            origin,
            plot.left,
            plot.width,
            plot.y(db),
            palette.fg.opacity(alpha),
            window,
        );
        db -= 6.;
    }
    dividers(
        origin,
        plot,
        0.,
        plot.bottom(),
        palette.fg.opacity(0.13),
        window,
    );
    for (i, zone) in ZONES.iter().enumerate() {
        let center = (plot.x(zone.low) + plot.x(zone.high)) / 2.;
        let room = plot.x(zone.high) - plot.x(zone.low);
        let name = shape(
            window,
            SharedString::new_static(zone.name),
            frame.name_font,
            9.5,
            palette.fg.opacity(0.78),
        );
        if fits(&name, room) {
            put(&name, origin, center, 10., Align::Center, 9.5, window, cx);
        }
        if let Some(text) = frame.zone_text.get(i) {
            let level = shape(window, text.clone(), frame.mono_font, 10., palette.muted);
            if fits(&level, room) {
                put(&level, origin, center, 24., Align::Center, 10., window, cx);
            }
        }
    }
    hline(
        origin,
        plot.left,
        plot.width,
        plot.top,
        palette.fg.opacity(0.13),
        window,
    );

    let mask = ContentMask {
        bounds: plot.bounds(origin),
    };
    window.with_content_mask(Some(mask), |window| {
        curve(
            frame,
            origin,
            plot,
            frame.analyzer.levels(),
            palette.accent,
            Some((0.34, 0.03)),
            Some((1.4, 1.)),
            window,
        );
        curve(
            frame,
            origin,
            plot,
            frame.analyzer.peaks(),
            palette.fg,
            None,
            Some((0.9, 0.36)),
            window,
        );
    });
    hline(
        origin,
        plot.left,
        plot.width,
        plot.bottom(),
        palette.fg.opacity(0.2),
        window,
    );

    let mut db = plot.top_db;
    while db >= plot.bottom_db {
        let line = shape(window, db_label(db), frame.mono_font, 9.5, palette.muted);
        put(
            &line,
            origin,
            plot.right() + 7.,
            plot.y(db),
            Align::Left,
            9.5,
            window,
            cx,
        );
        db -= 12.;
    }
    let unit = shape(
        window,
        SharedString::new_static("dB"),
        frame.mono_font,
        9.5,
        palette.muted,
    );
    put(
        &unit,
        origin,
        plot.right() + 7.,
        24.,
        Align::Left,
        9.5,
        window,
        cx,
    );
    frequency_labels(
        frame,
        origin,
        plot,
        plot.bottom() + 11.,
        "20",
        "20k Hz",
        window,
        cx,
    );

    let legend_y = height - 8.;
    let readout_right = plot.right() + ANALYZER_GUTTER - 2.;
    let readout_left = readout(frame, origin, plot, readout_right, legend_y, window, cx);
    let legend = shape(
        window,
        frame.legend.clone(),
        frame.mono_font,
        10.,
        palette.muted,
    );
    let room = readout_left.unwrap_or(readout_right) - plot.left;
    if fits(&legend, room) {
        put(
            &legend,
            origin,
            plot.left,
            legend_y,
            Align::Left,
            10.,
            window,
            cx,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn curve(
    frame: &Frame,
    origin: Point<Pixels>,
    plot: Plot,
    values: &[f32],
    color: Hsla,
    fill_alpha: Option<(f32, f32)>,
    stroke: Option<(f32, f32)>,
    window: &mut Window,
) {
    let line = trace(plot, frame.analyzer.frequencies(), values);
    if line.len() < 2 {
        return;
    }
    if let Some((top, bottom)) = fill_alpha {
        window.paint_path(
            fill_path(origin, plot, &line),
            linear_gradient(
                180.,
                linear_color_stop(color.opacity(top), 0.),
                linear_color_stop(color.opacity(bottom), 1.),
            ),
        );
    }
    if let Some((width, alpha)) = stroke
        && let Some(path) = stroke_path(origin, &line, width)
    {
        window.paint_path(path, color.opacity(alpha));
    }
}

fn trace(plot: Plot, freqs: &[f32], values: &[f32]) -> Vec<Point<f32>> {
    let count = freqs.len().min(values.len());
    let mut line = Vec::with_capacity(count * 3);
    if count == 0 {
        return line;
    }
    let at = |i: usize| {
        let x = plot.x(freqs[i]);
        point(x, plot.y(values[i]).min(plot.floor(x)))
    };
    let mut prev = at(0);
    let mut start = prev;
    line.push(prev);
    for i in 1..count {
        let next = at(i);
        let end = point((prev.x + next.x) / 2., (prev.y + next.y) / 2.);
        let bend = (start.x - 2. * prev.x + end.x).hypot(start.y - 2. * prev.y + end.y);
        let steps = ((bend / (4. * FLATNESS)).sqrt().ceil() as usize).clamp(1, MAX_STEPS);
        for k in 1..=steps {
            let t = k as f32 / steps as f32;
            let u = 1. - t;
            line.push(point(
                u * u * start.x + 2. * u * t * prev.x + t * t * end.x,
                u * u * start.y + 2. * u * t * prev.y + t * t * end.y,
            ));
        }
        start = end;
        prev = next;
    }
    line.push(prev);
    line
}

fn fill_path(origin: Point<Pixels>, plot: Plot, line: &[Point<f32>]) -> Path<Pixels> {
    let at = |x: f32, y: f32| point(origin.x + px(x), origin.y + px(y));
    let mut path = Path::new(at(line[0].x, line[0].y));
    path.vertices.reserve(line.len() * 6);
    for pair in line.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let (top_a, top_b) = (at(a.x, a.y), at(b.x, b.y));
        let (bottom_a, bottom_b) = (at(a.x, plot.floor(a.x)), at(b.x, plot.floor(b.x)));
        path.push_triangle((top_a, top_b, bottom_b), SOLID);
        path.push_triangle((top_a, bottom_b, bottom_a), SOLID);
    }
    path
}

fn stroke_path(origin: Point<Pixels>, line: &[Point<f32>], width: f32) -> Option<Path<Pixels>> {
    let half = width / 2.;
    let normal = |a: Point<f32>, b: Point<f32>| {
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let len = dx.hypot(dy);
        (len > 1e-3).then(|| point(-dy / len, dx / len))
    };
    let at = |x: f32, y: f32| point(origin.x + px(x), origin.y + px(y));
    let mut path: Option<Path<Pixels>> = None;
    let mut last = None;
    for (j, &q) in line.iter().enumerate() {
        let incoming = j.checked_sub(1).and_then(|i| normal(line[i], q));
        let outgoing = line.get(j + 1).and_then(|&b| normal(q, b));
        let n = match (incoming, outgoing) {
            (Some(a), Some(b)) => {
                let (mx, my) = (a.x + b.x, a.y + b.y);
                let len = mx.hypot(my);
                if len < 1e-3 {
                    b
                } else {
                    let scale = len / (mx * b.x + my * b.y).max(MITER_MIN * len);
                    point(mx / len * scale, my / len * scale)
                }
            }
            (Some(n), None) | (None, Some(n)) => n,
            (None, None) => continue,
        };
        let left = at(q.x + n.x * half, q.y + n.y * half);
        let right = at(q.x - n.x * half, q.y - n.y * half);
        if let Some((left_0, right_0)) = last {
            let path = path.get_or_insert_with(|| {
                let mut path = Path::new(left_0);
                path.vertices.reserve(line.len() * 6);
                path
            });
            path.push_triangle((left_0, right_0, left), SOLID);
            path.push_triangle((right_0, right, left), SOLID);
        }
        last = Some((left, right));
    }
    path
}

fn rect(origin: Point<Pixels>, x: f32, y: f32, w: f32, h: f32, color: Hsla, window: &mut Window) {
    window.paint_quad(fill(
        Bounds::new(
            point(origin.x + px(x), origin.y + px(y)),
            size(px(w), px(h)),
        ),
        color,
    ));
}

fn hline(origin: Point<Pixels>, x: f32, w: f32, y: f32, color: Hsla, window: &mut Window) {
    rect(origin, x, y - 0.5, w, 1., color, window);
}

fn vline(origin: Point<Pixels>, x: f32, y0: f32, y1: f32, color: Hsla, window: &mut Window) {
    rect(origin, x - 0.5, y0, 1., y1 - y0, color, window);
}

fn dividers(origin: Point<Pixels>, plot: Plot, y0: f32, y1: f32, color: Hsla, window: &mut Window) {
    for zone in &ZONES[1..] {
        vline(origin, plot.x(zone.low), y0, y1, color, window);
    }
}

fn zone_names(
    frame: &Frame,
    origin: Point<Pixels>,
    plot: Plot,
    y: f32,
    color: Hsla,
    window: &mut Window,
    cx: &mut App,
) {
    for zone in &ZONES {
        let line = shape(
            window,
            SharedString::new_static(zone.name),
            frame.name_font,
            9.,
            color,
        );
        let center = (plot.x(zone.low) + plot.x(zone.high)) / 2.;
        if fits(&line, plot.x(zone.high) - plot.x(zone.low)) {
            put(&line, origin, center, y, Align::Center, 9., window, cx);
        }
    }
}

fn fits(line: &ShapedLine, room: f32) -> bool {
    f32::from(line.width()) + LABEL_PAD * 2. <= room
}

#[allow(clippy::too_many_arguments)]
fn frequency_labels(
    frame: &Frame,
    origin: Point<Pixels>,
    plot: Plot,
    y: f32,
    first: &'static str,
    last: &'static str,
    window: &mut Window,
    cx: &mut App,
) {
    let muted = frame.palette.muted;
    let first = shape(
        window,
        SharedString::new_static(first),
        frame.ui_font,
        9.5,
        muted,
    );
    let last = shape(
        window,
        SharedString::new_static(last),
        frame.ui_font,
        9.5,
        muted,
    );
    let free = (
        plot.left + f32::from(first.width()) + LABEL_PAD,
        plot.right() - f32::from(last.width()) - LABEL_PAD,
    );
    for (hz, text) in TICKS {
        let line = shape(
            window,
            SharedString::new_static(text),
            frame.ui_font,
            9.5,
            muted,
        );
        let half = f32::from(line.width()) / 2.;
        let x = plot.x(hz);
        if x - half >= free.0 && x + half <= free.1 {
            put(&line, origin, x, y, Align::Center, 9.5, window, cx);
        }
    }
    put(&first, origin, plot.left, y, Align::Left, 9.5, window, cx);
    put(
        &last,
        origin,
        plot.right(),
        y,
        Align::Right,
        9.5,
        window,
        cx,
    );
}

fn readout(
    frame: &Frame,
    origin: Point<Pixels>,
    plot: Plot,
    right: f32,
    y: f32,
    window: &mut Window,
    cx: &mut App,
) -> Option<f32> {
    let (i, text) = frame.readout?;
    let freqs = frame.analyzer.frequencies();
    let levels = frame.analyzer.levels();
    if i >= freqs.len() || i >= levels.len() {
        return None;
    }
    let (hz, db) = (freqs[i], levels[i]);
    let px_x = plot.x(hz);
    vline(
        origin,
        px_x,
        plot.top,
        plot.bottom(),
        frame.palette.fg.opacity(0.28),
        window,
    );
    let dot = 3.;
    let dy = plot.y(db);
    window.paint_quad(
        fill(
            Bounds::new(
                point(origin.x + px(px_x - dot), origin.y + px(dy - dot)),
                size(px(dot * 2.), px(dot * 2.)),
            ),
            frame.palette.accent,
        )
        .corner_radii(px(dot)),
    );
    let line = shape(
        window,
        text.clone(),
        frame.mono_font,
        10.5,
        frame.palette.fg,
    );
    put(&line, origin, right, y, Align::Right, 10.5, window, cx);
    Some(right - f32::from(line.width()))
}

fn shape(
    window: &mut Window,
    text: SharedString,
    font: &Font,
    size: f32,
    color: Hsla,
) -> ShapedLine {
    let run = TextRun {
        len: text.len(),
        font: font.clone(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window
        .text_system()
        .shape_line(text, px(size), &[run], None)
}

#[allow(clippy::too_many_arguments)]
fn put(
    line: &ShapedLine,
    origin: Point<Pixels>,
    x: f32,
    y_center: f32,
    align: Align,
    size: f32,
    window: &mut Window,
    cx: &mut App,
) {
    let line_height = size * 1.3;
    let width = f32::from(line.width());
    let left = match align {
        Align::Left => x,
        Align::Center => x - width / 2.,
        Align::Right => x - width,
    };
    let at = point(
        origin.x + px(left),
        origin.y + px(y_center - line_height / 2.),
    );
    if let Err(e) = line.paint(at, px(line_height), TextAlign::Left, None, window, cx) {
        log::debug!("spectrum: label paint failed: {e}");
    }
}

pub fn semibold(font: &Font) -> Font {
    Font {
        weight: FontWeight::SEMIBOLD,
        ..font.clone()
    }
}

fn db_label(db: f32) -> SharedString {
    match db as i32 {
        -12 => SharedString::new_static("−12"),
        -24 => SharedString::new_static("−24"),
        -36 => SharedString::new_static("−36"),
        -48 => SharedString::new_static("−48"),
        -60 => SharedString::new_static("−60"),
        -72 => SharedString::new_static("−72"),
        -84 => SharedString::new_static("−84"),
        -96 => SharedString::new_static("−96"),
        other => format!("−{}", -other).into(),
    }
}

pub fn db_value(db: f32) -> String {
    if db <= -199. {
        return "—".into();
    }
    let sign = if db < -0.05 {
        "−"
    } else if db > 0.05 {
        "+"
    } else {
        ""
    };
    format!("{sign}{:.1}", db.abs())
}

fn hz_label(hz: f32) -> String {
    if hz < 100. {
        format!("{hz:.1} Hz")
    } else if hz < 1_000. {
        format!("{hz:.0} Hz")
    } else if hz < 10_000. {
        format!("{:.2} kHz", hz / 1_000.)
    } else {
        format!("{:.1} kHz", hz / 1_000.)
    }
}

fn note(hz: f32) -> String {
    let midi = (69. + 12. * (hz / 440.).log2()).round() as i32;
    let name = NOTES[midi.rem_euclid(12) as usize];
    format!("{name}{}", midi.div_euclid(12) - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cover_skin::color::Oklch;

    #[test]
    fn labels_read_like_the_scale() {
        assert_eq!(db_value(-38.44), "−38.4");
        assert_eq!(db_value(0.), "0.0");
        assert_eq!(db_value(-200.), "—");
        assert_eq!(
            readout_text(SpectrumStyle::Analyzer, 100., -96.).to_string(),
            "100 Hz · G2 · −96.0 dB · BASS"
        );
        assert_eq!(
            readout_text(SpectrumStyle::Analyzer, 100., -150.).to_string(),
            "100 Hz · G2 · — dB · BASS"
        );
        assert_eq!(
            readout_text(SpectrumStyle::Zones, 100., -90.).to_string(),
            "100 Hz · G2 · — dB · BASS"
        );
        assert_eq!(hz_label(56.3), "56.3 Hz");
        assert_eq!(hz_label(1_240.), "1.24 kHz");
        assert_eq!(hz_label(12_500.), "12.5 kHz");
        assert_eq!(note(440.), "A4");
        assert_eq!(note(1_244.5), "D#6");
        assert_eq!(note(27.5), "A0");
    }

    #[test]
    fn plot_maps_the_range_onto_its_height() {
        let p = plot(SpectrumStyle::Analyzer, 664., 286.);
        assert_eq!(p.y(-12.), ANALYZER_PLOT.0);
        assert_eq!(p.y(-96.), 286. - ANALYZER_PLOT.1);
        assert_eq!(p.y(-300.), p.bottom());
        assert!((p.x(20.) - p.left).abs() < 1e-3);
        assert!((p.x(20_000.) - p.right()).abs() < 1e-3);
    }

    #[test]
    fn cover_curve_spans_the_cover_inside_its_corners() {
        let p = plot(SpectrumStyle::Cover, 300., 90.);
        assert!(p.left < 1. && p.right() > 299.);
        assert!((p.floor(p.left) - (p.bottom() - p.corner)).abs() < 1e-3);
        assert_eq!(p.floor(p.left + p.corner), p.bottom());
        assert_eq!(p.floor(150.), p.bottom());
        let x = p.right() - p.corner * (1. - std::f32::consts::FRAC_1_SQRT_2);
        let (cx, cy) = (p.right() - p.corner, p.bottom() - p.corner);
        assert!(((x - cx).hypot(p.floor(x) - cy) - p.corner).abs() < 1e-3);
        assert_eq!(plot(SpectrumStyle::Zones, 300., 150.).corner, 0.);
    }

    fn axis(count: usize) -> Vec<f32> {
        (0..count)
            .map(|i| spectrum::frequency(i as f32 / (count - 1) as f32))
            .collect()
    }

    fn area(path: &Path<Pixels>) -> f32 {
        path.vertices
            .chunks(3)
            .map(|t| {
                let [a, b, c] = [0, 1, 2].map(|i| t[i].xy_position.map(f32::from));
                ((b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y)).abs() / 2.
            })
            .sum()
    }

    #[test]
    fn fill_covers_the_area_under_the_curve_once() {
        let p = plot(SpectrumStyle::Zones, 600., 150.);
        let line = trace(p, &axis(300), &[-48.; 300]);
        assert!(line.windows(2).all(|w| w[1].x >= w[0].x));
        let expected = p.width * (p.bottom() - p.y(-48.));
        let got = area(&fill_path(point(px(0.), px(0.)), p, &line));
        assert!(
            (got - expected).abs() < expected * 1e-3,
            "{got} vs {expected}"
        );
    }

    #[test]
    fn stroke_keeps_its_width_on_a_slope() {
        let p = plot(SpectrumStyle::Zones, 600., 150.);
        let levels: Vec<f32> = (0..300).map(|i| -20. - i as f32 * 0.2).collect();
        let line = trace(p, &axis(300), &levels);
        let path = stroke_path(point(px(0.), px(0.)), &line, 2.).unwrap();
        let (a, b) = (line[0], line[line.len() - 1]);
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let len = dx.hypot(dy);
        for v in &path.vertices {
            let q = v.xy_position.map(f32::from);
            let distance = ((q.x - a.x) * dy - (q.y - a.y) * dx).abs() / len;
            assert!((distance - 1.).abs() < 1e-2, "{distance}");
        }
    }

    #[test]
    fn cover_ink_keeps_a_readable_accent_and_relights_only_its_lightness() {
        let accent = to_hsla(
            Oklch {
                l: 0.75,
                c: 0.1,
                h: 265.,
            },
            1.,
        );
        let kept = to_oklch(cover_ink(accent, 0.1));
        assert!((kept.l - 0.75).abs() < 0.01, "{kept:?}");
        let dark = to_oklch(cover_ink(accent, 0.9));
        assert!(dark.l <= COVER_INK_DARK_MAX_L + 0.01, "{dark:?}");
        assert!(
            (dark.h - 265.).abs() < 2. && (dark.c - 0.1).abs() < 0.02,
            "{dark:?}"
        );
        let dim = to_hsla(
            Oklch {
                l: 0.5,
                c: 0.1,
                h: 30.,
            },
            0.5,
        );
        let light = cover_ink(dim, 0.1);
        assert!((light.a - 0.5).abs() < 1e-6);
        let light = to_oklch(light);
        assert!(light.l >= COVER_INK_LIGHT_MIN_L - 0.01, "{light:?}");
        assert!((light.h - 30.).abs() < 2., "{light:?}");
    }

    #[test]
    fn cover_ink_keeps_its_distance_from_a_mid_tone_cover() {
        let accent = to_hsla(
            Oklch {
                l: 0.75,
                c: 0.1,
                h: 265.,
            },
            1.,
        );
        for luma in [0.42, 0.48, 0.52, 0.58] {
            let bottom = oklch(luma, luma, luma).l;
            let ink = to_oklch(cover_ink(accent, luma));
            assert!(
                (ink.l - bottom).abs() >= COVER_INK_GAP - 0.01,
                "{luma}: {ink:?}"
            );
        }
    }
}

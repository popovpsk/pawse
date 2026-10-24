use equalizer::{Band, Response};
use gpui::{Bounds, Hsla, Path, PathBuilder, Pixels, Size, Window, fill, point, px, size};

pub const HEIGHT: f32 = 240.;
pub const LEFT: f32 = 40.;
pub const RIGHT: f32 = 16.;
pub const TOP: f32 = 12.;
pub const BOTTOM: f32 = 24.;
pub const NODE: f32 = 22.;

const DB_RANGE: f64 = 15.;
const FREQ_MIN: f64 = 20.;
const FREQ_MAX: f64 = 20_000.;
const DISPLAY_RATE: f64 = 48_000.;
const POINTS: usize = 240;
const STROKE: f32 = 2.;

pub const FREQ_TICKS: [(f64, &str); 10] = [
    (20., "20"),
    (50., "50"),
    (100., "100"),
    (200., "200"),
    (500., "500"),
    (1_000., "1k"),
    (2_000., "2k"),
    (5_000., "5k"),
    (10_000., "10k"),
    (20_000., "20k"),
];

pub const DB_TICKS: [(f64, &str); 5] = [
    (12., "+12"),
    (6., "+6"),
    (0., "0"),
    (-6., "−6"),
    (-12., "−12"),
];

pub fn x_of(freq: f64) -> f32 {
    ((freq.clamp(FREQ_MIN, FREQ_MAX) / FREQ_MIN).ln() / (FREQ_MAX / FREQ_MIN).ln()) as f32
}

pub fn freq_at(x: f32) -> f64 {
    FREQ_MIN * (FREQ_MAX / FREQ_MIN).powf(f64::from(x.clamp(0., 1.)))
}

pub fn y_of(db: f64) -> f32 {
    (0.5 - db / (2. * DB_RANGE)).clamp(0., 1.) as f32
}

pub fn db_at(y: f32) -> f64 {
    DB_RANGE * (1. - 2. * f64::from(y.clamp(0., 1.)))
}

pub fn curve(bands: &[Band]) -> Vec<f32> {
    let response = Response::new(bands, DISPLAY_RATE);
    (0..POINTS)
        .map(|i| y_of(response.db(freq_at(i as f32 / (POINTS - 1) as f32))))
        .collect()
}

#[derive(Default)]
pub struct Plot {
    pub bounds: Bounds<Pixels>,
    pub curve: Vec<f32>,
    pub revision: u64,
    path: Option<(Size<Pixels>, u64, Path<Pixels>)>,
}

impl Plot {
    pub fn set_curve(&mut self, curve: Vec<f32>) {
        self.curve = curve;
        self.revision += 1;
    }

    pub fn fraction(&self, position: gpui::Point<Pixels>) -> Option<(f32, f32)> {
        let Bounds { origin, size } = self.bounds;
        if size.width <= px(0.) || size.height <= px(0.) {
            return None;
        }
        Some((
            (position.x - origin.x) / size.width,
            (position.y - origin.y) / size.height,
        ))
    }

    pub fn paint(&mut self, bounds: Bounds<Pixels>, colors: Colors, window: &mut Window) {
        let Bounds { origin, size: area } = bounds;
        for (freq, _) in FREQ_TICKS {
            let x = origin.x + area.width * x_of(freq);
            window.paint_quad(fill(
                Bounds::new(point(x, origin.y), size(px(1.), area.height)),
                colors.grid,
            ));
        }
        for (db, _) in DB_TICKS {
            let y = origin.y + area.height * y_of(db);
            let color = if db == 0. { colors.zero } else { colors.grid };
            window.paint_quad(fill(
                Bounds::new(point(origin.x, y), size(area.width, px(1.))),
                color,
            ));
        }
        let fresh = matches!(&self.path, Some((at, revision, _)) if *at == area && *revision == self.revision);
        if !fresh {
            self.path = self.build(area).map(|path| (area, self.revision, path));
        }
        if let Some((_, _, path)) = &self.path {
            let mut path = path.clone();
            path.bounds.origin += origin;
            for vertex in &mut path.vertices {
                vertex.xy_position += origin;
            }
            window.paint_path(path, colors.curve);
        }
    }

    fn build(&self, area: Size<Pixels>) -> Option<Path<Pixels>> {
        if self.curve.len() < 2 {
            return None;
        }
        let last = (self.curve.len() - 1) as f32;
        let at = |i: usize, y: f32| point(area.width * (i as f32 / last), area.height * y);
        let mut builder = PathBuilder::stroke(px(STROKE));
        builder.move_to(at(0, self.curve[0]));
        for (i, &y) in self.curve.iter().enumerate().skip(1) {
            builder.line_to(at(i, y));
        }
        builder.build().ok()
    }
}

#[derive(Clone, Copy)]
pub struct Colors {
    pub grid: Hsla,
    pub zero: Hsla,
    pub curve: Hsla,
}

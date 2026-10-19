use std::hint::black_box;
use std::time::{Duration, Instant};

use text_blur::plane::{Plane, overlap};
use text_blur::{Coverage, Mask, Stamp, bake, level_factor};

const LINE_CHARS: [usize; 49] = [
    38, 36, 43, 39, 36, 37, 28, 13, 34, 20, 23, 22, 41, 37, 49, 31, 30, 5, 30, 36, 30, 61, 31, 33,
    34, 31, 34, 20, 23, 22, 41, 37, 49, 31, 30, 5, 30, 10, 26, 88, 10, 11, 41, 37, 49, 31, 30, 5,
    30,
];

struct Song {
    glyphs: Vec<Coverage>,
    lines: Vec<Vec<(i32, i32, usize)>>,
}

fn glyph(size: f32, seed: usize) -> Coverage {
    let width = (size * (0.35 + 0.05 * (seed % 5) as f32)).ceil() as usize;
    let tall = seed.is_multiple_of(4);
    let height = (size * if tall { 0.95 } else { 0.72 }).ceil() as usize;
    let alpha = (0..width * height)
        .map(|i| {
            let (x, y) = (i % width, i / width);
            let stroke = x < width / 4 || x + width / 4 >= width || y < height / 6;
            let edge = x == 0 || y == 0 || x + 1 == width || y + 1 == height;
            match (stroke, edge) {
                (true, true) => 110,
                (true, false) => 255,
                _ => 0,
            }
        })
        .collect();
    Coverage {
        left: 1,
        top: -((size * 0.72).ceil() as i32),
        width,
        height,
        alpha,
    }
}

fn song(font: f32, panel: f32, scale: f32) -> Song {
    let size = font * scale;
    let glyphs: Vec<Coverage> = (0..40).map(|seed| glyph(size, seed)).collect();
    let advance = size * 0.52;
    let line_height = size * 1.25;
    let width = (panel - 32.) * scale;
    let lines = LINE_CHARS
        .iter()
        .enumerate()
        .map(|(line, &chars)| {
            let (mut x, mut y) = (0f32, size);
            (0..chars)
                .filter_map(|ix| {
                    if x + advance > width {
                        x = 0.;
                        y += line_height;
                    }
                    let at = x;
                    x += advance;
                    let seed = (line * 7 + ix * 13) % 47;
                    (seed < 40 && ix % 6 != 5).then_some((at as i32, y as i32, seed))
                })
                .collect()
        })
        .collect();
    Song { glyphs, lines }
}

fn stamps<'a>(song: &'a Song, line: &[(i32, i32, usize)]) -> Vec<Stamp<'a>> {
    line.iter()
        .map(|&(x, y, glyph)| Stamp {
            x,
            y,
            coverage: &song.glyphs[glyph],
        })
        .collect()
}

fn median(mut samples: Vec<Duration>) -> f64 {
    samples.sort();
    samples[samples.len() / 2].as_secs_f64() * 1e3
}

fn time(rounds: usize, mut f: impl FnMut()) -> f64 {
    f();
    median(
        (0..rounds)
            .map(|_| {
                let started = Instant::now();
                f();
                started.elapsed()
            })
            .collect(),
    )
}

fn main() {
    let rounds = std::env::args()
        .skip(1)
        .find_map(|arg| arg.parse().ok())
        .unwrap_or(30);
    println!(
        "text_blur bake, {} lines, median of {rounds}",
        LINE_CHARS.len()
    );
    for (font, panel) in [(16., 360.), (24., 560.)] {
        for scale in [1., 2.] {
            let song = song(font, panel, scale);
            let lines: Vec<Vec<Stamp>> = song.lines.iter().map(|l| stamps(&song, l)).collect();
            let step = level_factor(text_blur::MAX_SIGMA * scale);

            let total = time(rounds, || {
                for line in &lines {
                    black_box(bake(line, scale, [0.8, 0.8, 0.9, 1.]));
                }
            });
            let compose = time(rounds, || {
                for line in &lines {
                    black_box(Mask::compose(line, step));
                }
            });
            let masks: Vec<Plane> = lines
                .iter()
                .map(|line| Mask::compose(line, step).unwrap().plane)
                .collect();
            let halving = time(rounds, || {
                for mask in &masks {
                    let mut plane = mask.halved();
                    while plane.width >= step
                        && plane.height >= step
                        && plane.width.is_multiple_of(2)
                    {
                        plane = plane.halved();
                    }
                    black_box(plane);
                }
            });
            let first = level_factor(scale);
            let level0: Vec<Plane> = masks
                .iter()
                .map(|mask| {
                    let mut plane = mask.clone();
                    let mut factor = 1;
                    while factor < first {
                        plane = plane.halved();
                        factor *= 2;
                    }
                    plane.padded(4)
                })
                .collect();
            let gaussian = time(rounds, || {
                for plane in &level0 {
                    let mut plane = plane.clone();
                    plane.gaussian(0.87);
                    black_box(plane);
                }
            });
            let ratio = time(rounds, || {
                for plane in &level0 {
                    black_box(overlap(plane, plane, 0, 0));
                }
            });
            let bgra = time(rounds, || {
                for plane in &level0 {
                    black_box(plane.to_bgra([0.8, 0.8, 0.9, 1.]));
                }
            });
            let bytes: usize = lines
                .iter()
                .filter_map(|line| bake(line, scale, [1.; 4]))
                .flat_map(|pyramid| pyramid.levels)
                .map(|level| level.bgra.len())
                .sum();
            println!(
                "{font}pt / {panel}pt @{scale}x: bake {total:.2} ms per song ({:.0} us per line), images {:.2} MB",
                total * 1e3 / lines.len() as f64,
                bytes as f64 / 1e6
            );
            println!(
                "    compose {compose:.2} ms, halving to the last level {halving:.2} ms, level-0 gaussian {gaussian:.2} ms, overlap {ratio:.2} ms, bgra {bgra:.2} ms"
            );
        }
    }
}

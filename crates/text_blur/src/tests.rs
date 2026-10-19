use crate::mix::sharper_opacity;
use crate::plane::{Plane, overlap};
use crate::{Coverage, LEVEL_SIGMAS, LEVELS, MAX_SIGMA, Mask, Stamp, bake, level_factor, mix};

fn plane(width: usize, height: usize, mut fill: impl FnMut(usize, usize) -> f32) -> Plane {
    Plane {
        data: (0..width * height)
            .map(|i| fill(i % width, i / width))
            .collect(),
        width,
        height,
    }
}

fn noise(width: usize, height: usize, seed: u32) -> Plane {
    let mut state = seed.wrapping_mul(2_654_435_761).max(1);
    plane(width, height, |_, _| {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        if state.is_multiple_of(3) {
            0.
        } else {
            (state % 1000) as f32 / 999.
        }
    })
}

fn naive_gaussian(plane: &mut Plane, sigma: f32) {
    let radius = (sigma * 3.).ceil() as isize;
    let mut kernel: Vec<f32> = (-radius..=radius)
        .map(|i| (-((i * i) as f32) / (2. * sigma * sigma)).exp())
        .collect();
    let sum: f32 = kernel.iter().sum();
    kernel.iter_mut().for_each(|w| *w /= sum);
    let (width, height) = (plane.width as isize, plane.height as isize);
    let mut tmp = vec![0f32; plane.data.len()];
    for y in 0..height {
        for x in 0..width {
            let mut acc = 0.;
            for (k, w) in kernel.iter().enumerate() {
                let sx = x + k as isize - radius;
                if (0..width).contains(&sx) {
                    acc += w * plane.data[(y * width + sx) as usize];
                }
            }
            tmp[(y * width + x) as usize] = acc;
        }
    }
    for y in 0..height {
        for x in 0..width {
            let mut acc = 0.;
            for (k, w) in kernel.iter().enumerate() {
                let sy = y + k as isize - radius;
                if (0..height).contains(&sy) {
                    acc += w * tmp[(sy * width + x) as usize];
                }
            }
            plane.data[(y * width + x) as usize] = acc;
        }
    }
}

fn glyph(width: usize, height: usize) -> Coverage {
    let alpha = (0..width * height)
        .map(|i| {
            let (x, y) = (i % width, i / width);
            let edge = x == 0 || y == 0 || x + 1 == width || y + 1 == height;
            if edge { 96 } else { 255 }
        })
        .collect();
    Coverage {
        left: 1,
        top: -(height as i32),
        width,
        height,
        alpha,
    }
}

fn level_alpha(level: &crate::Level) -> impl Iterator<Item = (usize, usize, f32)> + '_ {
    let stride = level.width + 2;
    (0..level.height).flat_map(move |y| {
        (0..level.width).map(move |x| {
            let a = level.bgra[((y + 1) * stride + x + 1) * 4 + 3];
            (x, y, a as f32 / 255.)
        })
    })
}

#[test]
fn halving_keeps_the_ink() {
    let sharp = plane(
        16,
        8,
        |x, y| if (x + y).is_multiple_of(3) { 1. } else { 0. },
    );
    let halved = sharp.halved().halved();
    assert_eq!((halved.width, halved.height), (4, 2));
    assert!((sharp.ink() - halved.ink() * 16.).abs() < 1e-3);
}

#[test]
fn the_gaussian_spreads_a_dot_without_losing_ink() {
    let mut dot = plane(21, 21, |x, y| if x == 10 && y == 10 { 1. } else { 0. });
    dot.gaussian(2.);
    assert!((dot.ink() - 1.).abs() < 1e-3);
    assert!(dot.data[10 * 21 + 10] < 0.1);
    assert!(dot.data[10 * 21 + 12] > 0.);
}

#[test]
fn the_gaussian_matches_the_plain_convolution_bit_for_bit() {
    for (width, height, sigma, seed) in [
        (37, 23, 0.87, 1),
        (64, 16, 2.5, 2),
        (5, 3, 4., 3),
        (1, 9, 1.2, 4),
        (40, 40, 0.3, 5),
    ] {
        let mut fast = noise(width, height, seed).padded(3);
        let mut plain = fast.clone();
        fast.gaussian(sigma);
        naive_gaussian(&mut plain, sigma);
        assert_eq!(fast, plain, "{width}x{height} sigma {sigma}");
    }
}

#[test]
fn padding_surrounds_the_plane_with_zeros() {
    let source = noise(3, 2, 7);
    let padded = source.padded(2);
    assert_eq!((padded.width, padded.height), (7, 6));
    assert_eq!(&padded.row(2)[2..5], source.row(0));
    assert_eq!(padded.ink(), source.ink());
}

#[test]
fn the_overlap_ratio_reads_the_sharper_plane_at_its_offset() {
    let sharper = plane(4, 3, |x, _| x as f32);
    let blurrier = plane(12, 9, |x, y| {
        if (5..9).contains(&x) && (2..5).contains(&y) {
            0.5
        } else {
            0.
        }
    });
    assert!((overlap(&sharper, &blurrier, 5, 2) - 0.5).abs() < 1e-6);
    assert_eq!(overlap(&sharper, &blurrier, 0, 0), 0.);
}

#[test]
fn levels_are_stored_one_texel_per_sigma() {
    assert_eq!(LEVEL_SIGMAS.map(|s| level_factor(s * 2.)), [2, 4, 8, 16]);
    assert_eq!(LEVEL_SIGMAS.map(level_factor), [1, 2, 4, 8]);
    assert_eq!(level_factor(0.5), 1);
}

#[test]
fn no_blur_is_the_text_alone_and_full_blur_the_last_level() {
    let ratios = [0.7, 0.5, 0.35, 0.2];
    assert_eq!(mix(0., &ratios).layers, [None, None]);
    assert_eq!(mix(0., &ratios).text, 1.);
    let full = mix(MAX_SIGMA + 1., &ratios);
    assert_eq!(full.text, 0.);
    assert_eq!(full.layers, [Some((3, 1.)), None]);
}

#[test]
fn a_level_sigma_shows_that_level_alone() {
    let ratios = [0.7, 0.5, 0.35, 0.2];
    let at = mix(LEVEL_SIGMAS[1], &ratios);
    assert_eq!(at.text, 0.);
    assert_eq!(at.layers, [Some((2, 0.)), Some((1, 1.))]);
}

#[test]
fn between_levels_the_blurrier_one_lies_under_the_sharper() {
    let ratios = [0.7, 0.5, 0.35, 0.2];
    let half = mix(LEVEL_SIGMAS[0] / 2., &ratios);
    assert_eq!(half.layers[0], Some((0, 0.5)));
    assert!((half.text - 0.5 / (1. - 0.5 * 0.7)).abs() < 1e-5);
    let between = mix(3., &ratios);
    let (under, over) = (between.layers[0].unwrap(), between.layers[1].unwrap());
    assert_eq!((under.0, over.0), (2, 1));
    assert!(under.1 > 0. && under.1 < 1. && over.1 > 0. && over.1 <= 1.);
}

#[test]
fn the_sharper_opacity_keeps_the_total_ink_of_a_linear_crossfade() {
    let sharp = plane(32, 16, |x, y| {
        if (8..24).contains(&x) && (4..12).contains(&y) {
            1.
        } else {
            0.
        }
    });
    let mut blurred = sharp.clone();
    blurred.gaussian(2.);
    let r = overlap(&sharp, &blurred, 0, 0);
    for t in [0.25f32, 0.5, 0.75] {
        let p = sharper_opacity(t, r);
        let (mut got, mut linear) = (0f32, 0f32);
        for (a, b) in sharp.data.iter().zip(&blurred.data) {
            let under = b * t;
            got += a * p + under * (1. - a * p);
            linear += (1. - t) * a + t * b;
        }
        assert!((got / linear - 1.).abs() < 1e-3, "t {t}: {got} vs {linear}");
    }
}

#[test]
fn images_carry_a_transparent_ring_and_bgra_color() {
    let level = plane(2, 2, |_, _| 1.);
    let bytes = level.to_bgra([1., 0.5, 0., 1.]);
    assert_eq!(bytes.len(), 4 * 4 * 4);
    assert_eq!(&bytes[0..4], &[0, 128, 255, 0]);
    let inner = (4 + 1) * 4;
    assert_eq!(&bytes[inner..inner + 4], &[0, 128, 255, 255]);
}

#[test]
fn the_mask_covers_the_ink_on_an_aligned_grid() {
    let small = glyph(3, 5);
    let stamps = [
        Stamp {
            x: -7,
            y: 4,
            coverage: &small,
        },
        Stamp {
            x: 20,
            y: 30,
            coverage: &small,
        },
    ];
    let mask = Mask::compose(&stamps, 8).unwrap();
    assert_eq!((mask.left, mask.top), (-8, -8));
    assert_eq!((mask.plane.width, mask.plane.height), (32, 40));
    let at = |x: i32, y: i32| mask.plane.data[((y - mask.top) * 32 + x - mask.left) as usize];
    assert_eq!(at(-6, -1), 96. / 255.);
    assert_eq!(at(-5, 1), 1.);
    assert_eq!(at(-7, 1), 0.);
}

#[test]
fn nothing_drawn_bakes_nothing() {
    let empty = Coverage {
        left: 0,
        top: 0,
        width: 0,
        height: 0,
        alpha: Vec::new(),
    };
    assert!(bake(&[], 2., [1.; 4]).is_none());
    assert!(
        bake(
            &[Stamp {
                x: 0,
                y: 0,
                coverage: &empty
            }],
            2.,
            [1.; 4]
        )
        .is_none()
    );
}

#[test]
fn every_level_keeps_the_ink_in_place_and_inside_its_bounds() {
    let tall = glyph(16, 32);
    let wide = glyph(32, 16);
    let stamps = [
        Stamp {
            x: 15,
            y: 32,
            coverage: &tall,
        },
        Stamp {
            x: 47,
            y: 48,
            coverage: &wide,
        },
        Stamp {
            x: 79,
            y: 32,
            coverage: &tall,
        },
    ];
    let mask = Mask::compose(&stamps, 1).unwrap();
    let (mut ink, mut cx, mut cy) = (0f32, 0f32, 0f32);
    for (i, &a) in mask.plane.data.iter().enumerate() {
        let x = (i % mask.plane.width) as f32 + mask.left as f32 + 0.5;
        let y = (i / mask.plane.width) as f32 + mask.top as f32 + 0.5;
        ink += a;
        cx += a * x;
        cy += a * y;
    }
    let (cx, cy) = (cx / ink, cy / ink);

    for scale in [0.75, 1., 1.25, 1.5, 2.] {
        let pyramid = bake(&stamps, scale, [1.; 4]).unwrap();
        assert_eq!(pyramid.levels.len(), LEVELS);
        for (k, level) in pyramid.levels.iter().enumerate() {
            let f = level.factor as f32;
            let (mut got, mut gx, mut gy, mut border) = (0f32, 0f32, 0f32, 0f32);
            for (x, y, a) in level_alpha(level) {
                let px = level.left as f32 + (x as f32 + 0.5) * f;
                let py = level.top as f32 + (y as f32 + 0.5) * f;
                got += a * f * f;
                gx += a * f * f * px;
                gy += a * f * f * py;
                if x == 0 || y == 0 || x + 1 == level.width || y + 1 == level.height {
                    border = border.max(a);
                }
            }
            let what = format!("scale {scale} level {k}");
            assert!((got / ink - 1.).abs() < 0.02, "{what}: ink {got} vs {ink}");
            assert!(
                (gx / got - cx).abs() < 0.1 * f,
                "{what}: x {} vs {cx}",
                gx / got
            );
            assert!(
                (gy / got - cy).abs() < 0.1 * f,
                "{what}: y {} vs {cy}",
                gy / got
            );
            assert!(border <= 2. / 255., "{what}: border {border}");
            assert!(pyramid.ratios[k] > 0. && pyramid.ratios[k] < 1., "{what}");
        }
        assert!(
            pyramid
                .levels
                .windows(2)
                .all(|pair| pair[0].factor <= pair[1].factor)
        );
    }
}

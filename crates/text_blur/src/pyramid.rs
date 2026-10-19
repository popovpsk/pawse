use crate::mask::{Mask, Stamp};
use crate::plane::{Plane, overlap};
use crate::{LEVEL_SIGMAS, LEVELS};

#[derive(Debug, Clone, PartialEq)]
pub struct Level {
    pub left: i32,
    pub top: i32,
    pub width: usize,
    pub height: usize,
    pub factor: usize,
    pub bgra: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pyramid {
    pub levels: Vec<Level>,
    pub ratios: [f32; LEVELS],
}

pub fn level_factor(sigma_dev: f32) -> usize {
    let mut factor = 1;
    while (factor * 2) as f32 <= sigma_dev {
        factor *= 2;
    }
    factor
}

pub fn bake(stamps: &[Stamp], scale: f32, rgba: [f32; 4]) -> Option<Pyramid> {
    let factors = LEVEL_SIGMAS.map(|sigma| level_factor(sigma * scale));
    let mask = Mask::compose(stamps, factors[LEVELS - 1])?;
    let mut levels = Vec::with_capacity(LEVELS);
    let mut ratios = [0.; LEVELS];
    let (mut base, mut base_factor) = (mask.plane, 1);
    let mut sharper: Option<(Plane, usize, usize)> = None;
    let mut pad_dev = 0;
    for (k, &factor) in factors.iter().enumerate() {
        while base_factor < factor {
            base = base.halved();
            base_factor *= 2;
        }
        let sigma = residual(LEVEL_SIGMAS[k] * scale, factor);
        let align = factors.get(k + 1).copied().unwrap_or(factor);
        let radius_dev = (sigma * 3.).ceil() as usize * factor;
        pad_dev = radius_dev.max(pad_dev).div_ceil(align) * align;
        let pad = pad_dev / factor;

        let mut level = base.padded(pad);
        level.gaussian(sigma);
        ratios[k] = match sharper.take() {
            None => overlap(&base, &level, pad, pad),
            Some((mut plane, mut plane_factor, plane_pad_dev)) => {
                while plane_factor < factor {
                    plane = plane.halved();
                    plane_factor *= 2;
                }
                let offset = (pad_dev - plane_pad_dev) / factor;
                overlap(&plane, &level, offset, offset)
            }
        };
        levels.push(Level {
            left: mask.left - pad_dev as i32,
            top: mask.top - pad_dev as i32,
            width: level.width,
            height: level.height,
            factor,
            bgra: level.to_bgra(rgba),
        });
        sharper = Some((level, factor, pad_dev));
    }
    Some(Pyramid { levels, ratios })
}

fn residual(sigma_dev: f32, factor: usize) -> f32 {
    let f = factor as f32;
    (sigma_dev * sigma_dev - (f * f - 1.) / 12. - f * f / 6.)
        .max(0.)
        .sqrt()
        / f
}

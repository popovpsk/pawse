use crate::{LEVEL_SIGMAS, LEVELS, MAX_SIGMA};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mix {
    pub text: f32,
    pub layers: [Option<(usize, f32)>; 2],
}

pub fn mix(sigma: f32, ratios: &[f32; LEVELS]) -> Mix {
    let last = LEVELS - 1;
    if sigma <= 0. {
        return Mix {
            text: 1.,
            layers: [None, None],
        };
    }
    if sigma >= MAX_SIGMA {
        return Mix {
            text: 0.,
            layers: [Some((last, 1.)), None],
        };
    }
    if sigma < LEVEL_SIGMAS[0] {
        let t = sigma / LEVEL_SIGMAS[0];
        return Mix {
            text: sharper_opacity(t, ratios[0]),
            layers: [Some((0, t)), None],
        };
    }
    let lower = LEVEL_SIGMAS
        .windows(2)
        .position(|pair| sigma < pair[1])
        .unwrap_or(last - 1);
    let t = (sigma / LEVEL_SIGMAS[lower]).log2().clamp(0., 1.);
    Mix {
        text: 0.,
        layers: [
            Some((lower + 1, t)),
            Some((lower, sharper_opacity(t, ratios[lower + 1]))),
        ],
    }
}

pub(crate) fn sharper_opacity(t: f32, ratio: f32) -> f32 {
    ((1. - t) / (1. - t * ratio).max(f32::EPSILON)).clamp(0., 1.)
}

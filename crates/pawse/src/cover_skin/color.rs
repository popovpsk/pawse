use gpui::{Hsla, Rgba};

const GAMUT_STEPS: u32 = 16;
const GAMUT_FALLOFF: f32 = 0.9;
const GAMUT_SLACK: f32 = 0.001;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Oklch {
    pub l: f32,
    pub c: f32,
    pub h: f32,
}

fn srgb_to_linear(channel: f32) -> f32 {
    if channel <= 0.04045 {
        channel / 12.92
    } else {
        ((channel + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(channel: f32) -> f32 {
    if channel <= 0.003_130_8 {
        channel * 12.92
    } else {
        1.055 * channel.powf(1. / 2.4) - 0.055
    }
}

pub fn oklch(r: f32, g: f32, b: f32) -> Oklch {
    let r = srgb_to_linear(r);
    let g = srgb_to_linear(g);
    let b = srgb_to_linear(b);
    let long = (0.412_221_5 * r + 0.536_332_5 * g + 0.051_445_995 * b).cbrt();
    let medium = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let short = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    let l = 0.210_454_26 * long + 0.793_617_8 * medium - 0.004_072_047 * short;
    let a = 1.977_998_5 * long - 2.428_592_2 * medium + 0.450_593_7 * short;
    let b = 0.025_904_037 * long + 0.782_771_77 * medium - 0.808_675_77 * short;
    Oklch {
        l,
        c: (a * a + b * b).sqrt(),
        h: b.atan2(a).to_degrees().rem_euclid(360.),
    }
}

pub fn oklch_u8(r: u8, g: u8, b: u8) -> Oklch {
    oklch(r as f32 / 255., g as f32 / 255., b as f32 / 255.)
}

fn linear_rgb(color: Oklch) -> (f32, f32, f32) {
    let radians = color.h.to_radians();
    let a = color.c * radians.cos();
    let b = color.c * radians.sin();
    let long = (color.l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let medium = (color.l - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let short = (color.l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    (
        4.076_741_7 * long - 3.307_711_6 * medium + 0.230_969_94 * short,
        -1.268_438 * long + 2.609_757_4 * medium - 0.341_319_38 * short,
        -0.004_196_086_3 * long - 0.703_418_6 * medium + 1.707_614_7 * short,
    )
}

pub fn to_hsla(color: Oklch, alpha: f32) -> Hsla {
    let mut color = color;
    let bounds = -GAMUT_SLACK..=1. + GAMUT_SLACK;
    for _ in 0..GAMUT_STEPS {
        let (r, g, b) = linear_rgb(color);
        if bounds.contains(&r) && bounds.contains(&g) && bounds.contains(&b) {
            break;
        }
        color.c *= GAMUT_FALLOFF;
    }
    let (r, g, b) = linear_rgb(color);
    Hsla::from(Rgba {
        r: linear_to_srgb(r).clamp(0., 1.),
        g: linear_to_srgb(g).clamp(0., 1.),
        b: linear_to_srgb(b).clamp(0., 1.),
        a: alpha,
    })
}

pub fn to_oklch(color: Hsla) -> Oklch {
    let rgba = Rgba::from(color);
    oklch(rgba.r, rgba.g, rgba.b)
}

#[cfg(test)]
mod tests {
    use super::{Oklch, oklch_u8, to_hsla, to_oklch};

    #[test]
    fn primaries_land_on_the_hue_their_name_suggests() {
        assert!((oklch_u8(255, 0, 0).h - 29.).abs() < 3.);
        assert!((oklch_u8(0, 255, 0).h - 142.).abs() < 3.);
        assert!((oklch_u8(0, 0, 255).h - 264.).abs() < 3.);
    }

    #[test]
    fn a_color_survives_the_round_trip_through_hsla() {
        let source = Oklch {
            l: 0.55,
            c: 0.12,
            h: 137.,
        };
        let round_tripped = to_oklch(to_hsla(source, 1.));
        assert!((round_tripped.l - source.l).abs() < 0.01);
        assert!((round_tripped.c - source.c).abs() < 0.01);
        assert!((round_tripped.h - source.h).abs() < 1.);
    }

    #[test]
    fn an_out_of_gamut_request_comes_back_inside_srgb() {
        let color = to_hsla(
            Oklch {
                l: 0.55,
                c: 0.9,
                h: 137.,
            },
            1.,
        );
        assert!((0. ..=1.).contains(&color.s) && (0. ..=1.).contains(&color.l));
        assert!(to_oklch(color).c < 0.4);
    }
}

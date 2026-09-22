use gpui::Image;

use super::color::{Oklch, oklch_u8};

const RASTER_SIZE: u32 = 64;
const MIN_CHROMA: f32 = 0.025;
const MIN_LIGHTNESS: f32 = 0.12;
const MAX_LIGHTNESS: f32 = 0.92;
const HUE_SLOTS: usize = 360;
const HUE_WINDOW: i32 = 15;
const CUTOFF_PROPORTION: f32 = 0.01;
const FULL_TINT_CHROMA: f32 = 0.06;

#[derive(Clone, Debug, PartialEq)]
pub struct CoverPalette {
    pub l_median: f32,
    pub chroma_mass: f32,
    pub confidence: f32,
    pub accent: Option<Oklch>,
}

#[derive(Clone, Copy, Default)]
struct Slot {
    count: f32,
    weight: f32,
    lightness: f32,
    chroma: f32,
    sin: f32,
    cos: f32,
}

impl Slot {
    fn add(&mut self, other: &Slot) {
        self.count += other.count;
        self.weight += other.weight;
        self.lightness += other.lightness;
        self.chroma += other.chroma;
        self.sin += other.sin;
        self.cos += other.cos;
    }
}

pub fn palette_from_rgb(pixels: &[u8]) -> Option<CoverPalette> {
    let mut lightness = Vec::with_capacity(pixels.len() / 3);
    let mut slots = [Slot::default(); HUE_SLOTS];
    let mut chromatic = 0f32;

    for pixel in pixels.as_chunks::<3>().0 {
        let color = oklch_u8(pixel[0], pixel[1], pixel[2]);
        lightness.push(color.l);
        if color.c < MIN_CHROMA || color.l < MIN_LIGHTNESS || color.l > MAX_LIGHTNESS {
            continue;
        }
        chromatic += 1.;
        let slot = &mut slots[(color.h.round() as usize) % HUE_SLOTS];
        let radians = color.h.to_radians();
        let weight = color.c * color.c;
        slot.count += 1.;
        slot.weight += weight;
        slot.lightness += weight * color.l;
        slot.chroma += color.c;
        slot.sin += weight * radians.sin();
        slot.cos += weight * radians.cos();
    }

    if lightness.is_empty() {
        return None;
    }
    lightness.sort_by(f32::total_cmp);
    let counted = lightness.len() as f32;

    let mut best: Option<Slot> = None;
    for hue in 0..HUE_SLOTS as i32 {
        let mut window = Slot::default();
        for offset in -HUE_WINDOW..=HUE_WINDOW {
            window.add(&slots[(hue + offset).rem_euclid(HUE_SLOTS as i32) as usize]);
        }
        if window.count / counted < CUTOFF_PROPORTION {
            continue;
        }
        if best.is_none_or(|top| window.weight > top.weight) {
            best = Some(window);
        }
    }

    Some(CoverPalette {
        l_median: lightness[lightness.len() / 2],
        chroma_mass: chromatic / counted,
        confidence: best
            .map(|window| ((window.weight / counted).sqrt() / FULL_TINT_CHROMA).clamp(0., 1.))
            .unwrap_or(0.),
        accent: best.map(|window| Oklch {
            l: window.lightness / window.weight,
            c: window.chroma / window.count,
            h: window.sin.atan2(window.cos).to_degrees().rem_euclid(360.),
        }),
    })
}

pub fn palette_from_thumbnail(thumbnail: &Image) -> Option<CoverPalette> {
    let source = image::ImageReader::new(std::io::Cursor::new(thumbnail.bytes()))
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?
        .resize_to_fill(
            RASTER_SIZE,
            RASTER_SIZE,
            image::imageops::FilterType::Triangle,
        )
        .to_rgb8();
    palette_from_rgb(source.as_raw())
}

#[cfg(test)]
mod tests {
    use super::super::color::{Oklch, oklch_u8, to_hsla};
    use super::palette_from_rgb;
    use gpui::Rgba;

    fn solid(r: u8, g: u8, b: u8) -> Vec<u8> {
        [r, g, b].repeat(64 * 64)
    }

    fn patches(parts: &[(f32, f32, usize)]) -> Vec<u8> {
        let mut pixels = Vec::new();
        for (hue, chroma, count) in parts {
            let color = Rgba::from(to_hsla(
                Oklch {
                    l: 0.55,
                    c: *chroma,
                    h: *hue,
                },
                1.,
            ));
            let rgb = [
                (color.r * 255.) as u8,
                (color.g * 255.) as u8,
                (color.b * 255.) as u8,
            ];
            pixels.extend(rgb.repeat(*count));
        }
        pixels.extend([0u8, 0, 0].repeat(64 * 64 - pixels.len() / 3));
        pixels
    }

    #[test]
    fn a_flat_color_becomes_its_own_accent() {
        let palette = palette_from_rgb(&solid(90, 150, 70)).expect("palette");
        let accent = palette.accent.expect("accent");
        assert!((accent.h - oklch_u8(90, 150, 70).h).abs() < 1.);
        assert_eq!(palette.confidence, 1.);
    }

    #[test]
    fn a_gray_cover_yields_no_accent() {
        let palette = palette_from_rgb(&solid(128, 128, 128)).expect("palette");
        assert_eq!(palette.accent, None);
        assert_eq!(palette.confidence, 0.);
    }

    #[test]
    fn a_hue_split_across_neighbours_is_counted_as_one_colour() {
        let pixels = patches(&[(26., 0.13, 300), (34., 0.13, 300), (250., 0.13, 460)]);
        let accent = palette_from_rgb(&pixels).expect("palette").accent;
        assert!(
            (accent.expect("accent").h - 30.).abs() < 6.,
            "warm halves must outweigh the single blue block, got {accent:?}"
        );
    }

    #[test]
    fn a_hue_too_small_to_matter_is_ignored() {
        let pixels = patches(&[(250., 0.13, 600), (30., 0.30, 20)]);
        let accent = palette_from_rgb(&pixels).expect("palette").accent;
        assert!((accent.expect("accent").h - 250.).abs() < 8.);
    }

    #[test]
    fn a_muted_cover_tints_weakly_instead_of_not_at_all() {
        let mut pixels = solid(70, 70, 75);
        pixels.truncate(pixels.len() - 3 * 200);
        pixels.extend([200, 90, 60].repeat(200));
        let palette = palette_from_rgb(&pixels).expect("palette");
        assert!((palette.accent.expect("accent").h - oklch_u8(200, 90, 60).h).abs() < 5.);
        assert!(
            palette.confidence > 0.2 && palette.confidence < 0.7,
            "confidence {}",
            palette.confidence
        );
    }

    #[test]
    fn a_saturated_minority_outvotes_a_muted_majority() {
        let mut pixels = solid(70, 70, 75);
        pixels.truncate(pixels.len() - 3 * 200);
        pixels.extend([220, 40, 40].repeat(200));
        let accent = palette_from_rgb(&pixels).expect("palette").accent;
        assert!((accent.expect("accent").h - oklch_u8(220, 40, 40).h).abs() < 5.);
    }

    #[test]
    fn an_empty_cover_has_no_palette() {
        assert_eq!(palette_from_rgb(&[]), None);
    }
}

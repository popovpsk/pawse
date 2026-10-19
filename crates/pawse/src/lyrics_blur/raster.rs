use std::collections::HashMap;
use std::sync::Arc;

use gpui::{
    Bounds, Corners, FontId, Global, GlyphId, Hsla, Pixels, PlatformTextSystem, Point,
    RenderGlyphParams, RenderImage, SUBPIXEL_VARIANTS_X, SharedString, Size, Window, point, px,
    size,
};
use text_blur::{Coverage, LEVELS, Mix, Stamp};

use crate::lyrics_fill::LineFont;

pub struct GlyphRaster(pub Arc<dyn PlatformTextSystem>);

impl Global for GlyphRaster {}

struct PlacedGlyph {
    font_id: FontId,
    glyph_id: GlyphId,
    origin: Point<Pixels>,
}

pub struct LineGlyphs {
    glyphs: Vec<PlacedGlyph>,
    font_size: Pixels,
    pub size: Size<Pixels>,
}

#[derive(Default)]
pub struct GlyphCache(HashMap<RenderGlyphParams, Option<Coverage>>);

struct Level {
    image: Arc<RenderImage>,
    bounds: Bounds<Pixels>,
    ring: Pixels,
}

pub struct BlurredLine {
    levels: Vec<Level>,
    ratios: [f32; LEVELS],
}

impl BlurredLine {
    pub fn mix(&self, sigma: f32) -> Mix {
        text_blur::mix(sigma, &self.ratios)
    }

    pub fn images(&self) -> impl Iterator<Item = Arc<RenderImage>> + '_ {
        self.levels.iter().map(|level| level.image.clone())
    }

    pub fn paint(&self, level: usize, origin: Point<Pixels>, window: &mut Window) {
        let Some(level) = self.levels.get(level) else {
            return;
        };
        let bounds = Bounds {
            origin: origin + level.bounds.origin,
            size: level.bounds.size,
        };
        if !bounds.intersects(&window.content_mask().bounds) {
            return;
        }
        if let Err(err) = window.paint_image(
            bounds,
            bounds.dilate(level.ring),
            Corners::default(),
            level.image.clone(),
            0,
            false,
        ) {
            log::debug!("lyrics blur level not painted: {err}");
        }
    }
}

pub fn line_glyphs(
    window: &mut Window,
    text: &SharedString,
    width: Pixels,
    font: LineFont,
) -> Option<LineGlyphs> {
    if text.is_empty() || width <= px(0.) {
        return None;
    }

    let mut style = window.text_style();
    style.font_size = font.size.into();
    style.font_weight = font.weight;
    style.font_style = font.style;
    let line_height = window.pixel_snap(
        style
            .line_height
            .to_pixels(font.size.into(), window.rem_size()),
    );
    let run = style.to_run(text.len());

    let lines = window
        .text_system()
        .shape_text(text.clone(), font.size, &[run], Some(width), None)
        .ok()?;
    let wrapped = lines.into_iter().next()?;
    let layout = &wrapped.unwrapped_layout;
    let baseline = (line_height - layout.ascent - layout.descent) / 2. + layout.ascent;

    let mut wraps = wrapped.wrap_boundaries.iter().peekable();
    let mut glyphs = Vec::new();
    let (mut x, mut y, mut prev_x) = (px(0.), px(0.), px(0.));
    for (run_ix, run) in layout.runs.iter().enumerate() {
        for (glyph_ix, glyph) in run.glyphs.iter().enumerate() {
            x += glyph.position.x - prev_x;
            if wraps
                .peek()
                .is_some_and(|wrap| wrap.run_ix == run_ix && wrap.glyph_ix == glyph_ix)
            {
                wraps.next();
                x = px(0.);
                y += line_height;
            }
            prev_x = glyph.position.x;
            if !glyph.is_emoji {
                glyphs.push(PlacedGlyph {
                    font_id: run.font_id,
                    glyph_id: glyph.id,
                    origin: point(x, y + baseline + glyph.position.y),
                });
            }
        }
    }

    let rows = wrapped.wrap_boundaries.len() + 1;
    Some(LineGlyphs {
        glyphs,
        font_size: font.size,
        size: size(width, line_height * rows as f32),
    })
}

pub fn bake(
    raster: &dyn PlatformTextSystem,
    cache: &mut GlyphCache,
    line: &LineGlyphs,
    color: Hsla,
    scale: f32,
) -> Option<BlurredLine> {
    let dilation = raster.glyph_dilation_for_color(color);
    let variants = SUBPIXEL_VARIANTS_X as f32;
    let mut placed = Vec::with_capacity(line.glyphs.len());
    for glyph in &line.glyphs {
        let x = f32::from(glyph.origin.x) * scale;
        let y = f32::from(glyph.origin.y) * scale;
        let quantized = (x * variants).round() / variants;
        let params = RenderGlyphParams {
            font_id: glyph.font_id,
            glyph_id: glyph.glyph_id,
            font_size: line.font_size,
            subpixel_variant: point((quantized.fract() * variants) as u8, 0),
            scale_factor: scale,
            is_emoji: false,
            subpixel_rendering: false,
            dilation,
        };
        if !cache.0.contains_key(&params) {
            let coverage = rasterize(raster, &params);
            cache.0.insert(params.clone(), coverage);
        }
        placed.push((params, quantized.trunc() as i32, y.round() as i32));
    }
    let stamps: Vec<Stamp> = placed
        .iter()
        .filter_map(|(params, x, y)| {
            let coverage = cache.0.get(params)?.as_ref()?;
            Some(Stamp {
                x: *x,
                y: *y,
                coverage,
            })
        })
        .collect();

    let rgb = color.to_rgb();
    let pyramid = text_blur::bake(&stamps, scale, [rgb.r, rgb.g, rgb.b, color.a])?;
    let levels = pyramid
        .levels
        .into_iter()
        .map(|level| {
            let ring = level.factor as f32 / scale;
            let to_px = |v: i32| px(v as f32 / scale);
            let bounds = Bounds {
                origin: point(to_px(level.left), to_px(level.top)),
                size: size(
                    px(level.width as f32 * ring),
                    px(level.height as f32 * ring),
                ),
            };
            let buffer = image::RgbaImage::from_raw(
                level.width as u32 + 2,
                level.height as u32 + 2,
                level.bgra,
            )
            .expect("buffer length matches its dimensions");
            Level {
                image: Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])),
                bounds,
                ring: px(ring),
            }
        })
        .collect();
    Some(BlurredLine {
        levels,
        ratios: pyramid.ratios,
    })
}

fn rasterize(raster: &dyn PlatformTextSystem, params: &RenderGlyphParams) -> Option<Coverage> {
    let bounds = raster.glyph_raster_bounds(params).ok()?;
    if bounds.size.width.0 <= 0 || bounds.size.height.0 <= 0 {
        return None;
    }
    let (size, alpha) = raster.rasterize_glyph(params, bounds).ok()?;
    let (width, height) = (size.width.0 as usize, size.height.0 as usize);
    (alpha.len() == width * height).then_some(Coverage {
        left: bounds.origin.x.0,
        top: bounds.origin.y.0,
        width,
        height,
        alpha,
    })
}

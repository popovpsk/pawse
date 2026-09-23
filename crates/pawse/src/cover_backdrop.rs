use std::sync::Arc;
use std::time::{Duration, Instant};

use audio_engine::EngineEvent;
use gpui::prelude::FluentBuilder;
use gpui::{
    App, Context, Div, Global, Hsla, Image, ObjectFit, ParentElement, RenderImage, Styled,
    StyledImage, Subscription, Task, div, ease_in_out, img, linear_color_stop, linear_gradient,
};

use crate::cover_art_cache::drop_atlas_tile;
use crate::library_service::LibraryEvent;
use crate::services::Services;
use crate::settings_store::{BlurBackground, SettingsStore};

const RASTER_SIZE: u32 = 96;
const SATURATION: f32 = 1.5;
const IMAGE_OPACITY: f32 = 0.55;
const FADE: Duration = Duration::from_millis(320);
const VEIL_TOP: f32 = 0.15;
const VEIL_BOTTOM: f32 = 0.6;
const PANEL_VEIL: f32 = 0.55;
const CHROME_VEIL: f32 = 0.45;
const INSET_VEIL: f32 = 0.5;
const FIELD_VEIL: f32 = 0.2;
const POPOVER_VEIL: f32 = 0.85;

fn blur_enabled(cx: &App) -> bool {
    cx.global::<SettingsStore>().blur_background() != BlurBackground::Off
}

fn blur_sigma(cx: &App) -> f32 {
    cx.global::<SettingsStore>().blur_intensity()
}

#[derive(Clone, Copy)]
pub struct Veil {
    factor: f32,
    weight: f32,
}

pub fn veil_factor(cx: &App) -> Option<Veil> {
    let weight = presence(cx);
    (weight > 0.).then(|| Veil {
        factor: cx.global::<SettingsStore>().blur_interface_opacity() / 100.,
        weight,
    })
}

fn presence(cx: &App) -> f32 {
    cx.try_global::<Active>().map_or(0., |state| state.0)
}

struct Active(f32);

impl Global for Active {}

/// Whether the backdrop is painted on the window right now.
///
/// `MainView::render` is the only place that knows the answer — it depends on
/// the setting, the current view and whether a raster has finished baking — so
/// it publishes it here for the popovers and dropdowns that float above the
/// window and cannot be handed the flag through their constructors.
pub fn set_active(weight: f32, cx: &mut App) {
    if presence(cx) != weight {
        cx.set_global(Active(weight));
    }
}

pub fn is_active(cx: &App) -> bool {
    presence(cx) > 0.
}

pub struct Backdrop {
    pub image: Option<Arc<RenderImage>>,
    pub previous: Option<Arc<RenderImage>>,
    pub progress: f32,
}

impl Backdrop {
    pub fn presence(&self) -> f32 {
        match (self.image.is_some(), self.previous.is_some()) {
            (true, true) => 1.,
            (true, false) => self.progress,
            _ => 1. - self.progress,
        }
    }
}

pub struct CoverBackdrop {
    image: Option<Arc<RenderImage>>,
    previous: Option<Arc<RenderImage>>,
    swapped: Option<Instant>,
    cover_art_id: Option<i64>,
    enabled: bool,
    sigma: f32,
    _task: Option<Task<()>>,
    _fade: Option<Task<()>>,
    _engine_subscription: Subscription,
    _library_subscription: Subscription,
    _settings_subscription: Subscription,
}

impl CoverBackdrop {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let services = cx.global::<Services>();
        let engine_event_bus = services.engine_event_bus.clone();
        let library_event_bus = services.library_event_bus.clone();
        let engine_subscription =
            cx.subscribe(&engine_event_bus, |this, _, event: &EngineEvent, cx| {
                if matches!(
                    event,
                    EngineEvent::Loaded { .. } | EngineEvent::TrackEnded | EngineEvent::Stopped
                ) {
                    this.refresh(true, cx);
                }
            });
        let library_subscription =
            cx.subscribe(&library_event_bus, |this, _, event: &LibraryEvent, cx| {
                if let LibraryEvent::ScanComplete { changed: true } = event {
                    this.refresh(true, cx);
                }
            });
        let settings_subscription = cx.observe_global::<SettingsStore>(|this: &mut Self, cx| {
            let enabled = blur_enabled(cx);
            let sigma = blur_sigma(cx);
            let enabled_changed = enabled != this.enabled;
            let sigma_changed = sigma != this.sigma;
            this.enabled = enabled;
            this.sigma = sigma;
            if enabled_changed {
                this.refresh(false, cx);
            } else if sigma_changed && this.enabled {
                this.load(false, cx);
            }
        });

        let mut this = Self {
            image: None,
            previous: None,
            swapped: None,
            cover_art_id: None,
            enabled: blur_enabled(cx),
            sigma: blur_sigma(cx),
            _task: None,
            _fade: None,
            _engine_subscription: engine_subscription,
            _library_subscription: library_subscription,
            _settings_subscription: settings_subscription,
        };
        this.refresh(false, cx);
        this
    }

    pub fn frame(&self) -> Option<Backdrop> {
        let progress = match self.swapped {
            Some(at) => {
                ease_in_out((at.elapsed().as_secs_f32() / FADE.as_secs_f32()).clamp(0., 1.))
            }
            None => 1.,
        };
        let previous = (progress < 1.).then(|| self.previous.clone()).flatten();
        if self.image.is_none() && previous.is_none() {
            return None;
        }
        Some(Backdrop {
            image: self.image.clone(),
            previous,
            progress,
        })
    }

    fn refresh(&mut self, animate: bool, cx: &mut Context<Self>) {
        let cover_art_id = {
            let queue = cx.global::<Services>().playback_queue.borrow();
            queue.current_track().and_then(|track| track.cover_art_id)
        };
        let changed = cover_art_id != self.cover_art_id;
        self.cover_art_id = cover_art_id;
        if !self.enabled {
            self._task = None;
            self.set_image(None, false, cx);
        } else if changed || self.image.is_none() {
            self.load(changed && animate, cx);
        }
    }

    fn load(&mut self, fade: bool, cx: &mut Context<Self>) {
        self._task = None;
        let Some(id) = self.cover_art_id else {
            self.set_image(None, fade, cx);
            return;
        };
        let services = cx.global::<Services>();
        let thumbnail = services
            .cover_art_cache
            .borrow_mut()
            .get_small(Some(id), &services.library);
        let Some(thumbnail) = thumbnail else {
            self.set_image(None, fade, cx);
            return;
        };
        let sigma = self.sigma;
        let render = cx
            .background_executor()
            .spawn(async move { from_thumbnail(&thumbnail, sigma) });
        self._task = Some(cx.spawn(async move |this, cx| {
            let image = render.await;
            let _ = this.update(cx, |this, cx| {
                this._task = None;
                if this.enabled && this.cover_art_id == Some(id) {
                    this.set_image(image, fade, cx);
                }
            });
        }));
    }

    fn set_image(&mut self, image: Option<Arc<RenderImage>>, fade: bool, cx: &mut Context<Self>) {
        if image.is_none() && self.image.is_none() {
            return;
        }
        let replaced = std::mem::replace(&mut self.image, image);
        if let Some(stale) = self.previous.take() {
            drop_atlas_tile(stale, cx);
        }
        self._fade = None;
        if fade {
            self.previous = replaced;
            self.swapped = Some(Instant::now());
            self._fade = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(FADE).await;
                let _ = this.update(cx, |this, cx| {
                    this.swapped = None;
                    if let Some(stale) = this.previous.take() {
                        drop_atlas_tile(stale, cx);
                    }
                    cx.notify();
                });
            }));
        } else {
            self.swapped = None;
            if let Some(old) = replaced {
                drop_atlas_tile(old, cx);
            }
        }
        cx.notify();
    }
}

pub fn from_thumbnail(thumbnail: &Image, sigma: f32) -> Option<Arc<RenderImage>> {
    let source = image::ImageReader::new(std::io::Cursor::new(thumbnail.bytes()))
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?
        .resize_to_fill(
            RASTER_SIZE,
            RASTER_SIZE,
            image::imageops::FilterType::Triangle,
        );
    let mut raster = image::imageops::fast_blur(&source.to_rgba8(), sigma);
    for pixel in raster.as_chunks_mut::<4>().0 {
        let luma = 0.299 * pixel[0] as f32 + 0.587 * pixel[1] as f32 + 0.114 * pixel[2] as f32;
        for channel in pixel.iter_mut().take(3) {
            *channel = (luma + (*channel as f32 - luma) * SATURATION).clamp(0., 255.) as u8;
        }
        pixel.swap(0, 2);
    }
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(raster)])))
}

fn blend_opacity(progress: f32, layered: bool) -> (f32, f32) {
    let top = IMAGE_OPACITY * progress;
    let under = if layered {
        IMAGE_OPACITY * (1. - progress) / (1. - IMAGE_OPACITY * progress)
    } else {
        IMAGE_OPACITY * (1. - progress)
    };
    (top, under)
}

pub fn layers(backdrop: Backdrop, background: Hsla) -> Div {
    let Backdrop {
        image,
        previous,
        progress,
    } = backdrop;
    let (top, under) = blend_opacity(progress, image.is_some());
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .when_some(previous, |this, old| {
            this.child(
                img(old)
                    .absolute()
                    .size_full()
                    .object_fit(ObjectFit::Cover)
                    .opacity(under),
            )
        })
        .when_some(image, |this, current| {
            this.child(
                img(current)
                    .absolute()
                    .size_full()
                    .object_fit(ObjectFit::Cover)
                    .opacity(top),
            )
        })
        .child(div().absolute().size_full().bg(linear_gradient(
            180.,
            linear_color_stop(background.opacity(VEIL_TOP), 0.),
            linear_color_stop(background.opacity(VEIL_BOTTOM), 1.),
        )))
}

fn veiled(color: Hsla, veil: Option<Veil>, share: f32) -> Hsla {
    match veil {
        Some(veil) => color.opacity((1. - veil.weight * (1. - share * veil.factor)).clamp(0., 1.)),
        None => color,
    }
}

pub fn chrome_bg(color: Hsla, veil: Option<Veil>) -> Hsla {
    veiled(color, veil, CHROME_VEIL)
}

pub fn panel_bg(color: Hsla, veil: Option<Veil>) -> Hsla {
    veiled(color, veil, PANEL_VEIL)
}

pub fn inset_bg(color: Hsla, veil: Option<Veil>) -> Hsla {
    veiled(color, veil, INSET_VEIL)
}

pub fn field_bg(color: Hsla, veil: Option<Veil>) -> Hsla {
    veiled(color, veil, FIELD_VEIL)
}

pub fn popover_bg(color: Hsla, veil: Option<Veil>) -> Hsla {
    veiled(color, veil, POPOVER_VEIL)
}

#[cfg(test)]
mod tests {
    use super::{RASTER_SIZE, from_thumbnail};
    use gpui::{Image, ImageFormat};

    const TEST_SIGMA: f32 = 10.;

    fn encoded(pixels: image::RgbImage) -> Image {
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgb8(pixels)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .expect("png encode");
        Image::from_bytes(ImageFormat::Png, bytes)
    }

    fn solid(r: u8, g: u8, b: u8) -> Image {
        encoded(image::RgbImage::from_pixel(128, 128, image::Rgb([r, g, b])))
    }

    #[test]
    fn rasterizes_to_a_fixed_square_regardless_of_source_size() {
        let wide = encoded(image::RgbImage::from_pixel(
            640,
            120,
            image::Rgb([10, 120, 200]),
        ));
        let raster = from_thumbnail(&wide, TEST_SIGMA).expect("raster");
        let size = raster.size(0);
        assert_eq!(u32::from(size.width), RASTER_SIZE);
        assert_eq!(u32::from(size.height), RASTER_SIZE);
    }

    #[test]
    fn channels_are_stored_in_the_bgra_order_gpui_uploads() {
        let raster = from_thumbnail(&solid(255, 0, 0), TEST_SIGMA).expect("raster");
        let bytes = raster.as_bytes(0).expect("frame");
        assert_eq!(&bytes[..4], &[0, 0, 255, 255]);
    }

    #[test]
    fn saturation_pushes_a_muted_source_further_from_gray() {
        let raster = from_thumbnail(&solid(160, 100, 100), TEST_SIGMA).expect("raster");
        let bytes = raster.as_bytes(0).expect("frame");
        let (blue, green, red) = (bytes[0] as i32, bytes[1] as i32, bytes[2] as i32);
        assert!(red > 160, "red {red} should be pushed up from 160");
        assert!(green < 100, "green {green} should be pushed down from 100");
        assert_eq!(green, blue);
    }

    #[test]
    fn a_hard_edge_blurs_into_a_gradient() {
        let mut split = image::RgbImage::new(128, 128);
        for (x, _, pixel) in split.enumerate_pixels_mut() {
            *pixel = if x < 64 {
                image::Rgb([0, 0, 0])
            } else {
                image::Rgb([255, 255, 255])
            };
        }
        let raster = from_thumbnail(&encoded(split), TEST_SIGMA).expect("raster");
        let bytes = raster.as_bytes(0).expect("frame");
        let row = (RASTER_SIZE / 2) as usize * RASTER_SIZE as usize * 4;
        let seam = bytes[row + (RASTER_SIZE / 2) as usize * 4] as i32;
        assert!(
            (60..=195).contains(&seam),
            "seam pixel {seam} should be blended, not a hard black/white edge"
        );
    }

    #[test]
    fn a_crossfade_keeps_the_backdrop_at_a_constant_weight() {
        for progress in [0., 0.25, 0.5, 0.75, 1.] {
            let (top, under) = super::blend_opacity(progress, true);
            let covered = top + (1. - top) * under;
            assert!(
                (covered - super::IMAGE_OPACITY).abs() < 1e-5,
                "progress {progress}: {covered} instead of {}",
                super::IMAGE_OPACITY
            );
        }
    }

    #[test]
    fn a_fade_out_walks_the_last_image_down_to_nothing() {
        let (top, under) = super::blend_opacity(1., false);
        assert_eq!(top, super::IMAGE_OPACITY);
        assert_eq!(under, 0.);
        let (_, half) = super::blend_opacity(0.5, false);
        assert!((half - super::IMAGE_OPACITY * 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_settled_backdrop_veils_the_chrome_exactly_as_the_setting_asks() {
        let color = gpui::hsla(0.5, 0.5, 0.5, 1.);
        let veil = super::Veil {
            factor: 0.8,
            weight: 1.,
        };
        assert!((super::chrome_bg(color, Some(veil)).a - super::CHROME_VEIL * 0.8).abs() < 1e-6);
    }

    #[test]
    fn a_backdrop_fading_out_hands_the_chrome_back_opaque() {
        let color = gpui::hsla(0.5, 0.5, 0.5, 1.);
        let gone = super::Veil {
            factor: 0.8,
            weight: 0.,
        };
        assert_eq!(super::chrome_bg(color, Some(gone)).a, 1.);
        assert_eq!(super::chrome_bg(color, None).a, color.a);
        let half = super::Veil {
            factor: 0.8,
            weight: 0.5,
        };
        let alpha = super::chrome_bg(color, Some(half)).a;
        let full = super::CHROME_VEIL * 0.8;
        assert!((alpha - (full + 1.) * 0.5).abs() < 1e-6, "alpha {alpha}");
    }

    #[test]
    fn undecodable_bytes_yield_no_backdrop() {
        let garbage = Image::from_bytes(ImageFormat::Png, b"not an image".to_vec());
        assert!(from_thumbnail(&garbage, TEST_SIGMA).is_none());
    }
}

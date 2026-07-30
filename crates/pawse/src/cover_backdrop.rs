use std::sync::Arc;

use audio_engine::EngineEvent;
use gpui::{
    App, Context, Div, Global, Hsla, Image, ObjectFit, ParentElement, RenderImage, Styled,
    StyledImage, Subscription, Task, div, img, linear_color_stop, linear_gradient,
};

use crate::cover_art_cache::drop_atlas_tile;
use crate::library_service::LibraryEvent;
use crate::services::Services;
use crate::settings_store::{BlurBackground, SettingsStore};

const RASTER_SIZE: u32 = 96;
const BLUR_SIGMA: f32 = 10.;
const SATURATION: f32 = 1.5;
const IMAGE_OPACITY: f32 = 0.55;
const VEIL_TOP: f32 = 0.15;
const VEIL_BOTTOM: f32 = 0.6;
const PANEL_VEIL: f32 = 0.55;
const CHROME_VEIL: f32 = 0.45;
const INSET_VEIL: f32 = 0.5;
const FIELD_VEIL: f32 = 0.2;

fn blur_enabled(cx: &App) -> bool {
    cx.global::<SettingsStore>().blur_background() != BlurBackground::Off
}

struct Active(bool);

impl Global for Active {}

/// Whether the backdrop is painted on the window right now.
///
/// `MainView::render` is the only place that knows the answer — it depends on
/// the setting, the current view and whether a raster has finished baking — so
/// it publishes it here for the popovers and dropdowns that float above the
/// window and cannot be handed the flag through their constructors.
pub fn set_active(active: bool, cx: &mut App) {
    if is_active(cx) != active {
        cx.set_global(Active(active));
    }
}

pub fn is_active(cx: &App) -> bool {
    cx.try_global::<Active>().is_some_and(|state| state.0)
}

pub struct CoverBackdrop {
    image: Option<Arc<RenderImage>>,
    cover_art_id: Option<i64>,
    enabled: bool,
    _task: Option<Task<()>>,
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
                    this.refresh(cx);
                }
            });
        let library_subscription =
            cx.subscribe(&library_event_bus, |this, _, event: &LibraryEvent, cx| {
                if let LibraryEvent::ScanComplete { changed: true } = event {
                    this.refresh(cx);
                }
            });
        let settings_subscription = cx.observe_global::<SettingsStore>(|this: &mut Self, cx| {
            let enabled = blur_enabled(cx);
            if enabled != this.enabled {
                this.enabled = enabled;
                this.refresh(cx);
            }
        });

        let mut this = Self {
            image: None,
            cover_art_id: None,
            enabled: blur_enabled(cx),
            _task: None,
            _engine_subscription: engine_subscription,
            _library_subscription: library_subscription,
            _settings_subscription: settings_subscription,
        };
        this.refresh(cx);
        this
    }

    pub fn image(&self) -> Option<Arc<RenderImage>> {
        self.image.clone()
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let cover_art_id = {
            let queue = cx.global::<Services>().playback_queue.borrow();
            queue.current_track().and_then(|track| track.cover_art_id)
        };
        let changed = cover_art_id != self.cover_art_id;
        self.cover_art_id = cover_art_id;
        if !self.enabled {
            self._task = None;
            self.set_image(None, cx);
        } else if changed || self.image.is_none() {
            self.load(cx);
        }
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        self._task = None;
        let Some(id) = self.cover_art_id else {
            self.set_image(None, cx);
            return;
        };
        let services = cx.global::<Services>();
        let thumbnail = services
            .cover_art_cache
            .borrow_mut()
            .get_small(Some(id), &services.library);
        let Some(thumbnail) = thumbnail else {
            self.set_image(None, cx);
            return;
        };
        let render = cx
            .background_executor()
            .spawn(async move { from_thumbnail(&thumbnail) });
        self._task = Some(cx.spawn(async move |this, cx| {
            let image = render.await;
            let _ = this.update(cx, |this, cx| {
                this._task = None;
                if this.enabled && this.cover_art_id == Some(id) {
                    this.set_image(image, cx);
                }
            });
        }));
    }

    fn set_image(&mut self, image: Option<Arc<RenderImage>>, cx: &mut Context<Self>) {
        if image.is_none() && self.image.is_none() {
            return;
        }
        if let Some(old) = std::mem::replace(&mut self.image, image) {
            drop_atlas_tile(old, cx);
        }
        cx.notify();
    }
}

pub fn from_thumbnail(thumbnail: &Image) -> Option<Arc<RenderImage>> {
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
    let mut raster = image::imageops::fast_blur(&source.to_rgba8(), BLUR_SIGMA);
    for pixel in raster.chunks_exact_mut(4) {
        let luma = 0.299 * pixel[0] as f32 + 0.587 * pixel[1] as f32 + 0.114 * pixel[2] as f32;
        for channel in pixel.iter_mut().take(3) {
            *channel = (luma + (*channel as f32 - luma) * SATURATION).clamp(0., 255.) as u8;
        }
        pixel.swap(0, 2);
    }
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(raster)])))
}

pub fn layers(image: Arc<RenderImage>, background: Hsla) -> Div {
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .child(
            img(image)
                .absolute()
                .size_full()
                .object_fit(ObjectFit::Cover)
                .opacity(IMAGE_OPACITY),
        )
        .child(div().absolute().size_full().bg(linear_gradient(
            180.,
            linear_color_stop(background.opacity(VEIL_TOP), 0.),
            linear_color_stop(background.opacity(VEIL_BOTTOM), 1.),
        )))
}

pub fn chrome_bg(color: Hsla, over_backdrop: bool) -> Hsla {
    if over_backdrop {
        color.opacity(CHROME_VEIL)
    } else {
        color
    }
}

pub fn panel_bg(color: Hsla, over_backdrop: bool) -> Hsla {
    if over_backdrop {
        color.opacity(PANEL_VEIL)
    } else {
        color
    }
}

pub fn inset_bg(color: Hsla, over_backdrop: bool) -> Hsla {
    if over_backdrop {
        color.opacity(INSET_VEIL)
    } else {
        color
    }
}

pub fn field_bg(color: Hsla, over_backdrop: bool) -> Hsla {
    if over_backdrop {
        color.opacity(FIELD_VEIL)
    } else {
        color
    }
}

#[cfg(test)]
mod tests {
    use super::{RASTER_SIZE, from_thumbnail};
    use gpui::{Image, ImageFormat};

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
        let raster = from_thumbnail(&wide).expect("raster");
        let size = raster.size(0);
        assert_eq!(u32::from(size.width), RASTER_SIZE);
        assert_eq!(u32::from(size.height), RASTER_SIZE);
    }

    #[test]
    fn channels_are_stored_in_the_bgra_order_gpui_uploads() {
        let raster = from_thumbnail(&solid(255, 0, 0)).expect("raster");
        let bytes = raster.as_bytes(0).expect("frame");
        assert_eq!(&bytes[..4], &[0, 0, 255, 255]);
    }

    #[test]
    fn saturation_pushes_a_muted_source_further_from_gray() {
        let raster = from_thumbnail(&solid(160, 100, 100)).expect("raster");
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
        let raster = from_thumbnail(&encoded(split)).expect("raster");
        let bytes = raster.as_bytes(0).expect("frame");
        let row = (RASTER_SIZE / 2) as usize * RASTER_SIZE as usize * 4;
        let seam = bytes[row + (RASTER_SIZE / 2) as usize * 4] as i32;
        assert!(
            (60..=195).contains(&seam),
            "seam pixel {seam} should be blended, not a hard black/white edge"
        );
    }

    #[test]
    fn undecodable_bytes_yield_no_backdrop() {
        let garbage = Image::from_bytes(ImageFormat::Png, b"not an image".to_vec());
        assert!(from_thumbnail(&garbage).is_none());
    }
}

use std::sync::Arc;

use gpui::{
    Div, Hsla, Image, ObjectFit, ParentElement, RenderImage, Styled, StyledImage, div, img,
    linear_color_stop, linear_gradient,
};

const RASTER_SIZE: u32 = 96;
const BLUR_SIGMA: f32 = 10.;
const SATURATION: f32 = 1.5;
const IMAGE_OPACITY: f32 = 0.55;
const VEIL_TOP: f32 = 0.15;
const VEIL_BOTTOM: f32 = 0.6;
const PANEL_VEIL: f32 = 0.55;
const CHROME_VEIL: f32 = 0.45;

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

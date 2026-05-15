use std::io::Cursor;

const SMALL_SIZE: u32 = 128;
const LARGE_SIZE: u32 = 320;

pub struct Thumbnails {
    pub small: Vec<u8>,
    pub large: Vec<u8>,
}

pub fn generate_thumbnails(data: &[u8]) -> crate::error::Result<Thumbnails> {
    let img = image::load_from_memory(data)?;

    let small_img = image::DynamicImage::ImageRgba8(image::imageops::thumbnail(
        &img, SMALL_SIZE, SMALL_SIZE,
    ));
    let large_img = image::DynamicImage::ImageRgba8(image::imageops::thumbnail(
        &img, LARGE_SIZE, LARGE_SIZE,
    ));

    let small = encode_jpeg(&small_img)?;
    let large = encode_jpeg(&large_img)?;

    Ok(Thumbnails { small, large })
}

fn encode_jpeg(img: &image::DynamicImage) -> crate::error::Result<Vec<u8>> {
    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Jpeg)?;
    Ok(buf.into_inner())
}

use image::{RgbaImage, imageops};

/// A copy of the image shrunk to fit in a square of `max_side` pixels, each pixel
/// averaging those it covers. An image already small enough is copied as it is.
pub fn thumbnail(image: &RgbaImage, max_side: u32) -> RgbaImage {
    let (width, height) = image.dimensions();
    if width <= max_side && height <= max_side {
        return image.clone();
    }
    let scale = max_side as f64 / width.max(height) as f64;
    let new_width = ((width as f64 * scale).round() as u32).max(1);
    let new_height = ((height as f64 * scale).round() as u32).max(1);
    imageops::thumbnail(image, new_width, new_height)
}

/// A copy of the image at half its width and height, rounded up, each pixel
/// averaging the four it covers.
pub fn halve(image: &RgbaImage) -> RgbaImage {
    let (width, height) = image.dimensions();
    imageops::thumbnail(image, width.div_ceil(2), height.div_ceil(2))
}

use image::{Rgba, Rgba32FImage, RgbaImage, imageops, imageops::FilterType};

/// How an image's pixels are redrawn when it is stretched to another size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Interpolation {
    /// Each pixel takes the colour of the nearest one, which keeps edges hard.
    Nearest,
    Bilinear,
    #[default]
    Bicubic,
    Lanczos,
}

impl Interpolation {
    pub const ALL: [Self; 4] = [Self::Nearest, Self::Bilinear, Self::Bicubic, Self::Lanczos];

    pub fn name(self) -> &'static str {
        match self {
            Self::Nearest => "Nearest Neighbor",
            Self::Bilinear => "Bilinear",
            Self::Bicubic => "Bicubic",
            Self::Lanczos => "Lanczos",
        }
    }
}

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

/// A copy of the image stretched to another size, which must not be empty.
pub fn resize(image: &RgbaImage, width: u32, height: u32, interpolation: Interpolation) -> RgbaImage {
    let filter = match interpolation {
        Interpolation::Nearest => return imageops::resize(image, width, height, FilterType::Nearest),
        Interpolation::Bilinear => FilterType::Triangle,
        Interpolation::Bicubic => FilterType::CatmullRom,
        Interpolation::Lanczos => FilterType::Lanczos3,
    };
    // The pixels mixed are weighed by how opaque they are, or the colour see-through
    // ones happen to have would bleed into their neighbours.
    let premultiplied = Rgba32FImage::from_fn(image.width(), image.height(), |x, y| {
        let [r, g, b, a] = image.get_pixel(x, y).0.map(|channel| channel as f32 / 255.0);
        Rgba([r * a, g * a, b * a, a])
    });
    let resized = imageops::resize(&premultiplied, width, height, filter);
    // Bicubic and Lanczos overshoot around sharp edges, past what a channel holds.
    let to_byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    RgbaImage::from_fn(width, height, |x, y| {
        let [r, g, b, a] = resized.get_pixel(x, y).0;
        if a <= 0.0 {
            return Rgba([0, 0, 0, 0]);
        }
        Rgba([to_byte(r / a), to_byte(g / a), to_byte(b / a), to_byte(a)])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resizing_keeps_see_through_colours_out() {
        // Opaque red next to transparent green, which must not tint the red.
        let image = RgbaImage::from_fn(4, 1, |x, _| if x < 2 { Rgba([255, 0, 0, 255]) } else { Rgba([0, 255, 0, 0]) });
        for interpolation in Interpolation::ALL {
            let resized = resize(&image, 8, 2, interpolation);
            assert_eq!(resized.dimensions(), (8, 2));
            for pixel in resized.pixels().filter(|pixel| pixel[3] > 0) {
                assert_eq!((pixel[0], pixel[1]), (255, 0), "{interpolation:?}");
            }
        }
        let nearest = resize(&image, 2, 1, Interpolation::Nearest);
        assert_eq!(nearest.as_raw(), &[255, 0, 0, 255, 0, 255, 0, 0]);
    }
}

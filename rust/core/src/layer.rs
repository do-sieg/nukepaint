use image::RgbaImage;

/// One sheet of pixels in a document, in straight (not premultiplied) RGBA.
pub struct Layer {
    pub name: String,
    pub pixels: RgbaImage,
}

impl Layer {
    pub fn new(name: impl Into<String>, pixels: RgbaImage) -> Self {
        Self { name: name.into(), pixels }
    }
}

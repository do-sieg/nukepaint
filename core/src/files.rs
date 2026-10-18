use std::{
    fmt,
    fs::{self, File},
    io::{self, BufWriter},
    path::Path,
};

use image::{DynamicImage, ImageFormat, RgbaImage, codecs::jpeg::JpegEncoder};
use resvg::{tiny_skia, usvg};

use crate::open_raster::is_open_raster;

/// File extensions that can be opened, lower case.
pub const OPEN_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "bmp", "webp", "tga", "svg", "ora"];

/// File extensions that can be saved to, lower case. The first is the one to use
/// when a file has none of them and the image is flat, the last when it has layers.
pub const SAVE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp", "tga", "ora"];

/// How good JPEG files are made, from 1 to 100.
const JPEG_QUALITY: u8 = 90;

#[derive(Debug)]
pub enum OpenError {
    Io(io::Error),
    Image(image::ImageError),
    Svg(usvg::Error),
    /// The SVG has no area to draw in.
    EmptySvg,
    Zip(zip::result::ZipError),
    Xml(roxmltree::Error),
    /// The OpenRaster file has no size, or no layer that can be read.
    NoLayers,
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Image(error) => error.fmt(f),
            Self::Svg(error) => error.fmt(f),
            Self::EmptySvg => f.write_str("the SVG image is empty"),
            Self::Zip(error) => error.fmt(f),
            Self::Xml(error) => error.fmt(f),
            Self::NoLayers => f.write_str("the OpenRaster file has no layers"),
        }
    }
}

impl std::error::Error for OpenError {}

/// Reads an image file into straight RGBA pixels. An SVG is drawn at its own size.
pub fn load_image(path: &Path) -> Result<RgbaImage, OpenError> {
    let is_svg = path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"));
    if is_svg {
        return load_svg(path);
    }
    let mut reader = image::ImageReader::open(path)
        .map_err(OpenError::Io)?
        .with_guessed_format()
        .map_err(OpenError::Io)?;
    // The default limits refuse images over 512 MB, which large photos and scans reach.
    reader.no_limits();
    Ok(reader.decode().map_err(OpenError::Image)?.into_rgba8())
}

fn load_svg(path: &Path) -> Result<RgbaImage, OpenError> {
    render_svg(&fs::read(path).map_err(OpenError::Io)?, 1.0)
}

/// Draws an SVG image at `scale` times its own size, into straight RGBA pixels.
pub fn render_svg(data: &[u8], scale: f32) -> Result<RgbaImage, OpenError> {
    let tree = usvg::Tree::from_data(data, &usvg::Options::default()).map_err(OpenError::Svg)?;
    render_tree(&tree, scale)
}

/// Draws an SVG image `width` pixels wide, its height in proportion, into straight RGBA pixels.
pub fn render_svg_to_width(data: &[u8], width: f32) -> Result<RgbaImage, OpenError> {
    let tree = usvg::Tree::from_data(data, &usvg::Options::default()).map_err(OpenError::Svg)?;
    render_tree(&tree, width / tree.size().width())
}

fn render_tree(tree: &usvg::Tree, scale: f32) -> Result<RgbaImage, OpenError> {
    let size = tree.size().to_int_size().scale_by(scale).ok_or(OpenError::EmptySvg)?;
    let mut pixmap = tiny_skia::Pixmap::new(size.width(), size.height()).ok_or(OpenError::EmptySvg)?;
    resvg::render(tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());

    // The pixmap is premultiplied.
    let pixels = pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let color = pixel.demultiply();
            [color.red(), color.green(), color.blue(), color.alpha()]
        })
        .collect();
    Ok(RgbaImage::from_raw(size.width(), size.height(), pixels).expect("pixmap size matches"))
}

/// Whether a file of that name keeps the layers of an image, from its extension.
pub fn keeps_layers(path: &Path) -> bool {
    is_open_raster(path)
}

/// Whether a file of that name can be saved to, from its extension.
pub fn is_savable(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        SAVE_EXTENSIONS.iter().any(|savable| extension.eq_ignore_ascii_case(savable))
    })
}

/// Writes straight RGBA pixels to a file, in the format its extension names. JPEG
/// files have no transparency, so it is dropped from them.
pub fn save_image(path: &Path, image: &RgbaImage) -> Result<(), image::ImageError> {
    let format = ImageFormat::from_path(path)?;
    if format == ImageFormat::Jpeg {
        let rgb = DynamicImage::ImageRgba8(image.clone()).into_rgb8();
        let file = BufWriter::new(File::create(path)?);
        return rgb.write_with_encoder(JpegEncoder::new_with_quality(file, JPEG_QUALITY));
    }
    image.save_with_format(path, format)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn savable_from_the_extension() {
        assert!(is_savable(Path::new("a/b.PNG")));
        assert!(is_savable(Path::new("b.jpeg")));
        assert!(!is_savable(Path::new("b.svg")));
        assert!(!is_savable(Path::new("png")));
        assert!(is_savable(Path::new("b.ora")));
    }

    #[test]
    fn saved_pixels_open_again() {
        let dir = std::env::temp_dir().join(format!("nukepaint-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let image = RgbaImage::from_fn(3, 2, |x, y| image::Rgba([x as u8 * 80, y as u8 * 100, 50, 128]));
        for extension in ["png", "webp", "tga"] {
            let path = dir.join(format!("image.{extension}"));
            save_image(&path, &image).unwrap();
            assert_eq!(load_image(&path).unwrap(), image, "{extension}");
        }
        // JPEG keeps the size, not the transparency.
        let path = dir.join("image.jpg");
        save_image(&path, &image).unwrap();
        let loaded = load_image(&path).unwrap();
        assert_eq!(loaded.dimensions(), (3, 2));
        assert!(loaded.pixels().all(|pixel| pixel[3] == 255));
        fs::remove_dir_all(&dir).unwrap();
    }
}

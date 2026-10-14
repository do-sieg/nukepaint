use std::{fmt, fs, io, path::Path};

use image::RgbaImage;
use resvg::{tiny_skia, usvg};

/// File extensions that can be opened, lower case.
pub const OPEN_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "bmp", "webp", "tga", "svg"];

#[derive(Debug)]
pub enum OpenError {
    Io(io::Error),
    Image(image::ImageError),
    Svg(usvg::Error),
    /// The SVG has no area to draw in.
    EmptySvg,
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Image(error) => error.fmt(f),
            Self::Svg(error) => error.fmt(f),
            Self::EmptySvg => f.write_str("the SVG image is empty"),
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
    let data = fs::read(path).map_err(OpenError::Io)?;
    let tree = usvg::Tree::from_data(&data, &usvg::Options::default()).map_err(OpenError::Svg)?;
    let size = tree.size().to_int_size();
    let mut pixmap = tiny_skia::Pixmap::new(size.width(), size.height()).ok_or(OpenError::EmptySvg)?;
    resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());

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

//! OpenRaster files (.ora), which keep the layers of an image and which other
//! editors, GIMP and Krita among them, open as well. Such a file is a zip archive: a
//! PNG for each layer, and a list of the layers, top one first.

use std::{
    fs::File,
    io::{self, BufReader, BufWriter, Cursor, Read, Write},
    path::Path,
};

use image::{ImageError, ImageFormat, ImageReader, RgbaImage, codecs::png::PngEncoder, imageops};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::{BlendMode, Layer, MAX_OPACITY, OpenError, thumbnail};

pub const EXTENSION: &str = "ora";
const MIME_TYPE: &str = "image/openraster";
/// Longest side of the small picture of the image that the file must hold.
const THUMBNAIL_SIZE: u32 = 256;

/// What the file calls a blend mode. Those the format does not know are given a name
/// of our own, which other editors take for Normal.
fn blend_name(mode: BlendMode) -> &'static str {
    match mode {
        BlendMode::Normal => "svg:src-over",
        BlendMode::Multiply => "svg:multiply",
        BlendMode::Additive => "svg:plus",
        BlendMode::ColorBurn => "svg:color-burn",
        BlendMode::ColorDodge => "svg:color-dodge",
        BlendMode::Reflect => "nukepaint:reflect",
        BlendMode::Glow => "nukepaint:glow",
        BlendMode::Overlay => "svg:overlay",
        BlendMode::Difference => "svg:difference",
        BlendMode::Negation => "nukepaint:negation",
        BlendMode::Lighten => "svg:lighten",
        BlendMode::Darken => "svg:darken",
        BlendMode::Screen => "svg:screen",
        BlendMode::Xor => "nukepaint:xor",
        BlendMode::GrainMerge => "nukepaint:grain-merge",
    }
}

/// Whether the file at that path is an OpenRaster one, from its extension.
pub fn is_open_raster(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case(EXTENSION))
}

/// Writes layers, bottom one first, all of the image's size, and the image they give
/// once blended together.
pub fn save(path: &Path, layers: &[Layer], merged: &RgbaImage) -> Result<(), ImageError> {
    let (width, height) = merged.dimensions();
    let mut stack = format!("<?xml version='1.0' encoding='UTF-8'?>\n<image version=\"0.0.3\" w=\"{width}\" h=\"{height}\">\n<stack>\n");
    for (index, layer) in layers.iter().enumerate().rev() {
        stack += &format!(
            "<layer name=\"{}\" src=\"data/layer{index}.png\" x=\"0\" y=\"0\" opacity=\"{}\" visibility=\"{}\" composite-op=\"{}\"/>\n",
            escape(&layer.name),
            f32::from(layer.opacity) / f32::from(MAX_OPACITY),
            if layer.visible { "visible" } else { "hidden" },
            blend_name(layer.blend_mode),
        );
    }
    stack += "</stack>\n</image>\n";

    let mut zip = ZipWriter::new(BufWriter::new(File::create(path)?));
    // PNG files are compressed already, and the format wants its type first in the
    // archive, as it is.
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let start = |zip: &mut ZipWriter<_>, name: &str| zip.start_file(name, options).map_err(io::Error::from);
    start(&mut zip, "mimetype")?;
    zip.write_all(MIME_TYPE.as_bytes())?;
    start(&mut zip, "stack.xml")?;
    zip.write_all(stack.as_bytes())?;
    for (index, layer) in layers.iter().enumerate() {
        start(&mut zip, &format!("data/layer{index}.png"))?;
        layer.pixels.write_with_encoder(PngEncoder::new(&mut zip))?;
    }
    start(&mut zip, "mergedimage.png")?;
    merged.write_with_encoder(PngEncoder::new(&mut zip))?;
    start(&mut zip, "Thumbnails/thumbnail.png")?;
    thumbnail(merged, THUMBNAIL_SIZE).write_with_encoder(PngEncoder::new(&mut zip))?;
    zip.finish().map_err(io::Error::from)?.flush()?;
    Ok(())
}

fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

/// Reads the layers of a file, bottom one first, each made as large as the image.
/// Groups of layers are not kept: their layers are, in the place the groups put them.
/// Layers whose pixels cannot be read are left out.
pub fn load(path: &Path) -> Result<Vec<Layer>, OpenError> {
    let mut zip = ZipArchive::new(BufReader::new(File::open(path).map_err(OpenError::Io)?)).map_err(OpenError::Zip)?;
    let stack = read_entry(&mut zip, "stack.xml")?;
    let stack = String::from_utf8_lossy(&stack);
    let tree = roxmltree::Document::parse(&stack).map_err(OpenError::Xml)?;
    let image = tree.root_element();
    let size = |name| image.attribute(name).and_then(|value| value.trim().parse::<u32>().ok()).filter(|&side| side > 0);
    let (Some(width), Some(height)) = (size("w"), size("h")) else {
        return Err(OpenError::NoLayers);
    };
    let mut layers = Vec::new();
    if let Some(stack) = image.children().find(|node| node.has_tag_name("stack")) {
        read_stack(&mut zip, stack, (0, 0), (width, height), &mut layers);
    }
    if layers.is_empty() {
        return Err(OpenError::NoLayers);
    }
    // The file lists the top layer first.
    layers.reverse();
    Ok(layers)
}

/// Adds the layers of a stack, top one first, placed from where the stack is.
fn read_stack<R: Read + io::Seek>(
    zip: &mut ZipArchive<R>,
    stack: roxmltree::Node,
    origin: (i64, i64),
    size: (u32, u32),
    layers: &mut Vec<Layer>,
) {
    for node in stack.children().filter(roxmltree::Node::is_element) {
        let offset = |name| node.attribute(name).and_then(|value| value.trim().parse::<f64>().ok()).unwrap_or(0.0) as i64;
        let place = (origin.0 + offset("x"), origin.1 + offset("y"));
        if node.has_tag_name("stack") {
            read_stack(zip, node, place, size, layers);
            continue;
        }
        if !node.has_tag_name("layer") {
            continue;
        }
        let Some(pixels) = node.attribute("src").and_then(|source| read_png(zip, source)) else {
            continue;
        };
        let pixels = if place == (0, 0) && pixels.dimensions() == size {
            pixels
        } else {
            let mut placed = RgbaImage::new(size.0, size.1);
            imageops::replace(&mut placed, &pixels, place.0, place.1);
            placed
        };
        let mut layer = Layer::new(node.attribute("name").unwrap_or_default(), pixels);
        layer.visible = node.attribute("visibility") != Some("hidden");
        if let Some(opacity) = node.attribute("opacity").and_then(|value| value.trim().parse::<f32>().ok()) {
            layer.opacity = (opacity.clamp(0.0, 1.0) * f32::from(MAX_OPACITY)).round() as u8;
        }
        if let Some(name) = node.attribute("composite-op") {
            layer.blend_mode = BlendMode::ALL.into_iter().find(|&mode| blend_name(mode) == name).unwrap_or_default();
        }
        layers.push(layer);
    }
}

fn read_entry<R: Read + io::Seek>(zip: &mut ZipArchive<R>, name: &str) -> Result<Vec<u8>, OpenError> {
    let mut entry = zip.by_name(name).map_err(OpenError::Zip)?;
    let mut data = Vec::new();
    entry.read_to_end(&mut data).map_err(OpenError::Io)?;
    Ok(data)
}

fn read_png<R: Read + io::Seek>(zip: &mut ZipArchive<R>, name: &str) -> Option<RgbaImage> {
    let data = read_entry(zip, name).ok()?;
    let mut reader = ImageReader::with_format(Cursor::new(data), ImageFormat::Png);
    // As for other files, large images are not refused.
    reader.no_limits();
    Some(reader.decode().ok()?.into_rgba8())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composite;

    #[test]
    fn saved_layers_open_again() {
        let dir = std::env::temp_dir().join(format!("nukepaint-ora-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("image.ora");
        let mut top = Layer::new("Top & \"quoted\" <name>", RgbaImage::from_fn(3, 2, |x, y| image::Rgba([x as u8 * 80, y as u8 * 100, 50, 128])));
        top.opacity = 100;
        top.visible = false;
        top.blend_mode = BlendMode::Xor;
        let bottom = Layer::new("Background", RgbaImage::from_pixel(3, 2, image::Rgba([10, 20, 30, 255])));
        let layers = vec![bottom, top];
        save(&path, &layers, &composite(&layers)).unwrap();

        let loaded = load(&path).unwrap();
        assert_eq!(loaded.len(), 2);
        for (loaded, layer) in loaded.iter().zip(&layers) {
            assert_eq!(loaded.name, layer.name);
            assert_eq!(loaded.pixels, layer.pixels);
            assert_eq!(loaded.opacity, layer.opacity);
            assert_eq!(loaded.visible, layer.visible);
            assert_eq!(loaded.blend_mode, layer.blend_mode);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn layers_are_placed_where_their_groups_put_them() {
        let dir = std::env::temp_dir().join(format!("nukepaint-ora-groups-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("image.ora");
        let stack = r#"<image w="4" h="3"><stack>
            <stack x="1" y="1"><layer src="a.png" x="1" y="0" opacity="0.5" composite-op="svg:multiply"/></stack>
            <layer name="bottom" src="b.png"/>
        </stack></image>"#;
        let mut zip = ZipWriter::new(File::create(&path).unwrap());
        let options = SimpleFileOptions::default();
        zip.start_file("mimetype", options).unwrap();
        zip.write_all(MIME_TYPE.as_bytes()).unwrap();
        zip.start_file("stack.xml", options).unwrap();
        zip.write_all(stack.as_bytes()).unwrap();
        for (name, side) in [("a.png", 1), ("b.png", 4)] {
            zip.start_file(name, options).unwrap();
            let pixels = RgbaImage::from_pixel(side, side.min(3), image::Rgba([255, 0, 0, 255]));
            let mut png = Vec::new();
            pixels.write_to(&mut Cursor::new(&mut png), ImageFormat::Png).unwrap();
            zip.write_all(&png).unwrap();
        }
        zip.finish().unwrap();

        let loaded = load(&path).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].name, "bottom");
        let top = &loaded[1];
        assert_eq!(top.pixels.dimensions(), (4, 3));
        assert_eq!(top.pixels.get_pixel(2, 1)[3], 255);
        assert_eq!(top.pixels.pixels().filter(|pixel| pixel[3] > 0).count(), 1);
        assert_eq!(top.opacity, 128);
        assert_eq!(top.blend_mode, BlendMode::Multiply);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

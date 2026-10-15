//! Layers blended together, the way each says, as `layer_blend.gdshader` does on the
//! GPU in the Godot version. Large images are cut into bands of rows, blended at the
//! same time on as many threads as there are cores.

use std::{num::NonZero, thread};

use image::{RgbaImage, imageops};

use crate::{BlendMode, Layer, MAX_OPACITY, PixelRect};

/// Fewest pixels worth sharing between threads.
const PARALLEL_PIXELS: usize = 1 << 16;

/// The layers that show, the first one at the bottom, blended into one image as large
/// as they are. They must all be the same size.
pub fn composite(layers: &[Layer]) -> RgbaImage {
    let (width, height) = layers[0].pixels.dimensions();
    let mut shown = layers.iter().filter(|layer| layer.shows());
    let Some(bottom) = shown.next() else {
        return RgbaImage::new(width, height);
    };
    // Over nothing, a layer shows as it is in any mode, only as opaque as it is set to.
    let mut image = bottom.pixels.clone();
    if bottom.opacity < MAX_OPACITY {
        let opacity = bottom.opacity as f32 / MAX_OPACITY as f32;
        for pixel in image.pixels_mut() {
            pixel[3] = to_byte(pixel[3] as f32 / 255.0 * opacity);
        }
    }
    for layer in shown {
        blend_layer(&mut image, layer);
    }
    image
}

/// The layers that show blended together over one area of the image only, which
/// must be on it.
pub fn composite_area(layers: &[Layer], area: PixelRect) -> RgbaImage {
    let (left, top, width, height) = (area.left as u32, area.top as u32, area.width() as u32, area.height() as u32);
    let shown = layers.iter().filter(|layer| layer.shows());
    let cropped: Vec<Layer> = shown
        .map(|layer| Layer {
            name: String::new(),
            pixels: imageops::crop_imm(&layer.pixels, left, top, width, height).to_image(),
            ..*layer
        })
        .collect();
    if cropped.is_empty() {
        return RgbaImage::new(width, height);
    }
    composite(&cropped)
}

/// Blends the layer over the pixels under it, as opaque as it is set to and in its
/// mode, whether it shows or not. They must be the same size.
pub fn blend_layer(under: &mut RgbaImage, layer: &Layer) {
    let opacity = layer.opacity as f32 / MAX_OPACITY as f32;
    let mode = layer.blend_mode;
    let row_len = under.width() as usize * 4;
    let rows = under.height() as usize;
    let blend_rows = |under: &mut [u8], over: &[u8]| {
        for (under, over) in under.as_chunks_mut::<4>().0.iter_mut().zip(over.as_chunks::<4>().0) {
            blend_pixel(under, over, opacity, mode);
        }
    };
    let threads = thread::available_parallelism().map_or(1, NonZero::get);
    if threads == 1 || row_len * rows / 4 < PARALLEL_PIXELS {
        blend_rows(under, &layer.pixels);
        return;
    }
    let band = rows.div_ceil(threads) * row_len;
    thread::scope(|scope| {
        for (under, over) in under.chunks_mut(band).zip(layer.pixels.chunks(band)) {
            scope.spawn(|| blend_rows(under, over));
        }
    });
}

/// One straight RGBA pixel over another. The two only mix where both are: elsewhere,
/// each shows as it is.
fn blend_pixel(under: &mut [u8; 4], over: &[u8; 4], opacity: f32, mode: BlendMode) {
    let over_alpha = over[3] as f32 / 255.0 * opacity;
    if over_alpha <= 0.0 {
        return;
    }
    let under_alpha = under[3] as f32 / 255.0;
    let both = over_alpha * under_alpha;
    let alpha = over_alpha + under_alpha - both;
    for channel in 0..3 {
        let (below, above) = (under[channel], over[channel]);
        let mixed = if both > 0.0 { blend(mode, below, above) } else { 0.0 };
        let color = (over_alpha - both) * (above as f32 / 255.0)
            + (under_alpha - both) * (below as f32 / 255.0)
            + both * mixed;
        under[channel] = to_byte(color / alpha);
    }
    under[3] = to_byte(alpha);
}

/// The colour two opaque channels give, in 0–1: one of the layer over one of what is
/// under it.
fn blend(mode: BlendMode, under: u8, over: u8) -> f32 {
    const TINY: f32 = 0.0001;
    let (u, o) = (under as f32 / 255.0, over as f32 / 255.0);
    match mode {
        BlendMode::Normal => o,
        BlendMode::Multiply => u * o,
        BlendMode::Additive => (u + o).min(1.0),
        BlendMode::ColorBurn if over == 0 => 0.0,
        BlendMode::ColorBurn => (1.0 - (1.0 - u) / o.max(TINY)).max(0.0),
        BlendMode::ColorDodge | BlendMode::Reflect if over == 255 => 1.0,
        BlendMode::ColorDodge => (u / (1.0 - o).max(TINY)).min(1.0),
        BlendMode::Reflect => (u * u / (1.0 - o).max(TINY)).min(1.0),
        BlendMode::Glow if under == 255 => 1.0,
        BlendMode::Glow => (o * o / (1.0 - u).max(TINY)).min(1.0),
        BlendMode::Overlay if u < 0.5 => 2.0 * u * o,
        BlendMode::Overlay => 1.0 - 2.0 * (1.0 - u) * (1.0 - o),
        BlendMode::Difference => (u - o).abs(),
        BlendMode::Negation => 1.0 - (1.0 - u - o).abs(),
        BlendMode::Lighten => u.max(o),
        BlendMode::Darken => u.min(o),
        BlendMode::Screen => u + o - u * o,
        BlendMode::Xor => (under ^ over) as f32 / 255.0,
    }
}

fn to_byte(value: f32) -> u8 {
    (value * 255.0).round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use image::Rgba;

    use super::*;

    fn layer(color: [u8; 4]) -> Layer {
        Layer::new("", RgbaImage::from_pixel(2, 2, Rgba(color)))
    }

    fn top_left(layers: &[Layer]) -> [u8; 4] {
        composite(layers)[(0, 0)].0
    }

    #[test]
    fn opaque_normal_layer_hides_what_is_under() {
        assert_eq!(top_left(&[layer([255, 0, 0, 255]), layer([0, 0, 255, 255])]), [0, 0, 255, 255]);
    }

    #[test]
    fn hidden_layers_are_left_out() {
        let mut top = layer([0, 0, 255, 255]);
        top.visible = false;
        assert_eq!(top_left(&[layer([255, 0, 0, 255]), top.clone()]), [255, 0, 0, 255]);
        let mut bottom = layer([255, 0, 0, 255]);
        bottom.opacity = 0;
        top.visible = true;
        assert_eq!(top_left(&[bottom.clone(), top]), [0, 0, 255, 255]);
        bottom.visible = false;
        assert_eq!(top_left(&[bottom]), [0, 0, 0, 0]);
    }

    #[test]
    fn an_area_blends_as_the_whole_does() {
        let bottom = Layer::new("", RgbaImage::from_fn(5, 4, |x, y| Rgba([x as u8 * 50, y as u8 * 60, 0, 255])));
        let mut top = Layer::new("", RgbaImage::from_fn(5, 4, |x, y| Rgba([0, 0, 255, (x + y) as u8 * 30])));
        top.blend_mode = BlendMode::Multiply;
        let layers = [bottom, top];
        let area = PixelRect { left: 1, top: 2, right: 4, bottom: 4 };
        let whole = composite(&layers);
        assert_eq!(composite_area(&layers, area), imageops::crop_imm(&whole, 1, 2, 3, 2).to_image());
    }

    #[test]
    fn opacity_mixes_with_what_is_under() {
        let mut top = layer([0, 0, 255, 255]);
        top.opacity = 128;
        assert_eq!(top_left(&[layer([255, 0, 0, 255]), top.clone()]), [127, 0, 128, 255]);
        // Over nothing, it is only see-through.
        assert_eq!(top_left(&[top]), [0, 0, 255, 128]);
    }

    #[test]
    fn modes_mix_where_both_are() {
        let mut top = layer([128, 255, 0, 255]);
        top.blend_mode = BlendMode::Multiply;
        assert_eq!(top_left(&[layer([255, 128, 255, 255]), top.clone()]), [128, 128, 0, 255]);
        // Over nothing, the layer shows as it is.
        assert_eq!(top_left(&[layer([0, 0, 0, 0]), top.clone()]), [128, 255, 0, 255]);
        top.blend_mode = BlendMode::Xor;
        assert_eq!(top_left(&[layer([255, 255, 255, 255]), top]), [127, 0, 255, 255]);
    }

    #[test]
    fn large_images_blend_the_same_in_bands() {
        let size = 300;
        let gradient = |x: u32, y: u32| Rgba([x as u8, y as u8, (x + y) as u8, 200]);
        let bottom = Layer::new("", RgbaImage::from_fn(size, size, gradient));
        let mut top = Layer::new("", RgbaImage::from_fn(size, size, |x, y| gradient(y, x)));
        top.blend_mode = BlendMode::Overlay;
        let image = composite(&[bottom.clone(), top.clone()]);
        let mut pixel = bottom.pixels[(17, 250)].0;
        blend_pixel(&mut pixel, &top.pixels[(17, 250)].0, 1.0, BlendMode::Overlay);
        assert_eq!(image[(17, 250)].0, pixel);
    }
}

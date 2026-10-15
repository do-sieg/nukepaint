//! Selected pixels lifted out of a layer, to be moved, stretched and turned, then drawn
//! back into it. Each placing starts again from the pixels as they were lifted, so
//! nothing degrades however many times they are placed.

use image::{Rgba, RgbaImage};

use crate::{Affine, PixelRect, Selection, copy_selected, erase_selected};

pub struct FloatingPixels {
    /// Where on the layer the pixels were lifted from.
    area: PixelRect,
    /// The pixels of that area, the selected ones only: the others are transparent.
    pixels: RgbaImage,
    /// The layer with a transparent hole where they were.
    base: RgbaImage,
}

impl FloatingPixels {
    /// Lifts the selected pixels of the layer, if any of the selection is on it.
    pub fn lift(layer: &RgbaImage, selection: &Selection) -> Option<Self> {
        let (area, pixels) = copy_selected(layer, selection)?;
        let mut base = layer.clone();
        erase_selected(&mut base, selection);
        Some(Self { area, pixels, base })
    }

    /// Pixels put over the layer with their top left corner at `at`, which may leave
    /// some of them off it.
    pub fn pasted(layer: &RgbaImage, pixels: RgbaImage, at: (i32, i32)) -> Self {
        let (width, height) = (pixels.width() as i32, pixels.height() as i32);
        let area = PixelRect { left: at.0, top: at.1, right: at.0 + width, bottom: at.1 + height };
        Self { area, pixels, base: layer.clone() }
    }

    pub fn area(&self) -> PixelRect {
        self.area
    }

    pub fn pixels(&self) -> &RgbaImage {
        &self.pixels
    }

    /// The layer without the pixels.
    pub fn base(&self) -> &RgbaImage {
        &self.base
    }

    /// The layer with the pixels drawn over it where `map` carries them. Pixels that
    /// only move by whole pixels, or are mirrored, are copied as they are; others are
    /// blended from the four nearest.
    pub fn place(&self, map: &Affine) -> RgbaImage {
        let mut layer = self.base.clone();
        let Some(back) = map.inverse() else {
            return layer;
        };
        let (width, height) = layer.dimensions();
        // Where the area lands, a pixel wider for the blended edges, within the layer.
        let PixelRect { left, top, right, bottom } = self.area;
        let corners = [(left, top), (right, top), (right, bottom), (left, bottom)];
        let corners = corners.map(|(x, y)| map.apply((x as f64, y as f64)));
        let min_x = corners.iter().map(|c| c.0).fold(f64::INFINITY, f64::min);
        let max_x = corners.iter().map(|c| c.0).fold(f64::NEG_INFINITY, f64::max);
        let min_y = corners.iter().map(|c| c.1).fold(f64::INFINITY, f64::min);
        let max_y = corners.iter().map(|c| c.1).fold(f64::NEG_INFINITY, f64::max);
        let (from_x, to_x) = ((min_x.floor() - 1.0).max(0.0) as u32, (max_x.ceil() + 1.0).min(width as f64) as u32);
        let (from_y, to_y) = ((min_y.floor() - 1.0).max(0.0) as u32, (max_y.ceil() + 1.0).min(height as f64) as u32);

        let unit = |value: f64| (value.abs() - 1.0).abs() < 1e-9;
        let none = |value: f64| value.abs() < 1e-9;
        let exact =
            unit(map.xx) && unit(map.yy) && none(map.xy) && none(map.yx) && is_whole(map.dx) && is_whole(map.dy);
        for y in from_y..to_y {
            for x in from_x..to_x {
                let (sx, sy) = back.apply((x as f64 + 0.5, y as f64 + 0.5));
                // In pixels of the area, from the middle of its first one.
                let (sx, sy) = (sx - left as f64 - 0.5, sy - top as f64 - 0.5);
                let color = if exact { self.nearest(sx, sy) } else { self.bilinear(sx, sy) };
                if color[3] > 0.0 {
                    let below = layer.get_pixel_mut(x, y);
                    *below = blend_over(color, *below);
                }
            }
        }
        layer
    }

    /// The pixel at a place, premultiplied, in 0–1.
    fn premultiplied(&self, x: i64, y: i64) -> [f32; 4] {
        if x < 0 || y < 0 || x >= self.pixels.width() as i64 || y >= self.pixels.height() as i64 {
            return [0.0; 4];
        }
        let Rgba([r, g, b, a]) = *self.pixels.get_pixel(x as u32, y as u32);
        let alpha = a as f32 / 255.0;
        [r as f32 / 255.0 * alpha, g as f32 / 255.0 * alpha, b as f32 / 255.0 * alpha, alpha]
    }

    fn nearest(&self, x: f64, y: f64) -> [f32; 4] {
        self.premultiplied(x.round() as i64, y.round() as i64)
    }

    fn bilinear(&self, x: f64, y: f64) -> [f32; 4] {
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = ((x - x0) as f32, (y - y0) as f32);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let mut color = [0.0; 4];
        let (gx, gy) = (1.0 - fx, 1.0 - fy);
        let weights = [(0, 0, gx * gy), (1, 0, fx * gy), (0, 1, gx * fy), (1, 1, fx * fy)];
        for (dx, dy, weight) in weights {
            if weight > 0.0 {
                let sample = self.premultiplied(x0 + dx, y0 + dy);
                for (channel, value) in color.iter_mut().zip(sample) {
                    *channel += value * weight;
                }
            }
        }
        color
    }
}

fn is_whole(value: f64) -> bool {
    (value - value.round()).abs() < 1e-9
}

/// A premultiplied colour over a straight one.
fn blend_over([r, g, b, a]: [f32; 4], below: Rgba<u8>) -> Rgba<u8> {
    let below_alpha = below[3] as f32 / 255.0;
    let left = below_alpha * (1.0 - a);
    let alpha = a + left;
    if alpha <= 0.0 {
        return Rgba([0, 0, 0, 0]);
    }
    let channel = |top: f32, below: u8| {
        ((top + below as f32 / 255.0 * left) / alpha * 255.0).round().clamp(0.0, 255.0) as u8
    };
    Rgba([channel(r, below[0]), channel(g, below[1]), channel(b, below[2]), (alpha * 255.0).round() as u8])
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Rgba<u8> = Rgba([255, 0, 0, 255]);
    const BLUE: Rgba<u8> = Rgba([0, 0, 255, 255]);
    const CLEAR: Rgba<u8> = Rgba([0, 0, 0, 0]);

    /// A blue layer with a red square at 2, 2 to 4, 4.
    fn layer() -> RgbaImage {
        RgbaImage::from_fn(8, 8, |x, y| if (2..4).contains(&x) && (2..4).contains(&y) { RED } else { BLUE })
    }

    fn square() -> Selection {
        Selection::rect(PixelRect { left: 2, top: 2, right: 4, bottom: 4 })
    }

    #[test]
    fn lifting_leaves_a_hole() {
        let floating = FloatingPixels::lift(&layer(), &square()).unwrap();
        assert_eq!(floating.area(), PixelRect { left: 2, top: 2, right: 4, bottom: 4 });
        assert!(floating.pixels().pixels().all(|&pixel| pixel == RED));
        assert_eq!(*floating.base().get_pixel(2, 3), CLEAR);
        assert_eq!(*floating.base().get_pixel(4, 3), BLUE);
    }

    #[test]
    fn pasting_keeps_what_is_under() {
        let pasted = FloatingPixels::pasted(&layer(), RgbaImage::from_pixel(2, 2, RED), (6, 6));
        assert_eq!(pasted.base(), &layer());
        let placed = pasted.place(&Affine::IDENTITY);
        assert_eq!(*placed.get_pixel(7, 7), RED);
        // Moved away, the layer is as it was.
        assert_eq!(pasted.place(&Affine::translation(-6.0, 0.0)).get_pixel(7, 7), &BLUE);
        // Off the layer, nothing is placed.
        assert_eq!(pasted.place(&Affine::translation(10.0, 10.0)), layer());
    }

    #[test]
    fn nothing_to_lift_off_the_layer() {
        let outside = Selection::rect(PixelRect { left: 20, top: 20, right: 24, bottom: 24 });
        assert!(FloatingPixels::lift(&layer(), &outside).is_none());
    }

    #[test]
    fn moving_by_whole_pixels_copies_them() {
        let floating = FloatingPixels::lift(&layer(), &square()).unwrap();
        let placed = floating.place(&Affine::translation(3.0, 1.0));
        assert_eq!(*placed.get_pixel(5, 3), RED);
        assert_eq!(*placed.get_pixel(6, 4), RED);
        assert_eq!(*placed.get_pixel(3, 2), CLEAR);
        assert_eq!(*placed.get_pixel(4, 2), BLUE);
        assert_eq!(*placed.get_pixel(0, 0), BLUE);
        // Nothing changes until placed.
        assert_eq!(floating.place(&Affine::IDENTITY), layer());
    }

    #[test]
    fn doubling_fills_four_times_the_pixels() {
        let floating = FloatingPixels::lift(&layer(), &square()).unwrap();
        let double = Affine::translation(-2.0, -2.0).then(Affine::scale(2.0, 2.0)).then(Affine::translation(2.0, 2.0));
        let placed = floating.place(&double);
        // Inside, away from the blended edge, all red.
        assert_eq!(*placed.get_pixel(3, 3), RED);
        assert_eq!(*placed.get_pixel(4, 4), RED);
        assert_eq!(*placed.get_pixel(7, 7), BLUE);
    }

    #[test]
    fn blending_half_over() {
        assert_eq!(blend_over([0.5, 0.0, 0.0, 0.5], BLUE), Rgba([128, 0, 128, 255]));
        assert_eq!(blend_over([0.0; 4], BLUE), BLUE);
        assert_eq!(blend_over([0.5, 0.0, 0.0, 0.5], CLEAR), Rgba([255, 0, 0, 128]));
    }
}

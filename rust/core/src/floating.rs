//! Selected pixels lifted out of a layer, to be moved, stretched and turned, then drawn
//! back into it. Each placing starts again from the pixels as they were lifted, so
//! nothing degrades however many times they are placed.

use image::{Rgba, RgbaImage};

use crate::{Affine, Interpolation, PixelRect, Projective, Selection, SelectionMask, copy_selected, erase_selected};

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

    /// Lifts the pixels of the layer that `mask` holds, or all of them without one, if
    /// any. The mask must be as large as the layer.
    pub fn lift_masked(layer: &RgbaImage, mask: Option<&SelectionMask>) -> Option<Self> {
        let (width, height) = layer.dimensions();
        let whole = PixelRect { left: 0, top: 0, right: width as i32, bottom: height as i32 };
        let area = match mask {
            Some(mask) => mask.bounds()?,
            None if whole.is_empty() => return None,
            None => whole,
        };
        let mut pixels = RgbaImage::new(area.width() as u32, area.height() as u32);
        let mut base = layer.clone();
        for y in area.top as u32..area.bottom as u32 {
            for x in area.left as u32..area.right as u32 {
                if mask.is_none_or(|mask| mask.contains(x, y)) {
                    pixels.put_pixel(x - area.left as u32, y - area.top as u32, *layer.get_pixel(x, y));
                    base.put_pixel(x, y, Rgba([0, 0, 0, 0]));
                }
            }
        }
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
    /// redrawn from those around as `interpolation` says. Nothing is drawn if `map` tilts
    /// part of them behind the eye.
    pub fn place(&self, map: impl Into<Projective>, interpolation: Interpolation) -> RgbaImage {
        self.place_wrapped(map, interpolation, None)
    }

    /// The layer with the pixels drawn over it as [`FloatingPixels::place`] does, but
    /// wrapped around the edges of `wrap` if there is one, as on a tile repeated side by
    /// side: those carried off one edge come back in at the other, and none land
    /// outside it.
    pub fn place_wrapped(
        &self,
        map: impl Into<Projective>,
        interpolation: Interpolation,
        wrap: Option<PixelRect>,
    ) -> RgbaImage {
        let map = &map.into();
        let mut layer = self.base.clone();
        let Some(area) = wrap.filter(|area| !area.is_empty()) else {
            let (width, height) = layer.dimensions();
            let whole = PixelRect { left: 0, top: 0, right: width as i32, bottom: height as i32 };
            self.draw_onto(&mut layer, map, interpolation, whole);
            return layer;
        };
        let (width, height) = (area.width() as f64, area.height() as f64);
        let Some((min_x, min_y, max_x, max_y)) = self.landing(map) else {
            return layer;
        };
        // The copies of the area they land on, counted from the area itself, with room
        // for the blended edges of any interpolation.
        let (min_x, min_y, max_x, max_y) = (min_x - 3.0, min_y - 3.0, max_x + 3.0, max_y + 3.0);
        let copies = |from: f64, to: f64, start: i32, size: f64| {
            ((from - start as f64) / size).floor() as i64..((to - start as f64) / size).ceil() as i64
        };
        for y in copies(min_y, max_y, area.top, height) {
            for x in copies(min_x, max_x, area.left, width) {
                let back_on = Affine::translation(-x as f64 * width, -y as f64 * height);
                self.draw_onto(&mut layer, &map.then(back_on.into()), interpolation, area);
            }
        }
        layer
    }

    /// The smallest rectangle around where `map` carries the area: left, top, right,
    /// bottom. Nothing if it tilts part of it behind the eye, or as far as it.
    fn landing(&self, map: &Projective) -> Option<(f64, f64, f64, f64)> {
        let PixelRect { left, top, right, bottom } = self.area;
        let corners = [(left, top), (right, top), (right, bottom), (left, bottom)].map(|(x, y)| (x as f64, y as f64));
        // Of one sign all over the area, as it changes evenly across it.
        let sides = corners.map(|corner| map.w(corner));
        if !(sides.iter().all(|&w| w > 1e-9) || sides.iter().all(|&w| w < -1e-9)) {
            return None;
        }
        let corners = corners.map(|corner| map.apply(corner));
        let min_x = corners.iter().map(|c| c.0).fold(f64::INFINITY, f64::min);
        let max_x = corners.iter().map(|c| c.0).fold(f64::NEG_INFINITY, f64::max);
        let min_y = corners.iter().map(|c| c.1).fold(f64::INFINITY, f64::min);
        let max_y = corners.iter().map(|c| c.1).fold(f64::NEG_INFINITY, f64::max);
        Some((min_x, min_y, max_x, max_y))
    }

    /// Draws the pixels over `layer` where `map` carries them, on its pixels in `within` alone.
    fn draw_onto(&self, layer: &mut RgbaImage, map: &Projective, interpolation: Interpolation, within: PixelRect) {
        let (Some(back), Some((min_x, min_y, max_x, max_y))) = (map.inverse(), self.landing(map)) else {
            return;
        };
        // Where the area lands, wider for the blended edges, within the layer.
        let margin = match interpolation {
            Interpolation::Nearest | Interpolation::Bilinear => 1.0,
            Interpolation::Bicubic => 2.0,
            Interpolation::Lanczos => 3.0,
        };
        let (from_x, to_x) = (
            (min_x.floor() - margin).max(within.left as f64) as u32,
            (max_x.ceil() + margin).min(within.right as f64).max(within.left as f64) as u32,
        );
        let (from_y, to_y) = (
            (min_y.floor() - margin).max(within.top as f64) as u32,
            (max_y.ceil() + margin).min(within.bottom as f64).max(within.top as f64) as u32,
        );
        let (left, top) = (self.area.left, self.area.top);

        let exact = Self::copies_exactly(map);
        for y in from_y..to_y {
            for x in from_x..to_x {
                let (sx, sy) = back.apply((x as f64 + 0.5, y as f64 + 0.5));
                // In pixels of the area, from the middle of its first one.
                let (sx, sy) = (sx - left as f64 - 0.5, sy - top as f64 - 0.5);
                let color = match interpolation {
                    _ if exact => self.nearest(sx, sy),
                    Interpolation::Nearest => self.nearest(sx, sy),
                    Interpolation::Bilinear => self.filtered(sx, sy, 1, triangle),
                    Interpolation::Bicubic => self.filtered(sx, sy, 2, catmull_rom),
                    Interpolation::Lanczos => self.filtered(sx, sy, 3, lanczos3),
                };
                if color[3] > 0.0 {
                    let below = layer.get_pixel_mut(x, y);
                    *below = blend_over(color, *below);
                }
            }
        }
    }

    /// Whether `map` only moves pixels by whole pixels, or mirrors them, which copies
    /// them as they are, however they would be resampled.
    pub fn copies_exactly(map: &Projective) -> bool {
        let Some(map) = map.affine() else {
            return false;
        };
        let unit = |value: f64| (value.abs() - 1.0).abs() < 1e-9;
        let none = |value: f64| value.abs() < 1e-9;
        unit(map.xx) && unit(map.yy) && none(map.xy) && none(map.yx) && is_whole(map.dx) && is_whole(map.dy)
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

    /// The pixels within `radius` of a place, weighed by `kernel` along each axis.
    /// The kernels that overshoot could leave a channel out of what it can hold, or more
    /// colour than there is alpha to carry it: those are clamped.
    fn filtered(&self, x: f64, y: f64, radius: usize, kernel: fn(f64) -> f64) -> [f32; 4] {
        let taps = |at: f64| {
            let first = (at - radius as f64).floor() as i64 + 1;
            let mut taps = [(0, 0.0); 2 * MAX_RADIUS];
            for (i, tap) in taps.iter_mut().take(2 * radius).enumerate() {
                let index = first + i as i64;
                *tap = (index, kernel(at - index as f64));
            }
            // The weights add up to one, which those of Lanczos only come close to.
            let total: f64 = taps.iter().map(|tap| tap.1).sum();
            if total.abs() > 1e-9 {
                for tap in &mut taps {
                    tap.1 /= total;
                }
            }
            taps
        };
        let (columns, rows) = (taps(x), taps(y));
        let mut color = [0.0_f32; 4];
        for &(row, row_weight) in rows.iter().filter(|tap| tap.1 != 0.0) {
            for &(column, column_weight) in columns.iter().filter(|tap| tap.1 != 0.0) {
                let weight = (row_weight * column_weight) as f32;
                let sample = self.premultiplied(column, row);
                for (channel, value) in color.iter_mut().zip(sample) {
                    *channel += value * weight;
                }
            }
        }
        let alpha = color[3].clamp(0.0, 1.0);
        [color[0].clamp(0.0, alpha), color[1].clamp(0.0, alpha), color[2].clamp(0.0, alpha), alpha]
    }
}

/// The widest reach of the kernels, in pixels on each side.
const MAX_RADIUS: usize = 3;

fn triangle(t: f64) -> f64 {
    (1.0 - t.abs()).max(0.0)
}

/// The bicubic kernel that goes through the pixels themselves.
fn catmull_rom(t: f64) -> f64 {
    let t = t.abs();
    if t < 1.0 {
        1.5 * t * t * t - 2.5 * t * t + 1.0
    } else if t < 2.0 {
        -0.5 * t * t * t + 2.5 * t * t - 4.0 * t + 2.0
    } else {
        0.0
    }
}

fn lanczos3(t: f64) -> f64 {
    let sinc = |x: f64| if x.abs() < 1e-9 { 1.0 } else { (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x) };
    if t.abs() < 3.0 { sinc(t) * sinc(t / 3.0) } else { 0.0 }
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
        let placed = pasted.place(Affine::IDENTITY, Interpolation::Bilinear);
        assert_eq!(*placed.get_pixel(7, 7), RED);
        // Moved away, the layer is as it was.
        assert_eq!(pasted.place(Affine::translation(-6.0, 0.0), Interpolation::Bilinear).get_pixel(7, 7), &BLUE);
        // Off the layer, nothing is placed.
        assert_eq!(pasted.place(Affine::translation(10.0, 10.0), Interpolation::Bilinear), layer());
    }

    #[test]
    fn nothing_to_lift_off_the_layer() {
        let outside = Selection::rect(PixelRect { left: 20, top: 20, right: 24, bottom: 24 });
        assert!(FloatingPixels::lift(&layer(), &outside).is_none());
    }

    #[test]
    fn moving_by_whole_pixels_copies_them() {
        let floating = FloatingPixels::lift(&layer(), &square()).unwrap();
        let placed = floating.place(Affine::translation(3.0, 1.0), Interpolation::Bilinear);
        assert_eq!(*placed.get_pixel(5, 3), RED);
        assert_eq!(*placed.get_pixel(6, 4), RED);
        assert_eq!(*placed.get_pixel(3, 2), CLEAR);
        assert_eq!(*placed.get_pixel(4, 2), BLUE);
        assert_eq!(*placed.get_pixel(0, 0), BLUE);
        // Nothing changes until placed.
        assert_eq!(floating.place(Affine::IDENTITY, Interpolation::Bilinear), layer());
    }

    #[test]
    fn doubling_fills_four_times_the_pixels() {
        let floating = FloatingPixels::lift(&layer(), &square()).unwrap();
        let double = Affine::translation(-2.0, -2.0).then(Affine::scale(2.0, 2.0)).then(Affine::translation(2.0, 2.0));
        for interpolation in Interpolation::ALL {
            let placed = floating.place(double, interpolation);
            // Inside, away from the blended edge, all red.
            assert_eq!(*placed.get_pixel(3, 3), RED, "{interpolation:?}");
            assert_eq!(*placed.get_pixel(4, 4), RED, "{interpolation:?}");
        }
        // Bicubic and Lanczos reach a little further around the edge than the others.
        assert_eq!(*floating.place(double, Interpolation::Bilinear).get_pixel(7, 7), BLUE);
    }

    #[test]
    fn nearest_keeps_edges_hard() {
        let floating = FloatingPixels::lift(&layer(), &square()).unwrap();
        // Half a pixel along, which the others blend.
        let half = Affine::translation(0.5, 0.0);
        let placed = floating.place(half, Interpolation::Nearest);
        assert!(placed.pixels().all(|&pixel| [RED, BLUE, CLEAR].contains(&pixel)));
        let blended = floating.place(half, Interpolation::Bilinear);
        assert!(blended.pixels().any(|&pixel| ![RED, BLUE, CLEAR].contains(&pixel)));
    }

    #[test]
    fn perspective_narrows_the_top() {
        let all = Selection::rect(PixelRect { left: 0, top: 0, right: 8, bottom: 8 });
        let floating = FloatingPixels::lift(&RgbaImage::from_pixel(8, 8, RED), &all).unwrap();
        let from = [(0.0, 0.0), (8.0, 0.0), (8.0, 8.0), (0.0, 8.0)];
        let map = Projective::between(from, [(2.0, 0.0), (6.0, 0.0), (8.0, 8.0), (0.0, 8.0)]).unwrap();
        assert!(!FloatingPixels::copies_exactly(&map));
        let placed = floating.place(map, Interpolation::Nearest);
        assert_eq!(*placed.get_pixel(0, 0), CLEAR);
        assert_eq!(*placed.get_pixel(4, 0), RED);
        assert_eq!(*placed.get_pixel(0, 7), RED);
        // Tilted behind the eye, nothing is put down.
        let behind = Projective { wx: -1.0, ..Projective::IDENTITY };
        assert!(floating.place(behind, Interpolation::Nearest).pixels().all(|&pixel| pixel == CLEAR));
    }

    #[test]
    fn blending_half_over() {
        assert_eq!(blend_over([0.5, 0.0, 0.0, 0.5], BLUE), Rgba([128, 0, 128, 255]));
        assert_eq!(blend_over([0.0; 4], BLUE), BLUE);
        assert_eq!(blend_over([0.5, 0.0, 0.0, 0.5], CLEAR), Rgba([255, 0, 0, 128]));
    }

    #[test]
    fn placed_wrapped_comes_back_in_at_the_other_edge() {
        let mut layer = RgbaImage::new(10, 4);
        layer.put_pixel(6, 1, Rgba([255, 0, 0, 255]));
        layer.put_pixel(7, 1, Rgba([0, 255, 0, 255]));
        let selection = Selection::rect(PixelRect { left: 6, top: 1, right: 8, bottom: 2 });
        let floating = FloatingPixels::lift(&layer, &selection).unwrap();
        // Two to the right, in an area from 2 to 8 across: the first lands on 8, past it,
        // so back on 2, and the second on 3.
        let area = PixelRect { left: 2, top: 0, right: 8, bottom: 4 };
        let placed = floating.place_wrapped(Affine::translation(2.0, 0.0), Interpolation::Nearest, Some(area));
        let shown: Vec<u32> = (0..10).filter(|&x| placed.get_pixel(x, 1)[3] > 0).collect();
        assert_eq!(shown, [2, 3]);
        assert_eq!(placed.get_pixel(2, 1), &Rgba([255, 0, 0, 255]));
    }
}

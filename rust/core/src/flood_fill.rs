//! The pixels of an image whose colour is close enough to that of one of them, the
//! seed, as the Paint Bucket fills them: those that reach it through others like it,
//! or all those in the image.

use image::{Rgba, RgbaImage};

use crate::{BlendMode, PixelRect, SelectionMask, blend::blend_pixel};

/// Leeway for rounding, well under the difference between two bytes.
const LEEWAY: f32 = 0.0005;

pub struct FloodFill {
    width: u32,
    height: u32,
    /// Whether each pixel was taken in, row by row.
    found: Vec<bool>,
    /// The pixels just outside those found, next to one of them, with how much of each
    /// a smooth edge covers: all of those just too far from the seed, and less and less
    /// of those further from it, down to none of those half as far again as the
    /// tolerance allows.
    fringe: Vec<((u32, u32), f32)>,
}

impl FloodFill {
    /// Finds the pixels of `sample` whose colour is within `tolerance`, from 0 for the
    /// same colour only to 1 for any, of the seed's: those that reach it through others
    /// like it with `contiguous`, or all of them. With a mask, only the selected pixels
    /// are taken in, and the seed must be one of them.
    pub fn new(
        sample: &RgbaImage,
        seed: (u32, u32),
        tolerance: f32,
        contiguous: bool,
        mask: Option<&SelectionMask>,
    ) -> Self {
        let (width, height) = sample.dimensions();
        let seed_color = *sample.get_pixel(seed.0, seed.1);
        let selected = |x: u32, y: u32| mask.is_none_or(|mask| mask.contains(x, y));
        let difference: Vec<f32> =
            sample.enumerate_pixels().map(|(_, _, &pixel)| color_difference(pixel, seed_color)).collect();
        let close: Vec<bool> = (0..height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .zip(&difference)
            .map(|((x, y), &difference)| difference <= tolerance + LEEWAY && selected(x, y))
            .collect();
        let found = if contiguous { reach(&close, width, height, seed) } else { close };

        let mut fringe = Vec::new();
        if tolerance > 0.0 {
            let index = |x: u32, y: u32| (y * width + x) as usize;
            for y in 0..height {
                for x in 0..width {
                    if found[index(x, y)] || !selected(x, y) {
                        continue;
                    }
                    let neighbours = [
                        (x > 0).then(|| index(x - 1, y)),
                        (x + 1 < width).then(|| index(x + 1, y)),
                        (y > 0).then(|| index(x, y - 1)),
                        (y + 1 < height).then(|| index(x, y + 1)),
                    ];
                    if !neighbours.into_iter().flatten().any(|neighbour| found[neighbour]) {
                        continue;
                    }
                    let coverage = ((1.5 * tolerance - difference[index(x, y)]) / (0.5 * tolerance)).clamp(0.0, 1.0);
                    if coverage > 0.0 {
                        fringe.push(((x, y), coverage));
                    }
                }
            }
        }
        Self { width, height, found, fringe }
    }

    /// Fills the pixels found of `layer`, as large as the image looked at, with `color`,
    /// blended over them if it is see-through, and with `smooth` the fringe around them
    /// as much as it is covered. Returns the area that changed, if any did.
    pub fn paint(&self, layer: &mut RgbaImage, color: Rgba<u8>, smooth: bool) -> Option<PixelRect> {
        let mut area: Option<PixelRect> = None;
        let mut paint = |x: u32, y: u32, coverage: f32| {
            let pixel = layer.get_pixel_mut(x, y);
            let before = *pixel;
            blend_pixel(&mut pixel.0, &color.0, coverage, BlendMode::Normal);
            if *pixel == before {
                return;
            }
            let (x, y) = (x as i32, y as i32);
            area = Some(match area {
                None => PixelRect { left: x, top: y, right: x + 1, bottom: y + 1 },
                Some(area) => PixelRect {
                    left: area.left.min(x),
                    top: area.top.min(y),
                    right: area.right.max(x + 1),
                    bottom: area.bottom.max(y + 1),
                },
            });
        };
        for y in 0..self.height {
            for x in 0..self.width {
                if self.found[(y * self.width + x) as usize] {
                    paint(x, y, 1.0);
                }
            }
        }
        if smooth {
            for &((x, y), coverage) in &self.fringe {
                paint(x, y, coverage);
            }
        }
        area
    }
}

/// How far apart two colours are, weighted by how opaque they are so all see-through
/// pixels are alike: from 0 for the same to 1 for opaque white and anything see-through.
fn color_difference(pixel: Rgba<u8>, seed: Rgba<u8>) -> f32 {
    let weighted = |color: Rgba<u8>| {
        let alpha = color[3] as f32 / 255.0;
        [color[0], color[1], color[2]].map(|channel| channel as f32 / 255.0 * alpha).into_iter().chain([alpha])
    };
    weighted(pixel).zip(weighted(seed)).map(|(a, b)| (a - b) * (a - b)).sum::<f32>().sqrt() / 2.0
}

/// The close pixels the seed reaches through others, going a row of them at a time.
/// Nothing if the seed is not close itself.
fn reach(close: &[bool], width: u32, height: u32, seed: (u32, u32)) -> Vec<bool> {
    let mut found = vec![false; close.len()];
    let index = |x: u32, y: u32| (y * width + x) as usize;
    let mut pending = vec![seed];
    while let Some((x, y)) = pending.pop() {
        if !close[index(x, y)] || found[index(x, y)] {
            continue;
        }
        // The whole row of close pixels it is in.
        let mut left = x;
        while left > 0 && close[index(left - 1, y)] && !found[index(left - 1, y)] {
            left -= 1;
        }
        let mut right = x + 1;
        while right < width && close[index(right, y)] && !found[index(right, y)] {
            right += 1;
        }
        found[index(left, y)..index(right, y)].fill(true);
        // The rows above and below that touch it, from where each starts.
        for next_y in [y.checked_sub(1), Some(y + 1).filter(|&next| next < height)].into_iter().flatten() {
            let mut in_row = false;
            for next_x in left..right {
                let open = close[index(next_x, next_y)] && !found[index(next_x, next_y)];
                if open && !in_row {
                    pending.push((next_x, next_y));
                }
                in_row = open;
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Selection;

    const RED: Rgba<u8> = Rgba([255, 0, 0, 255]);
    const BLUE: Rgba<u8> = Rgba([0, 0, 255, 255]);
    const WHITE: Rgba<u8> = Rgba([255, 255, 255, 255]);

    /// White, cut in two by a blue column at x = 2, with a pale blue pixel at 0, 0.
    fn image() -> RgbaImage {
        RgbaImage::from_fn(5, 3, |x, y| match (x, y) {
            (2, _) => BLUE,
            (0, 0) => Rgba([230, 230, 255, 255]),
            _ => WHITE,
        })
    }

    fn filled(fill: &FloodFill, smooth: bool) -> RgbaImage {
        let mut layer = image();
        fill.paint(&mut layer, RED, smooth);
        layer
    }

    #[test]
    fn contiguous_stays_on_its_side() {
        let layer = filled(&FloodFill::new(&image(), (1, 1), 0.0, true, None), false);
        assert_eq!(layer[(1, 1)], RED);
        assert_eq!(layer[(0, 2)], RED);
        assert_eq!(layer[(0, 0)], Rgba([230, 230, 255, 255]), "not quite the same colour");
        assert_eq!(layer[(2, 1)], BLUE);
        assert_eq!(layer[(3, 1)], WHITE, "the other side");
        let layer = filled(&FloodFill::new(&image(), (1, 1), 0.0, false, None), false);
        assert_eq!(layer[(3, 1)], RED);
        assert_eq!(layer[(2, 1)], BLUE);
    }

    #[test]
    fn tolerance_takes_in_close_colours_and_smooths_the_edge() {
        let fill = FloodFill::new(&image(), (1, 1), 0.1, true, None);
        let layer = filled(&fill, false);
        assert_eq!(layer[(0, 0)], RED);
        assert_eq!(layer[(2, 1)], BLUE);
        // Blue is too far off to be covered at all.
        assert_eq!(filled(&fill, true)[(2, 1)], BLUE);
        let fill = FloodFill::new(&image(), (1, 1), 0.5, true, None);
        let edge = filled(&fill, true)[(2, 1)];
        assert!(edge != BLUE && edge != RED, "partly covered: {edge:?}");
    }

    #[test]
    fn only_fills_the_selection() {
        let selection = Selection::rect(PixelRect { left: 0, top: 1, right: 2, bottom: 3 });
        let mask = SelectionMask::of(&selection, 5, 3).unwrap();
        let fill = FloodFill::new(&image(), (1, 1), 0.0, true, Some(&mask));
        let mut layer = image();
        let area = fill.paint(&mut layer, RED, true);
        assert_eq!(area, Some(PixelRect { left: 0, top: 1, right: 2, bottom: 3 }));
        assert_eq!(layer[(1, 0)], WHITE);
    }

    #[test]
    fn see_through_colours_are_alike() {
        let clear = Rgba([0, 0, 0, 0]);
        assert_eq!(color_difference(Rgba([255, 0, 0, 0]), clear), 0.0);
        assert!((color_difference(WHITE, clear) - 1.0).abs() < 1e-6);
    }
}

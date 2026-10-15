//! Lines drawn with the Pencil: hard-edged, and of a colour that replaces what was
//! under it rather than blending in.

use image::{Rgba, RgbaImage};

use crate::{PixelRect, SelectionMask};

/// Widest the Pencil draws, in pixels.
pub const MAX_PENCIL_SIZE: u32 = 64;

/// A line being drawn, from the pixel it was pressed on to wherever it is taken.
pub struct PencilStroke {
    color: Rgba<u8>,
    /// How far the tip reaches up and left of the pixel it is on.
    reach: i64,
    /// The pixels the tip covers, as rows from the top left of the square it fits in:
    /// where each starts and ends, past its last pixel.
    tip: Vec<(i64, i64)>,
    /// The pixels it may change, if not all of them.
    mask: Option<SelectionMask>,
    /// The pixel it last reached.
    last: (i64, i64),
    changed: bool,
}

impl PencilStroke {
    /// Starts a line `size` pixels wide on the pixel at `at`, and draws that pixel,
    /// changing only those `mask` holds if there is one. Returns the area it changed,
    /// if any of it is on the image.
    pub fn begin(
        image: &mut RgbaImage,
        at: (i64, i64),
        color: Rgba<u8>,
        size: u32,
        mask: Option<SelectionMask>,
    ) -> (Self, Option<PixelRect>) {
        let size = size.clamp(1, MAX_PENCIL_SIZE);
        let mut stroke = Self { color, reach: (size / 2) as i64, tip: tip(size), mask, last: at, changed: false };
        let area = stroke.draw_to(image, at);
        (stroke, area)
    }

    /// Draws on from the pixel the line last reached to the one at `at`. Returns the
    /// area it changed, if any of it is on the image.
    pub fn draw_to(&mut self, image: &mut RgbaImage, at: (i64, i64)) -> Option<PixelRect> {
        let from = std::mem::replace(&mut self.last, at);
        let (dx, dy) = (at.0 - from.0, at.1 - from.1);
        let count = dx.abs().max(dy.abs());
        for i in 0..=count {
            let along = |delta: i64| (delta as f64 * i as f64 / count.max(1) as f64).round() as i64;
            self.stamp(image, (from.0 + along(dx), from.1 + along(dy)));
        }
        // The square the tip fits in, at either end.
        let size = self.tip.len() as i64;
        let (left, top) = (from.0.min(at.0) - self.reach, from.1.min(at.1) - self.reach);
        let (right, bottom) = (from.0.max(at.0) - self.reach + size, from.1.max(at.1) - self.reach + size);
        let area = PixelRect {
            left: left.max(0) as i32,
            top: top.max(0) as i32,
            right: right.min(image.width() as i64) as i32,
            bottom: bottom.min(image.height() as i64) as i32,
        };
        (!area.is_empty()).then_some(area)
    }

    /// Whether it changed any pixel yet.
    pub fn changed(&self) -> bool {
        self.changed
    }

    /// Puts the tip down on the pixel at `at`.
    fn stamp(&mut self, image: &mut RgbaImage, at: (i64, i64)) {
        let (width, height) = (image.width() as i64, image.height() as i64);
        let (left, top) = (at.0 - self.reach, at.1 - self.reach);
        for (row, &(from, to)) in self.tip.iter().enumerate() {
            let y = top + row as i64;
            if !(0..height).contains(&y) {
                continue;
            }
            for x in (left + from).max(0)..(left + to).min(width) {
                let (x, y) = (x as u32, y as u32);
                if self.mask.as_ref().is_some_and(|mask| !mask.contains(x, y)) {
                    continue;
                }
                let pixel = image.get_pixel_mut(x, y);
                if *pixel != self.color {
                    *pixel = self.color;
                    self.changed = true;
                }
            }
        }
    }
}

/// The pixels a Pencil `size` pixels wide covers on the pixel at `at`, on or off the
/// image, as rows from the top: each one's height, and where it starts and ends, past
/// its last pixel.
pub fn pencil_tip(at: (i64, i64), size: u32) -> impl Iterator<Item = (i64, i64, i64)> {
    let size = size.clamp(1, MAX_PENCIL_SIZE);
    let reach = (size / 2) as i64;
    let (left, top) = (at.0 - reach, at.1 - reach);
    tip(size).into_iter().enumerate().map(move |(row, (from, to))| (top + row as i64, left + from, left + to))
}

/// The pixels a round tip `size` pixels wide covers, as rows from the top left of the
/// square it fits in.
fn tip(size: u32) -> Vec<(i64, i64)> {
    let middle = (size as f64 - 1.0) / 2.0;
    // A little short of the true radius, which keeps small tips from looking square.
    let reach = (size as f64 * size as f64 / 4.0 - 0.5).max(0.0);
    (0..size as i64)
        .map(|y| {
            let dy = y as f64 - middle;
            let inside = |x: &i64| (*x as f64 - middle).powi(2) + dy * dy <= reach;
            let from = (0..size as i64).find(inside).unwrap_or(0);
            let to = (0..size as i64).rev().find(inside).map_or(from, |last| last + 1);
            (from, to)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Selection;

    const BLACK: Rgba<u8> = Rgba([0, 0, 0, 255]);
    const CLEAR: Rgba<u8> = Rgba([0, 0, 0, 0]);

    fn drawn(image: &RgbaImage) -> Vec<(u32, u32)> {
        image.enumerate_pixels().filter(|(.., pixel)| **pixel == BLACK).map(|(x, y, _)| (x, y)).collect()
    }

    #[test]
    fn small_tips_are_round() {
        assert_eq!(tip(1), [(0, 1)]);
        assert_eq!(tip(2), [(0, 2), (0, 2)]);
        assert_eq!(tip(3), [(1, 2), (0, 3), (1, 2)]);
        assert_eq!(tip(4), [(1, 3), (0, 4), (0, 4), (1, 3)]);
    }

    #[test]
    fn the_tip_is_around_the_pixel() {
        assert_eq!(pencil_tip((5, 5), 1).collect::<Vec<_>>(), [(5, 5, 6)]);
        assert_eq!(pencil_tip((5, 5), 3).collect::<Vec<_>>(), [(4, 5, 6), (5, 4, 7), (6, 5, 6)]);
        assert_eq!(pencil_tip((5, 5), 2).collect::<Vec<_>>(), [(4, 4, 6), (5, 4, 6)]);
    }

    #[test]
    fn a_press_draws_the_pixel() {
        let mut image = RgbaImage::new(4, 4);
        let (stroke, area) = PencilStroke::begin(&mut image, (2, 1), BLACK, 1, None);
        assert_eq!(drawn(&image), [(2, 1)]);
        assert_eq!(area, Some(PixelRect { left: 2, top: 1, right: 3, bottom: 2 }));
        assert!(stroke.changed());
    }

    #[test]
    fn lines_leave_no_gaps() {
        let mut image = RgbaImage::new(8, 8);
        let (mut stroke, _) = PencilStroke::begin(&mut image, (0, 0), BLACK, 1, None);
        let area = stroke.draw_to(&mut image, (6, 3));
        assert_eq!(area, Some(PixelRect { left: 0, top: 0, right: 7, bottom: 4 }));
        let pixels = drawn(&image);
        assert_eq!(pixels.len(), 7);
        for x in 0..7 {
            assert_eq!(pixels.iter().filter(|(drawn_x, _)| *drawn_x == x).count(), 1, "column {x}");
        }
    }

    #[test]
    fn replaces_the_colour_under_it() {
        let mut image = RgbaImage::from_pixel(2, 1, BLACK);
        let (stroke, _) = PencilStroke::begin(&mut image, (0, 0), CLEAR, 1, None);
        assert_eq!(image[(0, 0)], CLEAR);
        assert!(stroke.changed());
        let (stroke, _) = PencilStroke::begin(&mut image, (1, 0), BLACK, 1, None);
        assert!(!stroke.changed());
    }

    #[test]
    fn only_draws_on_selected_pixels() {
        let mut image = RgbaImage::new(6, 6);
        let selection = Selection::rect(PixelRect { left: 2, top: 0, right: 4, bottom: 6 });
        let mask = SelectionMask::of(&selection, 6, 6);
        let (mut stroke, _) = PencilStroke::begin(&mut image, (0, 2), BLACK, 1, mask);
        assert!(!stroke.changed());
        stroke.draw_to(&mut image, (5, 2));
        assert_eq!(drawn(&image), [(2, 2), (3, 2)]);
    }

    #[test]
    fn stops_at_the_edges() {
        let mut image = RgbaImage::new(4, 4);
        let (mut stroke, area) = PencilStroke::begin(&mut image, (-5, -5), BLACK, 3, None);
        assert_eq!((area, stroke.changed()), (None, false));
        let area = stroke.draw_to(&mut image, (0, 0));
        assert_eq!(area, Some(PixelRect { left: 0, top: 0, right: 2, bottom: 2 }));
        assert_eq!(drawn(&image), [(0, 0), (1, 0), (0, 1)]);
    }
}

//! Lines drawn with the Pencil, the Paintbrush and the Eraser. The Pencil's are
//! hard-edged, and of a colour that replaces what was under it. The Paintbrush blends
//! its colour in, and the Eraser rubs out what is under it, both with smooth edges if
//! asked to. However often a line goes over a pixel, it changes it no more than the
//! once it went over it most.

use image::{Rgba, RgbaImage};

use crate::{BlendMode, PixelRect, SelectionMask, blend::blend_pixel};

/// Widest the drawing tools draw, in pixels.
pub const MAX_TIP_SIZE: u32 = 64;

const CLEAR: Rgba<u8> = Rgba([0, 0, 0, 0]);

/// What a line leaves on the pixels it goes over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    /// A colour that replaces what was there, as the Pencil's does.
    Replace(Rgba<u8>),
    /// A colour blended over what was there, as the Paintbrush's is.
    Paint(Rgba<u8>),
    /// Less of what was there, as the Eraser leaves.
    Erase,
}

/// A line being drawn, from the point it was pressed on to wherever it is taken.
pub struct BrushStroke {
    ink: Ink,
    /// Whether its edges fade out over a pixel rather than stop at one.
    smooth: bool,
    size: u32,
    /// How far the hard tip reaches up and left of the pixel it is on.
    reach: i64,
    /// The pixels the hard tip covers, as rows from the top left of the square it fits
    /// in: where each starts and ends, past its last pixel.
    tip: Vec<(i64, i64)>,
    /// The pixels as they were before the line, which it is drawn over.
    before: RgbaImage,
    /// How much of each pixel the line covered so far, out of 255.
    coverage: Vec<u8>,
    /// The pixels it may change, if not all of them.
    mask: Option<SelectionMask>,
    /// The point it last reached, on the image, in pixels.
    last: (f64, f64),
    changed: bool,
}

impl BrushStroke {
    /// Starts a line `size` pixels wide at the point `at` of the image, and draws
    /// there, changing only the pixels `mask` holds if there is one. A line that
    /// replaces the colour is never smooth. Returns the area it changed, if any of it
    /// is on the image.
    pub fn begin(
        image: &mut RgbaImage,
        at: (f64, f64),
        ink: Ink,
        size: u32,
        smooth: bool,
        mask: Option<SelectionMask>,
    ) -> (Self, Option<PixelRect>) {
        let size = size.clamp(1, MAX_TIP_SIZE);
        let mut stroke = Self {
            ink,
            smooth: smooth && !matches!(ink, Ink::Replace(_)),
            size,
            reach: (size / 2) as i64,
            tip: tip(size),
            before: image.clone(),
            coverage: vec![0; image.width() as usize * image.height() as usize],
            mask,
            last: at,
            changed: false,
        };
        let area = stroke.draw_to(image, at);
        (stroke, area)
    }

    /// Draws on from the point the line last reached to `at`. Returns the area it
    /// changed, if any of it is on the image.
    pub fn draw_to(&mut self, image: &mut RgbaImage, at: (f64, f64)) -> Option<PixelRect> {
        let from = std::mem::replace(&mut self.last, at);
        let area = if self.smooth { self.draw_smooth(image, from, at) } else { self.draw_hard(image, from, at) };
        let area = PixelRect {
            left: area.left.max(0),
            top: area.top.max(0),
            right: area.right.min(image.width() as i32),
            bottom: area.bottom.min(image.height() as i32),
        };
        (!area.is_empty()).then_some(area)
    }

    /// Whether it changed any pixel yet.
    pub fn changed(&self) -> bool {
        self.changed
    }

    /// Puts the tip down on each pixel from the one under `from` to the one under `to`.
    /// Returns the area it went over, on or off the image.
    fn draw_hard(&mut self, image: &mut RgbaImage, from: (f64, f64), to: (f64, f64)) -> PixelRect {
        let (from, to) = (pixel_of(from), pixel_of(to));
        let (dx, dy) = (to.0 - from.0, to.1 - from.1);
        let count = dx.abs().max(dy.abs());
        for i in 0..=count {
            let along = |delta: i64| (delta as f64 * i as f64 / count.max(1) as f64).round() as i64;
            self.stamp(image, (from.0 + along(dx), from.1 + along(dy)));
        }
        // The square the tip fits in, at either end.
        let size = self.tip.len() as i64;
        let (left, top) = (from.0.min(to.0) - self.reach, from.1.min(to.1) - self.reach);
        let (right, bottom) = (from.0.max(to.0) - self.reach + size, from.1.max(to.1) - self.reach + size);
        PixelRect { left: left as i32, top: top as i32, right: right as i32, bottom: bottom as i32 }
    }

    /// Puts the hard tip down on the pixel at `at`.
    fn stamp(&mut self, image: &mut RgbaImage, at: (i64, i64)) {
        let (width, height) = (image.width() as i64, image.height() as i64);
        let (left, top) = (at.0 - self.reach, at.1 - self.reach);
        for row in 0..self.tip.len() {
            let (from, to) = self.tip[row];
            let y = top + row as i64;
            if !(0..height).contains(&y) {
                continue;
            }
            for x in (left + from).max(0)..(left + to).min(width) {
                self.cover(image, x as u32, y as u32, u8::MAX);
            }
        }
    }

    /// Covers the pixels within half the size of the segment from `from` to `to`, the
    /// edge fading out over one pixel, half of it on either side of the outline.
    /// Returns the area it went over, on or off the image.
    fn draw_smooth(&mut self, image: &mut RgbaImage, from: (f64, f64), to: (f64, f64)) -> PixelRect {
        let outer = self.size as f64 / 2.0 + 0.5;
        let area = PixelRect {
            left: (from.0.min(to.0) - outer).floor() as i32,
            top: (from.1.min(to.1) - outer).floor() as i32,
            right: (from.0.max(to.0) + outer).ceil() as i32,
            bottom: (from.1.max(to.1) + outer).ceil() as i32,
        };
        for y in area.top.max(0)..area.bottom.min(image.height() as i32) {
            for x in area.left.max(0)..area.right.min(image.width() as i32) {
                let middle = (x as f64 + 0.5, y as f64 + 0.5);
                let coverage = (outer - distance_to_segment(middle, from, to)).min(1.0);
                let coverage = (coverage * 255.0).round();
                if coverage > 0.0 {
                    self.cover(image, x as u32, y as u32, coverage as u8);
                }
            }
        }
        area
    }

    /// Covers the pixel at `x`, `y` of the image by `coverage` out of 255, unless the
    /// line already covered it as much, or it is not one the line may change.
    fn cover(&mut self, image: &mut RgbaImage, x: u32, y: u32, coverage: u8) {
        if self.mask.as_ref().is_some_and(|mask| !mask.contains(x, y)) {
            return;
        }
        let covered = &mut self.coverage[y as usize * image.width() as usize + x as usize];
        if coverage <= *covered {
            return;
        }
        *covered = coverage;
        let before = *self.before.get_pixel(x, y);
        let after = match self.ink {
            Ink::Replace(color) => color,
            Ink::Paint(color) => {
                let mut pixel = before.0;
                blend_pixel(&mut pixel, &color.0, coverage as f32 / 255.0, BlendMode::Normal);
                Rgba(pixel)
            }
            Ink::Erase => {
                let alpha = (before[3] as u32 * (255 - coverage as u32) + 127) / 255;
                if alpha == 0 { CLEAR } else { Rgba([before[0], before[1], before[2], alpha as u8]) }
            }
        };
        let pixel = image.get_pixel_mut(x, y);
        if *pixel != after {
            *pixel = after;
            self.changed = true;
        }
    }
}

/// The pixels a hard tip `size` pixels wide covers on the pixel at `at`, on or off the
/// image, as rows from the top: each one's height, and where it starts and ends, past
/// its last pixel.
pub fn hard_tip(at: (i64, i64), size: u32) -> impl Iterator<Item = (i64, i64, i64)> {
    let size = size.clamp(1, MAX_TIP_SIZE);
    let reach = (size / 2) as i64;
    let (left, top) = (at.0 - reach, at.1 - reach);
    tip(size).into_iter().enumerate().map(move |(row, (from, to))| (top + row as i64, left + from, left + to))
}

/// The pixel a point of the image is on.
pub(crate) fn pixel_of(point: (f64, f64)) -> (i64, i64) {
    (point.0.floor() as i64, point.1.floor() as i64)
}

/// How far `point` is from the nearest point of the segment from `from` to `to`.
pub(crate) fn distance_to_segment(point: (f64, f64), from: (f64, f64), to: (f64, f64)) -> f64 {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let length_squared = dx * dx + dy * dy;
    let along = if length_squared > 0.0 {
        (((point.0 - from.0) * dx + (point.1 - from.1) * dy) / length_squared).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let nearest = (from.0 + along * dx, from.1 + along * dy);
    (point.0 - nearest.0).hypot(point.1 - nearest.1)
}

/// The pixels a round tip `size` pixels wide covers, as rows from the top left of the
/// square it fits in.
pub(crate) fn tip(size: u32) -> Vec<(i64, i64)> {
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
    const PENCIL: Ink = Ink::Replace(BLACK);

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
        assert_eq!(hard_tip((5, 5), 1).collect::<Vec<_>>(), [(5, 5, 6)]);
        assert_eq!(hard_tip((5, 5), 3).collect::<Vec<_>>(), [(4, 5, 6), (5, 4, 7), (6, 5, 6)]);
        assert_eq!(hard_tip((5, 5), 2).collect::<Vec<_>>(), [(4, 4, 6), (5, 4, 6)]);
    }

    #[test]
    fn a_press_draws_the_pixel() {
        let mut image = RgbaImage::new(4, 4);
        let (stroke, area) = BrushStroke::begin(&mut image, (2.7, 1.2), PENCIL, 1, false, None);
        assert_eq!(drawn(&image), [(2, 1)]);
        assert_eq!(area, Some(PixelRect { left: 2, top: 1, right: 3, bottom: 2 }));
        assert!(stroke.changed());
    }

    #[test]
    fn lines_leave_no_gaps() {
        let mut image = RgbaImage::new(8, 8);
        let (mut stroke, _) = BrushStroke::begin(&mut image, (0.0, 0.0), PENCIL, 1, false, None);
        let area = stroke.draw_to(&mut image, (6.0, 3.0));
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
        let (stroke, _) = BrushStroke::begin(&mut image, (0.0, 0.0), Ink::Replace(CLEAR), 1, true, None);
        assert_eq!(image[(0, 0)], CLEAR);
        assert!(stroke.changed());
        let (stroke, _) = BrushStroke::begin(&mut image, (1.0, 0.0), PENCIL, 1, false, None);
        assert!(!stroke.changed());
    }

    #[test]
    fn only_draws_on_selected_pixels() {
        let mut image = RgbaImage::new(6, 6);
        let selection = Selection::rect(PixelRect { left: 2, top: 0, right: 4, bottom: 6 });
        let mask = SelectionMask::of(&selection, 6, 6);
        let (mut stroke, _) = BrushStroke::begin(&mut image, (0.0, 2.0), PENCIL, 1, false, mask);
        assert!(!stroke.changed());
        stroke.draw_to(&mut image, (5.0, 2.0));
        assert_eq!(drawn(&image), [(2, 2), (3, 2)]);
    }

    #[test]
    fn stops_at_the_edges() {
        let mut image = RgbaImage::new(4, 4);
        let (mut stroke, area) = BrushStroke::begin(&mut image, (-5.0, -5.0), PENCIL, 3, false, None);
        assert_eq!((area, stroke.changed()), (None, false));
        let area = stroke.draw_to(&mut image, (0.0, 0.0));
        assert_eq!(area, Some(PixelRect { left: 0, top: 0, right: 2, bottom: 2 }));
        assert_eq!(drawn(&image), [(0, 0), (1, 0), (0, 1)]);
    }

    #[test]
    fn paint_blends_in_without_building_up() {
        let white = Rgba([255, 255, 255, 255]);
        let mut image = RgbaImage::from_pixel(3, 1, white);
        let half_black = Ink::Paint(Rgba([0, 0, 0, 128]));
        let (mut stroke, _) = BrushStroke::begin(&mut image, (0.0, 0.0), half_black, 1, false, None);
        stroke.draw_to(&mut image, (2.0, 0.0));
        stroke.draw_to(&mut image, (0.0, 0.0));
        assert_eq!(image[(0, 0)], Rgba([127, 127, 127, 255]));
        assert_eq!(image[(0, 0)], image[(2, 0)]);
    }

    #[test]
    fn smooth_edges_cover_pixels_in_part() {
        let mut image = RgbaImage::new(4, 1);
        // On the line between two pixels, a tip one pixel wide covers half of each.
        let (stroke, area) = BrushStroke::begin(&mut image, (2.0, 0.5), Ink::Paint(BLACK), 1, true, None);
        assert!(stroke.changed());
        assert_eq!(area, Some(PixelRect { left: 1, top: 0, right: 3, bottom: 1 }));
        assert_eq!([image[(1, 0)][3], image[(2, 0)][3]], [128, 128]);
        assert_eq!([image[(0, 0)][3], image[(3, 0)][3]], [0, 0]);
        // On the middle of a pixel, it covers that one whole.
        let mut image = RgbaImage::new(3, 1);
        BrushStroke::begin(&mut image, (1.5, 0.5), Ink::Paint(BLACK), 1, true, None);
        assert_eq!(drawn(&image), [(1, 0)]);
        assert_eq!([image[(0, 0)][3], image[(2, 0)][3]], [0, 0]);
    }

    #[test]
    fn erasing_clears_what_it_covers() {
        let mut image = RgbaImage::from_pixel(4, 1, Rgba([10, 20, 30, 255]));
        let (mut stroke, _) = BrushStroke::begin(&mut image, (0.0, 0.0), Ink::Erase, 1, false, None);
        stroke.draw_to(&mut image, (1.0, 0.0));
        assert_eq!([image[(0, 0)], image[(1, 0)]], [CLEAR, CLEAR]);
        assert_eq!(image[(2, 0)], Rgba([10, 20, 30, 255]));
        let mut image = RgbaImage::from_pixel(2, 1, Rgba([10, 20, 30, 200]));
        BrushStroke::begin(&mut image, (1.0, 0.5), Ink::Erase, 1, true, None);
        assert_eq!(image[(0, 0)], Rgba([10, 20, 30, 100]));
    }
}

//! Strokes of the Blur, the Sharpen and the Jumble tools, which change the pixels they
//! go over without leaving a colour of their own. Each time the stroke moves, it goes
//! over the pixels whose middle is within its tip along the way, each worked out from
//! the pixels as they were before, never from those just changed, so going over the
//! same place again does it more. The Blur mixes each pixel with the average of the
//! 3 × 3 pixels around it, each counting for as much as it is opaque. The Sharpen takes
//! each colour further away from that average, and leaves how opaque it is as it was.
//! The Jumble swaps some for a pixel next to where the tip was a moment before, the
//! faster it goes the further back, which drags them along the stroke: it only moves
//! colours around, and never makes new ones. A stroke may wrap around the image's
//! edges, as on a tile repeated side by side: then the pixels by one edge are worked
//! out from those by the other too.

use image::{Rgba, RgbaImage};

use crate::{
    PixelRect, SelectionMask,
    spray::Random,
    stroke::{MAX_TIP_SIZE, distance_to_segment},
    wrap,
};

const CLEAR: Rgba<u8> = Rgba([0, 0, 0, 0]);

/// What a retouch stroke does to the pixels it goes over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Retouch {
    Blur,
    Sharpen,
    Jumble,
}

/// A retouch stroke, from the point it was pressed on to wherever it is taken.
pub struct RetouchStroke {
    retouch: Retouch,
    /// Half the width of what it goes over, in pixels.
    radius: f64,
    /// How much each pass does, from 0 to 1.
    strength: f64,
    /// The pixels it may change, if not all of them.
    mask: Option<SelectionMask>,
    /// The area it wraps around the edges of, if it does.
    wrap: Option<PixelRect>,
    /// The point it last reached, on the image, in pixels.
    last: (f64, f64),
    random: Random,
    changed: bool,
}

impl RetouchStroke {
    /// Starts a stroke `size` pixels wide at the point `at` of the image, at `strength`
    /// percent, and goes over the pixels there, changing only those `mask` holds if
    /// there is one. `seed` picks which pixels the Jumble swaps. It wraps around the
    /// edges if `wrap` is set. Returns the area it changed, if any of it is on the image.
    #[allow(clippy::too_many_arguments)]
    pub fn begin(
        image: &mut RgbaImage,
        at: (f64, f64),
        retouch: Retouch,
        size: u32,
        strength: u32,
        mask: Option<SelectionMask>,
        seed: u64,
        wrap: Option<PixelRect>,
    ) -> (Self, Option<PixelRect>) {
        let mut stroke = Self {
            retouch,
            radius: size.clamp(1, MAX_TIP_SIZE) as f64 / 2.0,
            strength: strength.clamp(1, 100) as f64 / 100.0,
            mask,
            wrap,
            last: at,
            random: Random::new(seed),
            changed: false,
        };
        let area = stroke.draw_to(image, at);
        (stroke, area)
    }

    /// Goes over the pixels from the point the stroke last reached to `at`. Returns
    /// the area it changed, if any of it is on the image.
    pub fn draw_to(&mut self, image: &mut RgbaImage, at: (f64, f64)) -> Option<PixelRect> {
        let last = std::mem::replace(&mut self.last, at);
        let (from, to) = if let Some(area) = self.wrap { wrap::segment(area, last, at) } else { (last, at) };
        let (width, height) = (image.width() as i32, image.height() as i32);
        let grow = self.radius + 1.0;
        let reached = PixelRect {
            left: (from.0.min(to.0) - grow).floor() as i32,
            top: (from.1.min(to.1) - grow).floor() as i32,
            right: (from.0.max(to.0) + grow).ceil() as i32,
            bottom: (from.1.max(to.1) + grow).ceil() as i32,
        };
        let area = if let Some(wrap) = self.wrap {
            // No wider or taller than the area, for no pixel to be gone over twice.
            PixelRect {
                right: reached.right.min(reached.left + wrap.width()),
                bottom: reached.bottom.min(reached.top + wrap.height()),
                ..reached
            }
        } else {
            PixelRect {
                left: reached.left.max(0),
                top: reached.top.max(0),
                right: reached.right.min(width),
                bottom: reached.bottom.min(height),
            }
        };
        if area.is_empty() {
            return None;
        }
        // Where the Jumble picks pixels from: half the way back to where it last was,
        // no further than its tip reaches.
        let (mut back_x, mut back_y) = ((from.0 - to.0) / 2.0, (from.1 - to.1) / 2.0);
        let length = back_x.hypot(back_y);
        if length > self.radius {
            (back_x, back_y) = (back_x * self.radius / length, back_y * self.radius / length);
        }
        let behind = (back_x.round() as i32, back_y.round() as i32);
        // The pixels looked at, which go as far as the pixels any of them is worked out from.
        let reach = 1 + if self.retouch == Retouch::Jumble { behind.0.abs().max(behind.1.abs()) } else { 0 };
        let grown = PixelRect {
            left: area.left - reach,
            top: area.top - reach,
            right: area.right + reach,
            bottom: area.bottom + reach,
        };
        let (read, before) = if let Some(wrap) = self.wrap {
            let before = RgbaImage::from_fn(grown.width() as u32, grown.height() as u32, |x, y| {
                let (x, y) = wrap::pixel(wrap, (grown.left + x as i32) as i64, (grown.top + y as i32) as i64);
                *image.get_pixel(x, y)
            });
            (grown, before)
        } else {
            let read = PixelRect {
                left: grown.left.max(0),
                top: grown.top.max(0),
                right: grown.right.min(width),
                bottom: grown.bottom.min(height),
            };
            let (left, top) = (read.left as u32, read.top as u32);
            let before = image::imageops::crop_imm(image, left, top, read.width() as u32, read.height() as u32);
            (read, before.to_image())
        };
        // A pixel looked at, kept within those read, as the image's edges are.
        let around = |x: i32, y: i32| {
            let x = (x - read.left).clamp(0, read.width() - 1) as u32;
            let y = (y - read.top).clamp(0, read.height() - 1) as u32;
            before.get_pixel(x, y).0
        };
        for y in area.top..area.bottom {
            for x in area.left..area.right {
                let middle = (x as f64 + 0.5, y as f64 + 0.5);
                let (on_x, on_y) = match self.wrap {
                    Some(wrap) => wrap::pixel(wrap, x as i64, y as i64),
                    None => (x as u32, y as u32),
                };
                if distance_to_segment(middle, from, to) > self.radius
                    || self.mask.as_ref().is_some_and(|mask| !mask.contains(on_x, on_y))
                {
                    continue;
                }
                let own = around(x, y);
                let after = match self.retouch {
                    Retouch::Jumble => {
                        if self.random.next() >= self.strength {
                            continue;
                        }
                        let nudge = |random: &mut Random| (random.next() * 3.0) as i32 - 1;
                        let picked_x = x + behind.0 + nudge(&mut self.random);
                        let picked_y = y + behind.1 + nudge(&mut self.random);
                        around(picked_x, picked_y)
                    }
                    Retouch::Blur | Retouch::Sharpen => {
                        // The colours are multiplied by how opaque they are, then mixed
                        // with the pixel's own, then divided by how opaque the result is.
                        let mut sums = [0.0; 4];
                        for around_y in y - 1..=y + 1 {
                            for around_x in x - 1..=x + 1 {
                                let pixel = around(around_x, around_y);
                                let alpha = pixel[3] as f64;
                                for channel in 0..3 {
                                    sums[channel] += pixel[channel] as f64 * alpha;
                                }
                                sums[3] += alpha;
                            }
                        }
                        let own_alpha = own[3] as f64;
                        if self.retouch == Retouch::Sharpen {
                            // See-through pixels have no colour to sharpen.
                            if own[3] == 0 || sums[3] == 0.0 {
                                continue;
                            }
                            let mut after = own;
                            for channel in 0..3 {
                                let average = sums[channel] / sums[3];
                                let own = own[channel] as f64;
                                after[channel] = (own + (own - average) * self.strength).round().clamp(0.0, 255.0) as u8;
                            }
                            after
                        } else {
                            let mix = |own: f64, average: f64| own + (average - own) * self.strength;
                            let mixed_alpha = mix(own_alpha, sums[3] / 9.0);
                            if mixed_alpha < 0.5 {
                                CLEAR.0
                            } else {
                                let mut after = [0; 4];
                                for channel in 0..3 {
                                    let mixed = mix(own[channel] as f64 * own_alpha, sums[channel] / 9.0);
                                    after[channel] = (mixed / mixed_alpha).round().clamp(0.0, 255.0) as u8;
                                }
                                after[3] = mixed_alpha.round() as u8;
                                after
                            }
                        }
                    }
                };
                if after != own {
                    image.put_pixel(on_x, on_y, Rgba(after));
                    self.changed = true;
                }
            }
        }
        match self.wrap {
            Some(wrap) => wrap::changed_area(wrap, area),
            None => Some(area),
        }
    }

    /// Whether it changed any pixel yet.
    pub fn changed(&self) -> bool {
        self.changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Selection;

    const BLACK: Rgba<u8> = Rgba([0, 0, 0, 255]);
    const WHITE: Rgba<u8> = Rgba([255, 255, 255, 255]);
    const WHOLE: PixelRect = PixelRect { left: 0, top: 0, right: 9, bottom: 9 };

    /// A white image with a black column down the middle.
    fn striped() -> RgbaImage {
        RgbaImage::from_fn(9, 9, |x, _| if x == 4 { BLACK } else { WHITE })
    }

    #[test]
    fn blur_softens_edges() {
        let mut image = striped();
        let (stroke, area) = RetouchStroke::begin(&mut image, (4.5, 4.5), Retouch::Blur, 3, 100, None, 1, None);
        assert!(stroke.changed());
        assert!(area.is_some());
        // At full strength, a pixel is the average of the 3 × 3 around it.
        assert_eq!(image[(4, 4)], Rgba([170, 170, 170, 255]));
        assert_eq!(image[(3, 4)], Rgba([170, 170, 170, 255]));
        // Out of the tip, nothing changes.
        assert_eq!(image[(4, 0)], BLACK);
        assert_eq!(image[(0, 4)], WHITE);
    }

    #[test]
    fn blur_ignores_the_colour_of_clear_pixels() {
        let mut image = RgbaImage::new(3, 3);
        image.put_pixel(1, 1, Rgba([200, 0, 0, 255]));
        RetouchStroke::begin(&mut image, (0.5, 0.5), Retouch::Blur, 1, 100, None, 1, None);
        let pixel = image[(0, 0)];
        assert_eq!([pixel[0], pixel[1], pixel[2]], [200, 0, 0]);
        assert_eq!(pixel[3], 28);
    }

    #[test]
    fn going_over_again_blurs_more() {
        let mut image = striped();
        let (mut stroke, _) = RetouchStroke::begin(&mut image, (4.5, 4.5), Retouch::Blur, 1, 50, None, 1, None);
        let once = image[(4, 4)][0];
        stroke.draw_to(&mut image, (4.5, 4.5));
        assert!(image[(4, 4)][0] > once);
    }

    #[test]
    fn sharpen_brings_out_edges_and_keeps_alpha() {
        let gray = Rgba([100, 100, 100, 128]);
        let mut image = RgbaImage::from_fn(9, 9, |x, _| if x == 4 { gray } else { Rgba([150, 150, 150, 128]) });
        let (stroke, _) = RetouchStroke::begin(&mut image, (4.5, 4.5), Retouch::Sharpen, 3, 100, None, 1, None);
        assert!(stroke.changed());
        assert!(image[(4, 4)][0] < 100);
        assert!(image[(3, 4)][0] > 150);
        assert_eq!(image[(4, 4)][3], 128);
        // A flat area has nothing to sharpen.
        let mut flat = RgbaImage::from_pixel(5, 5, gray);
        let (stroke, _) = RetouchStroke::begin(&mut flat, (2.5, 2.5), Retouch::Sharpen, 5, 100, None, 1, None);
        assert!(!stroke.changed());
    }

    #[test]
    fn jumble_only_moves_colours_around() {
        let mut image = RgbaImage::from_fn(16, 16, |x, y| Rgba([(x * 16) as u8, (y * 16) as u8, 0, 255]));
        let colours: Vec<_> = image.pixels().copied().collect();
        let (mut stroke, _) = RetouchStroke::begin(&mut image, (2.0, 8.0), Retouch::Jumble, 8, 100, None, 3, None);
        stroke.draw_to(&mut image, (14.0, 8.0));
        assert!(stroke.changed());
        assert!(image.pixels().all(|pixel| colours.contains(pixel)));
    }

    #[test]
    fn only_retouches_selected_pixels() {
        let mut image = striped();
        let selection = Selection::rect(PixelRect { left: 0, top: 0, right: 4, bottom: 9 });
        let mask = SelectionMask::of(&selection, 9, 9);
        RetouchStroke::begin(&mut image, (4.5, 4.5), Retouch::Blur, 5, 100, mask, 1, None);
        assert_ne!(image[(3, 4)], WHITE);
        assert_eq!(image[(4, 4)], BLACK);
        assert_eq!(image[(5, 4)], WHITE);
    }

    #[test]
    fn stops_at_the_edges() {
        let mut image = striped();
        let (stroke, area) = RetouchStroke::begin(&mut image, (-20.0, -20.0), Retouch::Blur, 3, 100, None, 1, None);
        assert_eq!((area, stroke.changed()), (None, false));
    }

    #[test]
    fn a_wrapping_blur_mixes_in_the_other_edge() {
        // White, but for a black column at the left edge.
        let mut image = RgbaImage::from_fn(9, 9, |x, _| if x == 0 { BLACK } else { WHITE });
        let (_, area) = RetouchStroke::begin(&mut image, (8.5, 4.5), Retouch::Blur, 1, 100, None, 1, Some(WHOLE));
        // The pixel by the right edge has the black column beside it, past the edge.
        assert!(image[(8, 4)][0] < 255);
        // Over the edge, so all the way across.
        assert_eq!(area, Some(PixelRect { left: 0, top: 3, right: 9, bottom: 6 }));
    }
}

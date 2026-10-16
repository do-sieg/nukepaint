//! Dots scattered by the Spray, as long as it is held: each one a pixel of the colour
//! blended in, so they build up where it lingers. They gather towards the middle of
//! the round area it covers. With smooth edges, a dot can land between pixels, and is
//! then shared between the ones it overlaps. It may wrap around the image's edges,
//! the dots that land off one edge coming in at the other.

use image::{Rgba, RgbaImage};

use crate::{BlendMode, PixelRect, SelectionMask, blend::blend_pixel, wrap};

/// Widest the area the Spray covers can be, in pixels.
pub const MAX_SPRAY_SIZE: u32 = 256;
/// Dots it leaves every second at full speed.
const MAX_RATE: f64 = 4000.0;
/// How long a press leaves dots for, in seconds, however short it is.
const PRESS_TIME: f64 = 1.0 / 60.0;

/// The Spray, from when it is pressed until it is let go.
pub struct Spray {
    color: Rgba<u8>,
    size: u32,
    /// Dots it leaves every second.
    rate: f64,
    smooth: bool,
    /// The pixels it may change, if not all of them.
    mask: Option<SelectionMask>,
    /// The area it wraps around the edges of, if it does.
    wrap: Option<PixelRect>,
    /// The point it last reached, on the image, in pixels.
    last: (f64, f64),
    /// The part of a dot owed for the time gone by, carried over to the next time.
    due: f64,
    random: Random,
    changed: bool,
}

impl Spray {
    /// Starts spraying the round area `size` pixels wide around the point `at` of the
    /// image, at `speed` percent of the fastest it can, changing only the pixels `mask`
    /// holds if there is one. `seed` picks where the dots fall. It wraps around the
    /// edges if `wrap` is set. Returns the area it changed, if any of it is on the image.
    #[allow(clippy::too_many_arguments)]
    pub fn begin(
        image: &mut RgbaImage,
        at: (f64, f64),
        color: Rgba<u8>,
        size: u32,
        speed: u32,
        smooth: bool,
        mask: Option<SelectionMask>,
        seed: u64,
        wrap: Option<PixelRect>,
    ) -> (Self, Option<PixelRect>) {
        let mut spray = Self {
            color,
            size: size.clamp(1, MAX_SPRAY_SIZE),
            rate: MAX_RATE * speed.clamp(1, 100) as f64 / 100.0,
            smooth,
            mask,
            wrap,
            last: at,
            due: 0.0,
            random: Random::new(seed),
            changed: false,
        };
        let area = spray.spray(image, at, PRESS_TIME);
        (spray, area)
    }

    /// Leaves the dots due over `seconds`, spread along the way from the point it last
    /// reached to `at`. Returns the area it changed, if any of it is on the image.
    pub fn spray(&mut self, image: &mut RgbaImage, at: (f64, f64), seconds: f64) -> Option<PixelRect> {
        let last = std::mem::replace(&mut self.last, at);
        let (from, at) = if let Some(area) = self.wrap { wrap::segment(area, last, at) } else { (last, at) };
        self.due += self.rate * seconds;
        let count = self.due as u64;
        self.due -= count as f64;
        let radius = self.size as f64 / 2.0;
        for _ in 0..count {
            let along = self.random.next();
            let angle = self.random.next() * std::f64::consts::TAU;
            let distance = self.random.next() * radius;
            let dot = (
                from.0 + (at.0 - from.0) * along + angle.cos() * distance,
                from.1 + (at.1 - from.1) * along + angle.sin() * distance,
            );
            if !self.smooth {
                self.dot(image, dot.0.floor() as i64, dot.1.floor() as i64, 1.0);
                continue;
            }
            // The dot's top left corner, the pixel that is in and how far into it.
            let corner = (dot.0 - 0.5, dot.1 - 0.5);
            let (x, y) = (corner.0.floor(), corner.1.floor());
            let (past_x, past_y) = (corner.0 - x, corner.1 - y);
            let (x, y) = (x as i64, y as i64);
            self.dot(image, x, y, (1.0 - past_x) * (1.0 - past_y));
            self.dot(image, x + 1, y, past_x * (1.0 - past_y));
            self.dot(image, x, y + 1, (1.0 - past_x) * past_y);
            self.dot(image, x + 1, y + 1, past_x * past_y);
        }
        let reach = radius + 2.0;
        let area = PixelRect {
            left: (from.0.min(at.0) - reach).floor() as i32,
            top: (from.1.min(at.1) - reach).floor() as i32,
            right: (from.0.max(at.0) + reach).ceil() as i32,
            bottom: (from.1.max(at.1) + reach).ceil() as i32,
        };
        if let Some(wrap) = self.wrap {
            return wrap::changed_area(wrap, area);
        }
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

    /// Blends as much of a dot as lands on the pixel at `x`, `y` into it. Off the
    /// image, only a spray that wraps around reaches it, at the other edge.
    fn dot(&mut self, image: &mut RgbaImage, x: i64, y: i64, share: f64) {
        let off = x < 0 || y < 0 || x >= image.width() as i64 || y >= image.height() as i64;
        if share <= 0.0 || (off && self.wrap.is_none()) {
            return;
        }
        let (x, y) = match self.wrap {
            Some(area) => wrap::pixel(area, x, y),
            None => (x as u32, y as u32),
        };
        if self.mask.as_ref().is_some_and(|mask| !mask.contains(x, y)) {
            return;
        }
        let pixel = image.get_pixel_mut(x, y);
        let before = pixel.0;
        blend_pixel(&mut pixel.0, &self.color.0, share as f32, BlendMode::Normal);
        self.changed |= pixel.0 != before;
    }
}

/// Numbers that look random enough to scatter dots and jumble pixels with: xorshift64*.
pub(crate) struct Random(u64);

impl Random {
    pub(crate) fn new(seed: u64) -> Self {
        // Never zero, which it would stay at.
        Self(seed | 1)
    }

    /// A number from 0 up to 1, not 1 itself.
    pub(crate) fn next(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let bits = self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11;
        bits as f64 / (1u64 << 53) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Selection;

    const BLACK: Rgba<u8> = Rgba([0, 0, 0, 255]);

    fn sprayed(image: &RgbaImage) -> Vec<(u32, u32)> {
        image.enumerate_pixels().filter(|(.., pixel)| pixel[3] > 0).map(|(x, y, _)| (x, y)).collect()
    }

    #[test]
    fn a_press_leaves_dots_within_the_area() {
        let mut image = RgbaImage::new(40, 40);
        let (spray, area) = Spray::begin(&mut image, (20.0, 20.0), BLACK, 10, 100, false, None, 7, None);
        assert!(spray.changed());
        assert_eq!(area, Some(PixelRect { left: 13, top: 13, right: 27, bottom: 27 }));
        let dots = sprayed(&image);
        assert!(!dots.is_empty());
        for (x, y) in dots {
            let (dx, dy) = (x as f64 + 0.5 - 20.0, y as f64 + 0.5 - 20.0);
            assert!(dx.hypot(dy) <= 5.0 + 0.75, "({x}, {y})");
            assert_eq!(image[(x, y)], BLACK);
        }
    }

    #[test]
    fn leaves_more_the_longer_it_is_held() {
        let mut image = RgbaImage::new(64, 64);
        let (mut spray, _) = Spray::begin(&mut image, (32.0, 32.0), BLACK, 64, 50, false, None, 1, None);
        let early = sprayed(&image).len();
        spray.spray(&mut image, (32.0, 32.0), 0.25);
        assert!(sprayed(&image).len() > early * 5);
    }

    #[test]
    fn dots_build_up() {
        let mut image = RgbaImage::new(1, 1);
        let faint = Rgba([0, 0, 0, 1]);
        let (mut spray, _) = Spray::begin(&mut image, (0.5, 0.5), faint, 1, 100, false, None, 3, None);
        let first = image[(0, 0)][3];
        spray.spray(&mut image, (0.5, 0.5), 0.01);
        assert!(image[(0, 0)][3] > first);
    }

    #[test]
    fn smooth_dots_share_pixels() {
        let mut image = RgbaImage::new(40, 40);
        Spray::begin(&mut image, (20.0, 20.0), BLACK, 20, 100, true, None, 5, None);
        assert!(image.pixels().any(|pixel| pixel[3] > 0 && pixel[3] < 255));
    }

    #[test]
    fn only_sprays_selected_pixels() {
        let mut image = RgbaImage::new(20, 20);
        let selection = Selection::rect(PixelRect { left: 0, top: 0, right: 10, bottom: 20 });
        let mask = SelectionMask::of(&selection, 20, 20);
        let (mut spray, _) = Spray::begin(&mut image, (10.0, 10.0), BLACK, 20, 100, true, mask, 9, None);
        spray.spray(&mut image, (10.0, 10.0), 0.5);
        let dots = sprayed(&image);
        assert!(!dots.is_empty());
        assert!(dots.iter().all(|&(x, _)| x < 10));
    }

    #[test]
    fn a_wrapping_spray_lands_by_the_other_edges() {
        let mut image = RgbaImage::new(20, 20);
        let whole = PixelRect { left: 0, top: 0, right: 20, bottom: 20 };
        Spray::begin(&mut image, (0.0, 0.0), BLACK, 10, 100, false, None, 7, Some(whole));
        let dots = sprayed(&image);
        let near = |at: u32| !(5..15).contains(&at);
        assert!(dots.iter().all(|&(x, y)| near(x) && near(y)));
        assert!(dots.iter().any(|&(x, _)| x >= 15) && dots.iter().any(|&(x, _)| x < 5));
    }
}

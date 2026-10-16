//! Gradients, as the Gradient tool draws them: from one colour at the point a drag
//! starts to another at the point it ends, in one of several shapes. In colour mode,
//! the gradient is blended over the pixels; in transparency mode, it fades them out
//! instead, from as they are at the start to clear at the end. Either way, it changes
//! only the selected pixels, if any are. Large images are cut into bands of rows, drawn
//! at the same time on as many threads as there are cores.

use std::{f64::consts::PI, num::NonZero, thread};

use image::{Rgba, RgbaImage};

use crate::{BlendMode, PixelRect, SelectionMask, blend::blend_pixel};

/// Fewest pixels worth sharing between threads.
const PARALLEL_PIXELS: usize = 1 << 16;
/// How many colours along a gradient are worked out before it is drawn, more than
/// a channel has values.
const COLOR_STEPS: usize = 1024;

/// How a gradient's colours spread out from where it starts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GradientKind {
    /// In bands across the line from the start to the end, the start colour before
    /// it and the end colour past it.
    #[default]
    Linear,
    /// As Linear, mirrored about the start.
    Reflected,
    /// In circles around the start, out to the end.
    Radial,
    /// In squares turned on a corner around the start, which reach the end.
    Diamond,
    /// Around the start, from the start colour towards the end to the end colour
    /// away from it.
    Conical,
}

impl GradientKind {
    pub const ALL: [Self; 5] = [Self::Linear, Self::Reflected, Self::Radial, Self::Diamond, Self::Conical];

    pub fn name(self) -> &'static str {
        match self {
            Self::Linear => "Linear",
            Self::Reflected => "Reflected",
            Self::Radial => "Radial",
            Self::Diamond => "Diamond",
            Self::Conical => "Conical",
        }
    }
}

/// What a gradient does to the pixels it is drawn on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GradientMode {
    /// Blends its colours over them.
    #[default]
    Color,
    /// Fades them out, keeping them as they are at the start, and clearing them at the end.
    Transparency,
}

impl GradientMode {
    pub const ALL: [Self; 2] = [Self::Color, Self::Transparency];

    pub fn name(self) -> &'static str {
        match self {
            Self::Color => "Color",
            Self::Transparency => "Transparency",
        }
    }
}

/// A gradient from the point `from` of the image to `to`, in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gradient {
    pub kind: GradientKind,
    pub mode: GradientMode,
    pub from: (f64, f64),
    pub to: (f64, f64),
    /// The colour at the start and the one at the end, in colour mode.
    pub colors: [Rgba<u8>; 2],
}

impl Gradient {
    /// How far the point `at` is along the gradient, from 0 at the start to 1 at the
    /// end and past it, or nothing if the start and the end are the same point.
    fn position(&self, at: (f64, f64)) -> Option<f64> {
        let (dx, dy) = (self.to.0 - self.from.0, self.to.1 - self.from.1);
        let length_squared = dx * dx + dy * dy;
        if length_squared < 1e-12 {
            return None;
        }
        let (px, py) = (at.0 - self.from.0, at.1 - self.from.1);
        // Along the line from the start to the end, and across it, in its lengths.
        let along = (px * dx + py * dy) / length_squared;
        let across = (px * dy - py * dx) / length_squared;
        let position = match self.kind {
            GradientKind::Linear => along,
            GradientKind::Reflected => along.abs(),
            GradientKind::Radial => (along * along + across * across).sqrt(),
            GradientKind::Diamond => along.abs() + across.abs(),
            GradientKind::Conical => across.atan2(along).abs() / PI,
        };
        Some(position.clamp(0.0, 1.0))
    }

    /// The colour at `position` along the gradient, the colours mixed as they would
    /// cover what is under them: a clear end lends none of its colour.
    fn color(&self, position: f64) -> [u8; 4] {
        let [start, end] = self.colors.map(|color| color.0.map(|channel| channel as f64));
        let alpha = start[3] + (end[3] - start[3]) * position;
        if alpha <= 0.0 {
            return [0; 4];
        }
        let mut color = [0; 4];
        for channel in 0..3 {
            let weighted = start[channel] * start[3] + (end[channel] * end[3] - start[channel] * start[3]) * position;
            color[channel] = (weighted / alpha).round().clamp(0.0, 255.0) as u8;
        }
        color[3] = alpha.round() as u8;
        color
    }

    /// The pixel `before` with the gradient drawn on it at `position`, its colours
    /// being `colors`, worked out at even steps along it.
    fn apply(&self, before: [u8; 4], position: f64, colors: &[[u8; 4]]) -> [u8; 4] {
        match self.mode {
            GradientMode::Color => {
                let color = colors[(position * (colors.len() - 1) as f64).round() as usize];
                if color[3] == u8::MAX || before[3] == 0 {
                    return color;
                }
                let mut pixel = before;
                blend_pixel(&mut pixel, &color, 1.0, BlendMode::Normal);
                pixel
            }
            GradientMode::Transparency => {
                let alpha = (before[3] as f64 * (1.0 - position)).round() as u8;
                if alpha == 0 { [0; 4] } else { [before[0], before[1], before[2], alpha] }
            }
        }
    }
}

/// A gradient being drawn on an image, which can be drawn again elsewhere over the
/// pixels as they were before it.
pub struct GradientFill {
    before: RgbaImage,
    /// The pixels it may change, if not all of them.
    mask: Option<SelectionMask>,
    changed: bool,
}

impl GradientFill {
    /// Starts drawing gradients on `image`, on the pixels `mask` holds if there is one.
    pub fn begin(image: &RgbaImage, mask: Option<SelectionMask>) -> Self {
        Self { before: image.clone(), mask, changed: false }
    }

    /// Draws `gradient` over the pixels as they were before, in place of the one drawn
    /// last, or puts them back as they were if its start and end are the same point.
    /// Returns the area it changed, if it changed any.
    pub fn draw(&mut self, image: &mut RgbaImage, gradient: &Gradient) -> Option<PixelRect> {
        let (width, height) = (image.width() as usize, image.height() as usize);
        let flags = self.mask.as_ref().map(SelectionMask::flags);
        let colors: Vec<[u8; 4]> =
            (0..COLOR_STEPS).map(|step| gradient.color(step as f64 / (COLOR_STEPS - 1) as f64)).collect();
        // Draws the rows from `top` on. Returns whether it changed any pixel from what it
        // was before any gradient, and from what the last one left.
        let draw_rows = |top: usize, pixels: &mut [u8], before: &[u8]| {
            let (mut changed, mut redrawn) = (false, false);
            let pixels = pixels.as_chunks_mut::<4>().0.iter_mut();
            for (i, (pixel, before)) in pixels.zip(before.as_chunks::<4>().0).enumerate() {
                let (x, y) = (i % width, top + i / width);
                let selected = flags.is_none_or(|flags| flags[y * width + x]);
                let position = gradient.position((x as f64 + 0.5, y as f64 + 0.5)).filter(|_| selected);
                let after = position.map_or(*before, |position| gradient.apply(*before, position, &colors));
                redrawn |= *pixel != after;
                changed |= after != *before;
                *pixel = after;
            }
            (changed, redrawn)
        };
        let threads = thread::available_parallelism().map_or(1, NonZero::get);
        let (changed, redrawn) = if threads == 1 || width * height < PARALLEL_PIXELS {
            draw_rows(0, image, &self.before)
        } else {
            let band_rows = height.div_ceil(threads);
            let band = band_rows * width * 4;
            thread::scope(|scope| {
                let bands = image.chunks_mut(band).zip(self.before.chunks(band)).enumerate();
                let handles: Vec<_> = bands
                    .map(|(i, (pixels, before))| scope.spawn(move || draw_rows(i * band_rows, pixels, before)))
                    .collect();
                handles.into_iter().map(|handle| handle.join().expect("drawing a band")).fold(
                    (false, false),
                    |(changed, redrawn), band| (changed || band.0, redrawn || band.1),
                )
            })
        };
        self.changed = changed;
        redrawn.then_some(PixelRect { left: 0, top: 0, right: width as i32, bottom: height as i32 })
    }

    /// Puts the pixels back as they were before any gradient was drawn.
    pub fn cancel(self, image: &mut RgbaImage) {
        *image = self.before;
    }

    /// Whether the gradient drawn last changed any pixel.
    pub fn changed(&self) -> bool {
        self.changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Selection;

    const RED: Rgba<u8> = Rgba([255, 0, 0, 255]);
    const BLUE: Rgba<u8> = Rgba([0, 0, 255, 255]);

    fn gradient(kind: GradientKind, from: (f64, f64), to: (f64, f64)) -> Gradient {
        Gradient { kind, mode: GradientMode::Color, from, to, colors: [RED, BLUE] }
    }

    #[test]
    fn linear_goes_from_one_colour_to_the_other() {
        let mut image = RgbaImage::new(10, 1);
        let mut fill = GradientFill::begin(&image, None);
        assert!(fill.draw(&mut image, &gradient(GradientKind::Linear, (0.0, 0.5), (10.0, 0.5))).is_some());
        assert!(fill.changed());
        assert_eq!(image[(0, 0)], Rgba([242, 0, 13, 255]));
        assert_eq!(image[(9, 0)], Rgba([13, 0, 242, 255]));
        assert!((0..9).all(|x| image[(x, 0)][0] > image[(x + 1, 0)][0]), "red fades along it");
    }

    #[test]
    fn before_and_past_the_ends_are_their_colours() {
        let mut image = RgbaImage::new(10, 1);
        let mut fill = GradientFill::begin(&image, None);
        fill.draw(&mut image, &gradient(GradientKind::Linear, (3.0, 0.0), (7.0, 0.0)));
        assert_eq!(image[(1, 0)], RED);
        assert_eq!(image[(8, 0)], BLUE);
    }

    #[test]
    fn shapes_spread_out_from_the_start() {
        let reflected = gradient(GradientKind::Reflected, (5.0, 5.0), (9.0, 5.0));
        assert_eq!(reflected.position((3.0, 5.0)), reflected.position((7.0, 5.0)));
        let radial = gradient(GradientKind::Radial, (5.0, 5.0), (9.0, 5.0));
        assert_eq!(radial.position((5.0, 9.0)), Some(1.0));
        assert_eq!(radial.position((5.0, 7.0)), Some(0.5));
        let diamond = gradient(GradientKind::Diamond, (5.0, 5.0), (9.0, 5.0));
        assert_eq!(diamond.position((7.0, 7.0)), Some(1.0));
        assert_eq!(diamond.position((5.0, 3.0)), Some(0.5));
        let conical = gradient(GradientKind::Conical, (5.0, 5.0), (9.0, 5.0));
        assert_eq!(conical.position((1.0, 5.0)), Some(1.0));
        assert_eq!(conical.position((5.0, 1.0)), Some(0.5));
        assert_eq!(conical.position((5.0, 9.0)), Some(0.5));
    }

    #[test]
    fn a_clear_end_lends_no_colour() {
        let fade = Gradient { colors: [RED, Rgba([0, 255, 0, 0])], ..gradient(GradientKind::Linear, (0.0, 0.0), (1.0, 0.0)) };
        assert_eq!(fade.color(0.5), [255, 0, 0, 128]);
    }

    #[test]
    fn transparency_fades_the_pixels_out() {
        let mut image = RgbaImage::from_pixel(4, 1, RED);
        let mut fill = GradientFill::begin(&image, None);
        let fade = Gradient { mode: GradientMode::Transparency, ..gradient(GradientKind::Linear, (0.5, 0.0), (3.5, 0.0)) };
        fill.draw(&mut image, &fade);
        assert_eq!(image[(0, 0)], RED);
        assert_eq!(image[(1, 0)], Rgba([255, 0, 0, 170]));
        assert_eq!(image[(3, 0)], Rgba([0, 0, 0, 0]));
    }

    #[test]
    fn draws_again_over_the_pixels_as_they_were() {
        let mut image = RgbaImage::new(10, 1);
        let mut fill = GradientFill::begin(&image, None);
        fill.draw(&mut image, &gradient(GradientKind::Linear, (0.0, 0.0), (10.0, 0.0)));
        fill.draw(&mut image, &gradient(GradientKind::Linear, (10.0, 0.0), (0.0, 0.0)));
        assert_eq!(image[(0, 0)], Rgba([13, 0, 242, 255]), "not blended over the first one");
        // Nowhere to go from the start: nothing is drawn.
        assert!(fill.draw(&mut image, &gradient(GradientKind::Linear, (4.0, 0.0), (4.0, 0.0))).is_some());
        assert!(!fill.changed());
        assert_eq!(image, RgbaImage::new(10, 1));
        fill.draw(&mut image, &gradient(GradientKind::Linear, (0.0, 0.0), (10.0, 0.0)));
        fill.cancel(&mut image);
        assert_eq!(image, RgbaImage::new(10, 1));
    }

    #[test]
    fn changes_only_the_selected_pixels() {
        let mut image = RgbaImage::new(4, 4);
        let selection = Selection::rect(PixelRect { left: 1, top: 1, right: 3, bottom: 3 });
        let mask = SelectionMask::of(&selection, 4, 4);
        let mut fill = GradientFill::begin(&image, mask);
        fill.draw(&mut image, &gradient(GradientKind::Linear, (0.0, 0.0), (4.0, 0.0)));
        assert_eq!(image[(0, 0)], Rgba([0, 0, 0, 0]));
        assert_eq!(image[(3, 3)], Rgba([0, 0, 0, 0]));
        assert_eq!(image[(1, 1)][3], 255);
    }
}

//! The effects of the Stylize menu, which draw what changes in the image rather than
//! the image itself: its edges, as lines or light, or its tones as a relief. How opaque
//! each pixel is stays the same.

use std::sync::atomic::AtomicBool;

use super::{
    Buffer, Effect, GRAIN_MIDDLE, Weighing,
    artistic::{clamped, each_pixel, emboss, lighting, lightness, straight},
    blur, fill_rows,
};

/// How the Edge Detect measures how fast the colours change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeAlgorithm {
    /// How much they change from one side of each pixel to the other, across, down or
    /// both, weighing the pixels beside it more than those at its corners.
    Sobel,
    /// How different each pixel is from the eight around it, which finds thin edges.
    Laplace,
    /// The difference between the image blurred by two radii, which finds the edges of
    /// details between those sizes and leaves out finer noise.
    DifferenceOfGaussians,
}

impl EdgeAlgorithm {
    pub const ALL: [Self; 3] = [Self::Sobel, Self::Laplace, Self::DifferenceOfGaussians];
}

/// The colours, multiplied by how opaque they are, as the stylizing effect makes them.
/// Gives up once `stop` is set.
pub(super) fn render(source: &Buffer, effect: &Effect, stop: &AtomicBool) -> Option<Buffer> {
    match *effect {
        Effect::EdgeDetect { algorithm, horizontal, vertical, radii, amount, normalize, invert } => {
            let strengths = match algorithm {
                EdgeAlgorithm::Sobel => sobel(source, horizontal, vertical, stop)?,
                EdgeAlgorithm::Laplace => laplace(source, stop)?,
                EdgeAlgorithm::DifferenceOfGaussians => difference_of_gaussians(source, radii, stop)?,
            };
            let factor = if normalize {
                let strongest = strengths.iter().flatten().fold(0.0_f32, |strongest, &strength| strongest.max(strength));
                if strongest > 0.0 { 1.0 / strongest } else { 0.0 }
            } else {
                amount / 100.0
            };
            each_pixel(source, stop, |x, y| {
                let alpha = source.pixels[y * source.width + x][3];
                let [r, g, b] = strengths[y * source.width + x].map(|strength| {
                    let edge = (strength * factor).clamp(0.0, 1.0);
                    if invert { 1.0 - edge } else { edge }
                });
                [r * alpha, g * alpha, b * alpha, alpha]
            })
        }
        Effect::Emboss { azimuth, elevation, depth } => {
            let heights = lightness(source);
            let lit = lighting(&heights, source.width, source.height, azimuth, elevation, depth);
            each_pixel(source, stop, |x, y| {
                let alpha = source.pixels[y * source.width + x][3];
                let grey = (GRAIN_MIDDLE + lit(x, y)).clamp(0.0, 1.0) * alpha;
                [grey, grey, grey, alpha]
            })
        }
        Effect::Relief { azimuth, elevation, depth } => emboss(source, &lightness(source), azimuth, elevation, depth, stop),
        Effect::Outline { thickness, intensity } => outline(source, thickness, intensity / 100.0, stop),
        _ => unreachable!("not a stylizing effect"),
    }
}

/// How fast red, green and blue change at each pixel, as the Sobel operator measures
/// it across if `horizontal` and down if `vertical`: 1 across a sharp edge from none of
/// a channel to all of it.
fn sobel(source: &Buffer, horizontal: bool, vertical: bool, stop: &AtomicBool) -> Option<Vec<[f32; 3]>> {
    let (width, height) = (source.width, source.height);
    let at = |x: isize, y: isize, channel: usize| source.pixels[clamped(x, y, width, height)][channel];
    strengths(source, stop, |x, y, channel| {
        let at = |dx: isize, dy: isize| at(x + dx, y + dy, channel);
        let across = at(1, -1) + 2.0 * at(1, 0) + at(1, 1) - at(-1, -1) - 2.0 * at(-1, 0) - at(-1, 1);
        let down = at(-1, 1) + 2.0 * at(0, 1) + at(1, 1) - at(-1, -1) - 2.0 * at(0, -1) - at(1, -1);
        let (across, down) = (across / 4.0 * horizontal as u8 as f32, down / 4.0 * vertical as u8 as f32);
        across.hypot(down)
    })
}

/// How different red, green and blue are at each pixel from the eight around it: 1
/// beside a sharp straight edge from none of a channel to all of it, where three of
/// them are on the other side.
fn laplace(source: &Buffer, stop: &AtomicBool) -> Option<Vec<[f32; 3]>> {
    let (width, height) = (source.width, source.height);
    let at = |x: isize, y: isize, channel: usize| source.pixels[clamped(x, y, width, height)][channel];
    strengths(source, stop, |x, y, channel| {
        let around: f32 = [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)]
            .into_iter()
            .map(|(dx, dy)| at(x + dx, y + dy, channel))
            .sum();
        (around - 8.0 * at(x, y, channel)).abs() / 3.0
    })
}

/// How far red, green and blue blurred by one of `radii` are from blurred by the other.
fn difference_of_gaussians(source: &Buffer, radii: [f32; 2], stop: &AtomicBool) -> Option<Vec<[f32; 3]>> {
    let blurred = |radius: f32| if radius < 1.0 { Some(None) } else { blur(source, Weighing::Gaussian, radius, 1.0, stop).map(Some) };
    let (first, second) = (blurred(radii[0])?, blurred(radii[1])?);
    let (first, second) = (first.as_ref().unwrap_or(source), second.as_ref().unwrap_or(source));
    strengths(source, stop, |x, y, channel| {
        let index = y as usize * source.width + x as usize;
        (first.pixels[index][channel] - second.pixels[index][channel]).abs()
    })
}

/// Red, green and blue's `strength` at each pixel, from where it is, across and down.
fn strengths(source: &Buffer, stop: &AtomicBool, strength: impl Fn(isize, isize, usize) -> f32 + Sync) -> Option<Vec<[f32; 3]>> {
    let mut strengths = vec![[0.0; 3]; source.pixels.len()];
    let done = fill_rows(&mut strengths, source.width, stop, |y, row| {
        for (x, out) in row.iter_mut().enumerate() {
            *out = std::array::from_fn(|channel| strength(x as isize, y as isize, channel));
        }
    });
    done.then_some(strengths)
}

/// Each pixel's own colour as much as its channels change within `thickness` of it,
/// `intensity` times more strongly, from 0 to 1, and white elsewhere: lines along the
/// edges, as wide as the thickness on either side.
fn outline(source: &Buffer, thickness: f32, intensity: f32, stop: &AtomicBool) -> Option<Buffer> {
    let reach = thickness.round().max(1.0) as usize;
    let lowest = extremes(source, reach, f32::min, stop)?;
    let highest = extremes(source, reach, f32::max, stop)?;
    // A change all the way shows fully with no intensity, one of a tenth half way up,
    // and one of a hundredth at the top.
    let gain = 100.0_f32.powf(intensity.clamp(0.0, 1.0));
    each_pixel(source, stop, |x, y| {
        let index = y * source.width + x;
        let (low, high) = (lowest.pixels[index], highest.pixels[index]);
        let change = (0..4).map(|channel| high[channel] - low[channel]).fold(0.0_f32, f32::max);
        let line = (change * gain).min(1.0);
        let own = straight(source.pixels[index]);
        let alpha = own[3];
        let [r, g, b] = std::array::from_fn(|channel| (1.0 - line + line * own[channel]) * alpha);
        [r, g, b, alpha]
    })
}

/// Each channel's lowest, or highest, as `pick` says, within the square `reach` from
/// each pixel on every side.
fn extremes(source: &Buffer, reach: usize, pick: fn(f32, f32) -> f32, stop: &AtomicBool) -> Option<Buffer> {
    let across = extreme_rows(source, reach, pick, stop)?;
    Some(extreme_rows(&across.transposed(), reach, pick, stop)?.transposed())
}

/// Each channel's lowest, or highest, within `reach` along each row on either side.
fn extreme_rows(source: &Buffer, reach: usize, pick: fn(f32, f32) -> f32, stop: &AtomicBool) -> Option<Buffer> {
    let width = source.width;
    let mut pixels = vec![[0.0; 4]; source.pixels.len()];
    let done = fill_rows(&mut pixels, width, stop, |y, row| {
        let line = &source.pixels[y * width..(y + 1) * width];
        for (x, out) in row.iter_mut().enumerate() {
            let window = &line[x.saturating_sub(reach)..(x + reach + 1).min(width)];
            *out = window.iter().copied().reduce(|a, b| std::array::from_fn(|channel| pick(a[channel], b[channel]))).unwrap();
        }
    });
    done.then(|| source.with_pixels(pixels))
}

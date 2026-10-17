//! The effects of the Noise menu, which scatter the image at random: the Add Noise
//! changes the colours of its pixels, the Spread moves them about. Each pixel is
//! scattered the same way for the same seed, wherever the selection is. How opaque
//! each pixel is stays the same.

use std::{f32::consts::PI, sync::atomic::AtomicBool};

use super::{
    Buffer, Effect, MIN_ALPHA,
    artistic::{clamped, each_pixel, lab, noise, straight},
};
use crate::adjustment::{hsv_to_rgb, rgb_to_hsv};

/// How far the Add Noise usually takes a colour at full intensity, from 0 to 1.
const NOISE_SPREAD: f32 = 0.2;

/// What the Add Noise changes the colours in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoiseColorSpace {
    /// Red, green and blue, all by as much, or each its own way the more saturated.
    Rgb,
    /// The value, then the saturation and the hue the more saturated.
    Hsv,
    /// The CIE lightness, then the chroma and the hue the more saturated: the colours
    /// change by as much as they look to.
    Lch,
}

impl NoiseColorSpace {
    pub const ALL: [Self; 3] = [Self::Rgb, Self::Hsv, Self::Lch];
}

/// The colours, multiplied by how opaque they are, as the noise effect makes them.
/// Gives up once `stop` is set.
pub(super) fn render(source: &Buffer, effect: &Effect, stop: &AtomicBool) -> Option<Buffer> {
    match *effect {
        Effect::AddNoise { intensity, saturation, coverage, color_space, seed } => {
            let (amount, saturation, coverage) = (intensity / 100.0 * NOISE_SPREAD, saturation / 100.0, coverage / 100.0);
            each_pixel(source, stop, |x, y| {
                let own = source.pixels[y * source.width + x];
                let random = |stream: u32| noise(x as i64, y as i64, seed.wrapping_mul(16).wrapping_add(stream));
                if own[3] < MIN_ALPHA || random(0) >= coverage {
                    return own;
                }
                // About normally spread, usually within 1 and always within 3 of 0.
                let normal = |index: u32| ((1..=3).map(|stream| random(3 * index + stream)).sum::<f32>() - 1.5) * 2.0;
                let changes = [0, 1, 2, 3].map(normal);
                let [r, g, b, alpha] = straight(own);
                let rgb = add_noise([r, g, b], changes, amount, saturation, color_space);
                let [r, g, b] = rgb.map(|channel| channel.clamp(0.0, 1.0) * alpha);
                [r, g, b, alpha]
            })
        }
        Effect::Spread { amount, downward, seed } => {
            let downward = (downward / 100.0).clamp(0.0, 1.0);
            let (width, height) = (source.width, source.height);
            each_pixel(source, stop, |x, y| {
                let random = |row: i64, stream: u32| noise(x as i64, row, seed.wrapping_mul(16).wrapping_add(stream));
                let across = (random(y as i64, 0) * 2.0 - 1.0) * amount * (1.0 - downward / 2.0);
                // Pulled from above, as far as the column it is in drips on average, which
                // streaks it down as if melting.
                let drip = (random(0, 2) + random(y as i64, 3)) * amount * downward;
                let down = (random(y as i64, 1) * 2.0 - 1.0) * amount * (1.0 - downward) - drip;
                source.pixels[clamped(x as isize + across.round() as isize, y as isize + down.round() as isize, width, height)]
            })
        }
        _ => unreachable!("not a noise effect"),
    }
}

/// The colour, from 0 to 1, changed by `amount` times each of `changes` in
/// `color_space`: the first for how light it is, the others for its colour, as much as
/// `saturation` says, from 0 to 1.
fn add_noise(rgb: [f32; 3], changes: [f32; 4], amount: f32, saturation: f32, color_space: NoiseColorSpace) -> [f32; 3] {
    let [light, first, second, third] = changes.map(|change| change * amount);
    let tinted = [first, second, third].map(|change| change * saturation);
    match color_space {
        NoiseColorSpace::Rgb => {
            std::array::from_fn(|channel| rgb[channel] + light * (1.0 - saturation) + [first, second, third][channel] * saturation)
        }
        NoiseColorSpace::Hsv => {
            let [hue, chroma, value] = rgb_to_hsv(rgb);
            hsv_to_rgb([(hue + tinted[1] / 2.0).rem_euclid(1.0), (chroma + tinted[0]).clamp(0.0, 1.0), (value + light).clamp(0.0, 1.0)])
        }
        NoiseColorSpace::Lch => {
            let [lightness, a, b] = lab(rgb);
            let chroma = (a.hypot(b) + tinted[0] * 100.0).max(0.0);
            let hue = b.atan2(a) + tinted[1] * PI;
            from_lab([lightness + light * 100.0, chroma * hue.cos(), chroma * hue.sin()])
        }
    }
}

/// The colour of CIE L*a*b*, as [`lab`] measures it, in red, green and blue, from 0
/// to 1, or the nearest one if it has none.
fn from_lab([lightness, a, b]: [f32; 3]) -> [f32; 3] {
    let fy = (lightness + 16.0) / 116.0;
    let (fx, fz) = (fy + a / 500.0, fy - b / 200.0);
    let f = |t: f32| if t > 0.206893 { t * t * t } else { (t - 16.0 / 116.0) / 7.787 };
    let (x, y, z) = (f(fx) * 0.95047, f(fy), f(fz) * 1.08883);
    let gamma = |channel: f32| {
        let channel = channel.clamp(0.0, 1.0);
        if channel <= 0.0031308 { channel * 12.92 } else { 1.055 * channel.powf(1.0 / 2.4) - 0.055 }
    };
    [
        gamma(3.2406 * x - 1.5372 * y - 0.4986 * z),
        gamma(-0.9689 * x + 1.8758 * y + 0.0415 * z),
        gamma(0.0557 * x - 0.2040 * y + 1.0570 * z),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lab_goes_back_to_the_colour_it_came_from() {
        for rgb in [[0.0, 0.0, 0.0], [1.0, 1.0, 1.0], [0.8, 0.2, 0.1], [0.1, 0.5, 0.9], [0.3, 0.7, 0.2]] {
            let back = from_lab(lab(rgb));
            assert!((0..3).all(|channel| (back[channel] - rgb[channel]).abs() < 0.002), "{rgb:?} came back as {back:?}");
        }
    }

    #[test]
    fn noise_without_saturation_changes_only_how_light_colours_are() {
        let changes = [1.0, -2.0, 0.5, 2.0];
        let grey = add_noise([0.5; 3], changes, 0.1, 0.0, NoiseColorSpace::Rgb);
        assert!(grey.iter().all(|&channel| (channel - 0.6).abs() < 1e-6), "{grey:?}");
        let [hue, chroma, _] = rgb_to_hsv(add_noise([0.8, 0.4, 0.2], changes, 0.1, 0.0, NoiseColorSpace::Hsv));
        let [same_hue, same_chroma, _] = rgb_to_hsv([0.8, 0.4, 0.2]);
        assert!((hue - same_hue).abs() < 1e-5 && (chroma - same_chroma).abs() < 1e-5);
        let [_, a, b] = lab(add_noise([0.8, 0.4, 0.2], changes, 0.1, 0.0, NoiseColorSpace::Lch));
        let [_, same_a, same_b] = lab([0.8, 0.4, 0.2]);
        assert!((a.atan2(b) - same_a.atan2(same_b)).abs() < 0.05);
    }
}

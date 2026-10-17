//! The effects of the Decor menu, which frame the selected pixels, or the whole image:
//! a bevel or a border along the edges of the selection, whatever its shape, or the
//! corners of the rectangle around it rounded off.

use std::sync::atomic::AtomicBool;

use image::RgbaImage;

use super::{
    Buffer, Effect,
    artistic::{each_pixel, opaque_as, straight},
    fill_rows, lightened,
};
use crate::{PixelRect, SelectionMask};

/// Where the Add Border is lit from, across and down: above, a little to the left, so
/// its top is as much lighter as its bottom is darker, and its left half as much as
/// its right is darker.
const BORDER_LIGHT: [f32; 2] = [-0.447_213_6, -0.894_427_2];

/// The pixels as the decor effect makes them, within those `mask` holds, if there is
/// one, or the whole image. Gives up once `stop` is set, returning nothing.
pub(super) fn render(pixels: &RgbaImage, effect: &Effect, mask: Option<&SelectionMask>, stop: &AtomicBool) -> Option<RgbaImage> {
    let (width, height) = (pixels.width() as usize, pixels.height() as usize);
    let selected = |index: usize| mask.is_none_or(|mask| mask.flags()[index]);
    match *effect {
        Effect::AddBevel { width: bevel, depth, azimuth } => {
            let source = Buffer::premultiplied(pixels);
            // Only the pixels that can be seen stand out, so a shape on a see-through layer does.
            let inside: Vec<bool> = (0..width * height).map(|index| selected(index) && source.pixels[index][3] >= 0.5).collect();
            let distances = Distances::to_outside(&inside, width, height, stop)?;
            let light = [azimuth.to_radians().cos(), -azimuth.to_radians().sin()];
            let depth = depth / 100.0;
            let bevelled = each_pixel(&source, stop, |x, y| {
                let own = source.pixels[y * width + x];
                let along = (distances.at(x, y) - 0.5) / bevel.max(1.0);
                if !inside[y * width + x] || along >= 1.0 {
                    return own;
                }
                let Some(facing) = distances.facing(x, y, light) else {
                    return own;
                };
                // As steep as a rounded edge is, from the outside in.
                let shade = facing * (1.0 - along) * depth;
                let [r, g, b, alpha] = straight(own);
                opaque_as([r, g, b].map(|channel| lightened(channel, shade)), alpha)
            })?;
            Some(bevelled.to_image())
        }
        Effect::AddBorder { size, color, shading } => {
            let source = Buffer::premultiplied(pixels);
            let inside: Vec<bool> = (0..width * height).map(selected).collect();
            let distances = Distances::to_outside(&inside, width, height, stop)?;
            let [r, g, b, alpha] = color.map(|channel| channel as f32 / 255.0);
            let shading = shading / 100.0;
            let bordered = each_pixel(&source, stop, |x, y| {
                let own = source.pixels[y * width + x];
                // Partly over the pixels its inner edge goes through, along a curve.
                let cover = (size + 1.0 - distances.at(x, y)).clamp(0.0, 1.0) * alpha;
                if cover <= 0.0 {
                    return own;
                }
                let shade = distances.facing(x, y, BORDER_LIGHT).map_or(0.0, |facing| (facing / -BORDER_LIGHT[1]).clamp(-1.0, 1.0)) * shading;
                let border = opaque_as([r, g, b].map(|channel| lightened(channel, shade)), 1.0);
                std::array::from_fn(|channel| border[channel] * cover + own[channel] * (1.0 - cover))
            })?;
            Some(bordered.to_image())
        }
        Effect::RoundCorners { radius } => {
            let whole = PixelRect { left: 0, top: 0, right: width as i32, bottom: height as i32 };
            let area = mask.and_then(SelectionMask::bounds).unwrap_or(whole);
            let radius = radius.min(area.width().min(area.height()) as f32 / 2.0);
            let (left, top, right, bottom) = (area.left as f32, area.top as f32, area.right as f32, area.bottom as f32);
            let mut rounded: Vec<[u8; 4]> = pixels.pixels().map(|pixel| pixel.0).collect();
            let done = fill_rows(&mut rounded, width, stop, |y, row| {
                let y = y as f32 + 0.5;
                for (x, pixel) in row.iter_mut().enumerate() {
                    let x = x as f32 + 0.5;
                    // How far past the middle of the nearest corner's arc, if in a corner.
                    let (dx, dy) = ((left + radius - x).max(x - (right - radius)), (top + radius - y).max(y - (bottom - radius)));
                    if dx <= 0.0 || dy <= 0.0 {
                        continue;
                    }
                    let cover = (radius - dx.hypot(dy) + 0.5).clamp(0.0, 1.0);
                    pixel[3] = (pixel[3] as f32 * cover).round() as u8;
                    if pixel[3] == 0 {
                        *pixel = [0; 4];
                    }
                }
            });
            done.then(|| RgbaImage::from_raw(width as u32, height as u32, rounded.into_flattened()).unwrap())
        }
        _ => unreachable!("not a decor effect"),
    }
}

/// How far each pixel of the image is from the nearest one outside a shape, from
/// middle to middle, past the edges of the image counting as outside: 0 for those
/// outside. Kept with a ring of pixels around the image, outside.
struct Distances {
    width: usize,
    values: Vec<f32>,
}

impl Distances {
    /// The distances of the pixels `inside` holds, rows of `width` of them. Worked out
    /// along each row, then along each column from those, as Felzenszwalb and
    /// Huttenlocher do. Gives up once `stop` is set.
    fn to_outside(inside: &[bool], width: usize, height: usize, stop: &AtomicBool) -> Option<Self> {
        let (padded_width, padded_height) = (width + 2, height + 2);
        let is_inside = |x: usize, y: usize| (1..=width).contains(&x) && (1..=height).contains(&y) && inside[(y - 1) * width + x - 1];
        // Along each row, which has a pixel outside at either end, as far as the nearest one, squared.
        let mut across = vec![0.0f64; padded_width * padded_height];
        let done = fill_rows(&mut across, padded_width, stop, |y, row| {
            let mut last = None;
            for (x, out) in row.iter_mut().enumerate() {
                if !is_inside(x, y) {
                    last = Some(x);
                }
                *out = last.map_or(f64::MAX, |last| (x - last) as f64);
            }
            let mut next = None;
            for (x, out) in row.iter_mut().enumerate().rev() {
                if !is_inside(x, y) {
                    next = Some(x);
                }
                if let Some(next) = next {
                    *out = out.min((next - x) as f64);
                }
                *out *= *out;
            }
        });
        if !done {
            return None;
        }
        let columns = Buffer::transposed_items(&across, padded_width, padded_height);
        let mut squared = vec![0.0f64; columns.len()];
        let done = fill_rows(&mut squared, padded_height, stop, |x, column| {
            lower_envelope(&columns[x * padded_height..(x + 1) * padded_height], column);
        });
        if !done {
            return None;
        }
        let values = Buffer::transposed_items(&squared, padded_height, padded_width).into_iter().map(|value| value.sqrt() as f32).collect();
        Some(Self { width: padded_width, values })
    }

    /// The distance of the pixel at `x` and `y` of the image.
    fn at(&self, x: usize, y: usize) -> f32 {
        self.values[(y + 1) * self.width + x + 1]
    }

    /// How much the edge nearest the pixel at `x` and `y` faces `light`, a way across
    /// and down one long, from 1 straight towards it to -1 straight away, or nothing
    /// where no edge is nearer one way than the others.
    fn facing(&self, x: usize, y: usize, light: [f32; 2]) -> Option<f32> {
        let at = |x: usize, y: usize| self.values[y * self.width + x];
        let (x, y) = (x + 1, y + 1);
        // The way in from the edge.
        let inwards = [at(x + 1, y) - at(x - 1, y), at(x, y + 1) - at(x, y - 1)];
        let length = inwards[0].hypot(inwards[1]);
        (length > f32::EPSILON).then(|| -(inwards[0] * light[0] + inwards[1] * light[1]) / length)
    }
}

/// The least, for each place along a line, of each value of `values` plus how far it
/// is from there, squared: the lower envelope of the parabolas those make.
fn lower_envelope(values: &[f64], out: &mut [f64]) {
    // Where the parabolas of the envelope are lowest, and where each starts being part of it.
    let mut lowest = vec![0usize; values.len()];
    let mut starts = vec![f64::NEG_INFINITY; values.len() + 1];
    starts[1] = f64::INFINITY;
    let mut last = 0;
    for at in 1..values.len() {
        loop {
            let other = lowest[last];
            let crossing = ((values[at] + (at * at) as f64) - (values[other] + (other * other) as f64)) / (2 * (at - other)) as f64;
            // The first starts endlessly far back, so is never left out.
            if crossing <= starts[last] {
                last -= 1;
                continue;
            }
            last += 1;
            lowest[last] = at;
            starts[last] = crossing;
            starts[last + 1] = f64::INFINITY;
            break;
        }
    }
    let mut parabola = 0;
    for (at, out) in out.iter_mut().enumerate() {
        while starts[parabola + 1] < at as f64 {
            parabola += 1;
        }
        let from = lowest[parabola];
        let away = at.abs_diff(from) as f64;
        *out = away * away + values[from];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distances_are_to_the_nearest_pixel_outside() {
        let (width, height) = (23, 17);
        // A ring and some specks left out of the shape.
        let inside: Vec<bool> = (0..width * height)
            .map(|index| {
                let (x, y) = ((index % width) as f32 - 11.0, (index / width) as f32 - 8.0);
                let ring = x.hypot(y);
                !(4.0..5.0).contains(&ring) && index % 37 != 0
            })
            .collect();
        let distances = Distances::to_outside(&inside, width, height, &AtomicBool::new(false)).unwrap();
        for y in 0..height {
            for x in 0..width {
                // Past the edges counts as outside.
                let mut nearest = [x + 1, width - x, y + 1, height - y].into_iter().min().unwrap() as f32;
                for (index, &inside) in inside.iter().enumerate() {
                    if !inside {
                        let (dx, dy) = ((index % width) as f32 - x as f32, (index / width) as f32 - y as f32);
                        nearest = nearest.min(dx.hypot(dy));
                    }
                }
                assert!((distances.at(x, y) - nearest).abs() < 1e-4, "at {x}, {y}: {} for {nearest}", distances.at(x, y));
            }
        }
    }
}

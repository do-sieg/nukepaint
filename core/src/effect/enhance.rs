//! The effects of the Enhance menu, which clean up or bring out what is in a picture
//! rather than change how it looks: noise, red eyes, jagged or interlaced edges and
//! details.

use std::sync::atomic::AtomicBool;

use image::RgbaImage;

use super::{Buffer, Effect, MIN_ALPHA, Weighing, add, blur, fill_rows, scale, variable_blur};

/// How much of red, green and blue the Red Eye Removal weighs, as GIMP's does: a
/// pixel is red enough once its weighed red is no less than the other two.
const RED_EYE_WEIGHTS: [f32; 3] = [0.513_333_3, 1.0, 0.193_333_3];
/// What a detail layer of the Wavelet-decompose holds where its scale has nothing:
/// the grey a Grain Merge leaves what is under it alone with.
pub(crate) const GRAIN_MIDDLE: f32 = 128.0 / 255.0;
/// Least difference between two colours for the Antialias to take them for different.
const SAME_COLOR: f32 = 1.0 / 512.0;

/// The colours, multiplied by how opaque they are, as the enhancing effect makes them.
/// Gives up once `stop` is set.
pub(super) fn render(source: &Buffer, effect: &Effect, stop: &AtomicBool) -> Option<Buffer> {
    match *effect {
        Effect::Antialias => antialias(source, stop),
        Effect::Deinterlace { keep_odd, vertical, size } => {
            if vertical {
                Some(deinterlace(&source.transposed(), keep_odd, size, stop)?.transposed())
            } else {
                deinterlace(source, keep_odd, size, stop)
            }
        }
        Effect::HighPass { radius, contrast } => high_pass(source, radius, contrast / 100.0, stop),
        Effect::RedEyeRemoval { threshold } => red_eye(source, threshold / 100.0, stop),
        Effect::ReduceNoise { radius, strength } => reduce_noise(source, radius, strength / 100.0, stop),
        _ => unreachable!("not an enhancing effect"),
    }
}

/// Smooths the stairs along the edges of flat colours, as GIMP's does: each pixel is
/// cut in nine as Scale3x would enlarge it, which rounds off the corners of the stairs,
/// and the nine are averaged back into one.
fn antialias(source: &Buffer, stop: &AtomicBool) -> Option<Buffer> {
    let (width, height) = (source.width as isize, source.height as isize);
    let at = |x: isize, y: isize| source.pixels[(y.clamp(0, height - 1) * width + x.clamp(0, width - 1)) as usize];
    let same = |a: [f32; 4], b: [f32; 4]| a.iter().zip(b).all(|(a, b)| (a - b).abs() < SAME_COLOR);
    let mut pixels = vec![[0.0; 4]; source.pixels.len()];
    let done = fill_rows(&mut pixels, source.width, stop, |y, row| {
        let y = y as isize;
        for (x, out) in row.iter_mut().enumerate() {
            let x = x as isize;
            // The pixels around, as Scale3x names them.
            let [a, b, c] = [-1, 0, 1].map(|dx| at(x + dx, y - 1));
            let [d, e, f] = [-1, 0, 1].map(|dx| at(x + dx, y));
            let [g, h, i] = [-1, 0, 1].map(|dx| at(x + dx, y + 1));
            if same(b, h) || same(d, f) {
                *out = e;
                continue;
            }
            let pick = |take: bool, color: [f32; 4]| if take { color } else { e };
            let (db, bf, dh, hf) = (same(d, b), same(b, f), same(d, h), same(h, f));
            let parts = [
                pick(db, d),
                pick((db && !same(e, c)) || (bf && !same(e, a)), b),
                pick(bf, f),
                pick((db && !same(e, g)) || (dh && !same(e, a)), d),
                e,
                pick((bf && !same(e, i)) || (hf && !same(e, c)), f),
                pick(dh, d),
                pick((dh && !same(e, i)) || (hf && !same(e, g)), h),
                pick(hf, f),
            ];
            let mut sum = [0.0; 4];
            for part in parts {
                add(&mut sum, part, 1.0);
            }
            *out = scale(sum, 1.0 / 9.0);
        }
    });
    done.then(|| source.with_pixels(pixels))
}

/// Keeps every other band of `size` rows, the even ones from the top or the odd ones,
/// and fills each of the others with the average of the kept rows `size` above and
/// below it, or the one of them there is.
fn deinterlace(source: &Buffer, keep_odd: bool, size: u32, stop: &AtomicBool) -> Option<Buffer> {
    let (width, height) = (source.width, source.height);
    let size = size.max(1) as usize;
    let kept = |y: usize| (y / size % 2 == 1) == keep_odd;
    let row = |y: usize| &source.pixels[y * width..(y + 1) * width];
    let mut pixels = vec![[0.0; 4]; source.pixels.len()];
    let done = fill_rows(&mut pixels, width, stop, |y, out| {
        let (above, below) = (y.checked_sub(size), Some(y + size).filter(|&below| below < height));
        let fill = match (kept(y), above, below) {
            (true, ..) | (false, None, None) => None,
            (false, Some(above), Some(below)) => Some((above, below)),
            (false, Some(only), None) | (false, None, Some(only)) => Some((only, only)),
        };
        match fill {
            None => out.copy_from_slice(row(y)),
            Some((above, below)) => {
                for ((out, &a), &b) in out.iter_mut().zip(row(above)).zip(row(below)) {
                    let mut sum = [0.0; 4];
                    add(&mut sum, a, 0.5);
                    add(&mut sum, b, 0.5);
                    *out = sum;
                }
            }
        }
    });
    done.then(|| source.with_pixels(pixels))
}

/// How far each colour is from the Gaussian blurred one, `contrast` times, around
/// middle grey, as the Wavelet-decompose's details are: the details, without the broad tones. How opaque each pixel is stays
/// the same.
fn high_pass(source: &Buffer, radius: f32, contrast: f32, stop: &AtomicBool) -> Option<Buffer> {
    let blurred = if radius < 1.0 { None } else { Some(blur(source, Weighing::Gaussian, radius, 1.0, stop)?) };
    let blurred = blurred.as_ref().unwrap_or(source);
    let width = source.width;
    let mut pixels = vec![[0.0; 4]; source.pixels.len()];
    let done = fill_rows(&mut pixels, width, stop, |y, row| {
        for (x, out) in row.iter_mut().enumerate() {
            let own = source.pixels[y * width + x];
            let (own_color, blur_color) = (straight(own), straight(blurred.pixels[y * width + x]));
            let alpha = own[3];
            *out = std::array::from_fn(|channel| {
                if channel == 3 {
                    alpha
                } else {
                    (GRAIN_MIDDLE + (own_color[channel] - blur_color[channel]) * contrast).clamp(0.0, 1.0) * alpha
                }
            });
        }
    });
    done.then(|| source.with_pixels(pixels))
}

/// Takes the red out of the pixels redder than the others, the more of them the
/// higher the threshold, from 0 to 1, as GIMP's does: their red becomes about as
/// light as their green and blue.
fn red_eye(source: &Buffer, threshold: f32, stop: &AtomicBool) -> Option<Buffer> {
    let threshold = (threshold * 0.8 - 0.4) * 2.0;
    let width = source.width;
    let mut pixels = vec![[0.0; 4]; source.pixels.len()];
    let done = fill_rows(&mut pixels, width, stop, |y, row| {
        for (x, out) in row.iter_mut().enumerate() {
            let own = source.pixels[y * width + x];
            let color = straight(own);
            let [red, green, blue] = std::array::from_fn(|channel| color[channel] * RED_EYE_WEIGHTS[channel]);
            *out = if own[3] >= MIN_ALPHA && red >= green - threshold && red >= blue - threshold {
                let red = ((green + blue) / (2.0 * RED_EYE_WEIGHTS[0])).clamp(0.0, 1.0);
                [red * own[3], own[1], own[2], own[3]]
            } else {
                own
            };
        }
    });
    done.then(|| source.with_pixels(pixels))
}

/// Averages the pixels within the radius, weighed as a Gaussian Blur does, and less
/// the more different from the pixel's own colour, so noise is smoothed but edges are
/// kept: the stronger, the more different the colours it still mixes in.
fn reduce_noise(source: &Buffer, radius: f32, strength: f32, stop: &AtomicBool) -> Option<Buffer> {
    let (width, height) = (source.width as isize, source.height as isize);
    let reach = radius.max(0.0) as isize;
    let sigma = (radius / 2.0).max(0.5);
    let spread = (strength * 0.25).max(f32::EPSILON);
    let near: Vec<(isize, isize, f32)> = (-reach..=reach)
        .flat_map(|dy| (-reach..=reach).map(move |dx| (dx, dy)))
        .filter(|&(dx, dy)| (dx * dx + dy * dy) as f32 <= radius * radius + radius)
        .map(|(dx, dy)| (dx, dy, (-((dx * dx + dy * dy) as f32) / (2.0 * sigma * sigma)).exp()))
        .collect();
    let at = |x: isize, y: isize| source.pixels[(y.clamp(0, height - 1) * width + x.clamp(0, width - 1)) as usize];
    let mut pixels = vec![[0.0; 4]; source.pixels.len()];
    let done = fill_rows(&mut pixels, source.width, stop, |y, row| {
        let y = y as isize;
        for (x, out) in row.iter_mut().enumerate() {
            let x = x as isize;
            let middle = at(x, y);
            let mut sum = [0.0; 4];
            let mut total = 0.0;
            for &(dx, dy, weight) in &near {
                let color = at(x + dx, y + dy);
                let difference: f32 = (0..4).map(|channel| (color[channel] - middle[channel]).powi(2)).sum();
                let weight = weight * (-difference / (2.0 * spread * spread)).exp();
                add(&mut sum, color, weight);
                total += weight;
            }
            *out = scale(sum, 1.0 / total);
        }
    });
    done.then(|| source.with_pixels(pixels))
}

/// The colour divided by how opaque it is again, or black if it cannot be seen.
fn straight(color: [f32; 4]) -> [f32; 3] {
    if color[3] >= MIN_ALPHA { [color[0] / color[3], color[1] / color[3], color[2] / color[3]] } else { [0.0; 3] }
}

/// The pixels split into `scales` layers of details, each twice as coarse as the one
/// before, and what is left once they are all taken out, as GIMP's Wavelet-decompose
/// does: the leftover first, which is the pixels blurred the most, then the coarsest
/// details to the finest, each the difference between two blurs around middle grey.
/// Blended over the leftover in the Grain Merge mode, the details give the pixels back.
/// Gives up once `stop` is set, returning nothing.
pub fn wavelet_decompose(pixels: &RgbaImage, scales: u32, stop: &AtomicBool) -> Option<Vec<RgbaImage>> {
    let source = Buffer::premultiplied(pixels);
    let mut blurs = vec![source.pixels.iter().map(|&color| straight(color)).collect::<Vec<_>>()];
    let mut coarsest = None;
    for scale in 1..=scales {
        let radius = 2f32.powi(scale as i32);
        let blurred = variable_blur(&source, vec![radius; source.pixels.len()], stop)?;
        blurs.push(blurred.pixels.iter().map(|&color| straight(color)).collect());
        coarsest = Some(blurred);
    }
    let alphas: Vec<f32> = source.pixels.iter().map(|color| color[3]).collect();
    let to_image = |colors: Vec<[f32; 4]>| {
        let bytes = colors.into_iter().flat_map(|color| color.map(|channel| (channel * 255.0).round().clamp(0.0, 255.0) as u8)).collect();
        RgbaImage::from_raw(pixels.width(), pixels.height(), bytes).unwrap()
    };
    let mut layers = vec![coarsest.map_or_else(|| pixels.clone(), |buffer| buffer.to_image())];
    for scale in (1..=scales as usize).rev() {
        let (finer, coarser) = (&blurs[scale - 1], &blurs[scale]);
        let details = finer
            .iter()
            .zip(coarser)
            .zip(&alphas)
            .map(|((finer, coarser), &alpha)| {
                let detail = |channel: usize| GRAIN_MIDDLE + finer[channel] - coarser[channel];
                [detail(0), detail(1), detail(2), alpha]
            })
            .collect();
        layers.push(to_image(details));
    }
    Some(layers)
}


//! The blurs and the Sharpen of the Filters menu, which, unlike the adjustments, look at
//! the pixels around each one, as `effect.gdshader` does on the GPU in the Godot
//! version. The colours are multiplied by how opaque they are while they are mixed, so
//! see-through pixels add nothing to those around them, and divided out again at the
//! end. Past its edges, the image goes on as its edge pixels. Points between pixels are
//! read between the four around them. Large images are cut into bands of rows, worked
//! out at the same time on as many threads as there are cores.

use std::{
    f32::consts::TAU,
    num::NonZero,
    sync::atomic::{AtomicBool, Ordering},
    thread,
};

use image::RgbaImage;

use crate::SelectionMask;

/// Fewest pixels worth sharing between threads.
const PARALLEL_PIXELS: usize = 1 << 14;
/// The power the Bokeh Blur and the Square Blur raise colours to at full highlights.
const MAX_HIGHLIGHTS: f32 = 3.0;
/// Most points the blurs that follow a line or an arc take, however long it is.
const MAX_SAMPLES: usize = 256;
/// Least opacity a pixel keeps its colour with: those under it are stored as see-through.
const MIN_ALPHA: f32 = 0.5 / 255.0;

/// An effect, with its settings: lengths in pixels, angles in degrees anticlockwise from
/// the right, and shares in percent. A centre is from -100, the left or top edge, to
/// 100, the right or bottom one, 0 being the middle of the image.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// Averages a disc of pixels, as an out-of-focus lens does. The highlights, from 0
    /// to 100, make bright colours stand out.
    BokehBlur { radius: f32, highlights: f32 },
    /// Averages copies of the image moved the same distance in directions evenly
    /// around a circle.
    FragmentBlur { count: u32, distance: f32, rotation: f32 },
    GaussianBlur { radius: f32 },
    /// Each channel's value that this share of the pixels within the radius are no
    /// lighter than.
    MedianBlur { radius: f32, percentile: f32 },
    /// Averages a line of pixels, as a camera that moves while the picture is taken
    /// does, as far behind as ahead if centred, or else only behind.
    MotionBlur { angle: f32, distance: f32, centered: bool },
    /// Averages an arc of pixels around the centre, as a picture that turns does.
    RadialBlur { angle: f32, center: [f32; 2] },
    /// Averages short lines of pixels whose way drifts slowly across the image, as
    /// coarse brush strokes would.
    SketchBlur { radius: f32 },
    SquareBlur { radius: f32, highlights: f32 },
    /// A Gaussian Blur that leaves out colours more different than the threshold from
    /// the pixel's, which keeps edges sharp.
    SurfaceBlur { radius: f32, threshold: f32 },
    /// Averages a line of pixels towards the centre, this much of the way, as a camera
    /// that zooms while the picture is taken does.
    ZoomBlur { amount: f32, center: [f32; 2] },
    /// Takes each colour further from the blurred one, by this much again, which brings
    /// out edges and details, as an unsharp mask does, unless it is closer to it than
    /// the threshold. How opaque each pixel is stays the same.
    Sharpen { radius: f32, amount: f32, threshold: f32 },
}

/// The pixels as the effect makes them, but for those `mask` leaves out, if there is
/// one, which stay as they were. The mask must be as large as the pixels. Gives up
/// once `stop` is set, returning nothing.
pub fn apply_effect(pixels: &RgbaImage, effect: &Effect, mask: Option<&SelectionMask>, stop: &AtomicBool) -> Option<RgbaImage> {
    if pixels.width() == 0 || pixels.height() == 0 {
        return Some(pixels.clone());
    }
    let mut result = match *effect {
        Effect::MedianBlur { radius, percentile } => median(pixels, radius, percentile / 100.0, stop)?,
        _ => match render(&Buffer::premultiplied(pixels), effect, stop)? {
            Some(buffer) => buffer.to_image(),
            None => return Some(pixels.clone()),
        },
    };
    if let Some(mask) = mask {
        for ((pixel, before), &selected) in result.pixels_mut().zip(pixels.pixels()).zip(mask.flags()) {
            if !selected {
                *pixel = *before;
            }
        }
    }
    Some(result)
}

/// The colours, multiplied by how opaque they are, as the effect makes them, or
/// nothing for settings that change nothing. Gives up once `stop` is set.
fn render(source: &Buffer, effect: &Effect, stop: &AtomicBool) -> Option<Option<Buffer>> {
    let size = [source.width as f32, source.height as f32];
    let center = |[x, y]: [f32; 2]| [size[0] / 2.0 * (1.0 + x / 100.0), size[1] / 2.0 * (1.0 + y / 100.0)];
    let highlights = |highlights: f32| 1.0 + highlights / 100.0 * (MAX_HIGHLIGHTS - 1.0);
    let rendered = match *effect {
        Effect::BokehBlur { radius, highlights: h } => bokeh(source, radius, highlights(h), stop),
        Effect::FragmentBlur { count, distance, rotation } => {
            let count = count.max(1);
            let rotation = rotation.to_radians();
            per_pixel(source, stop, |p| {
                let mut sum = [0.0; 4];
                for i in 0..count {
                    let turn = rotation + TAU * i as f32 / count as f32;
                    add(&mut sum, source.sample([p[0] + distance * turn.cos(), p[1] - distance * turn.sin()]), 1.0);
                }
                scale(sum, 1.0 / count as f32)
            })
        }
        Effect::GaussianBlur { radius } if radius < 1.0 => return Some(None),
        Effect::GaussianBlur { radius } => blur(source, Weighing::Gaussian, radius, 1.0, stop),
        Effect::MedianBlur { .. } => unreachable!("worked out on the bytes"),
        Effect::MotionBlur { angle, distance, centered } => {
            let angle = angle.to_radians();
            let direction = [angle.cos(), -angle.sin()];
            let samples = (distance.ceil().max(0.0) as usize + 1).min(MAX_SAMPLES);
            per_pixel(source, stop, |p| {
                let mut sum = [0.0; 4];
                for i in 0..samples {
                    let along = if samples > 1 { i as f32 / (samples - 1) as f32 } else { 0.0 };
                    let offset = if centered { (along - 0.5) * distance } else { -along * distance };
                    add(&mut sum, source.sample([p[0] + direction[0] * offset, p[1] + direction[1] * offset]), 1.0);
                }
                scale(sum, 1.0 / samples as f32)
            })
        }
        Effect::RadialBlur { angle, .. } if angle.abs() < f32::EPSILON => return Some(None),
        Effect::RadialBlur { angle, center: c } => {
            let angle = angle.to_radians();
            let [cx, cy] = center(c);
            per_pixel(source, stop, |p| {
                let (dx, dy) = (p[0] - cx, p[1] - cy);
                let length = dx.hypot(dy);
                let samples = ((angle * length).ceil() as usize + 1).clamp(1, MAX_SAMPLES);
                let mut sum = [0.0; 4];
                for i in 0..samples {
                    let turn = if samples > 1 { (i as f32 / (samples - 1) as f32 - 0.5) * angle } else { 0.0 };
                    let (sin, cos) = turn.sin_cos();
                    add(&mut sum, source.sample([cx + cos * dx - sin * dy, cy + sin * dx + cos * dy]), 1.0);
                }
                scale(sum, 1.0 / samples as f32)
            })
        }
        Effect::SketchBlur { radius } => per_pixel(source, stop, |p| sketch(source, radius, p)),
        Effect::SquareBlur { radius, .. } | Effect::SurfaceBlur { radius, .. } if radius < 1.0 => return Some(None),
        Effect::SquareBlur { radius, highlights: h } => blur(source, Weighing::Square, radius, highlights(h), stop),
        Effect::SurfaceBlur { radius, threshold } => {
            blur(source, Weighing::Surface { threshold: (threshold / 100.0).max(f32::EPSILON) }, radius, 1.0, stop)
        }
        Effect::ZoomBlur { amount, .. } if amount.abs() < f32::EPSILON => return Some(None),
        Effect::ZoomBlur { amount, center: c } => {
            let amount = amount / 100.0;
            let [cx, cy] = center(c);
            per_pixel(source, stop, |p| {
                let reach = [(cx - p[0]) * amount, (cy - p[1]) * amount];
                let samples = (reach[0].hypot(reach[1]).ceil() as usize + 1).clamp(1, MAX_SAMPLES);
                let mut sum = [0.0; 4];
                for i in 0..samples {
                    let along = if samples > 1 { i as f32 / (samples - 1) as f32 } else { 0.0 };
                    add(&mut sum, source.sample([p[0] + reach[0] * along, p[1] + reach[1] * along]), 1.0);
                }
                scale(sum, 1.0 / samples as f32)
            })
        }
        Effect::Sharpen { amount, .. } if amount.abs() < f32::EPSILON => return Some(None),
        Effect::Sharpen { radius, amount, threshold } => {
            let blurred = if radius < 1.0 { None } else { Some(blur(source, Weighing::Gaussian, radius, 1.0, stop)?) };
            let blurred = blurred.as_ref().unwrap_or(source);
            let (amount, threshold) = (amount / 100.0, threshold / 100.0);
            let mut pixels = vec![[0.0; 4]; source.pixels.len()];
            let width = source.width;
            fill_rows(&mut pixels, width, stop, |y, row| {
                for (x, out) in row.iter_mut().enumerate() {
                    let own = source.pixels[y * width + x];
                    let blur = blurred.pixels[y * width + x];
                    let difference: [f32; 3] = std::array::from_fn(|channel| own[channel] - blur[channel]);
                    *out = if difference.iter().all(|d| d.abs() < threshold) {
                        own
                    } else {
                        let sharp = |channel: usize| (own[channel] + difference[channel] * amount).clamp(0.0, own[3]);
                        [sharp(0), sharp(1), sharp(2), own[3]]
                    };
                }
            })
            .then(|| source.with_pixels(pixels))
        }
    };
    rendered.map(Some)
}

/// How the pixels along a line are weighed in a blur along one axis.
#[derive(Clone, Copy)]
enum Weighing {
    Gaussian,
    Square,
    /// As the Gaussian, less the more any channel is different from the pixel's own,
    /// up to this difference, from 0 to 1, which leaves it out.
    Surface { threshold: f32 },
}

/// Averages a square of pixels a row then a column at a time, which is the same. The
/// colours are raised to the power of `highlights` before, and lowered back after.
fn blur(source: &Buffer, weighing: Weighing, radius: f32, highlights: f32, stop: &AtomicBool) -> Option<Buffer> {
    let across = blur_rows(source, weighing, radius, highlights, 1.0, stop)?;
    Some(blur_rows(&across.transposed(), weighing, radius, 1.0, highlights, stop)?.transposed())
}

/// Averages the pixels along each row, as far as `radius` on either side. The colours
/// are raised to the power of `raise` before, and to that of one over `lower` after.
fn blur_rows(source: &Buffer, weighing: Weighing, radius: f32, raise: f32, lower: f32, stop: &AtomicBool) -> Option<Buffer> {
    let width = source.width;
    let reach = radius.max(0.0) as usize;
    let sigma = (radius / 2.0).max(0.5);
    let weights: Vec<f32> = (0..=2 * reach)
        .map(|i| {
            let offset = i as f32 - reach as f32;
            match weighing {
                Weighing::Square => 1.0,
                _ => (-offset * offset / (2.0 * sigma * sigma)).exp(),
            }
        })
        .collect();
    let weights_total: f32 = weights.iter().sum();
    let mut pixels = vec![[0.0; 4]; source.pixels.len()];
    let done = fill_rows(&mut pixels, width, stop, |y, row| {
        let line = &source.pixels[y * width..(y + 1) * width];
        let padded: Vec<[f32; 4]> =
            (0..width + 2 * reach).map(|i| raised(line[i.saturating_sub(reach).min(width - 1)], raise)).collect();
        for (x, out) in row.iter_mut().enumerate() {
            let window = &padded[x..=x + 2 * reach];
            let mut sum = [0.0; 4];
            let mut total = weights_total;
            if let Weighing::Surface { threshold } = weighing {
                let middle = line[x];
                total = 0.0;
                for (&color, &weight) in window.iter().zip(&weights) {
                    let most = (0..4).map(|channel| (color[channel] - middle[channel]).abs()).fold(0.0, f32::max);
                    let weight = weight * (1.0 - most / threshold).clamp(0.0, 1.0);
                    add(&mut sum, color, weight);
                    total += weight;
                }
            } else {
                for (&color, &weight) in window.iter().zip(&weights) {
                    add(&mut sum, color, weight);
                }
            }
            *out = lowered(scale(sum, 1.0 / total), lower);
        }
    });
    done.then(|| source.with_pixels(pixels))
}

/// Averages a disc of pixels, each row of it worked out from the sums of the row it
/// is on up to each pixel, raised to the power of `highlights` before and lowered
/// back after.
fn bokeh(source: &Buffer, radius: f32, highlights: f32, stop: &AtomicBool) -> Option<Buffer> {
    let source = source.with_pixels(source.pixels.iter().map(|&color| raised(color, highlights)).collect());
    let (width, height) = (source.width, source.height);
    let reach = radius.max(0.0) as usize;
    let half_widths = disc(radius);
    let count: usize = half_widths.iter().map(|half| half * 2 + 1).sum();
    let span = 2 * reach + 1;
    // Each row holds the sums up to each pixel, with as many pixels again past either edge as reach.
    let sums_width = width + 2 * reach + 1;
    let mut pixels = vec![[0.0; 4]; source.pixels.len()];
    fill_bands(&mut pixels, width, |first, band| {
        let mut sums = vec![[0.0f32; 4]; span * sums_width];
        let sum_row = |sums: &mut [[f32; 4]], y: isize| {
            let slot = y.rem_euclid(span as isize) as usize;
            let row = y.clamp(0, height as isize - 1) as usize;
            let line = &source.pixels[row * width..(row + 1) * width];
            let slot = &mut sums[slot * sums_width..(slot + 1) * sums_width];
            let mut total = [0.0; 4];
            slot[0] = total;
            for i in 0..width + 2 * reach {
                add(&mut total, line[i.saturating_sub(reach).min(width - 1)], 1.0);
                slot[i + 1] = total;
            }
        };
        for (i, row) in band.chunks_mut(width).enumerate() {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            let y = (first + i) as isize;
            let reach = reach as isize;
            if i == 0 {
                for row in y - reach..=y + reach {
                    sum_row(&mut sums, row);
                }
            } else {
                sum_row(&mut sums, y + reach);
            }
            for (x, out) in row.iter_mut().enumerate() {
                let mut sum = [0.0; 4];
                for (dy, &half) in (-reach..=reach).zip(&half_widths) {
                    let slot = (y + dy).rem_euclid(span as isize) as usize * sums_width;
                    let middle = x + reach as usize;
                    add(&mut sum, sums[slot + middle + half + 1], 1.0);
                    add(&mut sum, sums[slot + middle - half], -1.0);
                }
                *out = lowered(scale(sum, 1.0 / count as f32), highlights);
            }
        }
    });
    (!stop.load(Ordering::Relaxed)).then(|| source.with_pixels(pixels))
}

/// How far either side of the middle each row of a disc of pixels goes, from the top
/// row to the bottom one. Its edge is rounder for going half a pixel further than the
/// radius.
fn disc(radius: f32) -> Vec<usize> {
    let reach = radius.max(0.0) as isize;
    let limit = radius * radius + radius;
    (-reach..=reach)
        .map(|dy| {
            let left = limit - (dy * dy) as f32;
            let mut half = left.max(0.0).sqrt() as usize;
            while (half * half) as f32 > left {
                half -= 1;
            }
            half
        })
        .collect()
}

/// Each channel's value that this share, from 0 to 1, of the pixels within the
/// radius are no lighter than, kept as counts of each value the pixels along a row
/// have, which change as the disc moves along it. The colours are those of the pixels
/// that can be seen, so see-through ones do not darken those beside them.
fn median(image: &RgbaImage, radius: f32, percentile: f32, stop: &AtomicBool) -> Option<RgbaImage> {
    let (width, height) = (image.width() as usize, image.height() as usize);
    let reach = radius.max(0.0) as isize;
    let half_widths = disc(radius);
    let total: u32 = half_widths.iter().map(|&half| half as u32 * 2 + 1).sum();
    let wanted = |counted: u32| (percentile * (counted.max(1) - 1) as f32 + 0.5).floor() as u32 + 1;
    let pixel = |x: isize, y: isize| {
        image.get_pixel(x.clamp(0, width as isize - 1) as u32, y.clamp(0, height as isize - 1) as u32).0
    };
    let mut pixels = vec![[0u8; 4]; width * height];
    let done = fill_rows(&mut pixels, width, stop, |y, row| {
        let y = y as isize;
        let mut counts = Counts { values: Box::new([[0; 256]; 4]), seen: 0 };
        for (dy, &half) in (-reach..=reach).zip(&half_widths) {
            for dx in -(half as isize)..=half as isize {
                counts.tally(pixel(dx, y + dy), true);
            }
        }
        for (x, out) in row.iter_mut().enumerate() {
            let x = x as isize;
            if x > 0 {
                for (dy, &half) in (-reach..=reach).zip(&half_widths) {
                    counts.tally(pixel(x - 1 - half as isize, y + dy), false);
                    counts.tally(pixel(x + half as isize, y + dy), true);
                }
            }
            let alpha = counts.find(3, wanted(total));
            *out = if alpha == 0 {
                [0; 4]
            } else {
                let wanted = wanted(counts.seen);
                [counts.find(0, wanted), counts.find(1, wanted), counts.find(2, wanted), alpha]
            };
        }
    });
    done.then(|| RgbaImage::from_raw(width as u32, height as u32, pixels.into_flattened()).unwrap())
}

/// How many pixels of a disc have each value of red, green, blue and alpha, the
/// colours only of those that can be seen, and how many those are.
struct Counts {
    values: Box<[[u32; 256]; 4]>,
    seen: u32,
}

impl Counts {
    /// Counts a pixel in, or out.
    fn tally(&mut self, pixel: [u8; 4], add: bool) {
        let channels = if pixel[3] > 0 { 0..4 } else { 3..4 };
        for channel in channels {
            let count = &mut self.values[channel][pixel[channel] as usize];
            *count = if add { *count + 1 } else { *count - 1 };
        }
        if pixel[3] > 0 {
            self.seen = if add { self.seen + 1 } else { self.seen - 1 };
        }
    }

    /// The lowest value of the channel that this many pixels are at or under, or the
    /// highest if there are not that many.
    fn find(&self, channel: usize, wanted: u32) -> u8 {
        let mut under = 0;
        for (value, &count) in self.values[channel].iter().enumerate() {
            under += count;
            if under >= wanted {
                return value as u8;
            }
        }
        255
    }
}

/// Averages a short line of pixels, whose way drifts slowly across the image, as
/// coarse brush strokes would. The line is moved a little sideways, by as much for all
/// the pixels of a bristle, which runs along the stroke.
fn sketch(source: &Buffer, radius: f32, p: [f32; 2]) -> [f32; 4] {
    let cell = (radius * 2.0).max(4.0);
    let place = [p[0] / cell, p[1] / cell];
    let corner = place.map(f32::floor);
    let within = [0, 1].map(|axis| smoothstep(place[axis] - corner[axis]));
    let way_at = |dx: f32, dy: f32| stroke_way([corner[0] + dx, corner[1] + dy]);
    let mix = |a: [f32; 2], b: [f32; 2], t: f32| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
    let way = mix(
        mix(way_at(0.0, 0.0), way_at(1.0, 0.0), within[0]),
        mix(way_at(0.0, 1.0), way_at(1.0, 1.0), within[0]),
        within[1],
    );
    let turn = way[1].atan2(way[0]) / 2.0;
    let direction = [turn.cos(), turn.sin()];
    let across = [-direction[1], direction[0]];
    let bristle = ((p[0] * across[0] + p[1] * across[1]) / 1.5).floor();
    let shift = (hash([bristle, corner[0] + corner[1] * 101.0]) - 0.5) * radius * 0.5;
    let start = [p[0] + across[0] * shift, p[1] + across[1] * shift];
    let reach = radius.ceil() as i32;
    let mut sum = [0.0; 4];
    for i in -reach..=reach {
        let i = i as f32;
        add(&mut sum, source.sample([start[0] + direction[0] * i, start[1] + direction[1] * i]), 1.0);
    }
    scale(sum, 1.0 / (reach * 2 + 1) as f32)
}

fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn hash(cell: [f32; 2]) -> f32 {
    let value = (cell[0] * 12.9898 + cell[1] * 78.233).sin() * 43758.547;
    value - value.floor()
}

/// The way the strokes go around a corner of the Sketch Blur's grid, as twice its
/// angle: a line is the same whichever way along it is gone, and so is a line half a
/// turn round.
fn stroke_way(corner: [f32; 2]) -> [f32; 2] {
    let turn = hash(corner) * TAU;
    [turn.cos(), turn.sin()]
}

/// Works out each pixel from its middle, in pixels of the image.
fn per_pixel(source: &Buffer, stop: &AtomicBool, color: impl Fn([f32; 2]) -> [f32; 4] + Sync) -> Option<Buffer> {
    let mut pixels = vec![[0.0; 4]; source.pixels.len()];
    let done = fill_rows(&mut pixels, source.width, stop, |y, row| {
        for (x, out) in row.iter_mut().enumerate() {
            *out = color([x as f32 + 0.5, y as f32 + 0.5]);
        }
    });
    done.then(|| source.with_pixels(pixels))
}

/// Fills `items`, rows of `width` of them one after the other, each row from its
/// index, giving up between rows once `stop` is set. Returns whether it went through.
fn fill_rows<T: Send>(items: &mut [T], width: usize, stop: &AtomicBool, fill: impl Fn(usize, &mut [T]) + Sync) -> bool {
    fill_bands(items, width, |first, band| {
        for (index, row) in band.chunks_mut(width).enumerate() {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            fill(first + index, row);
        }
    });
    !stop.load(Ordering::Relaxed)
}

/// Fills `items`, rows of `width` of them one after the other, a band of rows on each
/// thread for many of them, each band from the index of its first row.
fn fill_bands<T: Send>(items: &mut [T], width: usize, fill: impl Fn(usize, &mut [T]) + Sync) {
    let rows = items.len() / width;
    let threads = thread::available_parallelism().map_or(1, NonZero::get);
    if threads == 1 || items.len() < PARALLEL_PIXELS {
        fill(0, items);
        return;
    }
    let band = rows.div_ceil(threads);
    let fill = &fill;
    thread::scope(|scope| {
        for (index, items) in items.chunks_mut(band * width).enumerate() {
            scope.spawn(move || fill(index * band, items));
        }
    });
}

/// Colours from 0 to 1, multiplied by how opaque they are, row after row.
struct Buffer {
    width: usize,
    height: usize,
    pixels: Vec<[f32; 4]>,
}

impl Buffer {
    fn premultiplied(image: &RgbaImage) -> Self {
        let pixels = image
            .pixels()
            .map(|pixel| {
                let [r, g, b, a] = pixel.0.map(|channel| channel as f32 / 255.0);
                [r * a, g * a, b * a, a]
            })
            .collect();
        Self { width: image.width() as usize, height: image.height() as usize, pixels }
    }

    fn with_pixels(&self, pixels: Vec<[f32; 4]>) -> Self {
        Self { width: self.width, height: self.height, pixels }
    }

    /// The colour at a point of the image, read between the four pixels around it.
    fn sample(&self, [x, y]: [f32; 2]) -> [f32; 4] {
        let along = |at: f32, size: usize| {
            let at = at.clamp(0.5, size as f32 - 0.5) - 0.5;
            let below = (at.floor() as usize).min(size - 1);
            (below, (below + 1).min(size - 1), at - below as f32)
        };
        let (left, right, tx) = along(x, self.width);
        let (top, bottom, ty) = along(y, self.height);
        let at = |x: usize, y: usize| self.pixels[y * self.width + x];
        let mut color = [0.0; 4];
        add(&mut color, at(left, top), (1.0 - tx) * (1.0 - ty));
        add(&mut color, at(right, top), tx * (1.0 - ty));
        add(&mut color, at(left, bottom), (1.0 - tx) * ty);
        add(&mut color, at(right, bottom), tx * ty);
        color
    }

    /// Its columns as rows.
    fn transposed(&self) -> Self {
        let mut pixels = Vec::with_capacity(self.pixels.len());
        for x in 0..self.width {
            pixels.extend((0..self.height).map(|y| self.pixels[y * self.width + x]));
        }
        Self { width: self.height, height: self.width, pixels }
    }

    /// The pixels, their colours divided by how opaque they are again. Those that
    /// would be stored as see-through are left without a colour.
    fn to_image(&self) -> RgbaImage {
        let bytes = self
            .pixels
            .iter()
            .flat_map(|&[r, g, b, a]| {
                let color = if a >= MIN_ALPHA { [r / a, g / a, b / a, a] } else { [0.0; 4] };
                color.map(|channel| (channel * 255.0).round().clamp(0.0, 255.0) as u8)
            })
            .collect();
        RgbaImage::from_raw(self.width as u32, self.height as u32, bytes).unwrap()
    }
}

fn add(sum: &mut [f32; 4], color: [f32; 4], weight: f32) {
    for (sum, channel) in sum.iter_mut().zip(color) {
        *sum += channel * weight;
    }
}

fn scale(color: [f32; 4], factor: f32) -> [f32; 4] {
    color.map(|channel| channel * factor)
}

/// The colour with red, green and blue raised to this power, which makes bright ones
/// stand out once averaged.
fn raised([r, g, b, a]: [f32; 4], power: f32) -> [f32; 4] {
    if power == 1.0 { [r, g, b, a] } else { [r.powf(power), g.powf(power), b.powf(power), a] }
}

/// The colour with red, green and blue lowered back from this power.
fn lowered([r, g, b, a]: [f32; 4], power: f32) -> [f32; 4] {
    if power == 1.0 {
        [r, g, b, a]
    } else {
        let lower = |channel: f32| channel.max(0.0).powf(1.0 / power);
        [lower(r), lower(g), lower(b), a]
    }
}

#[cfg(test)]
mod tests {
    use image::Rgba;

    use super::*;
    use crate::{PixelRect, Selection};

    const ALL: [Effect; 11] = [
        Effect::BokehBlur { radius: 4.0, highlights: 50.0 },
        Effect::FragmentBlur { count: 4, distance: 3.0, rotation: 10.0 },
        Effect::GaussianBlur { radius: 3.0 },
        Effect::MedianBlur { radius: 3.0, percentile: 50.0 },
        Effect::MotionBlur { angle: 25.0, distance: 6.0, centered: true },
        Effect::RadialBlur { angle: 10.0, center: [0.0, 0.0] },
        Effect::SketchBlur { radius: 4.0 },
        Effect::SquareBlur { radius: 3.0, highlights: 30.0 },
        Effect::SurfaceBlur { radius: 3.0, threshold: 15.0 },
        Effect::ZoomBlur { amount: 20.0, center: [10.0, -20.0] },
        Effect::Sharpen { radius: 3.0, amount: 100.0, threshold: 0.0 },
    ];

    fn apply(pixels: &RgbaImage, effect: &Effect) -> RgbaImage {
        apply_effect(pixels, effect, None, &AtomicBool::new(false)).unwrap()
    }

    /// A white dot in the middle of black.
    fn dot() -> RgbaImage {
        let mut pixels = RgbaImage::from_pixel(21, 21, Rgba([0, 0, 0, 255]));
        pixels.put_pixel(10, 10, Rgba([255, 255, 255, 255]));
        pixels
    }

    #[test]
    fn an_even_colour_stays_as_it_is() {
        for pixels in [RgbaImage::from_pixel(40, 30, Rgba([200, 100, 50, 180])), RgbaImage::new(40, 30)] {
            for effect in &ALL {
                assert_eq!(apply(&pixels, effect), pixels, "{effect:?}");
            }
        }
    }

    #[test]
    fn settings_that_change_nothing_leave_the_pixels_alone() {
        let pixels = dot();
        let unchanged = [
            Effect::GaussianBlur { radius: 0.0 },
            Effect::RadialBlur { angle: 0.0, center: [0.0, 0.0] },
            Effect::ZoomBlur { amount: 0.0, center: [0.0, 0.0] },
            Effect::Sharpen { radius: 3.0, amount: 0.0, threshold: 0.0 },
            Effect::FragmentBlur { count: 4, distance: 0.0, rotation: 0.0 },
            Effect::MotionBlur { angle: 0.0, distance: 0.0, centered: true },
        ];
        for effect in unchanged {
            assert_eq!(apply(&pixels, &effect), pixels, "{effect:?}");
        }
    }

    #[test]
    fn blurs_spread_a_dot_evenly() {
        for effect in [
            Effect::GaussianBlur { radius: 3.0 },
            Effect::SquareBlur { radius: 3.0, highlights: 0.0 },
            Effect::BokehBlur { radius: 3.0, highlights: 0.0 },
        ] {
            let blurred = apply(&dot(), &effect);
            let middle = blurred[(10, 10)][0];
            assert!(middle > 0 && middle < 255, "{effect:?}");
            assert!(blurred[(12, 10)][0] > 0, "{effect:?}");
            assert_eq!(blurred[(12, 10)], blurred[(8, 10)], "{effect:?}");
            assert_eq!(blurred[(12, 10)], blurred[(10, 12)], "{effect:?}");
            assert_eq!(blurred[(0, 0)], Rgba([0, 0, 0, 255]), "{effect:?}");
        }
        let square = apply(&dot(), &Effect::SquareBlur { radius: 1.0, highlights: 0.0 });
        assert_eq!(square[(11, 11)], Rgba([28, 28, 28, 255]), "a ninth of white");
    }

    #[test]
    fn a_line_blur_follows_its_way() {
        let blurred = apply(&dot(), &Effect::MotionBlur { angle: 0.0, distance: 4.0, centered: true });
        assert_eq!(blurred[(12, 10)][0], 51, "a fifth of white");
        assert_eq!(blurred[(10, 12)][0], 0);
        let behind = apply(&dot(), &Effect::MotionBlur { angle: 0.0, distance: 4.0, centered: false });
        // Each pixel averages those behind it, on its left, so the dot spreads to the right.
        assert!(behind[(12, 10)][0] > 0);
        assert_eq!(behind[(8, 10)][0], 0);
    }

    #[test]
    fn see_through_pixels_do_not_darken_their_neighbours() {
        let mut pixels = RgbaImage::new(9, 9);
        for x in 0..4 {
            for y in 0..9 {
                pixels.put_pixel(x, y, Rgba([255, 0, 0, 255]));
            }
        }
        for effect in [Effect::GaussianBlur { radius: 2.0 }, Effect::MedianBlur { radius: 2.0, percentile: 50.0 }] {
            let blurred = apply(&pixels, &effect);
            let edge = blurred[(4, 4)];
            assert!(edge[3] == 0 || edge.0[..3] == [255, 0, 0], "{effect:?}: {edge:?}");
            assert_eq!(blurred[(3, 4)].0[..3], [255, 0, 0], "{effect:?}");
        }
    }

    #[test]
    fn a_median_takes_out_specks() {
        assert_eq!(apply(&dot(), &Effect::MedianBlur { radius: 1.0, percentile: 50.0 })[(10, 10)], Rgba([0, 0, 0, 255]));
        let lightest = apply(&dot(), &Effect::MedianBlur { radius: 1.0, percentile: 100.0 });
        assert_eq!(lightest[(11, 11)], Rgba([255, 255, 255, 255]));
        assert_eq!(lightest[(12, 12)], Rgba([0, 0, 0, 255]));
    }

    #[test]
    fn sharpening_brings_out_the_dot() {
        let mut pixels = RgbaImage::from_pixel(21, 21, Rgba([100, 100, 100, 255]));
        pixels.put_pixel(10, 10, Rgba([150, 150, 150, 255]));
        let sharp = apply(&pixels, &Effect::Sharpen { radius: 2.0, amount: 100.0, threshold: 0.0 });
        assert!(sharp[(10, 10)][0] > 150);
        assert!(sharp[(11, 10)][0] < 100);
        assert_eq!(sharp[(0, 0)], Rgba([100, 100, 100, 255]));
        let kept = apply(&pixels, &Effect::Sharpen { radius: 2.0, amount: 100.0, threshold: 50.0 });
        assert_eq!(kept, pixels, "no difference reaches the threshold");
    }

    #[test]
    fn only_the_selection_changes() {
        let pixels = RgbaImage::from_fn(200, 200, |x, y| Rgba([(x % 2 * 255) as u8, (y % 2 * 255) as u8, 0, 255]));
        let selection = Selection::rect(PixelRect { left: 10, top: 150, right: 20, bottom: 160 });
        let mask = SelectionMask::of(&selection, 200, 200).unwrap();
        for effect in &ALL {
            let changed = apply_effect(&pixels, effect, Some(&mask), &AtomicBool::new(false)).unwrap();
            assert_eq!(changed[(5, 155)], pixels[(5, 155)], "{effect:?}");
            assert_eq!(changed[(15, 100)], pixels[(15, 100)], "{effect:?}");
        }
    }

    #[test]
    fn gives_up_once_stopped() {
        let pixels = RgbaImage::from_pixel(300, 300, Rgba([1, 2, 3, 255]));
        for effect in &ALL {
            assert!(apply_effect(&pixels, effect, None, &AtomicBool::new(true)).is_none(), "{effect:?}");
        }
    }
}

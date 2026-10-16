//! The blurs, the Sharpen and the Offset of the Filters menu, which, unlike the adjustments, look at
//! the pixels around each one, as `effect.gdshader` does on the GPU in the Godot
//! version. The colours are multiplied by how opaque they are while they are mixed, so
//! see-through pixels add nothing to those around them, and divided out again at the
//! end. Past its edges, the image goes on as its edge pixels. Points between pixels are
//! read between the four around them. Large images are cut into bands of rows, worked
//! out at the same time on as many threads as there are cores.

use std::{
    f32::consts::TAU,
    num::NonZero,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use image::{Rgba, RgbaImage};

use crate::{PixelRect, SelectionMask};

/// Fewest pixels worth sharing between threads.
const PARALLEL_PIXELS: usize = 1 << 14;
/// The power the Bokeh Blur and the Square Blur raise colours to at full highlights.
const MAX_HIGHLIGHTS: f32 = 3.0;
/// Most points the blurs that follow a line or an arc take, however long it is.
const MAX_SAMPLES: usize = 256;
/// Least opacity a pixel keeps its colour with: those under it are stored as see-through.
const MIN_ALPHA: f32 = 0.5 / 255.0;
/// How much of red, green and blue makes up how light a colour looks.
const LUMA: [f32; 3] = [0.299, 0.587, 0.114];
/// How many box blurs, one after the other, the blurs whose radius changes from pixel
/// to pixel take along each side: three are much like a Gaussian Blur.
const BOX_PASSES: usize = 3;

/// An effect, with its settings: lengths in pixels, angles in degrees anticlockwise from
/// the right, and shares in percent. A centre is from -100, the left or top edge, to
/// 100, the right or bottom one, 0 being the middle of the image.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// Averages a disc of pixels, as an out-of-focus lens does. The highlights, from 0
    /// to 100, make bright colours stand out.
    BokehBlur { radius: f32, highlights: f32 },
    /// Blurs more the further from the centre, as a camera focused on it does: not at
    /// all within `size` of it, a share of half the image's longer side, measured as
    /// the shape says, then more and more over `transition` further, up to `radius`.
    FocusBlur { radius: f32, shape: FocusShape, center: [f32; 2], size: f32, transition: f32 },
    /// Averages copies of the image moved the same distance in directions evenly
    /// around a circle.
    FragmentBlur { count: u32, distance: f32, rotation: f32 },
    GaussianBlur { radius: f32 },
    /// Smooths the colours along the edges between them rather than across them, a
    /// little more with each step, which takes out noise and specks but keeps the
    /// edges sharp.
    MeanCurvatureBlur { iterations: u32 },
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
    /// A Gaussian Blur within the rectangle around the selected pixels, or the whole
    /// image, that goes on past each of its edges with the pixels of the other, as on a
    /// tile repeated side by side, so they still tile once blurred.
    TileableBlur { radius: f32 },
    /// Blurs each pixel by up to `radius`, as a Gaussian Blur does, as much as the map
    /// is light there and can be seen: not at all where it is black or see-through, or
    /// the other way round if inverted. The map is as large as the pixels.
    VariableBlur { radius: f32, map: Arc<RgbaImage>, invert: bool },
    /// Averages a line of pixels towards the centre, this much of the way, as a camera
    /// that zooms while the picture is taken does.
    ZoomBlur { amount: f32, center: [f32; 2] },
    /// Takes each colour further from the blurred one, by this much again, which brings
    /// out edges and details, as an unsharp mask does, unless it is closer to it than
    /// the threshold. How opaque each pixel is stays the same.
    Sharpen { radius: f32, amount: f32, threshold: f32 },
    /// Moves the pixels right and down by this many, or left and up if below zero,
    /// within the rectangle around the selected ones, or the whole image, filling where
    /// they move away from as `edges` says.
    Offset { x: i32, y: i32, edges: OffsetEdges },
}

/// How the Focus Blur measures how far a pixel is from the centre, which gives the
/// shape of the area it leaves sharp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusShape {
    Circle,
    Square,
    Diamond,
    /// A band across the image, as a tilt-shift lens leaves sharp.
    Horizontal,
    /// A band down the image.
    Vertical,
}

impl FocusShape {
    pub const ALL: [Self; 5] = [Self::Circle, Self::Square, Self::Diamond, Self::Horizontal, Self::Vertical];

    /// How far a point is from the centre, `x` across and `y` down.
    fn distance(self, x: f32, y: f32) -> f32 {
        match self {
            Self::Circle => x.hypot(y),
            Self::Square => x.abs().max(y.abs()),
            Self::Diamond => x.abs() + y.abs(),
            Self::Horizontal => y.abs(),
            Self::Vertical => x.abs(),
        }
    }
}

/// What the Offset fills where the pixels move away from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OffsetEdges {
    /// With those moved off the other side, as on a tile repeated side by side.
    Wrap,
    Transparent,
    /// With the pixels along the edge they move away from, drawn out.
    Repeat,
}

impl OffsetEdges {
    pub const ALL: [Self; 3] = [Self::Wrap, Self::Transparent, Self::Repeat];

    pub fn name(self) -> &'static str {
        match self {
            Self::Wrap => "Wrap Around",
            Self::Transparent => "Transparent",
            Self::Repeat => "Repeat Edge Pixels",
        }
    }
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
        Effect::Offset { x, y, edges } => offset(pixels, x, y, edges, mask),
        Effect::TileableBlur { radius } if radius < 1.0 => return Some(pixels.clone()),
        Effect::TileableBlur { radius } => tileable(pixels, radius, mask, stop)?,
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

/// The pixels moved by `x` and `y` within the rectangle around those `mask` holds, or
/// the whole image, the rest filled as `edges` says.
fn offset(pixels: &RgbaImage, x: i32, y: i32, edges: OffsetEdges, mask: Option<&SelectionMask>) -> RgbaImage {
    let whole = PixelRect { left: 0, top: 0, right: pixels.width() as i32, bottom: pixels.height() as i32 };
    let area = mask.and_then(SelectionMask::bounds).unwrap_or(whole);
    let mut result = pixels.clone();
    // Where a pixel of the area comes from, along one side: before the move, if it is in the area.
    let source = |at: i32, by: i32, start: i32, end: i32| {
        let from = at - by;
        match edges {
            OffsetEdges::Wrap => Some(start + (from - start).rem_euclid(end - start)),
            OffsetEdges::Transparent => (start..end).contains(&from).then_some(from),
            OffsetEdges::Repeat => Some(from.clamp(start, end - 1)),
        }
    };
    for row in area.top..area.bottom {
        for column in area.left..area.right {
            let from = source(column, x, area.left, area.right).zip(source(row, y, area.top, area.bottom));
            let pixel = from.map_or(Rgba([0, 0, 0, 0]), |(from_x, from_y)| *pixels.get_pixel(from_x as u32, from_y as u32));
            result.put_pixel(column as u32, row as u32, pixel);
        }
    }
    result
}

/// The pixels Gaussian blurred within the rectangle around those `mask` holds, or the
/// whole image, which goes on past each of its edges with the pixels of the other.
/// Gives up once `stop` is set.
fn tileable(pixels: &RgbaImage, radius: f32, mask: Option<&SelectionMask>, stop: &AtomicBool) -> Option<RgbaImage> {
    let whole = PixelRect { left: 0, top: 0, right: pixels.width() as i32, bottom: pixels.height() as i32 };
    let area = mask.and_then(SelectionMask::bounds).unwrap_or(whole);
    let (width, height) = (area.width() as usize, area.height() as usize);
    // The area, with as far around it as the blur reaches, from the other side.
    let margin = radius.max(0.0) as usize;
    let (tile_width, tile_height) = (width + 2 * margin, height + 2 * margin);
    let wrapped = |at: usize, size: usize| (at as isize - margin as isize).rem_euclid(size as isize) as u32;
    let tile = Buffer {
        width: tile_width,
        height: tile_height,
        pixels: (0..tile_height)
            .flat_map(|y| {
                (0..tile_width).map(move |x| {
                    premultiply(*pixels.get_pixel(area.left as u32 + wrapped(x, width), area.top as u32 + wrapped(y, height)))
                })
            })
            .collect(),
    };
    let blurred = blur(&tile, Weighing::Gaussian, radius, 1.0, stop)?.to_image();
    let mut result = pixels.clone();
    for y in 0..height as u32 {
        for x in 0..width as u32 {
            let pixel = *blurred.get_pixel(x + margin as u32, y + margin as u32);
            result.put_pixel(area.left as u32 + x, area.top as u32 + y, pixel);
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
        Effect::FocusBlur { radius, .. } | Effect::VariableBlur { radius, .. } if radius <= 0.0 => return Some(None),
        Effect::FocusBlur { radius, shape, center: c, size: sharp, transition } => {
            let [cx, cy] = center(c);
            let unit = size[0].max(size[1]) / 2.0;
            let (sharp, transition) = (sharp / 100.0, transition / 100.0);
            let radii = (0..source.height)
                .flat_map(|y| {
                    (0..source.width).map(move |x| {
                        let distance = shape.distance((x as f32 + 0.5 - cx) / unit, (y as f32 + 0.5 - cy) / unit);
                        let past = distance - sharp;
                        let amount = if transition > 0.0 { smoothstep((past / transition).clamp(0.0, 1.0)) } else { (past > 0.0) as u8 as f32 };
                        radius * amount
                    })
                })
                .collect();
            variable_blur(source, radii, stop)
        }
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
        Effect::MeanCurvatureBlur { iterations: 0 } => return Some(None),
        Effect::MeanCurvatureBlur { iterations } => mean_curvature(source, iterations, stop),
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
        Effect::TileableBlur { .. } => unreachable!("worked out within the selection"),
        Effect::VariableBlur { radius, ref map, invert } => {
            let radii = (0..source.height as u32)
                .flat_map(|y| {
                    (0..source.width as u32).map(move |x| {
                        let light = map.get_pixel_checked(x, y).map_or(0.0, |&Rgba([r, g, b, a])| {
                            let luma = LUMA[0] * r as f32 + LUMA[1] * g as f32 + LUMA[2] * b as f32;
                            luma / 255.0 * a as f32 / 255.0
                        });
                        radius * if invert { 1.0 - light } else { light }
                    })
                })
                .collect();
            variable_blur(source, radii, stop)
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
        Effect::Offset { .. } => return Some(None),
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

/// Blurs each pixel by its own radius, from `radii`, row after row, much as a Gaussian
/// Blur of it would: a few box blurs along each row, one after the other, then as many
/// along each column.
fn variable_blur(source: &Buffer, radii: Vec<f32>, stop: &AtomicBool) -> Option<Buffer> {
    // How far either side each box goes for the boxes to spread the colours as far as
    // the Gaussian Blur does, whose spread is half its radius: a box as wide as `w`
    // spreads them by the root of (w² - 1) / 12.
    let passes = BOX_PASSES as f32;
    let halves: Vec<f32> = radii.iter().map(|&radius| ((3.0 * radius * radius / passes + 1.0).sqrt() - 1.0) / 2.0).collect();
    let mut buffer = box_rows(source, &halves, stop)?;
    for _ in 1..BOX_PASSES {
        buffer = box_rows(&buffer, &halves, stop)?;
    }
    let halves = Buffer::transposed_items(&halves, source.width, source.height);
    let mut buffer = buffer.transposed();
    for _ in 0..BOX_PASSES {
        buffer = box_rows(&buffer, &halves, stop)?;
    }
    Some(buffer.transposed())
}

/// Averages the pixels along each row, as far either side of each as `halves` says
/// for it, a share of a pixel past the last whole one counting for as much of it,
/// worked out from the sums of the row up to each pixel.
fn box_rows(source: &Buffer, halves: &[f32], stop: &AtomicBool) -> Option<Buffer> {
    let width = source.width;
    let mut pixels = vec![[0.0; 4]; source.pixels.len()];
    let done = fill_rows(&mut pixels, width, stop, |y, row| {
        let line = &source.pixels[y * width..(y + 1) * width];
        let halves = &halves[y * width..(y + 1) * width];
        // The row goes on as its edge pixels as far as the widest box reaches, and a pixel more.
        let reach = halves.iter().copied().fold(0.0, f32::max) as usize + 1;
        let at = |i: usize| line[i.saturating_sub(reach).min(width - 1)];
        let mut sums = Vec::with_capacity(width + 2 * reach + 1);
        let mut total = [0.0f64; 4];
        sums.push(total);
        for i in 0..width + 2 * reach {
            for (total, channel) in total.iter_mut().zip(at(i)) {
                *total += channel as f64;
            }
            sums.push(total);
        }
        for (x, out) in row.iter_mut().enumerate() {
            let half = halves[x];
            if half <= 0.0 {
                *out = line[x];
                continue;
            }
            let whole = half as usize;
            let part = half - whole as f32;
            let (first, last) = (x + reach - whole, x + reach + whole);
            let mut sum: [f32; 4] = std::array::from_fn(|channel| (sums[last + 1][channel] - sums[first][channel]) as f32);
            if part > 0.0 {
                add(&mut sum, at(first - 1), part);
                add(&mut sum, at(last + 1), part);
            }
            *out = scale(sum, 1.0 / ((2 * whole + 1) as f32 + 2.0 * part));
        }
    });
    done.then(|| source.with_pixels(pixels))
}

/// Moves each colour along the curve of the edge it is on, `iterations` times, which
/// smooths it along the edge but not across: the change at each step is a quarter of
/// how steep the colour is times how much its edge bends, as GIMP's does.
fn mean_curvature(source: &Buffer, iterations: u32, stop: &AtomicBool) -> Option<Buffer> {
    let (width, height) = (source.width as isize, source.height as isize);
    let mut current = source.pixels.clone();
    for _ in 0..iterations {
        let mut next = vec![[0.0; 4]; current.len()];
        let previous = &current;
        let at = |x: isize, y: isize| previous[(y.clamp(0, height - 1) * width + x.clamp(0, width - 1)) as usize];
        let done = fill_rows(&mut next, source.width, stop, |y, row| {
            let y = y as isize;
            for (x, out) in row.iter_mut().enumerate() {
                let x = x as isize;
                let [top_left, top, top_right] = [-1, 0, 1].map(|dx| at(x + dx, y - 1));
                let [left, middle, right] = [-1, 0, 1].map(|dx| at(x + dx, y));
                let [bottom_left, bottom, bottom_right] = [-1, 0, 1].map(|dx| at(x + dx, y + 1));
                let mut color: [f32; 4] = std::array::from_fn(|c| {
                    let (dx, dy) = (right[c] - left[c], bottom[c] - top[c]);
                    let steepness = dx * dx + dy * dy;
                    if steepness < f32::EPSILON {
                        return middle[c];
                    }
                    let dxx = right[c] + left[c] - 2.0 * middle[c];
                    let dyy = bottom[c] + top[c] - 2.0 * middle[c];
                    let dxy = 0.25 * (bottom_right[c] - top_right[c] - bottom_left[c] + top_left[c]);
                    middle[c] + 0.25 * (dx * dx * dyy + dy * dy * dxx - 2.0 * dx * dy * dxy) / steepness
                });
                let alpha = color[3].clamp(0.0, 1.0);
                color[3] = alpha;
                for channel in &mut color[..3] {
                    *channel = channel.clamp(0.0, alpha);
                }
                *out = color;
            }
        });
        if !done {
            return None;
        }
        current = next;
    }
    Some(source.with_pixels(current))
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
        let pixels = image.pixels().map(|&pixel| premultiply(pixel)).collect();
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
        Self { width: self.height, height: self.width, pixels: Self::transposed_items(&self.pixels, self.width, self.height) }
    }

    /// The columns of `items`, rows of `width` of them one after the other, as rows.
    fn transposed_items<T: Copy>(items: &[T], width: usize, height: usize) -> Vec<T> {
        let mut transposed = Vec::with_capacity(items.len());
        for x in 0..width {
            transposed.extend((0..height).map(|y| items[y * width + x]));
        }
        transposed
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

/// The colour from 0 to 1, multiplied by how opaque it is.
fn premultiply(pixel: Rgba<u8>) -> [f32; 4] {
    let [r, g, b, a] = pixel.0.map(|channel| channel as f32 / 255.0);
    [r * a, g * a, b * a, a]
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

    /// One of each effect, with settings that change something.
    fn all() -> Vec<Effect> {
        vec![
            Effect::BokehBlur { radius: 4.0, highlights: 50.0 },
            Effect::FocusBlur { radius: 6.0, shape: FocusShape::Circle, center: [10.0, 0.0], size: 10.0, transition: 40.0 },
            Effect::FragmentBlur { count: 4, distance: 3.0, rotation: 10.0 },
            Effect::GaussianBlur { radius: 3.0 },
            Effect::MeanCurvatureBlur { iterations: 5 },
            Effect::MedianBlur { radius: 3.0, percentile: 50.0 },
            Effect::MotionBlur { angle: 25.0, distance: 6.0, centered: true },
            Effect::RadialBlur { angle: 10.0, center: [0.0, 0.0] },
            Effect::SketchBlur { radius: 4.0 },
            Effect::SquareBlur { radius: 3.0, highlights: 30.0 },
            Effect::SurfaceBlur { radius: 3.0, threshold: 15.0 },
            Effect::TileableBlur { radius: 3.0 },
            Effect::VariableBlur { radius: 4.0, map: white_map(300, 300), invert: false },
            Effect::ZoomBlur { amount: 20.0, center: [10.0, -20.0] },
            Effect::Sharpen { radius: 3.0, amount: 100.0, threshold: 0.0 },
        ]
    }

    fn white_map(width: u32, height: u32) -> Arc<RgbaImage> {
        Arc::new(RgbaImage::from_pixel(width, height, Rgba([255, 255, 255, 255])))
    }

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
            for effect in &all() {
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
            Effect::MeanCurvatureBlur { iterations: 0 },
            Effect::TileableBlur { radius: 0.0 },
            Effect::VariableBlur { radius: 5.0, map: Arc::new(RgbaImage::new(21, 21)), invert: false },
            Effect::VariableBlur { radius: 5.0, map: white_map(21, 21), invert: true },
            Effect::FocusBlur { radius: 5.0, shape: FocusShape::Square, center: [0.0, 0.0], size: 100.0, transition: 0.0 },
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
        for effect in &all() {
            let changed = apply_effect(&pixels, effect, Some(&mask), &AtomicBool::new(false)).unwrap();
            assert_eq!(changed[(5, 155)], pixels[(5, 155)], "{effect:?}");
            assert_eq!(changed[(15, 100)], pixels[(15, 100)], "{effect:?}");
        }
    }

    #[test]
    fn gives_up_once_stopped() {
        let pixels = RgbaImage::from_pixel(300, 300, Rgba([1, 2, 3, 255]));
        for effect in &all() {
            assert!(apply_effect(&pixels, effect, None, &AtomicBool::new(true)).is_none(), "{effect:?}");
        }
    }

    #[test]
    fn offset_moves_the_pixels_within_the_selection() {
        // A row of four colours, with a pixel either side left out of the selection.
        let colors = [10, 20, 30, 40, 50, 60].map(|value| Rgba([value, 0, 0, 255]));
        let pixels = RgbaImage::from_fn(6, 1, |x, _| colors[x as usize]);
        let mask = SelectionMask::of(&Selection::rect(PixelRect { left: 1, top: 0, right: 5, bottom: 1 }), 6, 1);
        let reds = |effect: Effect| -> Vec<u8> {
            let result = apply_effect(&pixels, &effect, mask.as_ref(), &AtomicBool::new(false)).unwrap();
            result.pixels().map(|pixel| if pixel[3] == 0 { 0 } else { pixel[0] }).collect()
        };
        assert_eq!(reds(Effect::Offset { x: 1, y: 0, edges: OffsetEdges::Wrap }), [10, 50, 20, 30, 40, 60]);
        assert_eq!(reds(Effect::Offset { x: -1, y: 0, edges: OffsetEdges::Transparent }), [10, 30, 40, 50, 0, 60]);
        assert_eq!(reds(Effect::Offset { x: 2, y: 0, edges: OffsetEdges::Repeat }), [10, 20, 20, 20, 30, 60]);
        // A whole turn leaves it as it was.
        assert_eq!(reds(Effect::Offset { x: 4, y: 0, edges: OffsetEdges::Wrap }), [10, 20, 30, 40, 50, 60]);
    }

    #[test]
    fn a_variable_blur_with_an_even_map_is_much_as_a_gaussian_blur() {
        let gaussian = apply(&dot(), &Effect::GaussianBlur { radius: 6.0 });
        let variable = apply(&dot(), &Effect::VariableBlur { radius: 6.0, map: white_map(21, 21), invert: false });
        for (a, b) in gaussian.pixels().zip(variable.pixels()) {
            assert!(a[0].abs_diff(b[0]) <= 3, "{a:?} {b:?}");
        }
        assert_eq!(variable[(12, 10)], variable[(10, 12)]);
    }

    #[test]
    fn a_variable_blur_blurs_only_where_the_map_is_light() {
        let mut pixels = RgbaImage::from_pixel(41, 21, Rgba([0, 0, 0, 255]));
        pixels.put_pixel(5, 10, Rgba([255, 255, 255, 255]));
        pixels.put_pixel(35, 10, Rgba([255, 255, 255, 255]));
        // Light on the right half only.
        let map = Arc::new(RgbaImage::from_fn(41, 21, |x, _| if x > 20 { Rgba([255; 4]) } else { Rgba([0, 0, 0, 255]) }));
        let blurred = apply(&pixels, &Effect::VariableBlur { radius: 4.0, map: Arc::clone(&map), invert: false });
        assert_eq!(blurred[(5, 10)], pixels[(5, 10)]);
        assert!(blurred[(35, 10)][0] < 255 && blurred[(36, 10)][0] > 0);
        let inverted = apply(&pixels, &Effect::VariableBlur { radius: 4.0, map, invert: true });
        assert_eq!(inverted[(35, 10)], pixels[(35, 10)]);
        assert!(inverted[(5, 10)][0] < 255 && inverted[(6, 10)][0] > 0);
    }

    #[test]
    fn a_focus_blur_leaves_its_centre_sharp() {
        let pixels = RgbaImage::from_fn(60, 60, |x, y| Rgba([((x + y) % 2 * 255) as u8, 0, 0, 255]));
        for shape in FocusShape::ALL {
            let effect = Effect::FocusBlur { radius: 5.0, shape, center: [0.0, 0.0], size: 20.0, transition: 30.0 };
            let blurred = apply(&pixels, &effect);
            assert_eq!(blurred[(30, 30)], pixels[(30, 30)], "{shape:?}");
            let corner = blurred[(1, 1)][0];
            assert!(corner > 80 && corner < 180, "{shape:?}: {corner}");
        }
        // A band across leaves the whole of its middle row sharp.
        let band = apply(&pixels, &Effect::FocusBlur { radius: 5.0, shape: FocusShape::Horizontal, center: [0.0, 0.0], size: 20.0, transition: 30.0 });
        assert_eq!(band[(1, 30)], pixels[(1, 30)]);
    }

    #[test]
    fn a_mean_curvature_blur_rounds_off_specks_but_keeps_straight_edges() {
        let speck = RgbaImage::from_fn(21, 21, |x, y| {
            if (9..12).contains(&x) && (9..12).contains(&y) { Rgba([255, 255, 255, 255]) } else { Rgba([0, 0, 0, 255]) }
        });
        let smoothed = apply(&speck, &Effect::MeanCurvatureBlur { iterations: 10 });
        assert!(smoothed[(9, 9)][0] < 128, "the corners go: {:?}", smoothed[(9, 9)]);
        let light = |pixels: &RgbaImage| pixels.pixels().map(|pixel| pixel[0] as u32).sum::<u32>();
        assert!(light(&smoothed) < light(&speck));
        let edge = RgbaImage::from_fn(20, 20, |x, _| if x < 10 { Rgba([0, 0, 0, 255]) } else { Rgba([255, 255, 255, 255]) });
        assert_eq!(apply(&edge, &Effect::MeanCurvatureBlur { iterations: 10 }), edge);
    }

    #[test]
    fn a_tileable_blur_goes_on_past_each_edge_with_the_other() {
        let mut pixels = RgbaImage::from_pixel(21, 21, Rgba([0, 0, 0, 255]));
        for y in 0..21 {
            pixels.put_pixel(0, y, Rgba([255, 255, 255, 255]));
        }
        let blurred = apply(&pixels, &Effect::TileableBlur { radius: 3.0 });
        assert!(blurred[(20, 5)][0] > 0);
        assert_eq!(blurred[(20, 5)], blurred[(1, 5)]);
        // Within the selection, the far edge of the selection is what goes on past it.
        let selection = Selection::rect(PixelRect { left: 0, top: 0, right: 11, bottom: 21 });
        let mask = SelectionMask::of(&selection, 21, 21).unwrap();
        let within = apply_effect(&pixels, &Effect::TileableBlur { radius: 3.0 }, Some(&mask), &AtomicBool::new(false)).unwrap();
        assert_eq!(within[(10, 5)], blurred[(20, 5)]);
        assert_eq!(within[(20, 5)], pixels[(20, 5)]);
    }
}

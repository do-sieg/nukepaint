//! The effects of the Artistic menu, which make the image look drawn, painted, printed
//! or made of something else. Most of them keep how opaque each pixel is and change
//! only its colour; those that move colours around mix them as the blurs do.

use std::{
    cmp::Reverse,
    collections::BinaryHeap,
    f32::consts::{FRAC_1_SQRT_2, PI},
    sync::atomic::{AtomicBool, Ordering},
};

use image::Rgba;

use super::{Buffer, Effect, LUMA, MIN_ALPHA, Weighing, add, blur, blur_rows, disc, fill_rows, per_pixel, premultiply, scale};

/// How wide a thread of the Apply Canvas's canvas is, in pixels.
const CANVAS_THREAD: usize = 4;
/// How long the fibres along the Weave's ribbons are, in pixels.
const WEAVE_FIBRE: f32 = 16.0;
/// Least darker than around it a pixel is made black by the Cartoon and the Photocopy,
/// however many are to be, so the faint noise of a flat area is not.
const MIN_RAMP: f32 = 0.04;
/// How many pixels the Waterpixels floods between looks at whether to give up.
const FLOOD_CHECK: usize = 1 << 12;

/// The colours, multiplied by how opaque they are, as the artistic effect makes them.
/// Gives up once `stop` is set.
pub(super) fn render(source: &Buffer, effect: &Effect, stop: &AtomicBool) -> Option<Buffer> {
    match *effect {
        Effect::ApplyCanvas { azimuth, depth } => {
            let heights = field(source, canvas_height);
            emboss(source, &heights, azimuth, 45.0, depth / 4.0, stop)
        }
        Effect::Cartoon { radius, black } => cartoon(source, radius, black / 100.0, stop),
        Effect::Clothify { blur, azimuth, elevation, depth } => clothify(source, blur, azimuth, elevation, depth, stop),
        Effect::Cubism { size, saturation, background } => cubism(source, size, saturation, background, stop),
        Effect::GlassTile { width, height } => {
            let middle = |at: f32, size: f32| ((at / size).floor() + 0.5) * size;
            let (width, height) = (width.max(1.0), height.max(1.0));
            per_pixel(source, stop, |[x, y]| source.sample([2.0 * x - middle(x, width), 2.0 * y - middle(y, height)]))
        }
        Effect::Glow { radius, brightness, contrast } => glow(source, radius, brightness / 100.0, contrast / 100.0, stop),
        Effect::InkSketch { ink, coloring } => ink_sketch(source, ink / 100.0, coloring / 100.0, stop),
        Effect::OilPainting { radius, levels } => oil_painting(source, radius, levels, stop),
        Effect::PencilSketch { tip, range } => pencil_sketch(source, tip, range, stop),
        Effect::Photocopy { radius, sharpness, black, white } => {
            photocopy(source, radius, sharpness / 100.0, black / 100.0, white / 100.0, stop)
        }
        Effect::Predator { amount, pixel_size } => predator(source, amount, pixel_size, stop),
        Effect::Slic { size, compactness, iterations } => slic(source, size, compactness, iterations, stop),
        Effect::VanGogh { length, strength } => van_gogh(source, length, strength / 100.0, stop),
        Effect::Waterpixels { size, smoothness, regularization, random } => {
            waterpixels(source, size, smoothness, regularization, random, stop)
        }
        Effect::Weave { width, spacing, darkness, depth, threads } => {
            weave(source, width, spacing, darkness / 100.0, depth / 100.0, threads / 100.0, stop)
        }
        _ => unreachable!("not an artistic effect"),
    }
}

/// Dark lines along the edges, as if drawn in ink, over the colours faded towards
/// white, less the more `coloring` there is, both from 0 to 1.
fn ink_sketch(source: &Buffer, ink: f32, coloring: f32, stop: &AtomicBool) -> Option<Buffer> {
    let (width, height) = (source.width, source.height);
    let light = gaussian(&lightness(source), width, height, 2.0, stop)?;
    let slopes = slopes(&light, width, height);
    let kept = coloring.sqrt();
    each_pixel(source, stop, |x, y| {
        let index = y * width + x;
        let [dx, dy] = slopes[index];
        let line = smooth((dx.hypot(dy) * ink * 20.0 - 0.1).clamp(0.0, 1.0));
        let [r, g, b, alpha] = straight(source.pixels[index]);
        let paper = |channel: f32| (1.0 + (channel - 1.0) * kept) * (1.0 - line);
        opaque_as([paper(r), paper(g), paper(b)], alpha)
    })
}

/// Grey pencil lines where the image is darker than around it, as far as the tip
/// reaches, on white: each pixel's lightness divided by that of the blurred image,
/// darker or lighter as `range`, from -20 to 20, says.
fn pencil_sketch(source: &Buffer, tip: f32, range: f32, stop: &AtomicBool) -> Option<Buffer> {
    let (width, height) = (source.width, source.height);
    let light = lightness(source);
    let around = gaussian(&light, width, height, tip * 2.0 + 1.0, stop)?;
    let power = 2.0 * 2f32.powf(-range / 10.0);
    each_pixel(source, stop, |x, y| {
        let index = y * width + x;
        let ratio = if around[index] > f32::EPSILON { (light[index] / around[index]).min(1.0) } else { 1.0 };
        opaque_as([ratio.powf(power); 3], source.pixels[index][3])
    })
}

/// Each pixel the average of those within the radius whose lightness is the most
/// common there, the lightness cut into `levels` steps: fewer make broader strokes.
fn oil_painting(source: &Buffer, radius: f32, levels: u32, stop: &AtomicBool) -> Option<Buffer> {
    let (width, height) = (source.width, source.height);
    let levels = levels.clamp(2, 256) as usize;
    let steps: Vec<u8> = source.pixels.iter().map(|&color| (luma(straight(color)) * (levels - 1) as f32).round() as u8).collect();
    let reach = radius.max(0.0) as isize;
    let half_widths = disc(radius);
    let mut pixels = vec![[0.0; 4]; source.pixels.len()];
    let done = fill_rows(&mut pixels, width, stop, |y, row| {
        let mut counts = vec![0u32; levels];
        let mut sums = vec![[0.0f32; 4]; levels];
        for (x, out) in row.iter_mut().enumerate() {
            counts.fill(0);
            sums.fill([0.0; 4]);
            for (dy, &half) in (-reach..=reach).zip(&half_widths) {
                let row = (y as isize + dy).clamp(0, height as isize - 1) as usize;
                for dx in -(half as isize)..=half as isize {
                    let index = row * width + (x as isize + dx).clamp(0, width as isize - 1) as usize;
                    let step = steps[index] as usize;
                    counts[step] += 1;
                    add(&mut sums[step], source.pixels[index], 1.0);
                }
            }
            let most = (0..levels).max_by_key(|&step| counts[step]).unwrap_or(0);
            *out = scale(sums[most], 1.0 / counts[most].max(1) as f32);
        }
    });
    done.then(|| source.with_pixels(pixels))
}

/// The image blurred, made brighter and more contrasted by these shares, from -1 to
/// 1, then laid over it as a screen does, which lightens it with a soft halo.
fn glow(source: &Buffer, radius: f32, brightness: f32, contrast: f32, stop: &AtomicBool) -> Option<Buffer> {
    let blurred = if radius < 1.0 { None } else { Some(blur(source, Weighing::Gaussian, radius, 1.0, stop)?) };
    let blurred = blurred.as_ref().unwrap_or(source);
    let width = source.width;
    each_pixel(source, stop, |x, y| {
        let own = straight(source.pixels[y * width + x]);
        let halo = straight(blurred.pixels[y * width + x]);
        let screen = |channel: usize| {
            let halo = ((halo[channel] - 0.5) * (1.0 + contrast) + 0.5 + brightness).clamp(0.0, 1.0);
            1.0 - (1.0 - own[channel]) * (1.0 - halo)
        };
        opaque_as([screen(0), screen(1), screen(2)], own[3])
    })
}

/// How high the woven canvas the Apply Canvas lays the image on stands at a pixel,
/// from 0 to 1: a thread down the canvas, or across it on every other square, each
/// rounded across, with a little grain.
fn canvas_height(x: usize, y: usize) -> f32 {
    let rounded = |at: usize| (PI * ((at % CANVAS_THREAD) as f32 + 0.5) / CANVAS_THREAD as f32).sin();
    let down_on_top = (x / CANVAS_THREAD + y / CANVAS_THREAD).is_multiple_of(2);
    let thread = if down_on_top { rounded(x) } else { rounded(y) };
    0.75 * thread + 0.25 * noise(x as i64, y as i64, 1)
}

/// Darkens what is darker than around it, mask radius away, as the outlines of a
/// cartoon: this share of the pixels, the darkest compared with around them, black,
/// and the others less the less darker they are.
fn cartoon(source: &Buffer, radius: f32, black: f32, stop: &AtomicBool) -> Option<Buffer> {
    let (width, height) = (source.width, source.height);
    let light = lightness(source);
    let near = gaussian(&light, width, height, 2.0, stop)?;
    let wide = gaussian(&light, width, height, radius, stop)?;
    let darkness = darkness(&near, &wide);
    let ramp = ramp(&darkness, black);
    each_pixel(source, stop, |x, y| {
        let index = y * width + x;
        let [r, g, b, alpha] = source.pixels[index];
        let [r, g, b, _] = scale([r, g, b, 0.0], 1.0 - darkness[index].min(ramp) / ramp);
        [r, g, b, alpha]
    })
}

/// Black on white, as on a photocopy: black where the image is darker than around it,
/// mask radius away, the darkest `black` share of the pixels, white where it is not,
/// and where it is but among the least dark `white` share of those, or less dark than
/// that share of the black ones, grey between.
/// The sharper, the less the image is blurred before. All from 0 to 1.
fn photocopy(source: &Buffer, radius: f32, sharpness: f32, black: f32, white: f32, stop: &AtomicBool) -> Option<Buffer> {
    let (width, height) = (source.width, source.height);
    let light = lightness(source);
    let near = gaussian(&light, width, height, radius * (1.0 - sharpness) / 2.0, stop)?;
    let wide = gaussian(&light, width, height, radius, stop)?;
    let darkness = darkness(&near, &wide);
    let ramp = ramp(&darkness, black);
    let mut darker: Vec<f32> = darkness.iter().copied().filter(|&dark| dark > 0.0).collect();
    darker.sort_unstable_by(f32::total_cmp);
    let least = darker.get((white * darker.len() as f32) as usize).copied().unwrap_or(0.0);
    let cut = least.max(ramp * white).min(ramp);
    each_pixel(source, stop, |x, y| {
        let index = y * width + x;
        let grey = if ramp > cut { ((darkness[index] - cut) / (ramp - cut)).clamp(0.0, 1.0) } else { (darkness[index] > cut) as u8 as f32 };
        opaque_as([1.0 - grey; 3], source.pixels[index][3])
    })
}

/// How much darker each pixel is than around it, from 0, not darker, to 1, black on
/// something lighter: one less its lightness near it over that further around.
fn darkness(near: &[f32], wide: &[f32]) -> Vec<f32> {
    near.iter()
        .zip(wide)
        .map(|(&near, &wide)| if wide > f32::EPSILON && near < wide { 1.0 - near / wide } else { 0.0 })
        .collect()
}

/// How dark, as [`darkness`] measures it, the darkest pixels up to this share of all
/// of them are, which the Cartoon and the Photocopy make black: the least that is dark
/// at all when fewer are, but never less than [`MIN_RAMP`].
fn ramp(darkness: &[f32], share: f32) -> f32 {
    let mut darker: Vec<f32> = darkness.iter().copied().filter(|&dark| dark > 0.0).collect();
    if darker.is_empty() {
        return 1.0;
    }
    darker.sort_unstable_by(|a, b| b.total_cmp(a));
    let index = ((share * darkness.len() as f32) as usize).min(darker.len() - 1);
    darker[index].max(MIN_RAMP)
}

/// Rectangles turned every way, one from each square of a grid `size` wide, moved
/// anywhere within it, as large as `saturation` times it at most, each the colour of
/// the image at its middle, the last laid on top, on `background` where there are none.
fn cubism(source: &Buffer, size: f32, saturation: f32, background: [u8; 4], stop: &AtomicBool) -> Option<Buffer> {
    let size = size.max(1.0);
    let saturation = saturation.max(0.0);
    // The furthest a rectangle's middle can be from a pixel it covers, in squares.
    let reach = (saturation * FRAC_1_SQRT_2).ceil() as i64 + 1;
    // The furthest a rectangle's corner can be from its middle, in pixels, squared.
    let farthest = 0.5 * (size * saturation).powi(2);
    let background = premultiply(Rgba(background));
    each_pixel(source, stop, |x, y| {
        let at = [x as f32 + 0.5, y as f32 + 0.5];
        let square = at.map(|at| (at / size).floor() as i64);
        let mut top: Option<(f32, [f32; 2])> = None;
        for row in square[1] - reach..=square[1] + reach {
            for column in square[0] - reach..=square[0] + reach {
                let random = |seed: u32| noise(column, row, seed);
                let middle = [(column as f32 + random(0)) * size, (row as f32 + random(1)) * size];
                let (dx, dy) = (at[0] - middle[0], at[1] - middle[1]);
                if dx * dx + dy * dy > farthest {
                    continue;
                }
                let half = [2, 3].map(|seed| size * saturation * (0.25 + 0.25 * random(seed)));
                let (sin, cos) = (random(4) * 2.0 * PI).sin_cos();
                let (along, across) = (dx * cos + dy * sin, dy * cos - dx * sin);
                let order = random(5);
                if along.abs() <= half[0] && across.abs() <= half[1] && top.is_none_or(|(above, _)| order > above) {
                    top = Some((order, middle));
                }
            }
        }
        top.map_or(background, |(_, middle)| source.sample(middle))
    })
}

/// Pixels gathered into clusters about `size` across of those close in colour and
/// place, each filled with its average colour, as Simple Linear Iterative Clustering
/// does: the more compact, the more place counts against colour, which makes rounder
/// clusters. Each step moves each cluster's middle to the middle of its pixels.
fn slic(source: &Buffer, size: u32, compactness: f32, iterations: u32, stop: &AtomicBool) -> Option<Buffer> {
    let (width, height) = (source.width, source.height);
    let step = size.max(2) as usize;
    let features: Vec<[f32; 4]> = source
        .pixels
        .iter()
        .map(|&color| {
            let [r, g, b, alpha] = straight(color);
            let [l, a, b] = lab([r, g, b]);
            [l, a, b, alpha * 100.0]
        })
        .collect();
    let (columns, rows) = (width.div_ceil(step), height.div_ceil(step));
    // Each cluster's colour and place, starting at the middle of a square of the grid.
    let mut clusters: Vec<([f32; 4], [f32; 2])> = (0..rows)
        .flat_map(|row| (0..columns).map(move |column| (column, row)))
        .map(|(column, row)| {
            let (x, y) = ((column * step + step / 2).min(width - 1), (row * step + step / 2).min(height - 1));
            (features[y * width + x], [x as f32, y as f32])
        })
        .collect();
    let weight = (compactness / step as f32).powi(2);
    let mut labels = vec![0u32; width * height];
    for _ in 0..iterations.max(1) {
        let done = fill_rows(&mut labels, width, stop, |y, row| {
            let (square_y, clusters) = (y / step, &clusters);
            for (x, label) in row.iter_mut().enumerate() {
                let (square_x, feature) = (x / step, features[y * width + x]);
                let mut closest = (f32::INFINITY, 0);
                for cluster_row in square_y.saturating_sub(2)..(square_y + 3).min(rows) {
                    for cluster_column in square_x.saturating_sub(2)..(square_x + 3).min(columns) {
                        let index = cluster_row * columns + cluster_column;
                        let (color, [cx, cy]) = clusters[index];
                        let color: f32 = (0..4).map(|channel| (feature[channel] - color[channel]).powi(2)).sum();
                        let place = (x as f32 - cx).powi(2) + (y as f32 - cy).powi(2);
                        let distance = color + place * weight;
                        if distance < closest.0 {
                            closest = (distance, index);
                        }
                    }
                }
                *label = closest.1 as u32;
            }
        });
        if !done {
            return None;
        }
        let mut sums = vec![([0.0f64; 4], [0.0f64; 2], 0u32); clusters.len()];
        for (index, &label) in labels.iter().enumerate() {
            let sum = &mut sums[label as usize];
            for (total, channel) in sum.0.iter_mut().zip(features[index]) {
                *total += channel as f64;
            }
            sum.1[0] += (index % width) as f64;
            sum.1[1] += (index / width) as f64;
            sum.2 += 1;
        }
        for (cluster, (color, place, count)) in clusters.iter_mut().zip(sums) {
            if count > 0 {
                let count = count as f64;
                *cluster = (color.map(|channel| (channel / count) as f32), place.map(|at| (at / count) as f32));
            }
        }
    }
    Some(fill_regions(source, &labels, clusters.len(), false))
}

/// Pixels gathered into regions about `size` across, one grown from the flattest
/// pixel near the middle of each square of a grid out to where the image changes
/// most, as water rising from it would, each filled with its average colour or a
/// colour picked at random. The image is blurred by `smoothness` before, and the
/// regularization makes regions squarer, as if the ground rose away from each middle.
fn waterpixels(source: &Buffer, size: u32, smoothness: f32, regularization: f32, random: bool, stop: &AtomicBool) -> Option<Buffer> {
    let (width, height) = (source.width, source.height);
    let step = size.max(4) as usize;
    let light: Vec<f32> = source.pixels.iter().map(|&[r, g, b, a]| (LUMA[0] * r + LUMA[1] * g + LUMA[2] * b + a) / 2.0).collect();
    let smooth = gaussian(&light, width, height, smoothness * 2.0, stop)?;
    let mut ground: Vec<f32> = slopes(&smooth, width, height).iter().map(|&[dx, dy]| dx.hypot(dy)).collect();
    let steepest = ground.iter().copied().fold(0.0, f32::max);
    let half = step as f32 / 2.0;
    for (index, ground) in ground.iter_mut().enumerate() {
        let (x, y) = (index % width, index / width);
        let middle = |at: usize| (at / step * step) as f32 + half;
        let away = (x as f32 + 0.5 - middle(x)).hypot(y as f32 + 0.5 - middle(y)) / half;
        *ground = if steepest > 0.0 { *ground / steepest } else { 0.0 } + regularization / 10.0 * away;
    }
    // The lowest pixel of each square, away from its edges.
    let margin = step / 6;
    let mut sources = Vec::new();
    for top in (0..height).step_by(step) {
        for left in (0..width).step_by(step) {
            let (right, bottom) = ((left + step).min(width), (top + step).min(height));
            let inner = |start: usize, end: usize| if start + 2 * margin < end { start + margin..end - margin } else { start..end };
            let lowest = inner(top, bottom)
                .flat_map(|y| inner(left, right).map(move |x| y * width + x))
                .min_by(|&a, &b| ground[a].total_cmp(&ground[b]));
            sources.extend(lowest);
        }
    }
    if stop.load(Ordering::Relaxed) {
        return None;
    }
    let level = |index: usize| (ground[index] * 4096.0) as u32;
    let mut labels = vec![u32::MAX; width * height];
    let mut queue = BinaryHeap::new();
    for (label, &index) in sources.iter().enumerate() {
        labels[index] = label as u32;
        queue.push(Reverse((level(index), label, index)));
    }
    let mut order = sources.len();
    let mut flooded = 0usize;
    while let Some(Reverse((_, _, index))) = queue.pop() {
        flooded += 1;
        if flooded.is_multiple_of(FLOOD_CHECK) && stop.load(Ordering::Relaxed) {
            return None;
        }
        let (x, y) = (index % width, index / width);
        let neighbours = [
            (x > 0).then(|| index - 1),
            (x + 1 < width).then_some(index + 1),
            (y > 0).then(|| index - width),
            (y + 1 < height).then_some(index + width),
        ];
        for next in neighbours.into_iter().flatten() {
            if labels[next] == u32::MAX {
                labels[next] = labels[index];
                queue.push(Reverse((level(next), order, next)));
                order += 1;
            }
        }
    }
    Some(fill_regions(source, &labels, sources.len(), random))
}

/// Each of `count` regions, as `labels` gives each pixel's, filled with the average of
/// its colours, or with a colour picked at random as opaque as they are on average.
fn fill_regions(source: &Buffer, labels: &[u32], count: usize, random: bool) -> Buffer {
    let mut sums = vec![([0.0f64; 4], 0u32); count];
    for (&label, color) in labels.iter().zip(&source.pixels) {
        let sum = &mut sums[label as usize];
        for (total, channel) in sum.0.iter_mut().zip(color) {
            *total += *channel as f64;
        }
        sum.1 += 1;
    }
    let colors: Vec<[f32; 4]> = sums
        .iter()
        .enumerate()
        .map(|(label, (sum, count))| {
            let average = sum.map(|channel| (channel / (*count).max(1) as f64) as f32);
            if random {
                let channel = |seed: u32| noise(label as i64, 0, seed);
                opaque_as([channel(7), channel(8), channel(9)], average[3])
            } else {
                average
            }
        })
        .collect();
    source.with_pixels(labels.iter().map(|&label| colors[label as usize]).collect())
}

/// Lights the image as if it were woven cloth: noise blurred across by the first blur
/// and down by the second, as threads, the difference between the two embossed.
fn clothify(source: &Buffer, blur: [f32; 2], azimuth: f32, elevation: f32, depth: f32, stop: &AtomicBool) -> Option<Buffer> {
    let (width, height) = (source.width, source.height);
    let grain = Buffer {
        width,
        height,
        pixels: (0..width * height)
            .map(|index| {
                let (x, y) = ((index % width) as i64, (index / width) as i64);
                [noise(x, y, 2), noise(x, y, 3), 0.0, 0.0]
            })
            .collect(),
    };
    let across = blur_rows(&grain, Weighing::Gaussian, blur[0], 1.0, 1.0, stop)?;
    let down = blur_rows(&grain.transposed(), Weighing::Gaussian, blur[1], 1.0, 1.0, stop)?.transposed();
    let mut threads: Vec<f32> = across.pixels.iter().zip(&down.pixels).map(|(across, down)| (across[0] - down[1]).abs()).collect();
    let highest = threads.iter().copied().fold(0.0, f32::max);
    if highest > 0.0 {
        threads.iter_mut().for_each(|thread| *thread /= highest);
    }
    emboss(source, &threads, azimuth, elevation, depth, stop)
}

/// Bright edges on black, as a predator's heat vision sees them: of each colour only
/// its strongest channel kept, its edges, made `amount` times stronger, then cut into
/// squares `pixel_size` wide, if more than one, and only the strongest channel kept again.
fn predator(source: &Buffer, amount: f32, pixel_size: u32, stop: &AtomicBool) -> Option<Buffer> {
    let (width, height) = (source.width, source.height);
    let kept: Vec<[f32; 3]> = source.pixels.iter().map(|&color| strongest(straight(color))).collect();
    let edges = each_pixel(source, stop, |x, y| {
        let channel = |channel: usize| {
            let at = |x: isize, y: isize| kept[clamped(x, y, width, height)][channel];
            let [dx, dy] = slope(at, x as isize, y as isize);
            (dx.hypot(dy) * amount).min(1.0)
        };
        [channel(0), channel(1), channel(2), 0.0]
    })?;
    let size = pixel_size.max(1) as usize;
    let columns = width.div_ceil(size);
    let squares: Vec<[f32; 4]> = (0..height.div_ceil(size))
        .flat_map(|row| (0..columns).map(move |column| (column, row)))
        .map(|(column, row)| {
            let mut sum = [0.0; 4];
            let (xs, ys) = (column * size..((column + 1) * size).min(width), row * size..((row + 1) * size).min(height));
            let count = xs.len() * ys.len();
            for y in ys {
                for x in xs.clone() {
                    add(&mut sum, edges.pixels[y * width + x], 1.0);
                }
            }
            scale(sum, 1.0 / count as f32)
        })
        .collect();
    each_pixel(source, stop, |x, y| {
        let [r, g, b, _] = squares[y / size * columns + x / size];
        let [r, g, b] = strongest([r, g, b, 1.0]);
        opaque_as([r, g, b], source.pixels[y * width + x][3])
    })
}

/// The colour with only its strongest channels kept, the others black.
fn strongest([r, g, b, _]: [f32; 4]) -> [f32; 3] {
    let most = r.max(g).max(b);
    [r, g, b].map(|channel| if channel < most { 0.0 } else { channel })
}

/// Swirling brush strokes along the edges of the image, as Van Gogh painted: each
/// pixel the average of the image along the line that runs `length` either side of
/// it the way the edges around it go, shaded by noise averaged along the same line
/// as much as `strength`, from 0 to 1, says, as Line Integral Convolution does.
fn van_gogh(source: &Buffer, length: f32, strength: f32, stop: &AtomicBool) -> Option<Buffer> {
    let (width, height) = (source.width, source.height);
    let slopes = slopes(&lightness(source), width, height);
    // Which way the edges go around each pixel, from how it changes around it.
    let changes = source.with_pixels(slopes.iter().map(|&[dx, dy]| [dx * dx, dx * dy, dy * dy, 0.0]).collect());
    let changes = blur(&changes, Weighing::Gaussian, (length / 2.0).max(4.0), 1.0, stop)?;
    let ways: Vec<[f32; 2]> = changes
        .pixels
        .iter()
        .map(|&[xx, xy, yy, _]| {
            let across = 0.5 * (2.0 * xy).atan2(xx - yy);
            [-across.sin(), across.cos()]
        })
        .collect();
    let grain: Vec<f32> = (0..width * height).map(|index| noise((index % width) as i64, (index / width) as i64, 4)).collect();
    let steps = length.round().max(1.0) as usize;
    each_pixel(source, stop, |x, y| {
        let start = y * width + x;
        let (mut color, mut shade, mut count) = (source.pixels[start], grain[start], 1);
        for sign in [1.0, -1.0] {
            let mut at = [x as f32 + 0.5, y as f32 + 0.5];
            let mut way = ways[start].map(|along| along * sign);
            for _ in 0..steps {
                at = [at[0] + way[0], at[1] + way[1]];
                if at[0] < 0.0 || at[1] < 0.0 || at[0] >= width as f32 || at[1] >= height as f32 {
                    break;
                }
                let index = at[1] as usize * width + at[0] as usize;
                // A line goes the same way whichever way along it: keep on as before.
                let next = ways[index];
                way = if next[0] * way[0] + next[1] * way[1] < 0.0 { next.map(|along| -along) } else { next };
                add(&mut color, source.sample(at), 1.0);
                shade += grain[index];
                count += 1;
            }
        }
        // The average of so much noise spreads less: brought back to as much as one.
        let spread = (shade / count as f32 - 0.5) * (12.0 * count as f32).sqrt();
        let [r, g, b, alpha] = scale(color, 1.0 / count as f32);
        let shade = 1.0 + strength * 0.25 * spread.clamp(-2.0, 2.0);
        let shaded = |channel: f32| (channel * shade).clamp(0.0, alpha);
        [shaded(r), shaded(g), shaded(b), alpha]
    })
}

/// Shades the image as if woven of ribbons `width` wide with `spacing` between them,
/// over and under each other in turn: each darker as it goes under another, by
/// `darkness` and over `depth` of the way from the top, with fibres along it as dark
/// as `threads` says, and the gaps between them as dark as the darkness. All but the
/// lengths from 0 to 1.
fn weave(source: &Buffer, width: f32, spacing: f32, darkness: f32, depth: f32, threads: f32, stop: &AtomicBool) -> Option<Buffer> {
    let ribbon = width.max(1.0);
    let period = ribbon + spacing.max(0.0);
    let image_width = source.width;
    // How light a ribbon is, from how high it is there, along and across it.
    let shade = |high: f32, along: f32, across: f32, seed: u32| {
        let low = (1.0 - high) / 2.0;
        let shadow = if depth > 0.0 { ((low - (1.0 - depth)) / depth).clamp(0.0, 1.0) } else { 0.0 };
        let fibre = across.floor() as i64;
        let piece = ((along + noise(fibre, 0, seed) * WEAVE_FIBRE) / WEAVE_FIBRE).floor() as i64;
        (1.0 - darkness * shadow) * (1.0 - threads * 0.3 * noise(fibre, piece, seed))
    };
    each_pixel(source, stop, |x, y| {
        let at = [x as f32 + 0.5, y as f32 + 0.5];
        let square = at.map(|at| (at / period).floor());
        let within = [at[0] - square[0] * period, at[1] - square[1] * period];
        // How high each ribbon is, from 1 where it goes over the other to -1 under.
        let wave = |along: f32, other: f32| (PI * ((along - ribbon / 2.0) / period + other)).cos();
        let across_high = wave(at[0], square[1]);
        let down_high = -wave(at[1], square[0]);
        let (on_across, on_down) = (within[1] < ribbon, within[0] < ribbon);
        let light = match (on_across, on_down) {
            (true, true) if across_high >= down_high => shade(across_high, at[0], at[1], 5),
            (_, true) => shade(down_high, at[1], at[0], 6),
            (true, false) => shade(across_high, at[0], at[1], 5),
            (false, false) => 1.0 - darkness,
        };
        let [r, g, b, alpha] = source.pixels[y * image_width + x];
        [r * light, g * light, b * light, alpha]
    })
}

/// Lights the image as if it stood out as `heights` says, one for each pixel, `depth`
/// times as much, lit from `azimuth` degrees anticlockwise from the right and
/// `elevation` degrees up: lighter where it faces the light, darker where it faces
/// away, flat ground as it was.
pub(super) fn emboss(source: &Buffer, heights: &[f32], azimuth: f32, elevation: f32, depth: f32, stop: &AtomicBool) -> Option<Buffer> {
    let lit = lighting(heights, source.width, source.height, azimuth, elevation, depth);
    each_pixel(source, stop, |x, y| {
        let lit = lit(x, y);
        let [r, g, b, alpha] = source.pixels[y * source.width + x];
        let shaded = |channel: f32| (channel + lit * alpha).clamp(0.0, alpha);
        [shaded(r), shaded(g), shaded(b), alpha]
    })
}

/// How much lighter, or darker below zero, the pixel at `x` and `y` is lit than flat
/// ground, as [`emboss`] lights it, of an image `width` wide and `height` high.
pub(super) fn lighting(
    heights: &[f32],
    width: usize,
    height: usize,
    azimuth: f32,
    elevation: f32,
    depth: f32,
) -> impl Fn(usize, usize) -> f32 + Sync + '_ {
    let (azimuth, elevation) = (azimuth.to_radians(), elevation.clamp(0.0, 90.0).to_radians());
    let light = [azimuth.cos() * elevation.cos(), -azimuth.sin() * elevation.cos(), elevation.sin()];
    move |x, y| {
        let at = |x: isize, y: isize| heights[clamped(x, y, width, height)];
        let (x, y) = (x as isize, y as isize);
        let dx = (at(x + 1, y) - at(x - 1, y)) * depth / 2.0;
        let dy = (at(x, y + 1) - at(x, y - 1)) * depth / 2.0;
        (light[2] - dx * light[0] - dy * light[1]) / (dx * dx + dy * dy + 1.0).sqrt() - light[2]
    }
}

/// Works out each pixel from where it is, across and down.
pub(super) fn each_pixel(source: &Buffer, stop: &AtomicBool, color: impl Fn(usize, usize) -> [f32; 4] + Sync) -> Option<Buffer> {
    let mut pixels = vec![[0.0; 4]; source.pixels.len()];
    let done = fill_rows(&mut pixels, source.width, stop, |y, row| {
        for (x, out) in row.iter_mut().enumerate() {
            *out = color(x, y);
        }
    });
    done.then(|| source.with_pixels(pixels))
}

/// A value for each pixel, worked out from where it is, row after row.
fn field(source: &Buffer, value: impl Fn(usize, usize) -> f32) -> Vec<f32> {
    (0..source.height).flat_map(|y| (0..source.width).map(move |x| (x, y))).map(|(x, y)| value(x, y)).collect()
}

/// `values`, one for each pixel, Gaussian blurred.
pub(super) fn gaussian(values: &[f32], width: usize, height: usize, radius: f32, stop: &AtomicBool) -> Option<Vec<f32>> {
    if radius < 1.0 {
        return Some(values.to_vec());
    }
    let buffer = Buffer { width, height, pixels: values.iter().map(|&value| [value, 0.0, 0.0, 0.0]).collect() };
    Some(blur(&buffer, Weighing::Gaussian, radius, 1.0, stop)?.pixels.iter().map(|pixel| pixel[0]).collect())
}

/// How fast `values`, one for each pixel, change across and down at each, as the
/// Sobel operator measures it, per pixel.
fn slopes(values: &[f32], width: usize, height: usize) -> Vec<[f32; 2]> {
    let at = |x: isize, y: isize| values[clamped(x, y, width, height)];
    (0..height).flat_map(|y| (0..width).map(move |x| slope(at, x as isize, y as isize))).collect()
}

fn slope(at: impl Fn(isize, isize) -> f32, x: isize, y: isize) -> [f32; 2] {
    let across = at(x + 1, y - 1) + 2.0 * at(x + 1, y) + at(x + 1, y + 1) - at(x - 1, y - 1) - 2.0 * at(x - 1, y) - at(x - 1, y + 1);
    let down = at(x - 1, y + 1) + 2.0 * at(x, y + 1) + at(x + 1, y + 1) - at(x - 1, y - 1) - 2.0 * at(x, y - 1) - at(x + 1, y - 1);
    [across / 8.0, down / 8.0]
}

/// The index of the pixel at `x` and `y`, or the one at the edge nearest past it.
pub(super) fn clamped(x: isize, y: isize, width: usize, height: usize) -> usize {
    y.clamp(0, height as isize - 1) as usize * width + x.clamp(0, width as isize - 1) as usize
}

/// How light each pixel's colour looks, from 0 to 1, however opaque it is.
pub(super) fn lightness(source: &Buffer) -> Vec<f32> {
    source.pixels.iter().map(|&color| luma(straight(color))).collect()
}

fn luma([r, g, b, _]: [f32; 4]) -> f32 {
    LUMA[0] * r + LUMA[1] * g + LUMA[2] * b
}

/// The colour divided by how opaque it is again, or none for one that is see-through.
pub(super) fn straight([r, g, b, a]: [f32; 4]) -> [f32; 4] {
    if a >= MIN_ALPHA { [r / a, g / a, b / a, a] } else { [0.0; 4] }
}

/// Red, green and blue, from 0 to 1, multiplied by how opaque the colour is.
pub(super) fn opaque_as(rgb: [f32; 3], alpha: f32) -> [f32; 4] {
    let [r, g, b] = rgb.map(|channel| channel.clamp(0.0, 1.0) * alpha);
    [r, g, b, alpha]
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// The colour in CIE L*a*b*, which measures how different colours look evenly.
pub(super) fn lab([r, g, b]: [f32; 3]) -> [f32; 3] {
    let linear = |channel: f32| if channel <= 0.04045 { channel / 12.92 } else { ((channel + 0.055) / 1.055).powf(2.4) };
    let (r, g, b) = (linear(r), linear(g), linear(b));
    let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
    let f = |t: f32| if t > 0.008856 { t.cbrt() } else { 7.787 * t + 16.0 / 116.0 };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// A number from 0 to 1 that looks random, but is always the same for the same place
/// and seed.
pub(crate) fn noise(x: i64, y: i64, seed: u32) -> f32 {
    let mut hash = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ (seed as u64).wrapping_mul(0x1656_67B1_9E37_79F9);
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    hash ^= hash >> 33;
    (hash >> 40) as f32 / (1u64 << 24) as f32
}

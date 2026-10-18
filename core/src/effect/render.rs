//! The effects of the Render menu, which draw something new rather than change what
//! is there: noise and clouds, fractals, patterns, mazes and speed lines. Each is
//! drawn within an area, the rectangle around the selection, as if it were the whole
//! image, worked out on the straight colours, then blended over the pixels: in the mode the
//! Clouds & Noise is set to, or in the Normal one, where what it leaves see-through
//! keeps the image as it is.

use std::{
    f32::consts::{SQRT_2, TAU},
    sync::atomic::{AtomicBool, Ordering},
};

use image::RgbaImage;

use super::{Effect, artistic::noise as hash, fill_rows};
use crate::{BlendMode, PixelRect, blend::blend_pixel};

/// Most layers of finer and finer features a noise adds up: past these, they are finer
/// than a pixel.
const MAX_OCTAVES: u32 = 16;
/// How far apart the seeds of the layers of a noise are, and those of the Plasma's
/// red, green and blue, so that none looks like another.
const OCTAVE_SEED: u32 = 1013;
const CHANNEL_SEED: u32 = 7919;
/// How large the Fractal Explorer's view is across the image's shorter side at a zoom
/// of 100 percent, and how far from zero a point goes before it is taken as escaped,
/// far enough for the smooth colouring.
const FRACTAL_VIEW: f64 = 3.0;
const ESCAPE: f64 = 256.0;
/// How many escapes the Fractal Explorer's rainbow takes to go round the colours once.
const RAINBOW_ESCAPES: f32 = 32.0;
/// Largest Bayer matrix, as the power of 2 it is wide.
const MAX_BAYER_LEVEL: u32 = 8;
/// How many corridors the Maze carves between looks at whether to give up.
const MAZE_CHECKS: usize = 4096;

/// What a noise looks like.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoiseKind {
    /// Soft blobs, random values blended smoothly between the points of a grid.
    Clouds,
    /// Gradient noise, as Ken Perlin made it: hills and valleys with no blocks.
    Perlin,
    /// Gradient noise on a grid of triangles, which shows its grid less.
    Simplex,
    /// The Perlin noise's distance from its middle, which folds its valleys into
    /// sharp creases, as smoke and marble have.
    Turbulence,
    /// Cells around points scattered over a grid, as Steven Worley made them.
    Cells,
    /// Random values blended straight between the points of a grid, adding up detail
    /// down to single pixels whatever the detail is set to, as a plasma fractal has.
    Plasma,
}

impl NoiseKind {
    pub const ALL: [Self; 6] = [Self::Clouds, Self::Perlin, Self::Simplex, Self::Turbulence, Self::Cells, Self::Plasma];
}

/// What each cell of the Cell Noise shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellStyle {
    /// How far each pixel is from its cell's point: dark in the middle, light at the edges.
    Distance,
    /// How much nearer its cell's point each pixel is than the next one: dark along the
    /// edges between cells.
    Edges,
    /// A random shade for each cell.
    Flat,
}

impl CellStyle {
    pub const ALL: [Self; 3] = [Self::Distance, Self::Edges, Self::Flat];
}

/// A noise, from 0 to 1 at each point, as the Clouds & Noise draws it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Noise {
    pub kind: NoiseKind,
    /// How large its largest features are, in pixels.
    pub scale: f32,
    /// How many layers of features it adds up, each half as large as the one before.
    pub detail: u32,
    /// How strong each layer is next to the one before, in percent.
    pub roughness: f32,
    pub seed: u32,
    /// For the Cell Noise: what each cell shows, and how far its point is from the
    /// middle of its square of the grid, in percent of the most it can be.
    pub cells: CellStyle,
    pub jitter: f32,
}

impl Noise {
    /// How much of it there is at a point, in pixels of the image.
    fn at(&self, x: f32, y: f32) -> f32 {
        let scale = self.scale.max(1.0);
        let octaves = match self.kind {
            NoiseKind::Plasma => (scale.log2().ceil() as u32 + 1).max(self.detail),
            _ => self.detail.max(1),
        }
        .min(MAX_OCTAVES);
        let persistence = self.roughness / 100.0;
        let jitter = (self.jitter / 100.0).clamp(0.0, 1.0);
        let (mut sum, mut total, mut amplitude, mut frequency) = (0.0, 0.0, 1.0, 1.0 / scale);
        for octave in 0..octaves {
            let seed = self.seed.wrapping_add(octave.wrapping_mul(OCTAVE_SEED));
            let (u, v) = (x * frequency, y * frequency);
            let value = match self.kind {
                NoiseKind::Clouds => value_noise(u, v, seed, true),
                NoiseKind::Plasma => value_noise(u, v, seed, false),
                NoiseKind::Perlin => perlin(u, v, seed) * 0.5 + 0.5,
                NoiseKind::Simplex => simplex(u, v, seed) * 0.5 + 0.5,
                NoiseKind::Turbulence => (perlin(u, v, seed).abs() * 1.5).min(1.0),
                NoiseKind::Cells => cell_noise(u, v, seed, self.cells, jitter),
            };
            sum += amplitude * value;
            total += amplitude;
            amplitude *= persistence;
            frequency *= 2.0;
        }
        (sum / total).clamp(0.0, 1.0)
    }
}

/// What the Fractal Explorer draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FractalKind {
    /// The points `c` where `z² + c` stays near zero, from zero, over and over.
    Mandelbrot,
    /// The points `z` where `z² + c` stays near zero, over and over, for one `c`.
    Julia,
}

impl FractalKind {
    pub const ALL: [Self; 2] = [Self::Mandelbrot, Self::Julia];
}

/// The pixels with what the rendering effect draws within `area`, which must be on
/// them, blended over them there. Gives up once `stop` is set.
pub(super) fn render(pixels: &RgbaImage, effect: &Effect, area: PixelRect, stop: &AtomicBool) -> Option<RgbaImage> {
    let size = [area.width() as f32, area.height() as f32];
    let center = |[x, y]: [f32; 2]| [size[0] / 2.0 * (1.0 + x / 100.0), size[1] / 2.0 * (1.0 + y / 100.0)];
    let middle = |x: usize, y: usize| (x as f32 + 0.5, y as f32 + 0.5);
    match *effect {
        Effect::CloudsAndNoise { noise, colors, blend_mode, plasma_colors } => {
            let colored = plasma_colors && noise.kind == NoiseKind::Plasma;
            paint(pixels, area, blend_mode, stop, |x, y| {
                let (x, y) = middle(x, y);
                if colored {
                    let channel = |index: u32| {
                        let seed = noise.seed.wrapping_add(index * CHANNEL_SEED);
                        to_byte(Noise { seed, ..noise }.at(x, y))
                    };
                    [channel(1), channel(2), channel(3), 255]
                } else {
                    mix(colors, noise.at(x, y))
                }
            })
        }
        Effect::Fractal { kind, iterations, zoom, center: [cx, cy], julia, rainbow, colors, antialias } => {
            let unit = FRACTAL_VIEW * 100.0 / zoom.max(f64::EPSILON) / size[0].min(size[1]) as f64;
            // The Mandelbrot set lies around -0.75, the Julia sets around zero.
            let middle = match kind {
                FractalKind::Mandelbrot => [cx - 0.75, cy],
                FractalKind::Julia => [cx, cy],
            };
            let samples = if antialias { 2 } else { 1 };
            let iterations = iterations.max(1);
            paint(pixels, area, BlendMode::Normal, stop, |x, y| {
                let mut sum = [0.0f32; 4];
                for sample in 0..samples * samples {
                    let along = |at: usize, within: usize, side: f32| {
                        (at as f64 + (within as f64 + 0.5) / samples as f64 - side as f64 / 2.0) * unit
                    };
                    let re = middle[0] + along(x, sample % samples, size[0]);
                    let im = middle[1] - along(y, sample / samples, size[1]);
                    let color = fractal_color(escape(kind, [re, im], julia, iterations), iterations, rainbow, colors);
                    for (sum, channel) in sum.iter_mut().zip(color) {
                        *sum += channel as f32;
                    }
                }
                sum.map(|channel| (channel / (samples * samples) as f32).round() as u8)
            })
        }
        Effect::BayerMatrix { level, pixel_size, colors } => {
            let (level, pixel_size) = (level.clamp(1, MAX_BAYER_LEVEL), pixel_size.max(1) as usize);
            let side = 1usize << level;
            let most = (side * side - 1) as f32;
            paint(pixels, area, BlendMode::Normal, stop, |x, y| {
                let (column, row) = ((x / pixel_size) % side, (y / pixel_size) % side);
                mix(colors, bayer(column, row, level) as f32 / most)
            })
        }
        Effect::Checkerboard { size: square, colors } => {
            let square = square.max(1) as usize;
            paint(pixels, area, BlendMode::Normal, stop, |x, y| colors[(x / square + y / square) % 2])
        }
        Effect::Grid { width, height, line, offset: [ox, oy], color } => {
            let (width, height, line) = (width.max(1) as i64, height.max(1) as i64, line as i64);
            paint(pixels, area, BlendMode::Normal, stop, |x, y| {
                let on = |at: usize, by: i32, spacing: i64| (at as i64 - by as i64).rem_euclid(spacing) < line;
                if on(x, ox, width) || on(y, oy, height) { color } else { [0; 4] }
            })
        }
        Effect::Maze { size: passage, seed, branching, colors } => {
            let passage = passage.max(1) as usize;
            let (columns, rows) = (area.width() as usize / passage, area.height() as usize / passage);
            let open = maze(columns, rows, seed, branching, stop)?;
            paint(pixels, area, BlendMode::Normal, stop, |x, y| {
                let (column, row) = (x / passage, y / passage);
                let path = column < columns && row < rows && open[row * columns + column];
                colors[path as usize]
            })
        }
        Effect::SpeedLines { count, thickness, clear, randomness, center: c, seed, color } => {
            let [cx, cy] = center(c);
            // As far as the furthest corner.
            let reach = (cx.max(size[0] - cx)).hypot(cy.max(size[1] - cy)).max(1.0);
            let lines = speed_lines(count.max(1), thickness / 100.0, clear / 100.0 * reach, randomness / 100.0, reach, seed);
            let step = TAU / lines.len() as f32;
            paint(pixels, area, BlendMode::Normal, stop, |x, y| {
                let (x, y) = middle(x, y);
                let (dx, dy) = (x - cx, y - cy);
                let out = dx.hypot(dy);
                let turn = (-dy).atan2(dx).rem_euclid(TAU);
                let nearest = (turn / step).round() as isize;
                // A line is never more than half a step from where it would be, so one
                // of those either side of the nearest place is the one, if any.
                let cover = (nearest - 1..=nearest + 1)
                    .map(|index| {
                        let line = &lines[index.rem_euclid(lines.len() as isize) as usize];
                        if out <= line.start {
                            return 0.0;
                        }
                        let off = ((turn - line.angle + TAU / 2.0).rem_euclid(TAU) - TAU / 2.0).abs();
                        let half_width = line.half_width * (out - line.start) / (reach - line.start).max(1.0);
                        (half_width - off * out + 0.5).clamp(0.0, 1.0)
                    })
                    .fold(0.0, f32::max);
                [color[0], color[1], color[2], to_byte(color[3] as f32 / 255.0 * cover)]
            })
        }
        _ => unreachable!("not a rendering effect"),
    }
}

/// The pixels with what `paint` gives for each of those in `area`, from its column and
/// row within it, blended over it in `mode`.
fn paint(
    pixels: &RgbaImage,
    area: PixelRect,
    mode: BlendMode,
    stop: &AtomicBool,
    paint: impl Fn(usize, usize) -> [u8; 4] + Sync,
) -> Option<RgbaImage> {
    let (width, height) = pixels.dimensions();
    let (left, right) = (area.left as usize, area.right as usize);
    let rows = area.top as usize..area.bottom as usize;
    let mut painted: Vec<[u8; 4]> = pixels.pixels().map(|pixel| pixel.0).collect();
    let done = fill_rows(&mut painted, width as usize, stop, |y, row| {
        if !rows.contains(&y) {
            return;
        }
        for (x, pixel) in row[left..right].iter_mut().enumerate() {
            blend_pixel(pixel, &paint(x, y - rows.start), 1.0, mode);
        }
    });
    done.then(|| RgbaImage::from_raw(width, height, painted.into_flattened()).unwrap())
}

/// The colour this share of the way from the first colour to the second, which are
/// straight.
fn mix(colors: [[u8; 4]; 2], share: f32) -> [u8; 4] {
    std::array::from_fn(|channel| {
        let (from, to) = (colors[0][channel] as f32, colors[1][channel] as f32);
        (from + (to - from) * share).round() as u8
    })
}

fn to_byte(value: f32) -> u8 {
    (value * 255.0).round().clamp(0.0, 255.0) as u8
}

/// Smoother than a straight line from 0 to 1, flat at both ends and so where the
/// squares of a grid meet.
fn fade(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn lerp(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t
}

/// Random values from 0 to 1 at the points of a grid one apart, blended between them,
/// smoothly or in straight lines.
fn value_noise(x: f32, y: f32, seed: u32, smooth: bool) -> f32 {
    let (left, top) = (x.floor(), y.floor());
    let blend = |t: f32| if smooth { fade(t) } else { t };
    let (tx, ty) = (blend(x - left), blend(y - top));
    let at = |dx: i64, dy: i64| hash(left as i64 + dx, top as i64 + dy, seed);
    lerp(lerp(at(0, 0), at(1, 0), tx), lerp(at(0, 1), at(1, 1), tx), ty)
}

/// The way downhill of a gradient noise at a point of its grid: one of length 1.
fn gradient(x: i64, y: i64, seed: u32) -> (f32, f32) {
    let turn = hash(x, y, seed) * TAU;
    (turn.cos(), turn.sin())
}

/// Perlin's gradient noise, from about -1 to 1.
fn perlin(x: f32, y: f32, seed: u32) -> f32 {
    let (left, top) = (x.floor(), y.floor());
    let (fx, fy) = (x - left, y - top);
    let corner = |dx: i64, dy: i64| {
        let (gx, gy) = gradient(left as i64 + dx, top as i64 + dy, seed);
        gx * (fx - dx as f32) + gy * (fy - dy as f32)
    };
    let (tx, ty) = (fade(fx), fade(fy));
    // At most half the root of 2 with gradients of length 1.
    SQRT_2 * lerp(lerp(corner(0, 0), corner(1, 0), tx), lerp(corner(0, 1), corner(1, 1), tx), ty)
}

/// Perlin's simplex noise, from about -1 to 1: each point is in a triangle of a grid
/// of them, made by skewing that of squares, and gets something of each of its three
/// corners, less the further from it.
fn simplex(x: f32, y: f32, seed: u32) -> f32 {
    const SKEW: f32 = 0.366_025_42; // (√3 - 1) / 2
    const UNSKEW: f32 = 0.211_324_87; // (3 - √3) / 6
    let skewed = (x + y) * SKEW;
    let (i, j) = ((x + skewed).floor(), (y + skewed).floor());
    let unskewed = (i + j) * UNSKEW;
    let (x0, y0) = (x - (i - unskewed), y - (j - unskewed));
    let (i1, j1) = if x0 > y0 { (1.0, 0.0) } else { (0.0, 1.0) };
    let corners = [
        (x0, y0, 0.0, 0.0),
        (x0 - i1 + UNSKEW, y0 - j1 + UNSKEW, i1, j1),
        (x0 - 1.0 + 2.0 * UNSKEW, y0 - 1.0 + 2.0 * UNSKEW, 1.0, 1.0),
    ];
    let sum: f32 = corners
        .iter()
        .map(|&(dx, dy, di, dj)| {
            let left = 0.5 - dx * dx - dy * dy;
            if left <= 0.0 {
                return 0.0;
            }
            let (gx, gy) = gradient((i + di) as i64, (j + dj) as i64, seed);
            left.powi(4) * (gx * dx + gy * dy)
        })
        .sum();
    (70.0 * sum).clamp(-1.0, 1.0)
}

/// Worley's cell noise, from 0 to 1, with a point in each square of a grid one apart,
/// as far from its middle as `jitter`, from 0 to 1, lets it.
fn cell_noise(x: f32, y: f32, seed: u32, style: CellStyle, jitter: f32) -> f32 {
    let (column, row) = (x.floor() as i64, y.floor() as i64);
    // The nearest point and the next one, and the square of the nearest.
    let (mut nearest, mut next, mut cell) = (f32::MAX, f32::MAX, (0, 0));
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (c, r) = (column + dx, row + dy);
            let px = c as f32 + 0.5 + (hash(c, r, seed) - 0.5) * jitter;
            let py = r as f32 + 0.5 + (hash(c, r, seed.wrapping_add(1)) - 0.5) * jitter;
            let distance = (px - x).hypot(py - y);
            if distance < nearest {
                (next, nearest, cell) = (nearest, distance, (c, r));
            } else if distance < next {
                next = distance;
            }
        }
    }
    match style {
        CellStyle::Distance => nearest.min(1.0),
        CellStyle::Edges => (next - nearest).min(1.0),
        CellStyle::Flat => hash(cell.0, cell.1, seed.wrapping_add(2)),
    }
}

/// How many escapes, smoothed between whole ones, a point takes to get far from zero,
/// or none if it does not within `iterations`.
fn escape(kind: FractalKind, [re, im]: [f64; 2], julia: [f64; 2], iterations: u32) -> Option<f32> {
    let ([mut zr, mut zi], [cr, ci]) = match kind {
        FractalKind::Mandelbrot => ([0.0, 0.0], [re, im]),
        FractalKind::Julia => ([re, im], julia),
    };
    for count in 0..iterations {
        let (rr, ii) = (zr * zr, zi * zi);
        if rr + ii > ESCAPE * ESCAPE {
            // How far past the escape the point went, for the colours to change smoothly.
            let past = ((rr + ii).ln() / 2.0).ln() / std::f64::consts::LN_2;
            return Some((count as f64 + 1.0 - past).max(0.0) as f32);
        }
        zi = 2.0 * zr * zi + ci;
        zr = rr - ii + cr;
    }
    None
}

/// The colour of a point of the Fractal Explorer: the first of `colors` in the set,
/// and around it, going round the rainbow, or from the second of them far from the set
/// towards the first nearer it, as the escapes take longer.
fn fractal_color(escapes: Option<f32>, iterations: u32, rainbow: bool, colors: [[u8; 4]; 2]) -> [u8; 4] {
    let Some(escapes) = escapes else {
        return colors[0];
    };
    if rainbow {
        let [r, g, b] = hue((escapes / RAINBOW_ESCAPES).fract());
        [to_byte(r), to_byte(g), to_byte(b), 255]
    } else {
        // The root, for the points that escape quickly to be told apart from those far out.
        let share = (escapes / iterations as f32).sqrt();
        mix([colors[1], colors[0]], share.clamp(0.0, 1.0))
    }
}

/// The colour of this hue, from 0 to 1 round the colour wheel from red, fully
/// saturated and as light as it goes.
fn hue(turn: f32) -> [f32; 3] {
    // Each channel is full for a third of the way round, and none for another third.
    let channel = |offset: f32| {
        let at = (offset + turn * 6.0).rem_euclid(6.0);
        1.0 - at.min(4.0 - at).clamp(0.0, 1.0)
    };
    [channel(5.0), channel(3.0), channel(1.0)]
}

/// The place of a square in the order a Bayer matrix as wide as `1 << level` fills its
/// squares in: each next one as far as it can be from those before.
fn bayer(column: usize, row: usize, level: u32) -> usize {
    (0..level).fold(0, |value, bit| {
        let (x, y) = ((column >> bit) & 1, (row >> bit) & 1);
        value | (((x ^ y) << 1 | y) << (2 * (level - 1 - bit)))
    })
}

/// Which of the squares of a grid `columns` by `rows` are open, a maze between them:
/// one in every other square, odd column and odd row, each joined to those next to it
/// by opening the square between, one path between any two, with a way in on the left
/// and one out on the right. Branching, they are joined from a random one of those so
/// far, as Prim's algorithm does, for many short dead ends; or else from the last one
/// that has a square left to join, as a depth-first search does, for long winding
/// corridors. Gives up once `stop` is set.
fn maze(columns: usize, rows: usize, seed: u32, branching: bool, stop: &AtomicBool) -> Option<Vec<bool>> {
    let mut open = vec![false; columns * rows];
    let (cells_across, cells_down) = (columns.saturating_sub(1) / 2, rows.saturating_sub(1) / 2);
    if cells_across == 0 || cells_down == 0 {
        return Some(open);
    }
    let square = |cell: usize| (cell / cells_across * 2 + 1) * columns + cell % cells_across * 2 + 1;
    let neighbors = |cell: usize| {
        let (x, y) = (cell % cells_across, cell / cells_across);
        [
            (x > 0).then(|| cell - 1),
            (x + 1 < cells_across).then(|| cell + 1),
            (y > 0).then(|| cell - cells_across),
            (y + 1 < cells_down).then(|| cell + cells_across),
        ]
    };
    let mut random = Random::new(seed);
    let mut joined = vec![false; cells_across * cells_down];
    let start = random.below(joined.len());
    joined[start] = true;
    open[square(start)] = true;
    let mut growing = vec![start];
    let mut carved = 0;
    while !growing.is_empty() {
        carved += 1;
        if carved % MAZE_CHECKS == 0 && stop.load(Ordering::Relaxed) {
            return None;
        }
        let at = if branching { random.below(growing.len()) } else { growing.len() - 1 };
        let cell = growing[at];
        let free: Vec<usize> = neighbors(cell).into_iter().flatten().filter(|&next| !joined[next]).collect();
        if free.is_empty() {
            growing.swap_remove(at);
            continue;
        }
        let next = free[random.below(free.len())];
        joined[next] = true;
        let (from, to) = (square(cell), square(next));
        open[(from + to) / 2] = true;
        open[to] = true;
        growing.push(next);
    }
    open[columns] = true;
    open[(cells_down * 2 - 1) * columns + cells_across * 2] = true;
    Some(open)
}

/// Numbers that look random, but are always the same for the same seed.
struct Random(u64);

impl Random {
    fn new(seed: u32) -> Self {
        Self(seed as u64 ^ 0x9E37_79B9_7F4A_7C15)
    }

    /// One from 0 to under `count`, which must not be 0.
    fn below(&mut self, count: usize) -> usize {
        // SplitMix64.
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        value ^= value >> 31;
        (value % count as u64) as usize
    }
}

/// One of the Speed Lines: a wedge pointing at the centre, from `start` out, `angle`
/// round from the right, and as wide either side at the edge as `half_width`.
struct SpeedLine {
    angle: f32,
    start: f32,
    half_width: f32,
}

/// `count` lines evenly round, each taking up `thickness`, from 0 to 1, of the room
/// between two at the edge, starting `clear` from the centre, all in pixels as `reach`
/// is. The more random, the more their places, widths and starts change.
fn speed_lines(count: u32, thickness: f32, clear: f32, randomness: f32, reach: f32, seed: u32) -> Vec<SpeedLine> {
    let step = TAU / count as f32;
    (0..count)
        .map(|index| {
            let random = |which: i64| hash(index as i64, which, seed);
            // Never more than half a step from its place, so lines stay in their order.
            let angle = (index as f32 + (random(0) - 0.5) * randomness) * step;
            let width = 1.0 - randomness * 0.8 * random(1);
            let start = clear + randomness * random(2) * (reach - clear) * 0.5;
            SpeedLine { angle, start, half_width: thickness * width * step * reach / 2.0 }
        })
        .collect()
}

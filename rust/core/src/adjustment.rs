//! The adjustments of the Colors menu, which change the colours of each pixel on
//! its own: no pixel looks at any other, but for dithering, which spreads what each
//! pixel's colour is off by to those after it, and the automatic ones, which first
//! measure the colours of all the pixels they change, and how opaque each is stays as it was,
//! but for an inversion that turns it upside down as well and Color to Alpha. Those
//! that change each channel on its own go through a table of what each byte becomes.
//! Large images are cut into bands of rows, adjusted at the same time on as many
//! threads as there are cores.

use std::{
    f64::consts::{FRAC_PI_2, FRAC_PI_4},
    num::NonZero,
    thread,
};

use image::RgbaImage;

use crate::{SelectionMask, effect::noise};

/// Fewest pixels worth sharing between threads.
const PARALLEL_PIXELS: usize = 1 << 16;
/// Weights of red, green and blue in how light a colour looks.
const LUMA: [f32; 3] = [0.299, 0.587, 0.114];
/// Weights of linear red, green and blue in how much light a colour gives.
const LINEAR_LUMINANCE: [f32; 3] = [0.2126, 0.7152, 0.0722];
/// What red, green and blue each become in sepia, from each of them.
const SEPIA: [[f32; 3]; 3] = [[0.393, 0.769, 0.189], [0.349, 0.686, 0.168], [0.272, 0.534, 0.131]];

/// An adjustment, with its settings.
#[derive(Clone, Debug, PartialEq)]
pub enum Adjustment {
    /// Stretches the levels from the darkest to the lightest the pixels have to black
    /// and white, as `mode` says, leaving out `clip` percent of them at each end.
    AutoLevel { mode: AutoLevelMode, clip: f32 },
    /// Makes each pixel grey, as light as `mode` says it is.
    BlackAndWhite { mode: DesaturateMode },
    /// Each from -100 to 100.
    BrightnessContrast { brightness: f32, contrast: f32 },
    /// How far the shadows, the midtones and the highlights each go towards red, green
    /// and blue, from -100, cyan, magenta and yellow, to 100. With `preserve_luminosity`,
    /// each pixel stays as light as it was.
    ColorBalance { shadows: [f32; 3], midtones: [f32; 3], highlights: [f32; 3], preserve_luminosity: bool },
    /// Takes `color` out of each pixel, which becomes as transparent as it is close to
    /// it: fully so at `transparency` percent away or closer, not at all at `opacity`
    /// percent or further, the most a channel can be away being 100.
    ColorToAlpha { color: [u8; 3], transparency: f32, opacity: f32 },
    /// Gives each pixel this hue, in degrees, and saturation, in percent, keeping how
    /// light it is, moved from -100, black, to 100, white.
    Colorize { hue: f32, saturation: f32, lightness: f32 },
    Curves(Curves),
    /// Leaves each of red, green and blue `levels` levels, from 2 to 256, and makes up
    /// for those it loses as `method` says.
    Dither { levels: u32, method: DitherMethod },
    /// Spreads the levels so that each is as common as any other: each of red, green
    /// and blue on its own, or how light each pixel is, if `lightness`, which keeps
    /// its hue and how strong it is.
    Equalize { lightness: bool },
    /// In stops: each doubles or halves the light.
    Exposure { exposure: f32 },
    /// Each from -100, darker, to 100, lighter.
    HighlightsShadows { shadows: f32, highlights: f32 },
    /// The hue in degrees around the colour wheel, the saturation in percent, 0 for grey
    /// and 100 for no change, and the lightness from -100, black, to 100, white. If
    /// `perceptual`, the hue and how strong the colour is are those of OKLCh, which
    /// keeps how light each colour looks.
    HueSaturation { hue: f32, saturation: f32, lightness: f32, perceptual: bool },
    /// Turns red, green and blue upside down, each that `channels` says, as `mode` says,
    /// and how opaque each pixel is if `alpha`.
    Invert { mode: InvertMode, channels: [bool; 3], alpha: bool },
    /// Stretches the levels of each of red, green and blue that `channels` says from
    /// `input`, black and white, from 0 to 255, to `output`, bending them by `gamma`
    /// on the way: above 1 lightens the midtones.
    Levels { channels: [bool; 3], input: [f32; 2], gamma: f32, output: [f32; 2] },
    /// Leaves each of red, green and blue `levels` levels, from 2 to 256.
    Posterize { levels: u32 },
    Sepia,
    /// Each from -100 to 100: from blue to yellow, and from green to magenta. How light
    /// each pixel looks stays about the same.
    TemperatureTint { temperature: f32, tint: f32 },
    /// Makes white each pixel at least `low` and at most `high` light, from 0 to 255,
    /// and the others black.
    Threshold { low: f32, high: f32 },
    /// Makes the pixels' average colour grey, in linear light, by `strength` percent,
    /// keeping it as light: what a colour cast tints all of them is taken out.
    WhiteBalance { strength: f32 },
}

impl Adjustment {
    /// Inverts the colours and keeps how opaque they are.
    pub const INVERT_COLORS: Self = Self::Invert { mode: InvertMode::Normal, channels: [true; 3], alpha: false };
    /// Inverts how opaque each pixel is and keeps its colour.
    pub const INVERT_ALPHA: Self = Self::Invert { mode: InvertMode::Normal, channels: [false; 3], alpha: true };
}

/// How an inversion turns the colours upside down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvertMode {
    /// Each channel's byte, from 255 down.
    Normal,
    /// Each channel's light, in linear light, which keeps darks dark and lights light longer.
    Linear,
    /// Only how bright each colour is, its value, which keeps its hue and how strong it is.
    /// Each of red, green and blue is left to it.
    Value,
}

/// How light black and white makes each colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesaturateMode {
    /// As much light as it gives, weighing linear red, green and blue as the eye does,
    /// which is the most faithful.
    Luminance,
    /// Weighing red, green and blue as they are as video does, which is quicker and
    /// close to it.
    Luma,
    /// Halfway between its strongest and its weakest channel, as HSL has it.
    Lightness,
    /// The average of red, green and blue.
    Average,
    /// Its strongest channel, as HSV has it.
    Value,
}

/// What automatic levels stretch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoLevelMode {
    /// Each of red, green and blue on its own, which takes out a colour cast as well.
    Channels,
    /// Red, green and blue as one, which keeps the colours.
    Together,
    /// How strong and how bright each colour is, its saturation and its value, which
    /// keeps its hue.
    Hsv,
}

/// How dithering makes up for the levels it loses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DitherMethod {
    /// Spreads what each pixel is off by to the four after it, the most common way.
    FloydSteinberg,
    /// Spreads three quarters of what each pixel is off by to the six after it, which
    /// keeps more contrast, as the early Macintosh did.
    Atkinson,
    /// Nudges each pixel by a repeating 8 by 8 pattern before rounding it.
    Ordered,
    /// Nudges each pixel by noise before rounding it.
    Random,
}

/// Curves that map each level of a channel to the level it becomes: one for how light
/// each pixel is, or one for each of red, green and blue. Each goes through points
/// from 0 to 1 both ways, from left to right, at least two of them.
#[derive(Clone, Debug, PartialEq)]
pub struct Curves {
    /// Whether the curve of how light each pixel is applies, rather than those of each channel.
    pub luminosity: bool,
    pub light: Vec<[f32; 2]>,
    /// Red, green and blue.
    pub channels: [Vec<[f32; 2]>; 3],
}

impl Curves {
    /// A curve that changes nothing.
    pub const STRAIGHT: [[f32; 2]; 2] = [[0.0, 0.0], [1.0, 1.0]];
}

impl Default for Curves {
    fn default() -> Self {
        let straight = Self::STRAIGHT.to_vec();
        Self { luminosity: true, light: straight.clone(), channels: [straight.clone(), straight.clone(), straight] }
    }
}

/// The pixels as the adjustment makes them, but for those `mask` leaves out, if there
/// is one, which stay as they were. The mask must be as large as the pixels.
pub fn adjust(pixels: &RgbaImage, adjustment: &Adjustment, mask: Option<&SelectionMask>) -> RgbaImage {
    let mut adjusted = pixels.clone();
    let row_pixels = pixels.width() as usize;
    let rows = pixels.height() as usize;
    let selected = mask.map(SelectionMask::flags);
    if let Adjustment::Dither { levels, method } = *adjustment {
        dither(&mut adjusted, row_pixels, levels, method, selected);
        return adjusted;
    }
    let mapping = match *adjustment {
        Adjustment::AutoLevel { mode, clip } => auto_level(pixels, selected, mode, clip / 100.0),
        Adjustment::Equalize { lightness } => equalize(pixels, selected, lightness),
        Adjustment::WhiteBalance { strength } => white_balance(pixels, selected, strength / 100.0),
        _ => Mapping::of(adjustment),
    };
    let invert_alpha = matches!(adjustment, Adjustment::Invert { alpha: true, .. });
    let adjust_rows = |pixels: &mut [u8], selected: Option<&[bool]>| {
        for (index, pixel) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            if selected.is_none_or(|selected| selected[index]) {
                mapping.apply(pixel);
                if invert_alpha {
                    pixel[3] = 255 - pixel[3];
                }
            }
        }
    };
    let threads = thread::available_parallelism().map_or(1, NonZero::get);
    if threads == 1 || row_pixels * rows < PARALLEL_PIXELS {
        adjust_rows(&mut adjusted, selected);
        return adjusted;
    }
    let band = rows.div_ceil(threads) * row_pixels;
    thread::scope(|scope| {
        for (index, pixels) in adjusted.chunks_mut(band * 4).enumerate() {
            let selected = selected.map(|selected| &selected[index * band..index * band + pixels.len() / 4]);
            scope.spawn(move || adjust_rows(pixels, selected));
        }
    });
    adjusted
}

/// How many of the pixels have each level of each of the measures `measure` takes of
/// their colour, from 0 to 1, but for those `selected` leaves out and those that are
/// fully transparent, which show no colour.
fn histograms<const N: usize>(pixels: &RgbaImage, selected: Option<&[bool]>, measure: impl Fn([f32; 3]) -> [f32; N]) -> [[u64; 256]; N] {
    let mut counts = [[0; 256]; N];
    for (index, pixel) in pixels.pixels().enumerate() {
        if pixel[3] == 0 || selected.is_some_and(|selected| !selected[index]) {
            continue;
        }
        let measures = measure([0, 1, 2].map(|channel| pixel[channel] as f32 / 255.0));
        for (count, value) in counts.iter_mut().zip(measures) {
            count[to_byte(value) as usize] += 1;
        }
    }
    counts
}

/// The darkest and the lightest level of a histogram, from 0 to 1, past the `clip`
/// share of what it counts at each end, or none if it counts nothing.
fn level_range(counts: &[u64; 256], clip: f32) -> Option<(f32, f32)> {
    let total: u64 = counts.iter().sum();
    if total == 0 {
        return None;
    }
    let skipped = (total as f64 * f64::from(clip.clamp(0.0, 0.5))) as u64;
    let past = |mut levels: Box<dyn Iterator<Item = usize>>| {
        let mut seen = 0;
        levels.find(|&level| {
            seen += counts[level];
            seen > skipped
        })
    };
    let low = past(Box::new(0..256))?;
    let high = past(Box::new((0..256).rev()))?;
    Some((low as f32 / 255.0, high as f32 / 255.0))
}

/// Stretches `value` from the range to 0 to 1. A range of one level is left as it is.
fn stretch(value: f32, (low, high): (f32, f32)) -> f32 {
    if high - low < 0.5 / 255.0 { value } else { ((value - low) / (high - low)).clamp(0.0, 1.0) }
}

fn auto_level(pixels: &RgbaImage, selected: Option<&[bool]>, mode: AutoLevelMode, clip: f32) -> Mapping {
    match mode {
        AutoLevelMode::Channels => {
            let counts = histograms(pixels, selected, |c| c);
            let ranges = counts.map(|counts| level_range(&counts, clip));
            let tables = ranges.map(|range| {
                std::array::from_fn(|byte| range.map_or(byte as u8, |range| to_byte(stretch(byte as f32 / 255.0, range))))
            });
            Mapping::Channels(Box::new(tables))
        }
        AutoLevelMode::Together => {
            let [r, g, b] = histograms(pixels, selected, |c| c);
            let counts = std::array::from_fn(|level| r[level] + g[level] + b[level]);
            match level_range(&counts, clip) {
                Some(range) => Mapping::each_channel(|c| stretch(c, range)),
                None => Mapping::each_channel(|c| c),
            }
        }
        AutoLevelMode::Hsv => {
            let [_, saturation, value] = histograms(pixels, selected, rgb_to_hsv);
            let saturation = level_range(&saturation, clip).unwrap_or((0.0, 1.0));
            let value = level_range(&value, clip).unwrap_or((0.0, 1.0));
            Mapping::Colors(Box::new(move |c| {
                let [h, s, v] = rgb_to_hsv(c);
                hsv_to_rgb([h, stretch(s, saturation), stretch(v, value)])
            }))
        }
    }
}

/// What each level becomes for the levels counted to spread evenly from 0 to 1: how
/// many are below it, out of all but those of the darkest, which becomes black.
fn equalized(counts: &[u64; 256]) -> [f32; 256] {
    let darkest = counts.iter().copied().find(|&count| count > 0).unwrap_or(0);
    let total: u64 = counts.iter().sum();
    let mut below = 0;
    std::array::from_fn(|level| {
        below += counts[level];
        if total <= darkest { level as f32 / 255.0 } else { below.saturating_sub(darkest) as f32 / (total - darkest) as f32 }
    })
}

fn equalize(pixels: &RgbaImage, selected: Option<&[bool]>, lightness: bool) -> Mapping {
    if lightness {
        let [counts] = histograms(pixels, selected, |c| [luma(c)]);
        let light = equalized(&counts);
        // As for the curve of how light pixels are, the colour is shifted as a whole.
        Mapping::Colors(Box::new(move |c| {
            let l = luma(c);
            let shift = look_up(&light, l) - l;
            c.map(|channel| channel + shift)
        }))
    } else {
        let counts = histograms(pixels, selected, |c| c);
        Mapping::Channels(Box::new(counts.map(|counts| equalized(&counts).map(to_byte))))
    }
}

fn white_balance(pixels: &RgbaImage, selected: Option<&[bool]>, strength: f32) -> Mapping {
    let counts = histograms(pixels, selected, |c| c);
    // The average light of each channel.
    let means = counts.map(|counts| {
        let total: u64 = counts.iter().sum();
        let light: f64 = counts.iter().enumerate().map(|(level, &count)| f64::from(to_linear(level as f32 / 255.0)) * count as f64).sum();
        if total == 0 { 0.0 } else { (light / total as f64) as f32 }
    });
    let grey = dot(means, LINEAR_LUMINANCE);
    let gains = means.map(|mean| if mean > 1e-6 && grey > 1e-6 { 1.0 + (grey / mean - 1.0) * strength } else { 1.0 });
    let tables = gains.map(|gain| std::array::from_fn(|byte| to_byte(to_srgb(to_linear(byte as f32 / 255.0) * gain))));
    Mapping::Channels(Box::new(tables))
}

/// Leaves each of red, green and blue of the pixels, `row_pixels` to a row, `levels`
/// levels, but for those `selected` leaves out. What error diffusion spreads goes only
/// to selected pixels, along each row from left to right and back, which leaves fewer
/// patterns than always going the same way.
fn dither(pixels: &mut [u8], row_pixels: usize, levels: u32, method: DitherMethod, selected: Option<&[bool]>) {
    if row_pixels == 0 {
        return;
    }
    let steps = (levels.clamp(2, 256) - 1) as f32;
    let round = |value: f32| (value.clamp(0.0, 1.0) * steps).round() / steps;
    let spread: &[(isize, usize, f32)] = match method {
        DitherMethod::FloydSteinberg => &[(1, 0, 7.0 / 16.0), (-1, 1, 3.0 / 16.0), (0, 1, 5.0 / 16.0), (1, 1, 1.0 / 16.0)],
        DitherMethod::Atkinson => &[(1, 0, 0.125), (2, 0, 0.125), (-1, 1, 0.125), (0, 1, 0.125), (1, 1, 0.125), (0, 2, 0.125)],
        DitherMethod::Ordered | DitherMethod::Random => &[],
    };
    // What is spread to this row and the two after it, each starting where its row does.
    let mut errors = vec![[0.0f32; 3]; row_pixels * 3];
    for (y, row) in pixels.chunks_mut(row_pixels * 4).enumerate() {
        let backwards = !spread.is_empty() && y % 2 == 1;
        for step in 0..row_pixels {
            let x = if backwards { row_pixels - 1 - step } else { step };
            let index = y * row_pixels + x;
            if selected.is_some_and(|selected| !selected[index]) {
                continue;
            }
            let nudge = match method {
                DitherMethod::Ordered => (bayer(x, y) - 0.5) / steps,
                DitherMethod::Random => (noise(x as i64, y as i64, 0) - 0.5) / steps,
                _ => 0.0,
            };
            let error = errors[y % 3 * row_pixels + x];
            let mut off = [0.0; 3];
            for channel in 0..3 {
                let wanted = row[x * 4 + channel] as f32 / 255.0 + error[channel];
                let rounded = round(wanted + nudge);
                row[x * 4 + channel] = to_byte(rounded);
                off[channel] = wanted - rounded;
            }
            for &(across, down, weight) in spread {
                let to_x = if backwards { x as isize - across } else { x as isize + across };
                let to_y = y + down;
                if to_x < 0 || to_x as usize >= row_pixels {
                    continue;
                }
                // Past the last row, it is never read.
                let to = to_y % 3 * row_pixels + to_x as usize;
                for channel in 0..3 {
                    errors[to][channel] += off[channel] * weight;
                }
            }
        }
        // Spread to pixels left out, which drop it, and ready for the row three down.
        errors[y % 3 * row_pixels..][..row_pixels].fill([0.0; 3]);
    }
}

/// Where a pixel falls in an 8 by 8 Bayer matrix, from 0 to 1, each of its 64 places
/// as far as can be from those nearest in order.
fn bayer(x: usize, y: usize) -> f32 {
    let (mut x, mut y, mut order) = (x, y, 0);
    for _ in 0..3 {
        order = order << 2 | ((x ^ y) & 1) << 1 | (y & 1);
        x >>= 1;
        y >>= 1;
    }
    // The lowest bits, taken first, weigh most.
    (order as f32 + 0.5) / 64.0
}

/// The level each of the 256 levels of a channel becomes, from 0 to 1, along a smooth
/// curve through these points that never goes back down between two of them that go
/// up, nor up between two that go down: Fritsch and Carlson's monotone cubic
/// interpolation. Levels past the first and last points are mapped as those points are.
pub fn sample_curve(points: &[[f32; 2]]) -> [f32; 256] {
    let count = points.len();
    let mut levels = [0.0; 256];
    if count < 2 {
        levels.fill(points.first().map_or(0.0, |point| point[1]).clamp(0.0, 1.0));
        return levels;
    }
    // Slopes between points, then at each point.
    let secants: Vec<f32> = points.windows(2).map(|pair| (pair[1][1] - pair[0][1]) / (pair[1][0] - pair[0][0])).collect();
    let mut tangents = vec![0.0; count];
    tangents[0] = secants[0];
    tangents[count - 1] = secants[count - 2];
    for i in 1..count - 1 {
        tangents[i] = if secants[i - 1] * secants[i] <= 0.0 { 0.0 } else { (secants[i - 1] + secants[i]) / 2.0 };
    }
    for i in 0..count - 1 {
        if secants[i].abs() < f32::EPSILON {
            tangents[i] = 0.0;
            tangents[i + 1] = 0.0;
            continue;
        }
        let a = tangents[i] / secants[i];
        let b = tangents[i + 1] / secants[i];
        let length = (a * a + b * b).sqrt();
        if length > 3.0 {
            tangents[i] = 3.0 / length * a * secants[i];
            tangents[i + 1] = 3.0 / length * b * secants[i];
        }
    }

    let mut segment = 0;
    for (level, mapped) in levels.iter_mut().enumerate() {
        let x = level as f32 / 255.0;
        let y = if x <= points[0][0] {
            points[0][1]
        } else if x >= points[count - 1][0] {
            points[count - 1][1]
        } else {
            while points[segment + 1][0] < x {
                segment += 1;
            }
            let ([from_x, from_y], [to_x, to_y]) = (points[segment], points[segment + 1]);
            let width = to_x - from_x;
            let t = (x - from_x) / width;
            let (t2, t3) = (t * t, t * t * t);
            (2.0 * t3 - 3.0 * t2 + 1.0) * from_y
                + (t3 - 2.0 * t2 + t) * width * tangents[segment]
                + (-2.0 * t3 + 3.0 * t2) * to_y
                + (t3 - t2) * width * tangents[segment + 1]
        };
        *mapped = y.clamp(0.0, 1.0);
    }
    levels
}

/// How an adjustment changes a pixel, worked out once for all of them.
enum Mapping {
    /// What each byte of red, green and blue becomes.
    Channels(Box<[[u8; 256]; 3]>),
    /// What each colour becomes as a whole, from 0 to 1.
    Colors(Box<dyn Fn([f32; 3]) -> [f32; 3] + Sync>),
    /// What each pixel becomes, how opaque it is as well, from 0 to 1.
    Pixels(Box<dyn Fn([f32; 4]) -> [f32; 4] + Sync>),
}

impl Mapping {
    fn of(adjustment: &Adjustment) -> Self {
        match *adjustment {
            Adjustment::BlackAndWhite { mode } => {
                let grey: fn([f32; 3]) -> f32 = match mode {
                    DesaturateMode::Luminance => |c| to_srgb(dot(c.map(to_linear), LINEAR_LUMINANCE)),
                    DesaturateMode::Luma => luma,
                    DesaturateMode::Lightness => |c| (c[0].max(c[1]).max(c[2]) + c[0].min(c[1]).min(c[2])) / 2.0,
                    DesaturateMode::Average => |c| (c[0] + c[1] + c[2]) / 3.0,
                    DesaturateMode::Value => |c| c[0].max(c[1]).max(c[2]),
                };
                Self::Colors(Box::new(move |c| [grey(c); 3]))
            }
            Adjustment::BrightnessContrast { brightness, contrast } => {
                // Brightness moves each channel towards black or white, by up to half the way.
                let b = brightness / 200.0;
                // Contrast steepens or flattens the line through middle grey: flat grey at
                // -100, a threshold at 100.
                let angle = ((f64::from(contrast) / 100.0 + 1.0) * FRAC_PI_4).min(FRAC_PI_2 - 1e-6);
                let slant = angle.tan() as f32;
                Self::each_channel(|c| (lighten(c, b) - 0.5) * slant + 0.5)
            }
            Adjustment::ColorBalance { shadows, midtones, highlights, preserve_luminosity } => {
                let [shadows, midtones, highlights] = [shadows, midtones, highlights].map(|amounts| amounts.map(|amount| amount / 100.0));
                Self::Colors(Box::new(move |c| {
                    // How much each range weighs, from how light the colour is, as GIMP
                    // has it: the three always add up to the same.
                    let l = (c[0].max(c[1]).max(c[2]) + c[0].min(c[1]).min(c[2])) / 2.0;
                    let (ramp, middle, scale) = (0.25, 0.333, 0.7);
                    let dark = ((l - middle) / -ramp + 0.5).clamp(0.0, 1.0) * scale;
                    let mid = ((l - middle) / ramp + 0.5).clamp(0.0, 1.0) * ((l + middle - 1.0) / -ramp + 0.5).clamp(0.0, 1.0) * scale;
                    let light = ((l + middle - 1.0) / ramp + 0.5).clamp(0.0, 1.0) * scale;
                    let balanced: [f32; 3] = std::array::from_fn(|channel| {
                        (c[channel] + shadows[channel] * dark + midtones[channel] * mid + highlights[channel] * light).clamp(0.0, 1.0)
                    });
                    if preserve_luminosity { keep_luma(balanced, luma(c)) } else { balanced }
                }))
            }
            Adjustment::ColorToAlpha { color, transparency, opacity } => {
                let color = color.map(|channel| channel as f32 / 255.0);
                let transparency = transparency / 100.0;
                let opacity = (opacity / 100.0).max(transparency + 1.0 / 255.0);
                Self::Pixels(Box::new(move |pixel| color_to_alpha(pixel, color, transparency, opacity)))
            }
            Adjustment::Colorize { hue, saturation, lightness } => {
                let (hue, saturation, lightness) = (hue / 360.0, saturation / 100.0, lightness / 100.0);
                Self::Colors(Box::new(move |c| hsl_to_rgb([hue.rem_euclid(1.0), saturation, lighten(luma(c), lightness)])))
            }
            Adjustment::Curves(ref curves) if curves.luminosity => {
                let light = sample_curve(&curves.light);
                // The colour is shifted as a whole, which keeps its hue and how strong it is.
                Self::Colors(Box::new(move |c| {
                    let l = luma(c);
                    let shift = look_up(&light, l) - l;
                    c.map(|channel| channel + shift)
                }))
            }
            Adjustment::Curves(ref curves) => {
                let tables = curves.channels.each_ref().map(|points| sample_curve(points).map(to_byte));
                Self::Channels(Box::new(tables))
            }
            Adjustment::Dither { .. } => unreachable!("each pixel's dithering depends on those before it"),
            Adjustment::AutoLevel { .. } | Adjustment::Equalize { .. } | Adjustment::WhiteBalance { .. } => {
                unreachable!("they depend on the colours of all the pixels")
            }
            Adjustment::Exposure { exposure } => {
                // Light adds up linearly, so it is scaled before the colours are made sRGB again.
                let scale = exposure.exp2();
                Self::each_channel(|c| to_srgb(to_linear(c) * scale))
            }
            Adjustment::HighlightsShadows { shadows, highlights } => {
                let (shadows, highlights) = (shadows / 100.0, highlights / 100.0);
                // The shadows are moved most around a third of the way up from black, the
                // highlights around two thirds, and black and white not at all.
                Self::Colors(Box::new(move |c| {
                    let l = luma(c);
                    let lift = 2.0 * shadows * l * (1.0 - l) * (1.0 - l) + 2.0 * highlights * l * l * (1.0 - l);
                    c.map(|channel| channel + lift)
                }))
            }
            Adjustment::HueSaturation { hue, saturation, lightness, perceptual: true } => {
                let (turn, saturation, lightness) = (hue.to_radians(), saturation / 100.0, lightness / 100.0);
                let (sin, cos) = turn.sin_cos();
                Self::Colors(Box::new(move |c| {
                    let [l, a, b] = srgb_to_oklab(c);
                    let (a, b) = ((a * cos - b * sin) * saturation, (a * sin + b * cos) * saturation);
                    fit_oklab([lighten(l, lightness), a, b])
                }))
            }
            Adjustment::HueSaturation { hue, saturation, lightness, perceptual: false } => {
                let (hue, saturation, lightness) = (hue / 360.0, saturation / 100.0, lightness / 100.0);
                Self::Colors(Box::new(move |c| {
                    let [h, s, v] = rgb_to_hsv(c);
                    let c = hsv_to_rgb([(h + hue).rem_euclid(1.0), (s * saturation).clamp(0.0, 1.0), v]);
                    c.map(|channel| lighten(channel, lightness))
                }))
            }
            Adjustment::Invert { mode: InvertMode::Value, .. } => {
                Self::Colors(Box::new(|c| {
                    let [h, s, v] = rgb_to_hsv(c);
                    hsv_to_rgb([h, s, 1.0 - v])
                }))
            }
            Adjustment::Invert { mode, channels, .. } => {
                let invert: fn(f32) -> f32 = match mode {
                    InvertMode::Linear => |c| to_srgb(1.0 - to_linear(c)),
                    _ => |c| 1.0 - c,
                };
                let same: [u8; 256] = std::array::from_fn(|byte| byte as u8);
                let inverted: [u8; 256] = std::array::from_fn(|byte| to_byte(invert(byte as f32 / 255.0)));
                Self::Channels(Box::new(channels.map(|chosen| if chosen { inverted } else { same })))
            }
            Adjustment::Levels { channels, input: [black, white], gamma, output: [low, high] } => {
                let (black, low, high) = (black / 255.0, low / 255.0, high / 255.0);
                // Levels as close as black and white or closer make a threshold.
                let width = (white / 255.0 - black).max(1.0 / 512.0);
                let power = 1.0 / gamma.max(0.01);
                let same: [u8; 256] = std::array::from_fn(|byte| byte as u8);
                let leveled: [u8; 256] = std::array::from_fn(|byte| {
                    let level = ((byte as f32 / 255.0 - black) / width).clamp(0.0, 1.0).powf(power);
                    to_byte(low + level * (high - low))
                });
                Self::Channels(Box::new(channels.map(|chosen| if chosen { leveled } else { same })))
            }
            Adjustment::Posterize { levels } => {
                let steps = (levels.clamp(2, 256) - 1) as f32;
                Self::each_channel(move |c| (c * steps).round() / steps)
            }
            Adjustment::Sepia => Self::Colors(Box::new(|c| SEPIA.map(|weights| dot(c, weights)))),
            Adjustment::TemperatureTint { temperature, tint } => {
                let (warm, magenta) = (temperature / 200.0, tint / 200.0);
                // Light is scaled in linear light, by as much as keeps white as light.
                let gains = [1.0 + warm, 1.0 - magenta, 1.0 - warm];
                let white = dot(gains, LINEAR_LUMINANCE);
                let tables = gains.map(|gain| {
                    std::array::from_fn(|byte| to_byte(to_srgb(to_linear(byte as f32 / 255.0) * gain / white)))
                });
                Self::Channels(Box::new(tables))
            }
            Adjustment::Threshold { low, high } => Self::Colors(Box::new(move |c| {
                let level = (luma(c) * 255.0).round();
                [if (low..=high).contains(&level) { 1.0 } else { 0.0 }; 3]
            })),
        }
    }

    /// The same change made to each of red, green and blue, from 0 to 1.
    fn each_channel(change: impl Fn(f32) -> f32) -> Self {
        let table: [u8; 256] = std::array::from_fn(|byte| to_byte(change(byte as f32 / 255.0)));
        Self::Channels(Box::new([table; 3]))
    }

    fn apply(&self, pixel: &mut [u8; 4]) {
        match self {
            Self::Channels(tables) => {
                for (channel, table) in pixel.iter_mut().zip(tables) {
                    *channel = table[*channel as usize];
                }
            }
            Self::Colors(change) => {
                let color = change([0, 1, 2].map(|channel| pixel[channel] as f32 / 255.0));
                for (channel, value) in pixel.iter_mut().zip(color) {
                    *channel = to_byte(value);
                }
            }
            Self::Pixels(change) => *pixel = change(pixel.map(|channel| channel as f32 / 255.0)).map(to_byte),
        }
    }
}

/// The pixel with `color` taken out of it, as GIMP does: it becomes as transparent as
/// the channel furthest from the colour's lets it, past `transparency` and up to
/// `opacity`, and what is left of it is the colour that, over `color`, looks as it did.
fn color_to_alpha(pixel: [f32; 4], color: [f32; 3], transparency: f32, opacity: f32) -> [f32; 4] {
    let (mut alpha, mut distance) = (0.0f32, 0.0);
    for channel in 0..3 {
        let away = (pixel[channel] - color[channel]).abs();
        let opaque = if away <= transparency {
            0.0
        } else if away >= opacity {
            1.0
        } else {
            // As far as the channel could go from the colour, up to the opacity.
            let room = if pixel[channel] < color[channel] { color[channel] } else { 1.0 - color[channel] };
            (away - transparency) / (opacity.min(room) - transparency)
        };
        if opaque > alpha {
            alpha = opaque;
            distance = away;
        }
    }
    let mut taken = pixel;
    if alpha > 1e-6 {
        let ratio = transparency / distance;
        for channel in 0..3 {
            let near = color[channel] + (pixel[channel] - color[channel]) * ratio;
            taken[channel] = near + (pixel[channel] - near) / alpha;
        }
    }
    taken[3] = pixel[3] * alpha.min(1.0);
    taken
}

/// The colour moved as a whole until it is as light as `light`, which keeps its hue.
fn keep_luma(color: [f32; 3], light: f32) -> [f32; 3] {
    let shift = light - luma(color);
    color.map(|channel| channel + shift)
}

/// What a curve's table maps a level to, read between the two bytes it falls between.
fn look_up(table: &[f32; 256], level: f32) -> f32 {
    let place = level.clamp(0.0, 1.0) * 255.0;
    let below = place.floor() as usize;
    let above = (below + 1).min(255);
    let along = place - below as f32;
    table[below] + (table[above] - table[below]) * along
}

/// Moves a channel towards black, for a negative amount, or white, by that much of the
/// way, from -1 to 1.
fn lighten(channel: f32, amount: f32) -> f32 {
    if amount < 0.0 { channel * (1.0 + amount) } else { channel + (1.0 - channel) * amount }
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn luma(color: [f32; 3]) -> f32 {
    dot(color, LUMA)
}

fn to_byte(value: f32) -> u8 {
    (value * 255.0).round().clamp(0.0, 255.0) as u8
}

fn to_linear(channel: f32) -> f32 {
    if channel < 0.04045 { channel / 12.92 } else { ((channel + 0.055) / 1.055).powf(2.4) }
}

fn to_srgb(channel: f32) -> f32 {
    if channel < 0.0031308 { channel * 12.92 } else { 1.055 * channel.powf(1.0 / 2.4) - 0.055 }
}

/// Hue, saturation and lightness, each from 0 to 1, as red, green and blue.
fn hsl_to_rgb([h, s, l]: [f32; 3]) -> [f32; 3] {
    let reach = s * l.min(1.0 - l);
    [0.0, 8.0, 4.0].map(|offset| {
        let k = (offset + h * 12.0) % 12.0;
        l - reach * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0)
    })
}

/// OKLab's lightness, from 0 to 1, and how green to red and blue to yellow, of an
/// sRGB colour.
fn srgb_to_oklab(color: [f32; 3]) -> [f32; 3] {
    let [r, g, b] = color.map(to_linear);
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

/// The linear red, green and blue of an OKLab colour, which may be past 0 or 1.
fn oklab_to_linear([lightness, a, b]: [f32; 3]) -> [f32; 3] {
    let l = (lightness + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m = (lightness - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let s = (lightness - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    [
        4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s,
        -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s,
        -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s,
    ]
}

/// The sRGB colour of an OKLab one, made less strong as it must be for sRGB to show
/// it, which keeps how light it looks and its hue.
fn fit_oklab(color: [f32; 3]) -> [f32; 3] {
    let shows = |rgb: [f32; 3]| rgb.iter().all(|channel| (-1e-4..=1.0 + 1e-4).contains(channel));
    let [lightness, a, b] = color;
    let lightness = lightness.clamp(0.0, 1.0);
    let mut rgb = oklab_to_linear([lightness, a, b]);
    if !shows(rgb) {
        // How much of its strength is kept, found by halving.
        let (mut kept, mut lost) = (0.0, 1.0);
        rgb = oklab_to_linear([lightness, 0.0, 0.0]);
        for _ in 0..16 {
            let middle = (kept + lost) / 2.0;
            let tried = oklab_to_linear([lightness, a * middle, b * middle]);
            if shows(tried) {
                kept = middle;
                rgb = tried;
            } else {
                lost = middle;
            }
        }
    }
    rgb.map(|channel| to_srgb(channel.clamp(0.0, 1.0)))
}

/// Hue, saturation and value, each from 0 to 1.
pub(crate) fn rgb_to_hsv([r, g, b]: [f32; 3]) -> [f32; 3] {
    let max = r.max(g).max(b);
    let range = max - r.min(g).min(b);
    if range <= 0.0 {
        return [0.0, 0.0, max];
    }
    let sector = if max == r {
        (g - b) / range
    } else if max == g {
        (b - r) / range + 2.0
    } else {
        (r - g) / range + 4.0
    };
    [(sector / 6.0).rem_euclid(1.0), range / max, max]
}

pub(crate) fn hsv_to_rgb([h, s, v]: [f32; 3]) -> [f32; 3] {
    [1.0, 2.0 / 3.0, 1.0 / 3.0].map(|offset| {
        let p = ((h + offset).fract() * 6.0 - 3.0).abs();
        v * (1.0 + ((p - 1.0).clamp(0.0, 1.0) - 1.0) * s)
    })
}

#[cfg(test)]
mod tests {
    use image::Rgba;

    use super::*;
    use crate::{PixelRect, Selection};

    fn one(pixel: [u8; 4], adjustment: &Adjustment) -> [u8; 4] {
        adjust(&RgbaImage::from_pixel(1, 1, Rgba(pixel)), adjustment, None)[(0, 0)].0
    }

    #[test]
    fn settings_that_change_nothing_leave_the_pixels_alone() {
        let pixels = RgbaImage::from_fn(16, 16, |x, y| Rgba([(x * 16) as u8, (y * 16) as u8, (x * y) as u8, 200]));
        let unchanged = [
            Adjustment::BrightnessContrast { brightness: 0.0, contrast: 0.0 },
            Adjustment::Curves(Curves::default()),
            Adjustment::Curves(Curves { luminosity: false, ..Curves::default() }),
            Adjustment::Exposure { exposure: 0.0 },
            Adjustment::HighlightsShadows { shadows: 0.0, highlights: 0.0 },
            Adjustment::HueSaturation { hue: 0.0, saturation: 100.0, lightness: 0.0, perceptual: false },
            Adjustment::Invert { mode: InvertMode::Normal, channels: [false; 3], alpha: false },
            Adjustment::Invert { mode: InvertMode::Linear, channels: [false; 3], alpha: false },
            Adjustment::ColorBalance { shadows: [0.0; 3], midtones: [0.0; 3], highlights: [0.0; 3], preserve_luminosity: false },
            Adjustment::Levels { channels: [true; 3], input: [0.0, 255.0], gamma: 1.0, output: [0.0, 255.0] },
            Adjustment::Posterize { levels: 256 },
            Adjustment::TemperatureTint { temperature: 0.0, tint: 0.0 },
            Adjustment::ColorToAlpha { color: [255; 3], transparency: 0.0, opacity: 0.0 },
            Adjustment::Dither { levels: 256, method: DitherMethod::FloydSteinberg },
            Adjustment::Dither { levels: 256, method: DitherMethod::Ordered },
        ];
        for adjustment in unchanged {
            assert_eq!(adjust(&pixels, &adjustment, None), pixels, "{adjustment:?}");
        }
    }

    #[test]
    fn adjusts_the_colours_and_keeps_the_alpha() {
        let grey = |mode| one([255, 0, 0, 100], &Adjustment::BlackAndWhite { mode });
        assert_eq!(grey(DesaturateMode::Luma), [76, 76, 76, 100]);
        assert_eq!(grey(DesaturateMode::Luminance), [127, 127, 127, 100]);
        assert_eq!(grey(DesaturateMode::Lightness), [128, 128, 128, 100]);
        assert_eq!(grey(DesaturateMode::Average), [85, 85, 85, 100]);
        assert_eq!(grey(DesaturateMode::Value), [255, 255, 255, 100]);
        assert_eq!(one([255, 255, 255, 255], &Adjustment::Sepia), [255, 255, 239, 255]);
        let hue = Adjustment::HueSaturation { hue: 120.0, saturation: 100.0, lightness: 0.0, perceptual: false };
        assert_eq!(one([255, 0, 0, 255], &hue), [0, 255, 0, 255]);
        let grey = Adjustment::HueSaturation { hue: 0.0, saturation: 0.0, lightness: 0.0, perceptual: false };
        assert_eq!(one([255, 0, 0, 255], &grey), [255, 255, 255, 255], "the value is kept");
        let black = Adjustment::HueSaturation { hue: 0.0, saturation: 100.0, lightness: -100.0, perceptual: false };
        assert_eq!(one([30, 60, 90, 255], &black), [0, 0, 0, 255]);
        let brighter = Adjustment::BrightnessContrast { brightness: 100.0, contrast: 0.0 };
        assert_eq!(one([0, 128, 255, 255], &brighter), [128, 192, 255, 255]);
        let threshold = Adjustment::BrightnessContrast { brightness: 0.0, contrast: 100.0 };
        assert_eq!(one([100, 140, 255, 255], &threshold), [0, 255, 255, 255]);
        let flat = Adjustment::BrightnessContrast { brightness: 0.0, contrast: -100.0 };
        assert_eq!(one([0, 200, 255, 255], &flat), [128, 128, 128, 255]);
        assert_eq!(one([100, 0, 255, 255], &Adjustment::Exposure { exposure: 1.0 }), [138, 0, 255, 255]);
        let lifted = Adjustment::HighlightsShadows { shadows: 100.0, highlights: 0.0 };
        let [r, ..] = one([64, 64, 64, 255], &lifted);
        assert!(r > 64);
        assert_eq!(one([0, 0, 0, 255], &lifted), [0, 0, 0, 255], "black stays black");
    }

    #[test]
    fn perceptual_hue_keeps_how_light_colours_look() {
        let turned = Adjustment::HueSaturation { hue: 120.0, saturation: 100.0, lightness: 0.0, perceptual: true };
        let unchanged = Adjustment::HueSaturation { hue: 0.0, saturation: 100.0, lightness: 0.0, perceptual: true };
        let lightness = |pixel: [u8; 4]| srgb_to_oklab([0, 1, 2].map(|channel| pixel[channel] as f32 / 255.0))[0];
        for color in [[200, 40, 40, 255], [30, 120, 220, 255], [90, 90, 90, 255]] {
            let after = one(color, &unchanged);
            assert!(color.iter().zip(after).all(|(a, b)| a.abs_diff(b) <= 1), "{color:?} became {after:?}");
            let after = one(color, &turned);
            assert!((lightness(after) - lightness(color)).abs() < 0.01, "{color:?} became {after:?}");
        }
        let grey = Adjustment::HueSaturation { hue: 0.0, saturation: 0.0, lightness: 0.0, perceptual: true };
        let [r, g, b, _] = one([200, 40, 40, 255], &grey);
        assert!(r == g && g == b);
        let white = Adjustment::HueSaturation { hue: 0.0, saturation: 200.0, lightness: 100.0, perceptual: true };
        assert_eq!(one([200, 40, 40, 255], &white), [255, 255, 255, 255]);
    }

    #[test]
    fn levels_stretch_and_bend_the_chosen_channels() {
        let stretched = Adjustment::Levels { channels: [true; 3], input: [50.0, 150.0], gamma: 1.0, output: [0.0, 255.0] };
        assert_eq!(one([50, 75, 200, 255], &stretched), [0, 64, 255, 255]);
        let red = Adjustment::Levels { channels: [true, false, false], input: [0.0, 255.0], gamma: 1.0, output: [100.0, 200.0] };
        assert_eq!(one([0, 0, 255, 255], &red), [100, 0, 255, 255]);
        let lighter = Adjustment::Levels { channels: [true; 3], input: [0.0, 255.0], gamma: 2.0, output: [0.0, 255.0] };
        let [r, ..] = one([64, 0, 0, 255], &lighter);
        assert_eq!(r, 128);
    }

    #[test]
    fn temperature_warms_and_tint_turns_magenta() {
        let warm = one([128, 128, 128, 255], &Adjustment::TemperatureTint { temperature: 100.0, tint: 0.0 });
        assert!(warm[0] > 128 && warm[2] < 128, "{warm:?}");
        let magenta = one([128, 128, 128, 255], &Adjustment::TemperatureTint { temperature: 0.0, tint: 100.0 });
        assert!(magenta[1] < magenta[0] && magenta[0] == magenta[2], "{magenta:?}");
    }

    #[test]
    fn color_balance_moves_each_range_only() {
        let red_shadows = |preserve_luminosity| Adjustment::ColorBalance {
            shadows: [100.0, 0.0, 0.0],
            midtones: [0.0; 3],
            highlights: [0.0; 3],
            preserve_luminosity,
        };
        assert!(one([20, 20, 20, 255], &red_shadows(false))[0] > 150);
        assert_eq!(one([235, 235, 235, 255], &red_shadows(false)), [235, 235, 235, 255], "the highlights stay");
        let after = one([60, 60, 60, 255], &red_shadows(true));
        assert!(after[0] > 60 && after[1] < 60, "{after:?}");
    }

    #[test]
    fn threshold_colorize_and_posterize() {
        let threshold = Adjustment::Threshold { low: 128.0, high: 255.0 };
        assert_eq!(one([128, 128, 128, 9], &threshold), [255, 255, 255, 9]);
        assert_eq!(one([127, 127, 127, 9], &threshold), [0, 0, 0, 9]);
        let band = Adjustment::Threshold { low: 50.0, high: 100.0 };
        assert_eq!(one([200, 200, 200, 255], &band), [0, 0, 0, 255]);
        let red = Adjustment::Colorize { hue: 0.0, saturation: 100.0, lightness: 0.0 };
        assert_eq!(one([127, 127, 127, 255], &red), [254, 0, 0, 255]);
        assert_eq!(one([0, 0, 0, 255], &red), [0, 0, 0, 255]);
        let two = Adjustment::Posterize { levels: 2 };
        assert_eq!(one([20, 127, 128, 255], &two), [0, 0, 255, 255]);
        let three = Adjustment::Posterize { levels: 3 };
        assert_eq!(one([60, 100, 200, 255], &three), [0, 128, 255, 255]);
    }

    #[test]
    fn color_to_alpha_takes_the_colour_out() {
        let white = Adjustment::ColorToAlpha { color: [255; 3], transparency: 0.0, opacity: 100.0 };
        assert_eq!(one([255, 255, 255, 255], &white)[3], 0);
        assert_eq!(one([0, 0, 0, 255], &white), [0, 0, 0, 255]);
        // Grey over white: half transparent black.
        assert_eq!(one([128, 128, 128, 255], &white), [0, 0, 0, 127]);
        assert_eq!(one([255, 128, 128, 200], &white), [255, 0, 0, 100]);
        let near = Adjustment::ColorToAlpha { color: [255; 3], transparency: 10.0, opacity: 100.0 };
        assert_eq!(one([240, 240, 240, 255], &near)[3], 0);
    }

    #[test]
    fn dithering_leaves_only_the_levels_and_keeps_the_average() {
        let pixels = RgbaImage::from_pixel(32, 32, Rgba([64, 128, 191, 255]));
        for method in [DitherMethod::FloydSteinberg, DitherMethod::Atkinson, DitherMethod::Ordered, DitherMethod::Random] {
            let dithered = adjust(&pixels, &Adjustment::Dither { levels: 2, method }, None);
            assert!(dithered.pixels().all(|pixel| pixel.0[..3].iter().all(|&c| c == 0 || c == 255) && pixel[3] == 255));
            let mean = dithered.pixels().map(|pixel| pixel[0] as f32).sum::<f32>() / 1024.0;
            let tolerance = if method == DitherMethod::Atkinson { 40.0 } else { 16.0 };
            assert!((mean - 64.0).abs() < tolerance, "{method:?}: {mean}");
        }
        let mask = SelectionMask::of(&Selection::rect(PixelRect { left: 0, top: 0, right: 16, bottom: 32 }), 32, 32).unwrap();
        let half = adjust(&pixels, &Adjustment::Dither { levels: 2, method: DitherMethod::FloydSteinberg }, Some(&mask));
        assert_eq!(half[(20, 5)], Rgba([64, 128, 191, 255]));
    }

    #[test]
    fn the_bayer_matrix_holds_each_place_once() {
        let mut places: Vec<u32> = (0..64).map(|i| (bayer(i % 8, i / 8) * 64.0) as u32).collect();
        assert_eq!([bayer(0, 0), bayer(1, 0), bayer(0, 1), bayer(1, 1)].map(|b| (b * 64.0) as u32), [0, 32, 48, 16]);
        places.sort_unstable();
        assert_eq!(places, (0..64).collect::<Vec<_>>());
    }

    #[test]
    fn auto_level_stretches_to_black_and_white() {
        let pixels = RgbaImage::from_fn(2, 1, |x, _| if x == 0 { Rgba([50, 60, 70, 255]) } else { Rgba([150, 200, 170, 255]) });
        let channels = adjust(&pixels, &Adjustment::AutoLevel { mode: AutoLevelMode::Channels, clip: 0.0 }, None);
        assert_eq!(channels.as_raw(), &[0, 0, 0, 255, 255, 255, 255, 255]);
        let together = adjust(&pixels, &Adjustment::AutoLevel { mode: AutoLevelMode::Together, clip: 0.0 }, None);
        assert_eq!(together.as_raw(), &[0, 17, 34, 255, 170, 255, 204, 255]);
        let hsv = adjust(&pixels, &Adjustment::AutoLevel { mode: AutoLevelMode::Hsv, clip: 0.0 }, None);
        let [_, _, value] = rgb_to_hsv([0, 1, 2].map(|channel| hsv[(0, 0)][channel] as f32 / 255.0));
        assert_eq!(value, 0.0);
        assert_eq!(hsv[(1, 0)][1], 255);
        // Transparent pixels are not measured.
        let mut hidden = pixels.clone();
        hidden.put_pixel(1, 0, Rgba([255, 255, 255, 0]));
        let one = adjust(&hidden, &Adjustment::AutoLevel { mode: AutoLevelMode::Channels, clip: 0.0 }, None);
        assert_eq!(one[(0, 0)], pixels[(0, 0)], "a single colour is left as it is");
    }

    #[test]
    fn equalize_spreads_the_levels() {
        let pixels = RgbaImage::from_fn(4, 1, |x, _| Rgba([[10, 11, 12, 13][x as usize]; 4]));
        let equalized = adjust(&pixels, &Adjustment::Equalize { lightness: false }, None);
        assert_eq!(equalized.pixels().map(|pixel| pixel[0]).collect::<Vec<_>>(), [0, 85, 170, 255]);
        let light = adjust(&pixels, &Adjustment::Equalize { lightness: true }, None);
        assert_eq!(light[(3, 0)].0, [255, 255, 255, 13]);
    }

    #[test]
    fn white_balance_makes_the_average_grey() {
        let pixels = RgbaImage::from_pixel(2, 1, Rgba([200, 150, 100, 255]));
        let balanced = adjust(&pixels, &Adjustment::WhiteBalance { strength: 100.0 }, None);
        for pixel in balanced.pixels() {
            assert!(pixel[0].abs_diff(pixel[1]) <= 1 && pixel[1].abs_diff(pixel[2]) <= 1, "{pixel:?}");
        }
        assert_eq!(adjust(&pixels, &Adjustment::WhiteBalance { strength: 0.0 }, None), pixels);
    }

    #[test]
    fn inverts_the_chosen_channels() {
        assert_eq!(one([0, 100, 255, 30], &Adjustment::INVERT_COLORS), [255, 155, 0, 30]);
        assert_eq!(one([0, 100, 255, 30], &Adjustment::INVERT_ALPHA), [0, 100, 255, 225]);
        let red_and_alpha = Adjustment::Invert { mode: InvertMode::Normal, channels: [true, false, false], alpha: true };
        assert_eq!(one([10, 100, 200, 255], &red_and_alpha), [245, 100, 200, 0]);
        let linear = Adjustment::Invert { mode: InvertMode::Linear, channels: [true; 3], alpha: false };
        assert_eq!(one([0, 255, 188, 255], &linear), [255, 0, 187, 255], "half the light stays about half");
        let value = Adjustment::Invert { mode: InvertMode::Value, channels: [false; 3], alpha: false };
        assert_eq!(one([255, 0, 0, 200], &value), [0, 0, 0, 200]);
        assert_eq!(one([0, 0, 0, 255], &value), [255, 255, 255, 255]);
        assert_eq!(one([64, 32, 0, 255], &value), [191, 96, 0, 255], "the hue and saturation are kept");
    }

    #[test]
    fn curves_map_each_channel_or_the_lightness() {
        let inverted = vec![[0.0, 1.0], [1.0, 0.0]];
        let mut curves = Curves { luminosity: false, ..Curves::default() };
        curves.channels[0] = inverted.clone();
        assert_eq!(one([0, 10, 20, 255], &Adjustment::Curves(curves)), [255, 10, 20, 255]);
        let lighter = Curves { light: vec![[0.0, 0.5], [1.0, 1.0]], ..Curves::default() };
        assert_eq!(one([0, 0, 0, 255], &Adjustment::Curves(lighter)), [128, 128, 128, 255]);
    }

    #[test]
    fn a_curve_goes_through_its_points_without_overshooting() {
        let points = [[0.0, 0.0], [0.25, 0.6], [0.5, 0.6], [1.0, 1.0]];
        let levels = sample_curve(&points);
        assert_eq!(levels[0], 0.0);
        assert_eq!(levels[255], 1.0);
        // Flat between two points at the same height.
        assert!(levels[64..=127].iter().all(|&level| (level - 0.6).abs() < 1e-5));
        assert!(levels.windows(2).all(|pair| pair[0] <= pair[1] + 1e-6), "never goes back down");
        let straight = sample_curve(&Curves::STRAIGHT);
        assert!(straight.iter().enumerate().all(|(level, &y)| (y - level as f32 / 255.0).abs() < 1e-5));
    }

    #[test]
    fn only_the_selection_is_adjusted() {
        let pixels = RgbaImage::from_pixel(300, 300, Rgba([255, 0, 0, 255]));
        let selection = Selection::rect(PixelRect { left: 10, top: 250, right: 20, bottom: 260 });
        let mask = SelectionMask::of(&selection, 300, 300).unwrap();
        let adjusted = adjust(&pixels, &Adjustment::BlackAndWhite { mode: DesaturateMode::Luma }, Some(&mask));
        assert_eq!(adjusted[(15, 255)], Rgba([76, 76, 76, 255]));
        assert_eq!(adjusted[(5, 255)], Rgba([255, 0, 0, 255]));
        assert_eq!(adjusted[(15, 100)], Rgba([255, 0, 0, 255]));
    }
}

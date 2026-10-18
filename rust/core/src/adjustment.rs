//! The adjustments of the Colors menu, which change the colours of each pixel on
//! its own: no pixel looks at any other, and how opaque each is stays as it was, but
//! for an inversion that turns it upside down as well. Those that change each
//! channel on its own go through a table of what each byte becomes. Large images are
//! cut into bands of rows, adjusted at the same time on as many threads as there are
//! cores.

use std::{
    f64::consts::{FRAC_PI_2, FRAC_PI_4},
    num::NonZero,
    thread,
};

use image::RgbaImage;

use crate::SelectionMask;

/// Fewest pixels worth sharing between threads.
const PARALLEL_PIXELS: usize = 1 << 16;
/// Weights of red, green and blue in how light a colour looks.
const LUMA: [f32; 3] = [0.299, 0.587, 0.114];
/// What red, green and blue each become in sepia, from each of them.
const SEPIA: [[f32; 3]; 3] = [[0.393, 0.769, 0.189], [0.349, 0.686, 0.168], [0.272, 0.534, 0.131]];

/// An adjustment, with its settings.
#[derive(Clone, Debug, PartialEq)]
pub enum Adjustment {
    BlackAndWhite,
    /// Each from -100 to 100.
    BrightnessContrast { brightness: f32, contrast: f32 },
    Curves(Curves),
    /// In stops: each doubles or halves the light.
    Exposure { exposure: f32 },
    /// Each from -100, darker, to 100, lighter.
    HighlightsShadows { shadows: f32, highlights: f32 },
    /// The hue in degrees around the colour wheel, the saturation in percent, 0 for grey
    /// and 100 for no change, and the lightness from -100, black, to 100, white.
    HueSaturation { hue: f32, saturation: f32, lightness: f32 },
    /// Turns red, green and blue upside down, each that `channels` says, as `mode` says,
    /// and how opaque each pixel is if `alpha`.
    Invert { mode: InvertMode, channels: [bool; 3], alpha: bool },
    Sepia,
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
    let mapping = Mapping::of(adjustment);
    let mut adjusted = pixels.clone();
    let row_pixels = pixels.width() as usize;
    let rows = pixels.height() as usize;
    let selected = mask.map(SelectionMask::flags);
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
}

impl Mapping {
    fn of(adjustment: &Adjustment) -> Self {
        match *adjustment {
            Adjustment::BlackAndWhite => Self::Colors(Box::new(|c| [luma(c); 3])),
            Adjustment::BrightnessContrast { brightness, contrast } => {
                // Brightness moves each channel towards black or white, by up to half the way.
                let b = brightness / 200.0;
                // Contrast steepens or flattens the line through middle grey: flat grey at
                // -100, a threshold at 100.
                let angle = ((f64::from(contrast) / 100.0 + 1.0) * FRAC_PI_4).min(FRAC_PI_2 - 1e-6);
                let slant = angle.tan() as f32;
                Self::each_channel(|c| (lighten(c, b) - 0.5) * slant + 0.5)
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
            Adjustment::HueSaturation { hue, saturation, lightness } => {
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
            Adjustment::Sepia => Self::Colors(Box::new(|c| SEPIA.map(|weights| dot(c, weights)))),
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
        }
    }
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
            Adjustment::HueSaturation { hue: 0.0, saturation: 100.0, lightness: 0.0 },
            Adjustment::Invert { mode: InvertMode::Normal, channels: [false; 3], alpha: false },
            Adjustment::Invert { mode: InvertMode::Linear, channels: [false; 3], alpha: false },
        ];
        for adjustment in unchanged {
            assert_eq!(adjust(&pixels, &adjustment, None), pixels, "{adjustment:?}");
        }
    }

    #[test]
    fn adjusts_the_colours_and_keeps_the_alpha() {
        assert_eq!(one([255, 0, 0, 100], &Adjustment::BlackAndWhite), [76, 76, 76, 100]);
        assert_eq!(one([255, 255, 255, 255], &Adjustment::Sepia), [255, 255, 239, 255]);
        let hue = Adjustment::HueSaturation { hue: 120.0, saturation: 100.0, lightness: 0.0 };
        assert_eq!(one([255, 0, 0, 255], &hue), [0, 255, 0, 255]);
        let grey = Adjustment::HueSaturation { hue: 0.0, saturation: 0.0, lightness: 0.0 };
        assert_eq!(one([255, 0, 0, 255], &grey), [255, 255, 255, 255], "the value is kept");
        let black = Adjustment::HueSaturation { hue: 0.0, saturation: 100.0, lightness: -100.0 };
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
        let adjusted = adjust(&pixels, &Adjustment::BlackAndWhite, Some(&mask));
        assert_eq!(adjusted[(15, 255)], Rgba([76, 76, 76, 255]));
        assert_eq!(adjusted[(5, 255)], Rgba([255, 0, 0, 255]));
        assert_eq!(adjusted[(15, 100)], Rgba([255, 0, 0, 255]));
    }
}

//! The colours a document keeps at hand, in order.

use std::{collections::HashSet, ops::RangeInclusive};

use image::{Rgba, RgbaImage};

use crate::palette_presets::from_code;

/// DawnBringer's 32 colours, the palette documents start with: a common start for
/// pixel art.
const DB32: [u32; 32] = [
    0x000000, 0x222034, 0x45283c, 0x663931, 0x8f563b, 0xdf7126, 0xd9a066, 0xeec39a, //
    0xfbf236, 0x99e550, 0x6abe30, 0x37946e, 0x4b692f, 0x524b24, 0x323c39, 0x3f3f74, //
    0x306082, 0x5b6ee1, 0x639bff, 0x5fcde4, 0xcbdbfc, 0xffffff, 0x9badb7, 0x847e87, //
    0x696a6a, 0x595652, 0x76428a, 0xac3232, 0xd95763, 0xd77bba, 0x8f974a, 0x8a6f30,
];

pub const DEFAULT_PALETTE: [Rgba<u8>; 32] = {
    let mut colors = [Rgba([0, 0, 0, 255]); 32];
    let mut index = 0;
    while index < DB32.len() {
        colors[index] = from_code(DB32[index]);
        index += 1;
    }
    colors
};

/// How many entries a palette made from an image has at most.
pub const MAX_PALETTE_SIZE: usize = 256;

/// A document's palette: its colours in order, and which of them, if any, stands for
/// no colour at all, as the transparent index of an indexed image does.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Palette {
    pub colors: Vec<Rgba<u8>>,
    pub transparent: Option<usize>,
}

impl Palette {
    pub fn new(colors: Vec<Rgba<u8>>) -> Self {
        Self { colors, transparent: None }
    }

    pub fn len(&self) -> usize {
        self.colors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.colors.is_empty()
    }

    pub fn is_full(&self) -> bool {
        self.colors.len() >= MAX_PALETTE_SIZE
    }

    /// The colour entry `index` paints with: its own, or none at all for the
    /// transparent one.
    pub fn color(&self, index: usize) -> Rgba<u8> {
        let mut color = self.colors[index];
        if self.transparent == Some(index) {
            color[3] = 0;
        }
        color
    }

    /// Puts `color` before entry `index`, or at the end with `index` the length.
    pub fn insert(&mut self, index: usize, color: Rgba<u8>) {
        self.colors.insert(index, color);
        if let Some(transparent) = self.transparent.as_mut()
            && *transparent >= index
        {
            *transparent += 1;
        }
    }

    /// Takes out the entries of `range`. The transparent one among them leaves no
    /// entry transparent.
    pub fn remove(&mut self, range: RangeInclusive<usize>) {
        let (first, last) = (*range.start(), *range.end());
        self.colors.drain(range);
        self.transparent = match self.transparent {
            Some(transparent) if transparent > last => Some(transparent - (last + 1 - first)),
            Some(transparent) if transparent >= first => None,
            other => other,
        };
    }

    /// Cuts the palette down to `len` entries, or adds black ones up to it, no more
    /// than [`MAX_PALETTE_SIZE`].
    pub fn resize(&mut self, len: usize) {
        let len = len.min(MAX_PALETTE_SIZE);
        self.colors.resize(len, Rgba([0, 0, 0, 255]));
        self.transparent = self.transparent.filter(|&transparent| transparent < len);
    }

    /// Sets the entries between the ends of `range` to the colours evenly spaced
    /// between theirs.
    pub fn ramp(&mut self, range: RangeInclusive<usize>) {
        let (first, last) = (*range.start(), *range.end());
        let (from, to) = (self.colors[first], self.colors[last]);
        let steps = (last - first) as f32;
        for index in first + 1..last {
            let along = (index - first) as f32 / steps;
            let mix = |channel: usize| (from[channel] as f32 + (to[channel] as f32 - from[channel] as f32) * along).round() as u8;
            self.colors[index] = Rgba(std::array::from_fn(mix));
        }
    }
}

/// How many bits of each channel tell apart the colours cut down to a palette.
const BIN_BITS: u32 = 5;
const BIN_SIDE: usize = 1 << BIN_BITS;

/// A palette of the colours of `image`, darkest first: each of them, if there are no
/// more than `max`, or else `max` that stand for them. Those are found by cutting the
/// colours in two, again and again, across the widest spread of red, green or blue,
/// at the middle pixel, and averaging the pixels of each part. The pixels that do not
/// show at all are left out, and the others are taken as opaque.
pub fn palette_of(image: &RgbaImage, max: usize) -> Vec<Rgba<u8>> {
    let shown = || image.pixels().filter(|pixel| pixel[3] > 0).map(|pixel| [pixel[0], pixel[1], pixel[2]]);
    let mut palette = exact_colors(shown(), max).unwrap_or_else(|| cut_down(shown(), max));
    palette.sort_by_key(|&[red, green, blue]| 299 * red as u32 + 587 * green as u32 + 114 * blue as u32);
    palette.into_iter().map(|[red, green, blue]| Rgba([red, green, blue, 255])).collect()
}

/// Each of the colours, unless there are more than `max`.
fn exact_colors(colors: impl Iterator<Item = [u8; 3]>, max: usize) -> Option<Vec<[u8; 3]>> {
    let mut seen = HashSet::new();
    let mut last = None;
    for color in colors {
        // Pixels next to each other are often the same.
        if last != Some(color) && seen.insert(color) && seen.len() > max {
            return None;
        }
        last = Some(color);
    }
    Some(seen.into_iter().collect())
}

/// The pixels of the colours that fall in one bin, the colours with the same first
/// [`BIN_BITS`] of each channel.
#[derive(Clone, Copy, Default)]
struct Bin {
    /// Where the bin is along red, green and blue.
    at: [u8; 3],
    count: u64,
    sums: [u64; 3],
}

/// `max` colours that stand for those of `colors`.
fn cut_down(colors: impl Iterator<Item = [u8; 3]>, max: usize) -> Vec<[u8; 3]> {
    let mut bins = vec![Bin::default(); BIN_SIDE * BIN_SIDE * BIN_SIDE];
    for color in colors {
        let at = color.map(|channel| channel >> (8 - BIN_BITS));
        let bin = &mut bins[(at[0] as usize * BIN_SIDE + at[1] as usize) * BIN_SIDE + at[2] as usize];
        bin.at = at;
        bin.count += 1;
        for (sum, channel) in bin.sums.iter_mut().zip(color) {
            *sum += channel as u64;
        }
    }
    bins.retain(|bin| bin.count > 0);
    let mut parts = vec![bins.as_mut_slice()];
    while parts.len() < max {
        let widest = parts
            .iter()
            .enumerate()
            .filter(|(_, bins)| bins.len() > 1)
            .map(|(index, bins)| (index, widest_channel(bins)))
            .max_by_key(|&(_, (_, spread))| spread);
        let Some((index, (channel, _))) = widest else {
            break;
        };
        let bins = parts.swap_remove(index);
        bins.sort_unstable_by_key(|bin| bin.at[channel]);
        let half = bins.iter().map(|bin| bin.count).sum::<u64>() / 2;
        let mut seen = 0;
        let middle = bins
            .iter()
            .position(|bin| {
                seen += bin.count;
                seen > half
            })
            .unwrap_or(0)
            .clamp(1, bins.len() - 1);
        let (low, high) = bins.split_at_mut(middle);
        parts.extend([low, high]);
    }
    parts.iter().map(|bins| average(bins)).collect()
}

/// The channel whose bins are spread the widest, and how wide.
fn widest_channel(bins: &[Bin]) -> (usize, u8) {
    (0..3)
        .map(|channel| {
            let values = bins.iter().map(|bin| bin.at[channel]);
            let spread = values.clone().max().unwrap_or(0) - values.min().unwrap_or(0);
            (channel, spread)
        })
        .max_by_key(|&(_, spread)| spread)
        .unwrap_or((0, 0))
}

/// The colour of the pixels of `bins` on average.
fn average(bins: &[Bin]) -> [u8; 3] {
    let count = bins.iter().map(|bin| bin.count).sum::<u64>().max(1);
    std::array::from_fn(|channel| {
        let sum: u64 = bins.iter().map(|bin| bin.sums[channel]).sum();
        ((sum + count / 2) / count) as u8
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn few_colors_are_kept_as_they_are_darkest_first() {
        let mut image = RgbaImage::from_pixel(4, 1, Rgba([255, 255, 255, 255]));
        image.put_pixel(1, 0, Rgba([255, 0, 0, 255]));
        image.put_pixel(2, 0, Rgba([0, 0, 0, 128]));
        image.put_pixel(3, 0, Rgba([0, 255, 0, 0]));
        let palette = palette_of(&image, MAX_PALETTE_SIZE);
        assert_eq!(palette, [Rgba([0, 0, 0, 255]), Rgba([255, 0, 0, 255]), Rgba([255, 255, 255, 255])]);
    }

    #[test]
    fn many_colors_are_cut_down_to_the_most() {
        let image = RgbaImage::from_fn(64, 64, |x, y| Rgba([(x * 4) as u8, (y * 4) as u8, 128, 255]));
        let palette = palette_of(&image, 16);
        assert_eq!(palette.len(), 16);
        assert_eq!(palette, palette_of(&image, 16));
        // Each pixel has an entry near it.
        for pixel in image.pixels() {
            let nearest = palette
                .iter()
                .map(|entry| (0..3).map(|channel| entry[channel].abs_diff(pixel[channel]) as u32).max().unwrap())
                .min()
                .unwrap();
            assert!(nearest <= 40, "{pixel:?} is {nearest} away");
        }
    }

    fn gray(level: u8) -> Rgba<u8> {
        Rgba([level, level, level, 255])
    }

    fn grays(levels: &[u8]) -> Palette {
        Palette::new(levels.iter().map(|&level| gray(level)).collect())
    }

    #[test]
    fn the_transparent_entry_paints_with_no_color() {
        let mut palette = grays(&[10, 20]);
        palette.transparent = Some(1);
        assert_eq!(palette.color(0), gray(10));
        assert_eq!(palette.color(1), Rgba([20, 20, 20, 0]));
    }

    #[test]
    fn the_transparent_index_follows_its_entry() {
        let mut palette = grays(&[0, 1, 2, 3, 4]);
        palette.transparent = Some(3);
        palette.insert(1, gray(9));
        assert_eq!(palette.transparent, Some(4));
        palette.insert(5, gray(9));
        assert_eq!(palette.transparent, Some(4));
        palette.remove(0..=1);
        assert_eq!(palette.transparent, Some(2));
        assert_eq!(palette.color(2), Rgba([3, 3, 3, 0]));
        palette.remove(3..=3);
        assert_eq!(palette.transparent, Some(2));
        palette.remove(1..=2);
        assert_eq!(palette.transparent, None);
    }

    #[test]
    fn resizing_adds_black_entries_or_cuts_the_last_ones() {
        let mut palette = grays(&[10, 20, 30]);
        palette.transparent = Some(2);
        palette.resize(5);
        assert_eq!(palette, Palette { transparent: Some(2), ..grays(&[10, 20, 30, 0, 0]) });
        palette.resize(2);
        assert_eq!(palette, grays(&[10, 20]));
        palette.resize(1000);
        assert_eq!(palette.len(), MAX_PALETTE_SIZE);
    }

    #[test]
    fn a_ramp_spaces_the_colors_between_its_ends_evenly() {
        let mut palette = grays(&[7, 0, 10, 0, 0, 50, 7]);
        palette.colors[5][3] = 0;
        palette.ramp(2..=5);
        assert_eq!(palette.colors[..2], grays(&[7, 0]).colors);
        assert_eq!(palette.colors[2..6], [gray(10), Rgba([23, 23, 23, 170]), Rgba([37, 37, 37, 85]), Rgba([50, 50, 50, 0])]);
        assert_eq!(palette.colors[6], gray(7));
    }

    #[test]
    fn an_image_that_shows_nothing_has_an_empty_palette() {
        assert!(palette_of(&RgbaImage::new(3, 3), MAX_PALETTE_SIZE).is_empty());
    }
}

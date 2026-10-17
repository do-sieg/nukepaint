//! The colours a document keeps at hand, in order.

use std::collections::HashSet;

use image::{Rgba, RgbaImage};

/// The palette documents start with: DawnBringer's 32 colours, a common start for
/// pixel art.
pub const DEFAULT_PALETTE: [Rgba<u8>; 32] = {
    const CODES: [u32; 32] = [
        0x000000, 0x222034, 0x45283c, 0x663931, 0x8f563b, 0xdf7126, 0xd9a066, 0xeec39a, //
        0xfbf236, 0x99e550, 0x6abe30, 0x37946e, 0x4b692f, 0x524b24, 0x323c39, 0x3f3f74, //
        0x306082, 0x5b6ee1, 0x639bff, 0x5fcde4, 0xcbdbfc, 0xffffff, 0x9badb7, 0x847e87, //
        0x696a6a, 0x595652, 0x76428a, 0xac3232, 0xd95763, 0xd77bba, 0x8f974a, 0x8a6f30,
    ];
    let mut colors = [Rgba([0, 0, 0, 255]); 32];
    let mut index = 0;
    while index < CODES.len() {
        let [_, red, green, blue] = CODES[index].to_be_bytes();
        colors[index] = Rgba([red, green, blue, 255]);
        index += 1;
    }
    colors
};

/// How many entries a palette made from an image has at most.
pub const MAX_PALETTE_SIZE: usize = 256;

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

    #[test]
    fn an_image_that_shows_nothing_has_an_empty_palette() {
        assert!(palette_of(&RgbaImage::new(3, 3), MAX_PALETTE_SIZE).is_empty());
    }
}

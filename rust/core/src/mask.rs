//! Which pixels of an image the tools may change: the selected ones, or all of them
//! while nothing is selected.

use image::RgbaImage;

use crate::{PixelRect, Selection};

/// The selected pixels of an image, one flag for each of its pixels.
#[derive(Clone)]
pub struct SelectionMask {
    width: u32,
    height: u32,
    selected: Vec<bool>,
}

impl SelectionMask {
    /// The pixels the selection holds of an image of this size, or nothing while
    /// nothing is selected, as the whole image may then change. A selection off the
    /// image holds none of its pixels.
    pub fn of(selection: &Selection, width: u32, height: u32) -> Option<Self> {
        if selection.is_empty() {
            return None;
        }
        let mut selected = vec![false; width as usize * height as usize];
        let within = PixelRect { left: 0, top: 0, right: width as i32, bottom: height as i32 };
        for span in selection.spans(within) {
            for y in span.top..span.bottom {
                let row = y as usize * width as usize;
                selected[row + span.left as usize..row + span.right as usize].fill(true);
            }
        }
        Some(Self { width, height, selected })
    }

    /// Whether the pixel at `x`, `y` of the image is selected.
    pub fn contains(&self, x: u32, y: u32) -> bool {
        x < self.width && y < self.height && self.selected[(y * self.width + x) as usize]
    }

    /// The smallest rectangle around the selected pixels, if any are.
    pub fn bounds(&self) -> Option<PixelRect> {
        let width = self.width as usize;
        let mut rows = self.selected.chunks(width.max(1)).enumerate().filter(|(_, row)| row.contains(&true));
        let (top, _) = rows.next()?;
        let bottom = rows.next_back().map_or(top, |(y, _)| y) + 1;
        let columns = (0..width).filter(|&x| (top..bottom).any(|y| self.selected[y * width + x]));
        let (mut left, mut right) = (usize::MAX, 0);
        for x in columns {
            (left, right) = (left.min(x), x + 1);
        }
        Some(PixelRect { left: left as i32, top: top as i32, right: right as i32, bottom: bottom as i32 })
    }

    /// Whether each pixel of the image is selected, row after row.
    pub(crate) fn flags(&self) -> &[bool] {
        &self.selected
    }

    /// Puts the pixels of `area` that are not selected back as they were in `before`,
    /// undoing what changed them. Both images must be as large as the mask.
    pub fn restore_unselected(&self, before: &RgbaImage, after: &mut RgbaImage, area: PixelRect) {
        let left = area.left.max(0) as u32;
        let top = area.top.max(0) as u32;
        let right = (area.right.max(0) as u32).min(self.width);
        let bottom = (area.bottom.max(0) as u32).min(self.height);
        for y in top..bottom {
            for x in left..right {
                if !self.contains(x, y) {
                    after.put_pixel(x, y, *before.get_pixel(x, y));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use image::Rgba;

    use super::*;
    use crate::SelectionMode;

    #[test]
    fn nothing_selected_leaves_all_open() {
        assert!(SelectionMask::of(&Selection::default(), 4, 4).is_none());
    }

    #[test]
    fn holds_the_selected_pixels_on_the_image() {
        let square = Selection::rect(PixelRect { left: -2, top: 1, right: 3, bottom: 4 });
        let hole = Selection::rect(PixelRect { left: 1, top: 2, right: 2, bottom: 3 });
        let mask = SelectionMask::of(&square.combine(&hole, SelectionMode::Subtract), 4, 4).unwrap();
        assert!(mask.contains(0, 1) && mask.contains(2, 3));
        assert!(!mask.contains(1, 2), "in the hole");
        assert!(!mask.contains(3, 1) && !mask.contains(0, 0) && !mask.contains(9, 2));
        let off = Selection::rect(PixelRect { left: 10, top: 10, right: 12, bottom: 12 });
        let mask = SelectionMask::of(&off, 4, 4).unwrap();
        assert!((0..4).all(|y| (0..4).all(|x| !mask.contains(x, y))));
    }

    #[test]
    fn restores_what_is_not_selected() {
        let before = RgbaImage::new(4, 4);
        let mut after = RgbaImage::from_pixel(4, 4, Rgba([255, 0, 0, 255]));
        let selection = Selection::rect(PixelRect { left: 1, top: 1, right: 3, bottom: 3 });
        let mask = SelectionMask::of(&selection, 4, 4).unwrap();
        mask.restore_unselected(&before, &mut after, PixelRect { left: 0, top: 0, right: 3, bottom: 4 });
        assert_eq!(after[(1, 1)], Rgba([255, 0, 0, 255]));
        assert_eq!(after[(0, 0)], Rgba([0, 0, 0, 0]));
        assert_eq!(after[(2, 3)], Rgba([0, 0, 0, 0]));
        // Outside the area, nothing is put back.
        assert_eq!(after[(3, 0)], Rgba([255, 0, 0, 255]));
    }
}

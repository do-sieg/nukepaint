//! The pixels of an image that a selection holds: copied out of it, or erased.

use image::{Rgba, RgbaImage};

use crate::{PixelRect, Selection};

/// The selected pixels of the image, in the smallest rectangle around them on it, the
/// others transparent; and where that rectangle is. Nothing if none of the selection
/// is on the image.
pub fn copy_selected(image: &RgbaImage, selection: &Selection) -> Option<(PixelRect, RgbaImage)> {
    let area = area_on(image, selection)?;
    let spans = selection.spans(area);
    if spans.is_empty() {
        return None;
    }
    let mut copied = RgbaImage::new(area.width() as u32, area.height() as u32);
    for span in spans {
        for y in span.top..span.bottom {
            for x in span.left..span.right {
                let pixel = *image.get_pixel(x as u32, y as u32);
                copied.put_pixel((x - area.left) as u32, (y - area.top) as u32, pixel);
            }
        }
    }
    Some((area, copied))
}

/// Makes the selected pixels of the image transparent. Returns whether any were.
pub fn erase_selected(image: &mut RgbaImage, selection: &Selection) -> bool {
    let Some(area) = area_on(image, selection) else {
        return false;
    };
    let spans = selection.spans(area);
    for span in &spans {
        for y in span.top..span.bottom {
            for x in span.left..span.right {
                image.put_pixel(x as u32, y as u32, Rgba([0, 0, 0, 0]));
            }
        }
    }
    !spans.is_empty()
}

/// The part of the image around the selection.
fn area_on(image: &RgbaImage, selection: &Selection) -> Option<PixelRect> {
    let bounds = selection.bounds()?;
    let area = PixelRect {
        left: (bounds.left.floor() as i32).max(0),
        top: (bounds.top.floor() as i32).max(0),
        right: (bounds.right.ceil() as i32).min(image.width() as i32),
        bottom: (bounds.bottom.ceil() as i32).min(image.height() as i32),
    };
    (!area.is_empty()).then_some(area)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SelectionMode;

    const RED: Rgba<u8> = Rgba([255, 0, 0, 255]);
    const BLUE: Rgba<u8> = Rgba([0, 0, 255, 255]);
    const CLEAR: Rgba<u8> = Rgba([0, 0, 0, 0]);

    /// A blue image with a red pixel at 3, 3.
    fn image() -> RgbaImage {
        RgbaImage::from_fn(8, 8, |x, y| if (x, y) == (3, 3) { RED } else { BLUE })
    }

    #[test]
    fn copy_keeps_only_the_selected_pixels() {
        let hole = Selection::rect(PixelRect { left: 3, top: 3, right: 4, bottom: 4 });
        let square = Selection::rect(PixelRect { left: 2, top: 2, right: 5, bottom: 5 });
        let ring = square.combine(&hole, SelectionMode::Subtract);
        let (area, copied) = copy_selected(&image(), &ring).unwrap();
        assert_eq!(area, PixelRect { left: 2, top: 2, right: 5, bottom: 5 });
        assert_eq!(copied.dimensions(), (3, 3));
        assert_eq!(*copied.get_pixel(0, 0), BLUE);
        // The hole in the middle is not copied.
        assert_eq!(*copied.get_pixel(1, 1), CLEAR);
    }

    #[test]
    fn copy_stops_at_the_edges() {
        let over = Selection::rect(PixelRect { left: 6, top: -2, right: 12, bottom: 2 });
        let (area, copied) = copy_selected(&image(), &over).unwrap();
        assert_eq!(area, PixelRect { left: 6, top: 0, right: 8, bottom: 2 });
        assert!(copied.pixels().all(|&pixel| pixel == BLUE));
        let outside = Selection::rect(PixelRect { left: 20, top: 20, right: 24, bottom: 24 });
        assert!(copy_selected(&image(), &outside).is_none());
    }

    #[test]
    fn erase_clears_the_selected_pixels() {
        let mut erased = image();
        assert!(erase_selected(&mut erased, &Selection::rect(PixelRect { left: 3, top: 3, right: 5, bottom: 4 })));
        assert_eq!(*erased.get_pixel(3, 3), CLEAR);
        assert_eq!(*erased.get_pixel(4, 3), CLEAR);
        assert_eq!(*erased.get_pixel(5, 3), BLUE);
        assert_eq!(*erased.get_pixel(3, 4), BLUE);
        let outside = Selection::rect(PixelRect { left: 20, top: 20, right: 24, bottom: 24 });
        assert!(!erase_selected(&mut erased, &outside));
    }
}

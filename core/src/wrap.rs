//! Drawing that wraps around the edges of an area of the image, as on a tile repeated
//! side by side: what goes off one edge comes back in at the other. The area can be
//! the whole image, or a part of it.

use crate::PixelRect;

/// The segment from `last` to `at`, both moved by the same whole widths and heights
/// of `area` so that it starts in it, for drawing there. A stroke keeps `at` as it
/// was given, for the next segment to start from it in the same place. `at` is taken to go on from `last` without a
/// jump: off one edge, it goes on past it, to come back in at the other.
pub(crate) fn segment(area: PixelRect, last: (f64, f64), at: (f64, f64)) -> ((f64, f64), (f64, f64)) {
    let along = |last: f64, start: i32, size: i32| {
        let (start, size) = (start as f64, size as f64);
        ((last - start) / size).floor() * size
    };
    let shift = (along(last.0, area.left, area.width()), along(last.1, area.top, area.height()));
    ((last.0 - shift.0, last.1 - shift.1), (at.0 - shift.0, at.1 - shift.1))
}

/// Where the pixel at `x`, `y`, in `area` or out of it, is in it.
pub(crate) fn pixel(area: PixelRect, x: i64, y: i64) -> (u32, u32) {
    let along = |at: i64, start: i32, size: i32| (start as i64 + (at - start as i64).rem_euclid(size as i64)) as u32;
    (along(x, area.left, area.width()), along(y, area.top, area.height()))
}

/// The part of `area` that `changed`, in it or out of it, covers once wrapped around:
/// all the way across wherever it goes over an edge.
pub(crate) fn changed_area(area: PixelRect, changed: PixelRect) -> Option<PixelRect> {
    if changed.is_empty() {
        return None;
    }
    let along = |from: i32, to: i32, start: i32, size: i32| {
        let from_in = (from - start).rem_euclid(size);
        let to_in = from_in + (to - from);
        if to - from >= size || to_in > size { (start, start + size) } else { (start + from_in, start + to_in) }
    };
    let (left, right) = along(changed.left, changed.right, area.left, area.width());
    let (top, bottom) = along(changed.top, changed.bottom, area.top, area.height());
    Some(PixelRect { left, top, right, bottom })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> PixelRect {
        PixelRect { left, top, right, bottom }
    }

    #[test]
    fn segment_starts_in_the_area() {
        let whole = rect(0, 0, 10, 8);
        assert_eq!(segment(whole, (12.0, 3.0), (13.0, 4.0)), ((2.0, 3.0), (3.0, 4.0)));
        assert_eq!(segment(whole, (-1.0, 3.0), (1.0, -3.0)), ((9.0, 3.0), (11.0, -3.0)));
        let part = rect(4, 2, 8, 6);
        assert_eq!(segment(part, (9.0, 1.0), (10.0, 1.0)), ((5.0, 5.0), (6.0, 5.0)));
    }

    #[test]
    fn pixels_wrap_into_the_area() {
        let part = rect(4, 2, 8, 6);
        assert_eq!(pixel(part, 5, 3), (5, 3));
        assert_eq!(pixel(part, 8, 1), (4, 5));
        assert_eq!(pixel(part, -1, 10), (7, 2));
    }

    #[test]
    fn changed_area_goes_across_an_edge() {
        let whole = rect(0, 0, 10, 8);
        assert_eq!(changed_area(whole, rect(12, 2, 14, 4)), Some(rect(2, 2, 4, 4)));
        assert_eq!(changed_area(whole, rect(8, -1, 12, 1)), Some(rect(0, 0, 10, 8)));
        assert_eq!(changed_area(whole, rect(-30, 1, 30, 2)), Some(rect(0, 1, 10, 2)));
        assert_eq!(changed_area(whole, rect(1, 1, 1, 2)), None);
        assert_eq!(changed_area(rect(4, 2, 8, 6), rect(7, 3, 9, 4)), Some(rect(4, 3, 8, 4)));
    }
}

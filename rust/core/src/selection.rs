//! The part of an image the tools act on: the pixels whose middle is inside outlines
//! drawn over the image.

use i_overlay::core::{fill_rule::FillRule, overlay_rule::OverlayRule};
use i_overlay::float::single::SingleFloatOverlay;

use crate::Affine;

/// What a selection tool does to the selection there already is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectionMode {
    /// Selects what the tool draws, and nothing else.
    #[default]
    Replace,
    /// Selects what the tool draws as well as what was selected.
    Add,
    /// Deselects what the tool draws.
    Subtract,
    /// Keeps selected only what the tool draws over.
    Intersect,
    /// Selects what the tool draws that was not selected, and deselects what was.
    Invert,
}

impl SelectionMode {
    pub const ALL: [Self; 5] = [Self::Replace, Self::Add, Self::Subtract, Self::Intersect, Self::Invert];
}

/// A rectangle of whole pixels, from the corner at its top left to the one at its
/// bottom right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl PixelRect {
    /// The rectangle between two corners, given in any order.
    pub fn from_corners(a: (i32, i32), b: (i32, i32)) -> Self {
        Self { left: a.0.min(b.0), top: a.1.min(b.1), right: a.0.max(b.0), bottom: a.1.max(b.1) }
    }

    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }

    pub fn is_empty(&self) -> bool {
        self.width() <= 0 || self.height() <= 0
    }
}

/// A rectangle anywhere on the image's plane, from its top left to its bottom right.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

impl Bounds {
    pub fn width(&self) -> f64 {
        self.right - self.left
    }

    pub fn height(&self) -> f64 {
        self.bottom - self.top
    }

    pub fn center(&self) -> (f64, f64) {
        ((self.left + self.right) / 2.0, (self.top + self.bottom) / 2.0)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Selection {
    /// Closed outlines, in pixels of the image, which do not cross each other. They
    /// start on corners between pixels, and can be moved, stretched and turned from
    /// there. A pixel is selected if its middle is inside an odd number of them, so an
    /// outline inside another is a hole in it.
    outlines: Vec<Vec<(f64, f64)>>,
}

impl Selection {
    /// Selects the pixels of a rectangle, or nothing if it is empty.
    pub fn rect(rect: PixelRect) -> Self {
        if rect.is_empty() {
            return Self::default();
        }
        let PixelRect { left, top, right, bottom } = rect;
        Self::from_corners(vec![(left, top), (right, top), (right, bottom), (left, bottom)])
    }

    fn from_corners(corners: Vec<(i32, i32)>) -> Self {
        Self { outlines: vec![corners.into_iter().map(|(x, y)| (x as f64, y as f64)).collect()] }
    }

    /// Selects the pixels whose middle is in the ellipse that fits in a rectangle, or
    /// nothing if it is empty. Its outline goes around them in steps.
    pub fn ellipse(rect: PixelRect) -> Self {
        if rect.is_empty() {
            return Self::default();
        }
        let (radius_x, radius_y) = (rect.width() as f64 / 2.0, rect.height() as f64 / 2.0);
        let (center_x, center_y) = (rect.left as f64 + radius_x, rect.top as f64 + radius_y);
        // The pixels selected on each row, which are next to each other, from the top down.
        let mut rows = Vec::new();
        for y in rect.top..rect.bottom {
            let along = (y as f64 + 0.5 - center_y) / radius_y;
            let reach = radius_x * (1.0 - along * along).max(0.0).sqrt();
            let left = (center_x - reach - 0.5).ceil() as i32;
            let right = (center_x + reach - 0.5).floor() as i32 + 1;
            if left < right {
                rows.push((y, left, right));
            }
        }
        // Down the right side, then back up the left one.
        let mut outline = Vec::with_capacity(rows.len() * 4);
        for &(y, _, right) in &rows {
            outline.extend([(right, y), (right, y + 1)]);
        }
        for &(y, left, _) in rows.iter().rev() {
            outline.extend([(left, y + 1), (left, y)]);
        }
        // Steps that go on along the same line are one edge.
        outline.dedup();
        let mut straight = Vec::with_capacity(outline.len());
        for (i, &point) in outline.iter().enumerate() {
            let before = outline[(i + outline.len() - 1) % outline.len()];
            let after = outline[(i + 1) % outline.len()];
            if !(before.0 == point.0 && point.0 == after.0 || before.1 == point.1 && point.1 == after.1) {
                straight.push(point);
            }
        }
        Self::from_corners(straight)
    }

    pub fn is_empty(&self) -> bool {
        self.outlines.is_empty()
    }

    pub fn outlines(&self) -> &[Vec<(f64, f64)>] {
        &self.outlines
    }

    /// The smallest rectangle around the outlines, if there are any.
    pub fn bounds(&self) -> Option<Bounds> {
        let mut points = self.outlines.iter().flatten();
        let &(x, y) = points.next()?;
        let start = Bounds { left: x, top: y, right: x, bottom: y };
        Some(points.fold(start, |bounds, &(x, y)| Bounds {
            left: bounds.left.min(x),
            top: bounds.top.min(y),
            right: bounds.right.max(x),
            bottom: bounds.bottom.max(y),
        }))
    }

    /// The selection carried somewhere else by `map`.
    pub fn transformed(&self, map: &Affine) -> Selection {
        let outlines = self.outlines.iter();
        Self { outlines: outlines.map(|outline| outline.iter().map(|&point| map.apply(point)).collect()).collect() }
    }

    /// What a selection tool leaves selected after drawing `drawn` over this one.
    pub fn combine(&self, drawn: &Selection, mode: SelectionMode) -> Selection {
        use SelectionMode::*;
        if drawn.is_empty() {
            return if matches!(mode, Replace | Intersect) { Selection::default() } else { self.clone() };
        }
        if mode == Replace || self.is_empty() {
            return if matches!(mode, Subtract | Intersect) { Selection::default() } else { drawn.clone() };
        }
        let rule = match mode {
            Replace | Add => OverlayRule::Union,
            Subtract => OverlayRule::Difference,
            Intersect => OverlayRule::Intersect,
            Invert => OverlayRule::Xor,
        };
        self.overlay(drawn, rule)
    }

    fn overlay(&self, other: &Selection, rule: OverlayRule) -> Selection {
        let shapes = self.points().overlay(&other.points(), rule, FillRule::EvenOdd);
        // A shape is an outline and the outlines of its holes, which even-odd filling tells apart.
        let outlines = shapes.into_iter().flatten();
        Self { outlines: outlines.map(|outline| outline.into_iter().map(|[x, y]| (x, y)).collect()).collect() }
    }

    fn points(&self) -> Vec<Vec<[f64; 2]>> {
        let outlines = self.outlines.iter();
        outlines.map(|outline| outline.iter().map(|&(x, y)| [x, y]).collect()).collect()
    }

    /// Whether the pixel at `x`, `y` is selected.
    pub fn contains(&self, x: i64, y: i64) -> bool {
        // Whether a line from the pixel's middle going right crosses an odd number of edges.
        let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
        let mut inside = false;
        for ((ax, ay), (bx, by)) in self.edges() {
            if (ay > py) != (by > py) && px < ax + (py - ay) / (by - ay) * (bx - ax) {
                inside = !inside;
            }
        }
        inside
    }

    /// The selected pixels within `within`, as rectangles that do not overlap, in bands
    /// from top to bottom: rows that select the same pixels are one band.
    pub fn spans(&self, within: PixelRect) -> Vec<PixelRect> {
        let mut spans = Vec::new();
        let Some(bounds) = self.bounds() else {
            return spans;
        };
        // The rows whose middle can be inside.
        let first = ((bounds.top - 0.5).ceil() as i64).max(within.top as i64);
        let end = ((bounds.bottom - 0.5).ceil() as i64).min(within.bottom as i64);
        if first >= end {
            return spans;
        }
        // Where each row's middle crosses the edges, gone through edge by edge so that
        // each only looks at the rows it reaches.
        let mut crossings = vec![Vec::new(); (end - first) as usize];
        for ((ax, ay), (bx, by)) in self.edges() {
            if ay == by {
                continue;
            }
            let rows = ((ay.min(by) - 0.5).ceil() as i64).max(first)..((ay.max(by) - 0.5).ceil() as i64).min(end);
            for y in rows {
                let py = y as f64 + 0.5;
                crossings[(y - first) as usize].push(ax + (py - ay) / (by - ay) * (bx - ax));
            }
        }
        let mut previous: Vec<(i32, i32)> = Vec::new();
        let mut band_start = 0;
        let mut row = Vec::new();
        for (y, xs) in (first..end).zip(&mut crossings) {
            xs.sort_unstable_by(f64::total_cmp);
            row.clear();
            for &[from, to] in xs.as_chunks::<2>().0 {
                // The pixels whose middle is from `from` up to `to`.
                let left = ((from - 0.5).ceil() as i64).max(within.left as i64) as i32;
                let right = ((to - 0.5).ceil() as i64).min(within.right as i64) as i32;
                if left < right {
                    row.push((left, right));
                }
            }
            let y = y as i32;
            let goes_on = row == previous && spans.last().is_some_and(|span: &PixelRect| span.bottom == y);
            if goes_on {
                spans[band_start..].iter_mut().for_each(|span: &mut PixelRect| span.bottom += 1);
            } else {
                band_start = spans.len();
                spans.extend(row.iter().map(|&(left, right)| PixelRect { left, top: y, right, bottom: y + 1 }));
                std::mem::swap(&mut previous, &mut row);
            }
        }
        spans
    }

    fn edges(&self) -> impl Iterator<Item = ((f64, f64), (f64, f64))> + '_ {
        self.outlines.iter().flat_map(|outline| {
            let next = outline.iter().cycle().skip(1);
            outline.iter().copied().zip(next.copied())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> Selection {
        Selection::rect(PixelRect { left, top, right, bottom })
    }

    const EVERYWHERE: PixelRect = PixelRect { left: -1000, top: -1000, right: 1000, bottom: 1000 };

    fn area(selection: &Selection) -> i32 {
        selection.spans(EVERYWHERE).iter().map(|span| span.width() * span.height()).sum()
    }

    #[test]
    fn empty_rect_selects_nothing() {
        assert!(rect(3, 3, 3, 10).is_empty());
        assert!(Selection::rect(PixelRect::from_corners((5, 5), (5, 5))).is_empty());
    }

    #[test]
    fn corners_in_any_order() {
        assert_eq!(PixelRect::from_corners((8, 2), (3, 6)), PixelRect { left: 3, top: 2, right: 8, bottom: 6 });
    }

    #[test]
    fn ellipse_selects_the_pixels_whose_middle_is_in_it() {
        let ellipse = Selection::ellipse(PixelRect { left: 10, top: 20, right: 20, bottom: 26 });
        assert_eq!(ellipse.outlines().len(), 1);
        for y in 20..26 {
            for x in 10..20 {
                let (dx, dy) = ((x as f64 + 0.5 - 15.0) / 5.0, (y as f64 + 0.5 - 23.0) / 3.0);
                assert_eq!(ellipse.contains(x, y), dx * dx + dy * dy <= 1.0, "pixel {x}, {y}");
            }
        }
        assert!(!ellipse.contains(9, 23) && !ellipse.contains(20, 23));
        // Every edge is horizontal or vertical, so the spans cover exactly what is selected.
        let selected = (20..26).flat_map(|y| (10..20).map(move |x| (x, y)));
        assert_eq!(area(&ellipse), selected.filter(|&(x, y)| ellipse.contains(x, y)).count() as i32);
    }

    #[test]
    fn small_ellipses_fill_their_rect() {
        for (width, height) in [(1, 1), (2, 2), (1, 5), (3, 1)] {
            let ellipse = Selection::ellipse(PixelRect { left: 0, top: 0, right: width, bottom: height });
            assert_eq!(area(&ellipse), width * height, "{width} × {height}");
        }
        assert!(Selection::ellipse(PixelRect { left: 0, top: 0, right: 0, bottom: 4 }).is_empty());
    }

    #[test]
    fn replace_keeps_only_the_drawn_area() {
        let combined = rect(0, 0, 10, 10).combine(&rect(20, 20, 25, 25), SelectionMode::Replace);
        assert_eq!(combined, rect(20, 20, 25, 25));
        assert!(rect(0, 0, 10, 10).combine(&Selection::default(), SelectionMode::Replace).is_empty());
    }

    #[test]
    fn add_joins_overlapping_rects_into_one_outline() {
        let combined = rect(0, 0, 10, 10).combine(&rect(5, 5, 15, 15), SelectionMode::Add);
        assert_eq!(combined.outlines().len(), 1);
        assert_eq!(area(&combined), 100 + 100 - 25);
        assert!(combined.contains(12, 12));
        assert!(!combined.contains(12, 2));
    }

    #[test]
    fn add_keeps_apart_rects_that_do_not_touch() {
        let combined = rect(0, 0, 4, 4).combine(&rect(10, 0, 14, 4), SelectionMode::Add);
        assert_eq!(combined.outlines().len(), 2);
        assert_eq!(area(&combined), 32);
    }

    #[test]
    fn add_around_leaves_a_hole() {
        let mut ring = Selection::default();
        for part in [rect(0, 0, 9, 3), rect(0, 6, 9, 9), rect(0, 3, 3, 6), rect(6, 3, 9, 6)] {
            ring = ring.combine(&part, SelectionMode::Add);
        }
        assert_eq!(ring.outlines().len(), 2);
        assert_eq!(area(&ring), 81 - 9);
        assert!(!ring.contains(4, 4));
        assert!(ring.contains(1, 4));
    }

    #[test]
    fn subtract_cuts_out_the_drawn_area() {
        let cut = rect(0, 0, 10, 10).combine(&rect(5, 0, 15, 10), SelectionMode::Subtract);
        assert_eq!(area(&cut), 50);
        assert!(cut.contains(4, 4));
        assert!(!cut.contains(5, 4));
        let hole = rect(0, 0, 9, 9).combine(&rect(3, 3, 6, 6), SelectionMode::Subtract);
        assert_eq!(hole.outlines().len(), 2);
        assert_eq!(area(&hole), 81 - 9);
    }

    #[test]
    fn intersect_keeps_the_overlap() {
        let overlap = rect(0, 0, 10, 10).combine(&rect(5, 5, 15, 15), SelectionMode::Intersect);
        assert_eq!(area(&overlap), 25);
        assert!(overlap.contains(7, 7));
        assert!(rect(0, 0, 4, 4).combine(&rect(8, 8, 9, 9), SelectionMode::Intersect).is_empty());
    }

    #[test]
    fn invert_flips_what_the_drawn_area_covers() {
        let flipped = rect(0, 0, 10, 10).combine(&rect(5, 5, 15, 15), SelectionMode::Invert);
        assert_eq!(area(&flipped), 75 + 75);
        assert!(!flipped.contains(7, 7));
        assert!(flipped.contains(12, 12));
        assert!(flipped.contains(2, 2));
    }

    #[test]
    fn with_nothing_on_one_side() {
        use SelectionMode::*;
        let (some, none) = (rect(0, 0, 4, 4), Selection::default());
        for (mode, keeps) in [(Replace, false), (Add, true), (Subtract, true), (Intersect, false), (Invert, true)] {
            assert_eq!(some.combine(&none, mode).is_empty(), !keeps, "{mode:?} with nothing drawn");
        }
        for (mode, selects) in [(Replace, true), (Add, true), (Subtract, false), (Intersect, false), (Invert, true)] {
            assert_eq!(none.combine(&some, mode).is_empty(), !selects, "{mode:?} with nothing selected");
        }
    }

    #[test]
    fn spans_are_bands_of_the_same_rows() {
        let spans = rect(0, 0, 10, 10).combine(&rect(5, 5, 15, 15), SelectionMode::Add).spans(EVERYWHERE);
        assert_eq!(
            spans,
            [
                PixelRect { left: 0, top: 0, right: 10, bottom: 5 },
                PixelRect { left: 0, top: 5, right: 15, bottom: 10 },
                PixelRect { left: 5, top: 10, right: 15, bottom: 15 },
            ]
        );
    }

    #[test]
    fn spans_stay_within() {
        let spans = rect(-5, -5, 5, 5).spans(PixelRect { left: 0, top: 0, right: 3, bottom: 100 });
        assert_eq!(spans, [PixelRect { left: 0, top: 0, right: 3, bottom: 5 }]);
    }

    #[test]
    fn transformed_moves_the_outlines() {
        let moved = rect(0, 0, 4, 2).transformed(&Affine::translation(3.0, 1.0));
        assert_eq!(moved, rect(3, 1, 7, 3));
        assert_eq!(moved.bounds(), Some(Bounds { left: 3.0, top: 1.0, right: 7.0, bottom: 3.0 }));
    }

    #[test]
    fn turned_selection_holds_the_pixels_whose_middle_is_inside() {
        let turn = Affine::translation(-10.0, -10.0)
            .then(Affine::rotation(0.5))
            .then(Affine::translation(10.0, 10.0));
        let turned = rect(4, 6, 16, 14).transformed(&turn);
        let inside = (0..20).flat_map(|y| (0..20).map(move |x| (x, y)));
        let count = inside.filter(|&(x, y)| turned.contains(x, y)).count() as i32;
        assert_eq!(area(&turned), count);
        // About as many as the rectangle had.
        assert!((count - 96).abs() <= 6, "{count}");
        for span in turned.spans(EVERYWHERE) {
            for y in span.top..span.bottom {
                for x in span.left..span.right {
                    assert!(turned.contains(x as i64, y as i64));
                }
            }
        }
    }

    #[test]
    fn adding_nothing_keeps_the_selection() {
        let selection = rect(0, 0, 4, 4);
        assert_eq!(selection.combine(&Selection::default(), SelectionMode::Add), selection);
    }

    #[test]
    fn contains_whole_pixels_only() {
        let selection = rect(2, 2, 4, 4);
        assert!(selection.contains(2, 2));
        assert!(selection.contains(3, 3));
        assert!(!selection.contains(4, 3));
        assert!(!selection.contains(1, 2));
    }
}

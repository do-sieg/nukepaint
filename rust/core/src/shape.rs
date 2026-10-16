//! The shapes the shape tools draw: a Line, a Curve, a Rectangle or an Ellipse, empty
//! or filled, and a Contour or a Polygon, which are filled. Each goes through the
//! middle of the pixels its points are on, in a line as wide as asked, of a colour
//! blended over what is under it, with smooth edges if asked to. However often it goes
//! over a pixel, it changes it no more than the once it covered it most. A shape may
//! wrap around the edges of an area, coming back in at one edge as it goes off the other.

use std::f64::consts::{PI, TAU};

use image::{Rgba, RgbaImage};

use crate::{
    BlendMode, MAX_TIP_SIZE, Overlaps, PixelRect, Selection, SelectionMask,
    blend::blend_pixel,
    stroke::{distance_to_segment, pixel_of, tip},
    wrap,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeKind {
    Line,
    Curve,
    Rectangle,
    FilledRectangle,
    Ellipse,
    FilledEllipse,
    /// Drawn freehand.
    Contour,
    Polygon,
}

impl ShapeKind {
    /// Whether it fills what its outline goes around.
    pub fn is_filled(self) -> bool {
        matches!(self, Self::FilledRectangle | Self::FilledEllipse | Self::Contour | Self::Polygon)
    }

    /// Whether its outline goes back to where it began.
    pub fn is_closed(self) -> bool {
        !matches!(self, Self::Line | Self::Curve)
    }

    /// Whether it is placed by the rectangle it fits in.
    pub fn is_framed(self) -> bool {
        matches!(self, Self::Rectangle | Self::FilledRectangle | Self::Ellipse | Self::FilledEllipse)
    }
}

/// A shape, by the pixels its points are on: a Line's two ends; a Curve's two ends,
/// then the points it bends towards from each; opposite corners of the rectangle a
/// Rectangle or an Ellipse fits in; or the corners of a Contour or a Polygon.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub kind: ShapeKind,
    pub points: Vec<(i32, i32)>,
    /// How far it is turned about [`Shape::center`], in radians clockwise.
    pub angle: f64,
}

impl Shape {
    /// A shape that is not turned.
    pub fn new(kind: ShapeKind, points: Vec<(i32, i32)>) -> Self {
        Self { kind, points, angle: 0.0 }
    }

    /// The middle of the box around the middle of the pixels its points are on, which
    /// it turns about.
    pub fn center(&self) -> (f64, f64) {
        let xs = self.points.iter().map(|point| point.0);
        let ys = self.points.iter().map(|point| point.1);
        let middle = |low: Option<i32>, high: Option<i32>| (low.unwrap_or(0) + high.unwrap_or(0)) as f64 / 2.0 + 0.5;
        (middle(xs.clone().min(), xs.max()), middle(ys.clone().min(), ys.max()))
    }

    /// The points its outline goes through, on the image, in pixels.
    pub fn outline(&self) -> Vec<(f64, f64)> {
        let outline = self.upright_outline();
        if self.angle == 0.0 {
            return outline;
        }
        let ((center_x, center_y), (sin, cos)) = (self.center(), self.angle.sin_cos());
        let turned = |(x, y): (f64, f64)| {
            let (dx, dy) = (x - center_x, y - center_y);
            (center_x + dx * cos - dy * sin, center_y + dx * sin + dy * cos)
        };
        outline.into_iter().map(turned).collect()
    }

    /// Its outline as it would be if it were not turned.
    fn upright_outline(&self) -> Vec<(f64, f64)> {
        let points: Vec<(f64, f64)> = self.points.iter().map(|&(x, y)| (x as f64 + 0.5, y as f64 + 0.5)).collect();
        match self.kind {
            ShapeKind::Curve => curve_points(points[0], points[2], points[3], points[1]),
            kind if kind.is_framed() => {
                let (a, b) = (points[0], points[1]);
                // An ellipse no wider or no taller than a pixel is the line it then is.
                let round = matches!(kind, ShapeKind::Ellipse | ShapeKind::FilledEllipse);
                if round && a.0 != b.0 && a.1 != b.1 { ellipse_points(a, b) } else { vec![a, (b.0, a.1), b, (a.0, b.1)] }
            }
            _ => points,
        }
    }
}

/// Points along a cubic Bézier curve, near enough to each other to be joined by
/// straight lines.
fn curve_points(from: (f64, f64), bend_from: (f64, f64), bend_to: (f64, f64), to: (f64, f64)) -> Vec<(f64, f64)> {
    let length = |a: (f64, f64), b: (f64, f64)| (b.0 - a.0).hypot(b.1 - a.1);
    // The curve is no longer than the lines between its points.
    let reach = length(from, bend_from) + length(bend_from, bend_to) + length(bend_to, to);
    let steps = ((reach / 2.0).ceil() as usize).clamp(1, 1024);
    (0..=steps)
        .map(|i| {
            let t = i as f64 / steps as f64;
            let u = 1.0 - t;
            let weights = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
            let along = |pick: fn((f64, f64)) -> f64| {
                weights[0] * pick(from) + weights[1] * pick(bend_from) + weights[2] * pick(bend_to) + weights[3] * pick(to)
            };
            (along(|point| point.0), along(|point| point.1))
        })
        .collect()
}

/// Points around the ellipse that fits in the rectangle between two opposite corners.
fn ellipse_points(a: (f64, f64), b: (f64, f64)) -> Vec<(f64, f64)> {
    let radius = ((b.0 - a.0).abs() / 2.0, (b.1 - a.1).abs() / 2.0);
    let center = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
    // Enough sides to stay within a twentieth of a pixel of the true curve, and a
    // multiple of four so that the outline touches the middle of each side of the rectangle.
    let sides = (((PI * (10.0 * radius.0.max(radius.1)).sqrt() / 4.0).ceil() as usize) * 4).clamp(8, 512);
    (0..sides)
        .map(|i| {
            let (sin, cos) = (TAU * i as f64 / sides as f64).sin_cos();
            (center.0 + cos * radius.0, center.1 + sin * radius.1)
        })
        .collect()
}

/// How a shape is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShapeStyle {
    pub color: Rgba<u8>,
    /// How wide its line is, in pixels.
    pub size: u32,
    /// Whether its edges fade out over a pixel rather than stop at one.
    pub smooth: bool,
    /// What a Contour or a Polygon fills where its outline crosses itself.
    pub overlaps: Overlaps,
}

/// A layer with a shape drawn on it, which can be drawn again in another place or way
/// as often as asked, each time in place of the last.
pub struct ShapeCanvas {
    /// The layer's pixels without the shape.
    base: RgbaImage,
    /// With it.
    pixels: RgbaImage,
    /// The pixels it may change, if not all of them.
    mask: Option<SelectionMask>,
    /// Where it was drawn last, which is all of `pixels` that can differ from `base`.
    drawn: Option<PixelRect>,
    /// The area shapes wrap around the edges of, if they do.
    wrap: Option<PixelRect>,
}

impl ShapeCanvas {
    /// A canvas of the layer's pixels, on which shapes change only those `mask` holds,
    /// if there is one.
    pub fn new(base: RgbaImage, mask: Option<SelectionMask>) -> Self {
        Self { pixels: base.clone(), base, mask, drawn: None, wrap: None }
    }

    /// The canvas, with shapes wrapping around the edges of `area` if there is one.
    pub fn wrapping(self, area: Option<PixelRect>) -> Self {
        Self { wrap: area, ..self }
    }

    /// The layer's pixels with the shape drawn last.
    pub fn pixels(&self) -> &RgbaImage {
        &self.pixels
    }

    /// Draws a shape in place of the one drawn last. Returns the area that changed, if
    /// any of it is on the image.
    pub fn draw(&mut self, shape: &Shape, style: &ShapeStyle) -> Option<PixelRect> {
        let cleared = self.clear();
        self.drawn = draw(&mut self.pixels, &self.base, shape, style, self.mask.as_ref(), self.wrap);
        union(cleared, self.drawn)
    }

    /// Takes the shape drawn last off. Returns the area that changed, if any did.
    pub fn clear(&mut self) -> Option<PixelRect> {
        let area = self.drawn.take()?;
        let row = self.base.width() as usize * 4;
        let (from, to) = (area.left as usize * 4, area.right as usize * 4);
        let target: &mut [u8] = &mut self.pixels;
        for y in area.top as usize..area.bottom as usize {
            target[y * row + from..y * row + to].copy_from_slice(&self.base.as_raw()[y * row + from..y * row + to]);
        }
        Some(area)
    }

    /// Whether the shape drawn last changed any pixel.
    pub fn changed(&self) -> bool {
        let Some(area) = self.drawn else {
            return false;
        };
        (area.top..area.bottom).any(|y| {
            (area.left..area.right).any(|x| self.pixels.get_pixel(x as u32, y as u32) != self.base.get_pixel(x as u32, y as u32))
        })
    }
}

/// Draws a shape on `pixels`, over `base` where it covers them, and only on the pixels
/// `mask` holds if there is one, wrapping around the edges of `wrap` if there is one.
/// Returns the area it went over, if any of it is on the image.
fn draw(
    pixels: &mut RgbaImage,
    base: &RgbaImage,
    shape: &Shape,
    style: &ShapeStyle,
    mask: Option<&SelectionMask>,
    wrap: Option<PixelRect>,
) -> Option<PixelRect> {
    let outline = shape.outline();
    let size = style.size.clamp(1, MAX_TIP_SIZE);
    // Room for the line and its smooth edge on either side of the outline.
    let reach = size as f64 / 2.0 + 2.0;
    let (mut left, mut top, mut right, mut bottom) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for &(x, y) in &outline {
        (left, top, right, bottom) = (left.min(x), top.min(y), right.max(x), bottom.max(y));
    }
    let reached = PixelRect {
        left: (left - reach).floor() as i32,
        top: (top - reach).floor() as i32,
        right: (right + reach).ceil() as i32,
        bottom: (bottom + reach).ceil() as i32,
    };
    // Wrapping, it is drawn where it is, on or off the image, then folded into the area.
    let area = if wrap.is_some() {
        reached
    } else {
        PixelRect {
            left: reached.left.clamp(0, pixels.width() as i32),
            top: reached.top.clamp(0, pixels.height() as i32),
            right: reached.right.clamp(0, pixels.width() as i32),
            bottom: reached.bottom.clamp(0, pixels.height() as i32),
        }
    };
    if area.is_empty() {
        return None;
    }
    let mut coverage = Coverage::new(area);
    if shape.kind.is_filled() {
        // As with the lassos, an outline that crosses itself fills what the overlaps say.
        let overlaps = if shape.kind.is_framed() { Overlaps::Add } else { style.overlaps };
        for span in Selection::enclosed(&outline, overlaps).spans(area) {
            coverage.fill(span);
        }
    }
    let tip = tip(size);
    let segments = if shape.kind.is_closed() { outline.len() } else { outline.len().saturating_sub(1).max(1) };
    for i in 0..segments {
        let (from, to) = (outline[i], outline[(i + 1) % outline.len()]);
        if style.smooth {
            coverage.smooth_segment(from, to, size);
        } else {
            coverage.hard_segment(from, to, &tip);
        }
    }
    let (coverage, area) = match wrap {
        Some(wrap) => {
            let mut folded = Coverage::new(wrap);
            for y in area.top..area.bottom {
                for x in area.left..area.right {
                    let (on_x, on_y) = wrap::pixel(wrap, x as i64, y as i64);
                    folded.cover(on_x as i64, on_y as i64, coverage.at(x, y));
                }
            }
            (folded, wrap::changed_area(wrap, area)?)
        }
        None => (coverage, area),
    };
    for y in area.top..area.bottom {
        for x in area.left..area.right {
            let covered = coverage.at(x, y);
            let (x, y) = (x as u32, y as u32);
            if covered == 0 || mask.is_some_and(|mask| !mask.contains(x, y)) {
                continue;
            }
            let mut pixel = base.get_pixel(x, y).0;
            blend_pixel(&mut pixel, &style.color.0, covered as f32 / 255.0, BlendMode::Normal);
            pixels.put_pixel(x, y, Rgba(pixel));
        }
    }
    Some(area)
}

/// How much of each pixel of an area a shape covers, out of 255.
struct Coverage {
    area: PixelRect,
    values: Vec<u8>,
}

impl Coverage {
    fn new(area: PixelRect) -> Self {
        Self { area, values: vec![0; area.width() as usize * area.height() as usize] }
    }

    fn at(&self, x: i32, y: i32) -> u8 {
        self.values[((y - self.area.top) * self.area.width() + x - self.area.left) as usize]
    }

    /// Covers the pixel at `x`, `y` by `value`, unless it is covered as much already or
    /// is outside the area.
    fn cover(&mut self, x: i64, y: i64, value: u8) {
        let PixelRect { left, top, right, bottom } = self.area;
        if (left as i64..right as i64).contains(&x) && (top as i64..bottom as i64).contains(&y) {
            let index = ((y - top as i64) * self.area.width() as i64 + x - left as i64) as usize;
            self.values[index] = self.values[index].max(value);
        }
    }

    /// Covers a rectangle of the area whole.
    fn fill(&mut self, span: PixelRect) {
        for y in span.top..span.bottom {
            for x in span.left..span.right {
                self.cover(x as i64, y as i64, u8::MAX);
            }
        }
    }

    /// Puts the hard tip, as rows from the top left of the square it fits in, down on
    /// each pixel from the one under `from` to the one under `to`.
    fn hard_segment(&mut self, from: (f64, f64), to: (f64, f64), tip: &[(i64, i64)]) {
        let (from, to) = (pixel_of(from), pixel_of(to));
        let (dx, dy) = (to.0 - from.0, to.1 - from.1);
        let count = dx.abs().max(dy.abs());
        let reach = (tip.len() / 2) as i64;
        for i in 0..=count {
            let along = |delta: i64| (delta as f64 * i as f64 / count.max(1) as f64).round() as i64;
            let (left, top) = (from.0 + along(dx) - reach, from.1 + along(dy) - reach);
            for (row, &(start, end)) in tip.iter().enumerate() {
                for x in start..end {
                    self.cover(left + x, top + row as i64, u8::MAX);
                }
            }
        }
    }

    /// Covers the pixels within half of `size` of the segment from `from` to `to`, the
    /// edge fading out over one pixel, half of it on either side of the outline.
    fn smooth_segment(&mut self, from: (f64, f64), to: (f64, f64), size: u32) {
        let outer = size as f64 / 2.0 + 0.5;
        let PixelRect { left, top, right, bottom } = self.area;
        let rows = ((from.1.min(to.1) - outer).floor() as i32).max(top)..((from.1.max(to.1) + outer).ceil() as i32).min(bottom);
        let columns = ((from.0.min(to.0) - outer).floor() as i32).max(left)..((from.0.max(to.0) + outer).ceil() as i32).min(right);
        for y in rows {
            for x in columns.clone() {
                let middle = (x as f64 + 0.5, y as f64 + 0.5);
                let coverage = (outer - distance_to_segment(middle, from, to)).min(1.0);
                if coverage > 0.0 {
                    self.cover(x as i64, y as i64, (coverage * 255.0).round() as u8);
                }
            }
        }
    }
}

/// The smallest rectangle around both, if there is either.
fn union(a: Option<PixelRect>, b: Option<PixelRect>) -> Option<PixelRect> {
    match (a, b) {
        (Some(a), Some(b)) => Some(PixelRect {
            left: a.left.min(b.left),
            top: a.top.min(b.top),
            right: a.right.max(b.right),
            bottom: a.bottom.max(b.bottom),
        }),
        (a, b) => a.or(b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLACK: Rgba<u8> = Rgba([0, 0, 0, 255]);

    fn style(size: u32, smooth: bool) -> ShapeStyle {
        ShapeStyle { color: BLACK, size, smooth, overlaps: Overlaps::Add }
    }

    fn shape(kind: ShapeKind, points: &[(i32, i32)]) -> Shape {
        Shape::new(kind, points.to_vec())
    }

    /// The pixels drawn black.
    fn drawn(canvas: &ShapeCanvas) -> Vec<(u32, u32)> {
        let pixels = canvas.pixels().enumerate_pixels();
        pixels.filter(|(.., pixel)| **pixel == BLACK).map(|(x, y, _)| (x, y)).collect()
    }

    #[test]
    fn a_line_goes_from_end_to_end() {
        let mut canvas = ShapeCanvas::new(RgbaImage::new(8, 8), None);
        let area = canvas.draw(&shape(ShapeKind::Line, &[(1, 2), (5, 2)]), &style(1, false));
        assert_eq!(drawn(&canvas), [(1, 2), (2, 2), (3, 2), (4, 2), (5, 2)]);
        assert!(area.is_some() && canvas.changed());
    }

    #[test]
    fn a_rectangle_is_outlined_or_filled() {
        let corners = [(1, 1), (4, 4)];
        let mut canvas = ShapeCanvas::new(RgbaImage::new(8, 8), None);
        canvas.draw(&shape(ShapeKind::Rectangle, &corners), &style(1, false));
        let outlined = drawn(&canvas);
        assert_eq!(outlined.len(), 12);
        assert!(outlined.contains(&(1, 1)) && outlined.contains(&(4, 4)) && !outlined.contains(&(2, 2)));
        canvas.draw(&shape(ShapeKind::FilledRectangle, &corners), &style(1, false));
        assert_eq!(drawn(&canvas).len(), 16);
    }

    #[test]
    fn an_ellipse_fits_in_its_rectangle() {
        let mut canvas = ShapeCanvas::new(RgbaImage::new(12, 12), None);
        canvas.draw(&shape(ShapeKind::FilledEllipse, &[(1, 1), (9, 9)]), &style(1, false));
        let pixels = drawn(&canvas);
        assert!(pixels.contains(&(5, 5)) && pixels.contains(&(1, 5)) && pixels.contains(&(5, 9)));
        assert!(!pixels.contains(&(1, 1)) && !pixels.contains(&(9, 9)));
        assert!(pixels.iter().all(|&(x, y)| (1..=9).contains(&x) && (1..=9).contains(&y)));
    }

    #[test]
    fn a_curve_bent_nowhere_is_a_line() {
        let mut line = ShapeCanvas::new(RgbaImage::new(10, 10), None);
        line.draw(&shape(ShapeKind::Line, &[(0, 0), (9, 9)]), &style(1, false));
        let mut curve = ShapeCanvas::new(RgbaImage::new(10, 10), None);
        curve.draw(&shape(ShapeKind::Curve, &[(0, 0), (9, 9), (3, 3), (6, 6)]), &style(1, false));
        assert_eq!(drawn(&curve), drawn(&line));
        // Bent, it leaves the line.
        curve.draw(&shape(ShapeKind::Curve, &[(0, 0), (9, 9), (9, 0), (9, 0)]), &style(1, false));
        let bent = drawn(&curve);
        assert!(bent.contains(&(3, 0)) && bent.contains(&(9, 5)) && !bent.contains(&(4, 4)));
    }

    #[test]
    fn a_turned_shape_turns_about_its_middle() {
        let mut canvas = ShapeCanvas::new(RgbaImage::new(12, 12), None);
        // A wide flat rectangle, a quarter turn round, stands up.
        let turned = Shape { angle: PI / 2.0, ..shape(ShapeKind::FilledRectangle, &[(1, 5), (9, 6)]) };
        assert_eq!(turned.center(), (5.5, 6.0));
        canvas.draw(&turned, &style(1, false));
        let pixels = drawn(&canvas);
        assert!(pixels.contains(&(5, 2)) && pixels.contains(&(5, 9)) && pixels.contains(&(6, 6)));
        assert!(!pixels.contains(&(1, 5)) && !pixels.contains(&(9, 6)));
    }

    #[test]
    fn overlaps_say_what_a_crossing_polygon_fills() {
        // Around a square twice over: the second time round only its right half.
        let twice = [(0, 0), (8, 0), (8, 8), (0, 8), (0, 0), (4, 0), (8, 0), (8, 8), (4, 8), (4, 0)];
        let mut canvas = ShapeCanvas::new(RgbaImage::new(10, 10), None);
        canvas.draw(&shape(ShapeKind::Polygon, &twice), &style(1, false));
        assert!(drawn(&canvas).contains(&(6, 4)));
        canvas.draw(&shape(ShapeKind::Polygon, &twice), &ShapeStyle { overlaps: Overlaps::Subtract, ..style(1, false) });
        assert!(!drawn(&canvas).contains(&(6, 4)) && drawn(&canvas).contains(&(2, 4)));
    }

    #[test]
    fn drawing_again_takes_the_last_one_off() {
        let base = RgbaImage::from_pixel(10, 10, Rgba([255, 255, 255, 255]));
        let mut canvas = ShapeCanvas::new(base.clone(), None);
        canvas.draw(&shape(ShapeKind::FilledRectangle, &[(0, 0), (3, 3)]), &style(1, false));
        let area = canvas.draw(&shape(ShapeKind::FilledRectangle, &[(6, 6), (8, 8)]), &style(1, false));
        assert!(!drawn(&canvas).contains(&(1, 1)) && drawn(&canvas).contains(&(7, 7)));
        let area = area.expect("both are on the image");
        assert!(area.left == 0 && area.right >= 9, "the area both were drawn over");
        canvas.clear();
        assert!(*canvas.pixels() == base && !canvas.changed());
    }

    #[test]
    fn only_selected_pixels_change() {
        let selection = Selection::rect(PixelRect { left: 0, top: 0, right: 3, bottom: 10 });
        let mut canvas = ShapeCanvas::new(RgbaImage::new(10, 10), SelectionMask::of(&selection, 10, 10));
        canvas.draw(&shape(ShapeKind::Line, &[(0, 5), (9, 5)]), &style(1, false));
        assert_eq!(drawn(&canvas), [(0, 5), (1, 5), (2, 5)]);
        // Off the image, or the selection, it changes nothing.
        assert!(canvas.draw(&shape(ShapeKind::Line, &[(-20, -20), (-10, -20)]), &style(1, false)).is_some());
        assert!(!canvas.changed());
        canvas.draw(&shape(ShapeKind::Line, &[(5, 0), (5, 9)]), &style(1, false));
        assert!(!canvas.changed());
    }

    #[test]
    fn smooth_edges_cover_pixels_in_part_without_building_up() {
        let mut canvas = ShapeCanvas::new(RgbaImage::new(10, 10), None);
        canvas.draw(&shape(ShapeKind::Polygon, &[(1, 1), (8, 2), (3, 8)]), &style(2, true));
        let alphas: Vec<u8> = canvas.pixels().pixels().map(|pixel| pixel[3]).collect();
        assert!(alphas.iter().any(|&alpha| alpha > 0 && alpha < 255), "a soft edge");
        assert_eq!(canvas.pixels().get_pixel(4, 4)[3], 255, "the inside is covered whole");
        // A see-through colour goes no darker where the line meets itself, or the fill.
        let half = ShapeStyle { color: Rgba([0, 0, 0, 128]), ..style(3, false) };
        canvas.draw(&shape(ShapeKind::FilledRectangle, &[(1, 1), (8, 8)]), &half);
        assert_eq!(canvas.pixels().get_pixel(1, 1)[3], canvas.pixels().get_pixel(4, 4)[3]);
    }

    #[test]
    fn a_wrapping_shape_comes_back_in_at_the_other_edge() {
        let area = PixelRect { left: 2, top: 2, right: 8, bottom: 8 };
        let mut canvas = ShapeCanvas::new(RgbaImage::new(10, 10), None).wrapping(Some(area));
        let style = ShapeStyle { color: Rgba([0, 0, 0, 255]), size: 1, smooth: false, overlaps: Overlaps::Add };
        // A line from inside the area to past its right edge.
        let drawn = canvas.draw(&Shape::new(ShapeKind::Line, vec![(6, 4), (9, 4)]), &style);
        // All the way across, with the room kept around the line.
        assert_eq!(drawn, Some(PixelRect { left: 2, top: 2, right: 8, bottom: 7 }));
        let black: Vec<u32> = (0..10).filter(|&x| canvas.pixels().get_pixel(x, 4)[3] > 0).collect();
        assert_eq!(black, [2, 3, 6, 7]);
    }
}

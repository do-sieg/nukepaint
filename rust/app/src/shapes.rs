//! The shape tools: Line, Curve, Rectangle, Filled Rectangle, Ellipse, Filled Ellipse,
//! Contour and Polygon. A drag draws a Line, a Curve's line, or the Rectangle or the
//! Ellipse that fits in the rectangle it frames, at steps of 15° or as a square or a
//! circle with Shift, and a Contour along where it goes. The Polygon puts down a corner
//! at each click, and is finished as Polygon Select's outline is closed. Each is drawn on
//! the active layer, on the selected pixels alone if any are, and shows there as it is
//! drawn before it is part of it.
//!
//! Once finished, which is a step, a shape can still be changed, each change a step of
//! its own, for as long as the layers are as those steps left them: undoing and redoing
//! them brings it back as it was then. A Line's and a Curve's ends, the points a Curve
//! bends towards and a Polygon's corners move; a press on a Polygon's edge adds a corner
//! there, and a corner can be removed; the handles of the box around a Rectangle, an
//! Ellipse or a Contour resize it; dragging the shape itself moves it, and dragging
//! around it turns it about its middle, in steps of 15° with Shift. A Rectangle, an
//! Ellipse and a Contour turn with their box, and the others' points turn. Drawing it
//! in another colour or way redraws it in place of its step.

use std::f64::consts::PI;

use power_paint_core::{Document, PixelRect, RgbaImage, Shape, ShapeCanvas, ShapeKind, ShapeStyle};

/// Steps of the angle a Line's or a Curve's ends, and a shape being turned, keep to
/// with Shift.
const ANGLE_SNAP: f64 = PI / 12.0;

/// What of a shape a press changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    /// The point at that index, of those that place the shape.
    Point(usize),
    /// The sides of the box around the shape that a handle of its edges or corners
    /// moves: -1 for the left or top one, 1 for the right or bottom one, 0 for neither.
    Box(i32, i32),
    /// The shape itself, which moves it.
    Body,
    /// Around the shape, which turns it.
    Rotate,
}

/// A shape being drawn, or changed once drawn.
pub struct ShapeEdit {
    kind: ShapeKind,
    /// The layer it is drawn on.
    layer: usize,
    /// The layer's pixels before it, and with it as it was drawn last.
    canvas: ShapeCanvas,
    /// Whether it is drawn in the secondary colour rather than the primary one, which it
    /// keeps to as the colours change.
    pub secondary: bool,
    /// The shape while it is being drawn, until it is finished.
    draft: Option<Draft>,
    /// The shape as of each step that left it, with how it was drawn.
    versions: Vec<Version>,
    drag: Option<Drag>,
}

struct Draft {
    shape: Shape,
    /// Where the Polygon's outline goes on to, the pointer's pixel, once it closes.
    to: Option<(i32, i32)>,
    style: ShapeStyle,
    /// The step of the history and the content of the layers it began on: it goes on
    /// only while nothing else changes them.
    step: usize,
    content: usize,
}

struct Version {
    step: usize,
    content: usize,
    shape: Shape,
    style: ShapeStyle,
}

/// A handle or the shape being dragged.
struct Drag {
    handle: Handle,
    /// Where it was pressed on the image, in pixels, and the shape then, with the corner
    /// a press on an edge added.
    press: (f64, f64),
    from: Shape,
    /// The shape before the press, which giving up puts back.
    before: Shape,
    shape: Shape,
    style: ShapeStyle,
}

impl ShapeEdit {
    /// Begins drawing a shape on the active layer from the pixel `at`, in `style`,
    /// wrapping around the edges of `wrap` if there is one, as on a tiled image.
    /// Returns it, and the area of the layer it shows on.
    pub fn begin(
        document: &Document,
        kind: ShapeKind,
        at: (i32, i32),
        style: ShapeStyle,
        secondary: bool,
        wrap: Option<PixelRect>,
    ) -> (Self, Option<PixelRect>) {
        let points = match kind {
            ShapeKind::Curve => vec![at; 4],
            ShapeKind::Contour | ShapeKind::Polygon => vec![at],
            _ => vec![at; 2],
        };
        let draft = Draft {
            shape: Shape::new(kind, points),
            to: None,
            style,
            step: document.current_step(),
            content: document.current_content(),
        };
        let mut edit = Self {
            kind,
            layer: document.active_layer,
            canvas: ShapeCanvas::new(document.active().pixels.clone(), document.selection_mask()).wrapping(wrap),
            secondary,
            draft: Some(draft),
            versions: Vec::new(),
            drag: None,
        };
        let area = edit.show_draft();
        (edit, area)
    }

    pub fn kind(&self) -> ShapeKind {
        self.kind
    }

    /// The layer it is drawn on, and the layer's pixels with it as it now shows.
    pub fn layer(&self) -> usize {
        self.layer
    }

    pub fn pixels(&self) -> &RgbaImage {
        self.canvas.pixels()
    }

    pub fn is_drawing(&self) -> bool {
        self.draft.is_some()
    }

    /// Whether a drag is drawing it, which the Polygon is not, or changing it.
    pub fn is_dragging(&self) -> bool {
        self.drag.is_some() || (self.draft.is_some() && self.kind != ShapeKind::Polygon)
    }

    /// What of the shape is being dragged, once it is drawn.
    pub fn dragged_handle(&self) -> Option<Handle> {
        self.drag.as_ref().map(|drag| drag.handle)
    }

    /// Whether the document is still as it was when the shape being drawn began, for it
    /// to go on.
    pub fn is_current(&self, document: &Document) -> bool {
        self.draft.as_ref().is_some_and(|draft| {
            draft.step == document.current_step()
                && draft.content == document.current_content()
                && self.layer == document.active_layer
        })
    }

    /// Shows the shape being drawn as it would be finished now.
    fn show_draft(&mut self) -> Option<PixelRect> {
        let draft = self.draft.as_ref()?;
        let mut shape = draft.shape.clone();
        shape.points.extend(draft.to.filter(|to| shape.points.last() != Some(to)));
        self.canvas.draw(&shape, &draft.style)
    }

    /// The drag drawing the shape reaches the pixel `at`: a Line's or a Curve's end goes
    /// there, or the corner of the rectangle a Rectangle or an Ellipse fits in opposite
    /// the one it began on, or a Contour goes on to there. With `constrain`, the Line and
    /// the Curve keep to steps of 15°, and the Rectangle and the Ellipse to a square and
    /// a circle. The Curve bends nowhere until it is drawn: the points it bends towards
    /// are on its line. Returns the area of the layer it shows on, if it changed.
    pub fn draw_to(&mut self, at: (i32, i32), constrain: bool) -> Option<PixelRect> {
        let draft = self.draft.as_mut()?;
        let points = &mut draft.shape.points;
        match self.kind {
            ShapeKind::Polygon => return None,
            ShapeKind::Contour if points.last() == Some(&at) => return None,
            ShapeKind::Contour => points.push(at),
            kind => {
                let end = match kind {
                    _ if !constrain => at,
                    ShapeKind::Line | ShapeKind::Curve => snap_angle(points[0], at),
                    _ => square(points[0], at),
                };
                if points[1] == end {
                    return None;
                }
                points[1] = end;
                if kind == ShapeKind::Curve {
                    (points[2], points[3]) = (along(points[0], points[1], 1.0 / 3.0), along(points[0], points[1], 2.0 / 3.0));
                }
            }
        }
        self.show_draft()
    }

    /// The corners the Polygon has put down, while it is being drawn.
    pub fn corners(&self) -> &[(i32, i32)] {
        self.draft.as_ref().map_or(&[], |draft| &draft.shape.points)
    }

    /// Puts down a corner of the Polygon, unless its last one is there already.
    pub fn add_corner(&mut self, at: (i32, i32)) -> Option<PixelRect> {
        let draft = self.draft.as_mut()?;
        if draft.shape.points.last() == Some(&at) {
            return None;
        }
        draft.shape.points.push(at);
        self.show_draft()
    }

    /// Takes back the last corner the Polygon put down.
    pub fn remove_last(&mut self) -> Option<PixelRect> {
        self.draft.as_mut()?.shape.points.pop();
        self.show_draft()
    }

    /// Has the Polygon's outline go on to the pointer's pixel, or nowhere.
    pub fn reach(&mut self, to: Option<(i32, i32)>) -> Option<PixelRect> {
        let draft = self.draft.as_mut().filter(|draft| draft.to != to)?;
        draft.to = to;
        self.show_draft()
    }

    /// Finishes the shape being drawn, without where the Polygon's outline goes on to:
    /// it becomes part of the layer, as a step, if it is a shape at all and changes any
    /// pixel. The Polygon must have three corners. Returns whether it did.
    pub fn finish(&mut self, document: &mut Document, name: &str, icon: Option<&'static str>) -> bool {
        let Some(draft) = self.draft.take() else {
            return false;
        };
        let points = &draft.shape.points;
        let enough = match self.kind {
            ShapeKind::Polygon => points.len() >= 3,
            _ => points.iter().any(|point| *point != points[0]),
        };
        if enough {
            self.canvas.draw(&draft.shape, &draft.style);
        }
        if !enough || !self.canvas.changed() {
            self.canvas.clear();
            return false;
        }
        self.commit(document, draft.shape, draft.style, name, icon);
        true
    }

    /// Gives up the shape being drawn.
    pub fn cancel(&mut self) {
        if self.draft.take().is_some() {
            self.canvas.clear();
        }
    }

    /// Draws the shape being drawn in `style`, if it is not already. Returns the area of
    /// the layer it shows on.
    pub fn restyle_draft(&mut self, style: ShapeStyle) -> Option<PixelRect> {
        let draft = self.draft.as_mut().filter(|draft| draft.style != style)?;
        draft.style = style;
        self.show_draft()
    }

    /// Draws the shape on the layer, as a step, and records it with the step.
    fn commit(&mut self, document: &mut Document, shape: Shape, style: ShapeStyle, name: &str, icon: Option<&'static str>) {
        self.canvas.draw(&shape, &style);
        document.active_mut().pixels = self.canvas.pixels().clone();
        document.commit(name, icon);
        let (step, content) = (document.current_step(), document.current_content());
        self.versions.push(Version { step, content, shape, style });
    }

    /// Which of the versions the document is as of, if any is, once the shape is drawn.
    fn version_index(&self, document: &Document) -> Option<usize> {
        if self.draft.is_some() || self.layer != document.active_layer {
            return None;
        }
        let (step, content) = (document.current_step(), document.current_content());
        self.versions.iter().rposition(|version| version.step == step && version.content == content)
    }

    fn version(&self, document: &Document) -> Option<&Version> {
        self.version_index(document).map(|index| &self.versions[index])
    }

    /// The shape as it is being dragged, or else as the document now is, if it is as the
    /// shape left it: the one that can be changed.
    pub fn shape(&self, document: &Document) -> Option<&Shape> {
        match &self.drag {
            Some(drag) => Some(&drag.shape),
            None => self.version(document).map(|version| &version.shape),
        }
    }

    /// How wide the shape that can be changed is drawn, in pixels.
    pub fn size(&self, document: &Document) -> Option<u32> {
        match &self.drag {
            Some(drag) => Some(drag.style.size),
            None => self.version(document).map(|version| version.style.size),
        }
    }

    /// Begins dragging a handle, or the shape, pressed at the point `at` of the image.
    pub fn press(&mut self, document: &Document, handle: Handle, at: (f64, f64)) {
        let Some(version) = self.version(document) else {
            return;
        };
        let (shape, style) = (version.shape.clone(), version.style);
        self.drag = Some(Drag { handle, press: at, from: shape.clone(), before: shape.clone(), shape, style });
    }

    /// Adds a corner on the pixel at the point `at` of the image to the Polygon's edge
    /// from the corner at `index` to the next one, and begins moving it. Returns the area
    /// of the layer it shows on.
    pub fn press_edge(&mut self, document: &Document, index: usize, at: (f64, f64)) -> Option<PixelRect> {
        let version = self.version(document).filter(|_| self.kind == ShapeKind::Polygon)?;
        let (before, style) = (version.shape.clone(), version.style);
        let mut shape = before.clone();
        shape.points.insert(index + 1, pixel_of(at));
        let handle = Handle::Point(index + 1);
        self.drag = Some(Drag { handle, press: at, from: shape.clone(), before, shape, style });
        self.show_drag()
    }

    fn show_drag(&mut self) -> Option<PixelRect> {
        let drag = self.drag.as_ref()?;
        self.canvas.draw(&drag.shape, &drag.style)
    }

    /// The drag reaches the point `at` of the image. With `constrain`, a Line's or a
    /// Curve's end keeps to steps of 15° from the other one, a corner of a Rectangle or
    /// an Ellipse to a square, and the shape turns in steps of 15°. Returns the area of
    /// the layer it shows on, if it changed.
    pub fn drag_to(&mut self, at: (f64, f64), constrain: bool) -> Option<PixelRect> {
        let drag = self.drag.as_mut()?;
        let shape = dragged(&drag.from, drag.handle, drag.press, at, constrain);
        if shape == drag.shape {
            return None;
        }
        drag.shape = shape;
        self.show_drag()
    }

    /// The drag ends: the shape becomes part of the layer where it now is, as a step, if
    /// it changed. Returns whether it did.
    pub fn release(&mut self, document: &mut Document, name: &str, icon: Option<&'static str>) -> bool {
        let Some(drag) = self.drag.take() else {
            return false;
        };
        if drag.shape == drag.before {
            return false;
        }
        self.commit(document, drag.shape, drag.style, name, icon);
        true
    }

    /// Puts the shape back as it was before the drag. Returns the area of the layer it
    /// shows on.
    pub fn cancel_drag(&mut self) -> Option<PixelRect> {
        let drag = self.drag.take()?;
        self.canvas.draw(&drag.before, &drag.style)
    }

    /// Removes the Polygon's corner at `index`, as a step, if three are left without it.
    /// Returns whether it did.
    pub fn remove_corner(&mut self, document: &mut Document, index: usize, name: &str, icon: Option<&'static str>) -> bool {
        let Some(version) = self.version(document).filter(|version| version.shape.points.len() > 3) else {
            return false;
        };
        let (mut shape, style) = (version.shape.clone(), version.style);
        shape.points.remove(index);
        self.commit(document, shape, style, name, icon);
        true
    }

    /// Moves the shape by `offset`, in pixels, as a step, if it can be changed. Returns
    /// whether it did.
    pub fn nudge(&mut self, document: &mut Document, offset: (i32, i32), name: &str, icon: Option<&'static str>) -> bool {
        let Some(version) = self.version(document).filter(|_| self.drag.is_none()) else {
            return false;
        };
        let (mut shape, style) = (version.shape.clone(), version.style);
        for point in &mut shape.points {
            *point = (point.0 + offset.0, point.1 + offset.1);
        }
        self.commit(document, shape, style, name, icon);
        true
    }

    /// Draws the shape that can be changed again in `style`, if it was drawn otherwise,
    /// in place of the step it is. Returns whether it did.
    pub fn restyle(&mut self, document: &mut Document, style: ShapeStyle) -> bool {
        if self.drag.is_some() {
            return false;
        }
        let Some(index) = self.version_index(document).filter(|&index| self.versions[index].style != style) else {
            return false;
        };
        self.canvas.draw(&self.versions[index].shape, &style);
        document.active_mut().pixels = self.canvas.pixels().clone();
        document.amend();
        let version = &mut self.versions[index];
        (version.style, version.content) = (style, document.current_content());
        true
    }
}

/// Whether the shape is resized by the handles of the box around its points, which
/// turns with it, rather than changed from its points.
pub fn has_box(kind: ShapeKind) -> bool {
    kind.is_framed() || kind == ShapeKind::Contour
}

/// The box around a shape, which a drag around turns it: its middle, half its width and
/// height, in pixels, and how far it is turned. That around the middle of the pixels of
/// its points, for the shapes with a box, or else around its outline.
pub fn frame(shape: &Shape) -> ((f64, f64), (f64, f64), f64) {
    if has_box(shape.kind) {
        let (left, top, right, bottom) = bounds(&shape.points);
        return (shape.center(), ((right - left) as f64 / 2.0, (bottom - top) as f64 / 2.0), shape.angle);
    }
    let outline = shape.outline();
    let (mut left, mut top, mut right, mut bottom) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for &(x, y) in &outline {
        (left, top, right, bottom) = (left.min(x), top.min(y), right.max(x), bottom.max(y));
    }
    (((left + right) / 2.0, (top + bottom) / 2.0), ((right - left) / 2.0, (bottom - top) / 2.0), 0.0)
}

/// Where the handles that change a shape are, on the image, in pixels: on the points
/// that place a Line, a Curve or a Polygon, and on the corners and the middle of the
/// edges of the box of the others.
pub fn handles(shape: &Shape) -> Vec<(Handle, (f64, f64))> {
    let middle = |(x, y): (i32, i32)| (x as f64 + 0.5, y as f64 + 0.5);
    if !has_box(shape.kind) {
        return shape.points.iter().enumerate().map(|(index, &point)| (Handle::Point(index), middle(point))).collect();
    }
    let (center, half, angle) = frame(shape);
    let mut handles = Vec::with_capacity(8);
    for y in [-1, 0, 1] {
        for x in [-1, 0, 1] {
            if (x, y) != (0, 0) {
                let local = (x as f64 * half.0, y as f64 * half.1);
                handles.push((Handle::Box(x, y), add(center, rotate(local, angle))));
            }
        }
    }
    handles
}

/// The leftmost, topmost, rightmost and bottommost of the points.
pub fn bounds(points: &[(i32, i32)]) -> (i32, i32, i32, i32) {
    let xs = points.iter().map(|point| point.0);
    let ys = points.iter().map(|point| point.1);
    (xs.clone().min().unwrap_or(0), ys.clone().min().unwrap_or(0), xs.max().unwrap_or(0), ys.max().unwrap_or(0))
}

/// The shape `from` with what `handle` moves moved from the point `press` of the image
/// to `at`.
fn dragged(from: &Shape, handle: Handle, press: (f64, f64), at: (f64, f64), constrain: bool) -> Shape {
    let mut shape = from.clone();
    match handle {
        Handle::Rotate => {
            let (center, ..) = frame(from);
            let (to, by) = (sub(at, center), sub(press, center));
            let mut turn = to.1.atan2(to.0) - by.1.atan2(by.0);
            if has_box(from.kind) {
                shape.angle = snap(from.angle + turn, constrain);
            } else {
                turn = snap(turn, constrain);
                for point in &mut shape.points {
                    let middle = (point.0 as f64 + 0.5, point.1 as f64 + 0.5);
                    let turned = add(center, rotate(sub(middle, center), turn));
                    *point = ((turned.0 - 0.5).round() as i32, (turned.1 - 0.5).round() as i32);
                }
            }
        }
        // A turned shape is resized as it would be upright, then put back for the sides
        // that are not grabbed to stay where they are.
        Handle::Box(..) if from.angle != 0.0 => {
            let center = from.center();
            let upright = Shape { angle: 0.0, ..from.clone() };
            let unturn = |point| add(center, rotate(sub(point, center), -from.angle));
            shape = dragged(&upright, handle, unturn(press), unturn(at), constrain);
            let moved = sub(shape.center(), center);
            let offset = sub(add(center, rotate(moved, from.angle)), shape.center());
            let offset = (offset.0.round() as i32, offset.1.round() as i32);
            for point in &mut shape.points {
                *point = (point.0 + offset.0, point.1 + offset.1);
            }
            shape.angle = from.angle;
        }
        handle => drag_upright(&mut shape, handle, pixel_of(press), pixel_of(at), constrain),
    }
    shape
}

/// Moves what `handle` moves of a shape that is not turned from the pixel `press` to `at`.
fn drag_upright(shape: &mut Shape, handle: Handle, press: (i32, i32), at: (i32, i32), constrain: bool) {
    let kind = shape.kind;
    let points = &mut shape.points;
    match handle {
        Handle::Rotate => {}
        Handle::Body => {
            let offset = (at.0 - press.0, at.1 - press.1);
            for point in points {
                *point = (point.0 + offset.0, point.1 + offset.1);
            }
        }
        Handle::Point(index) => {
            // A Line's or a Curve's end keeps its angle to the other one in steps.
            let ends = matches!(kind, ShapeKind::Line | ShapeKind::Curve) && index < 2;
            points[index] = if constrain && ends { snap_angle(points[1 - index], at) } else { at };
        }
        Handle::Box(x, y) if kind.is_framed() => {
            let (mut left, mut top, mut right, mut bottom) = bounds(points);
            match x {
                -1 => left = at.0,
                1 => right = at.0,
                _ => {}
            }
            match y {
                -1 => top = at.1,
                1 => bottom = at.1,
                _ => {}
            }
            // A corner keeps to a square, as large as fits, with the opposite one.
            if constrain && x != 0 && y != 0 {
                let opposite = (if x < 0 { right } else { left }, if y < 0 { bottom } else { top });
                let corner = square(opposite, at);
                *(if x < 0 { &mut left } else { &mut right }) = corner.0;
                *(if y < 0 { &mut top } else { &mut bottom }) = corner.1;
            }
            *points = vec![(left, top), (right, bottom)];
        }
        // A Contour is stretched for the grabbed sides of the box around it to follow
        // the pointer, the opposite ones staying put.
        Handle::Box(x, y) => {
            let (left, top, right, bottom) = bounds(points);
            let stretch = |value: i32, side: i32, low: i32, high: i32, to: i32| {
                let (new_low, new_high) = match side {
                    -1 => (to, high),
                    1 => (low, to),
                    _ => return value,
                };
                if high == low {
                    return value + if side < 0 { new_low - low } else { new_high - high };
                }
                let share = (value - low) as f64 / (high - low) as f64;
                new_low + (share * (new_high - new_low) as f64).round() as i32
            };
            for point in points {
                *point = (stretch(point.0, x, left, right, at.0), stretch(point.1, y, top, bottom, at.1));
            }
        }
    }
}

/// The pixel a point of the image is on.
fn pixel_of(point: (f64, f64)) -> (i32, i32) {
    (point.0.floor() as i32, point.1.floor() as i32)
}

/// An angle, to the nearest step of 15° with `constrain`.
pub fn snap(angle: f64, constrain: bool) -> f64 {
    if constrain { (angle / ANGLE_SNAP).round() * ANGLE_SNAP } else { angle }
}

fn add(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    (a.0 + b.0, a.1 + b.1)
}

fn sub(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    (a.0 - b.0, a.1 - b.1)
}

fn rotate((x, y): (f64, f64), angle: f64) -> (f64, f64) {
    let (sin, cos) = angle.sin_cos();
    (x * cos - y * sin, x * sin + y * cos)
}

/// `at`, turned about `from` to the nearest step of 15°.
fn snap_angle(from: (i32, i32), at: (i32, i32)) -> (i32, i32) {
    let (dx, dy) = ((at.0 - from.0) as f64, (at.1 - from.1) as f64);
    let angle = (dy.atan2(dx) / ANGLE_SNAP).round() * ANGLE_SNAP;
    let length = dx.hypot(dy);
    (from.0 + (angle.cos() * length).round() as i32, from.1 + (angle.sin() * length).round() as i32)
}

/// The corner of a square from `from`, towards `at`, as large as fits between the two.
fn square(from: (i32, i32), at: (i32, i32)) -> (i32, i32) {
    let (dx, dy) = (at.0 - from.0, at.1 - from.1);
    let side = dx.abs().min(dy.abs());
    (from.0 + side * dx.signum(), from.1 + side * dy.signum())
}

/// The pixel a share of the way from `from` to `to`.
fn along(from: (i32, i32), to: (i32, i32), share: f64) -> (i32, i32) {
    let at = |from: i32, to: i32| (from as f64 + (to - from) as f64 * share).round() as i32;
    (at(from.0, to.0), at(from.1, to.1))
}

#[cfg(test)]
mod tests {
    use power_paint_core::{Overlaps, Rgba, Selection};

    use super::*;

    const BLACK: Rgba<u8> = Rgba([0, 0, 0, 255]);
    const STYLE: ShapeStyle = ShapeStyle { color: BLACK, fill: None, size: 1, smooth: false, overlaps: Overlaps::Add };
    const FILLED: ShapeStyle = ShapeStyle { fill: Some(BLACK), ..STYLE };

    /// The middle of the pixel at `x`, `y`.
    fn at(x: i32, y: i32) -> (f64, f64) {
        (x as f64 + 0.5, y as f64 + 0.5)
    }

    fn document() -> Document {
        Document::new(RgbaImage::new(20, 20))
    }

    fn black(document: &Document, x: u32, y: u32) -> bool {
        *document.active().pixels.get_pixel(x, y) == BLACK
    }

    /// A shape drawn with a drag from `from` to `to`, as a step.
    fn drawn(document: &mut Document, kind: ShapeKind, from: (i32, i32), to: (i32, i32)) -> ShapeEdit {
        drawn_with(document, kind, from, to, STYLE)
    }

    fn drawn_with(
        document: &mut Document,
        kind: ShapeKind,
        from: (i32, i32),
        to: (i32, i32),
        style: ShapeStyle,
    ) -> ShapeEdit {
        let (mut edit, _) = ShapeEdit::begin(document, kind, from, style, false, None);
        edit.draw_to(to, false);
        assert!(edit.finish(document, "Shape", None));
        edit
    }

    #[test]
    fn a_drag_draws_a_shape_as_a_step() {
        let mut document = document();
        let (mut edit, area) = ShapeEdit::begin(&document, ShapeKind::Line, (2, 2), STYLE, false, None);
        assert!(area.is_some() && edit.is_dragging());
        edit.draw_to((8, 2), false);
        // It only shows on the layer until it is finished.
        assert!(!black(&document, 5, 2) && *edit.pixels().get_pixel(5, 2) == BLACK);
        assert!(edit.finish(&mut document, "Line", None));
        assert!(black(&document, 5, 2) && document.current_step() == 1);
        assert_eq!(edit.shape(&document).unwrap().points, [(2, 2), (8, 2)]);
    }

    #[test]
    fn a_click_draws_nothing() {
        let mut document = document();
        let (mut edit, _) = ShapeEdit::begin(&document, ShapeKind::Rectangle, (2, 2), STYLE, false, None);
        assert!(!edit.finish(&mut document, "Rectangle", None));
        assert_eq!(document.current_step(), 0);
        assert!(!black(&document, 2, 2));
    }

    #[test]
    fn shift_keeps_to_squares_and_steps_of_the_angle() {
        let mut document = document();
        let (mut edit, _) = ShapeEdit::begin(&document, ShapeKind::Ellipse, (0, 0), STYLE, false, None);
        edit.draw_to((9, 4), true);
        let (mut line, _) = ShapeEdit::begin(&document, ShapeKind::Line, (0, 0), STYLE, false, None);
        line.draw_to((10, 1), true);
        edit.finish(&mut document, "Ellipse", None);
        assert_eq!(edit.shape(&document).unwrap().points, [(0, 0), (4, 4)]);
        assert_eq!(line.draft.as_ref().unwrap().shape.points[1], (10, 0));
    }

    #[test]
    fn a_curve_bends_from_points_along_its_line() {
        let mut document = document();
        let mut edit = drawn(&mut document, ShapeKind::Curve, (0, 0), (9, 0));
        assert_eq!(edit.shape(&document).unwrap().points, [(0, 0), (9, 0), (3, 0), (6, 0)]);
        edit.press(&document, Handle::Point(2), at(3, 0));
        edit.drag_to(at(3, 12), false);
        assert!(edit.release(&mut document, "Curve", None));
        assert!(!black(&document, 4, 0) && document.current_step() == 2);
    }

    #[test]
    fn handles_move_and_come_back_with_undo() {
        let mut document = document();
        let mut edit = drawn(&mut document, ShapeKind::Line, (0, 0), (5, 0));
        edit.press(&document, Handle::Point(1), at(5, 0));
        edit.drag_to(at(5, 5), false);
        assert!(!black(&document, 5, 5), "the layer waits for the drag to end");
        assert!(edit.release(&mut document, "Line", None));
        assert!(black(&document, 5, 5) && !black(&document, 5, 0));
        document.undo();
        assert_eq!(edit.shape(&document).unwrap().points, [(0, 0), (5, 0)]);
        document.redo();
        assert_eq!(edit.shape(&document).unwrap().points, [(0, 0), (5, 5)]);
        // Another step leaves it behind, as does one recorded after undoing it.
        document.selection = Selection::rect(PixelRect { left: 0, top: 0, right: 2, bottom: 2 });
        document.commit("Rectangle Select", None);
        assert!(edit.shape(&document).is_none());
        document.undo();
        document.undo();
        document.active_mut().pixels.put_pixel(19, 19, BLACK);
        document.commit("Pencil", None);
        assert!(edit.shape(&document).is_none(), "the same step, not the same layers");
    }

    #[test]
    fn the_body_moves_and_the_box_resizes() {
        let mut document = document();
        let mut edit = drawn_with(&mut document, ShapeKind::Rectangle, (2, 2), (5, 5), FILLED);
        edit.press(&document, Handle::Body, at(3, 3));
        edit.drag_to(at(6, 4), false);
        edit.release(&mut document, "Rectangle", None);
        assert_eq!(edit.shape(&document).unwrap().points, [(5, 3), (8, 6)]);
        edit.press(&document, Handle::Box(-1, 0), at(5, 4));
        edit.drag_to(at(1, 9), false);
        edit.release(&mut document, "Rectangle", None);
        assert_eq!(edit.shape(&document).unwrap().points, [(1, 3), (8, 6)]);
        assert!(black(&document, 1, 3) && black(&document, 4, 5) && !black(&document, 1, 9));
        // A corner with Shift keeps to a square from the opposite one.
        edit.press(&document, Handle::Box(1, 1), at(8, 6));
        edit.drag_to(at(15, 9), true);
        assert_eq!(edit.shape(&document).unwrap().points, [(1, 3), (7, 9)]);
    }

    #[test]
    fn a_contour_stretches_with_its_box() {
        let mut document = document();
        let (mut edit, _) = ShapeEdit::begin(&document, ShapeKind::Contour, (0, 0), FILLED, false, None);
        for at in [(4, 0), (4, 4), (0, 4)] {
            edit.draw_to(at, false);
        }
        assert!(edit.finish(&mut document, "Contour", None));
        assert!(handles(edit.shape(&document).unwrap()).contains(&(Handle::Box(1, 1), (4.5, 4.5))));
        edit.press(&document, Handle::Box(1, 0), at(4, 2));
        edit.drag_to(at(8, 2), false);
        assert_eq!(edit.shape(&document).unwrap().points, [(0, 0), (8, 0), (8, 4), (0, 4)]);
    }

    #[test]
    fn the_polygon_puts_down_corners_and_they_change() {
        let mut document = document();
        let (mut edit, _) = ShapeEdit::begin(&document, ShapeKind::Polygon, (0, 0), FILLED, false, None);
        assert!(edit.is_drawing() && !edit.is_dragging());
        edit.add_corner((8, 0));
        edit.reach(Some((8, 8)));
        assert!(*edit.pixels().get_pixel(6, 2) == BLACK, "it shows closed on the pointer");
        assert!(!edit.finish(&mut document, "Polygon", None), "two corners are not enough");

        let (mut edit, _) = ShapeEdit::begin(&document, ShapeKind::Polygon, (0, 0), FILLED, false, None);
        for at in [(8, 0), (8, 8), (0, 8)] {
            edit.add_corner(at);
        }
        edit.reach(Some((15, 15)));
        assert!(edit.finish(&mut document, "Polygon", None));
        assert_eq!(edit.shape(&document).unwrap().points.len(), 4, "not the pointer's");
        assert!(black(&document, 4, 4) && !black(&document, 12, 12));
        edit.press_edge(&document, 0, at(4, 0));
        edit.drag_to(at(4, -4), false);
        assert!(edit.release(&mut document, "Polygon", None));
        assert_eq!(edit.shape(&document).unwrap().points, [(0, 0), (4, -4), (8, 0), (8, 8), (0, 8)]);
        assert!(edit.remove_corner(&mut document, 1, "Polygon", None));
        assert!(edit.remove_corner(&mut document, 0, "Polygon", None));
        assert_eq!(edit.shape(&document).unwrap().points, [(8, 0), (8, 8), (0, 8)]);
        assert!(!edit.remove_corner(&mut document, 0, "Polygon", None), "three corners are left");
    }

    #[test]
    fn dragging_around_a_shape_turns_it() {
        let mut document = document();
        // A Rectangle turns with its box, about its middle.
        let mut edit = drawn_with(&mut document, ShapeKind::Rectangle, (2, 8), (16, 10), FILLED);
        edit.press(&document, Handle::Rotate, (18.0, 9.5));
        edit.drag_to((9.5, 17.0), false);
        assert!(edit.release(&mut document, "Rectangle", None));
        let shape = edit.shape(&document).unwrap();
        assert!((shape.angle - PI / 2.0).abs() < 1e-9 && shape.points == [(2, 8), (16, 10)]);
        assert!(black(&document, 9, 3) && black(&document, 9, 15) && !black(&document, 3, 9));
        // Its box handles are turned with it, and resize it as it is turned: the left
        // side, now at the top, goes up, and the bottom stays put.
        let handles = handles(shape);
        let (_, (x, y)) = handles.iter().find(|(handle, _)| *handle == Handle::Box(-1, 0)).unwrap();
        assert!((x - 9.5).abs() < 1e-9 && (y - 2.5).abs() < 1e-9);
        edit.press(&document, Handle::Box(-1, 0), (9.5, 2.5));
        edit.drag_to((9.5, 0.5), false);
        edit.release(&mut document, "Rectangle", None);
        assert!(black(&document, 9, 0) && black(&document, 9, 15) && !black(&document, 9, 17));
        // A Line's ends turn instead, in steps of 15° with Shift.
        let mut line = drawn(&mut document, ShapeKind::Line, (0, 0), (10, 0));
        line.press(&document, Handle::Rotate, (5.5, -4.0));
        line.drag_to((9.0, 0.6), true);
        let shape = line.shape(&document).unwrap();
        assert_eq!((shape.angle, &shape.points[..]), (0.0, &[(5, -5), (5, 5)][..]));
    }

    #[test]
    fn giving_up_a_drag_puts_the_shape_back() {
        let mut document = document();
        let mut edit = drawn(&mut document, ShapeKind::Rectangle, (2, 2), (6, 6));
        let before = edit.pixels().clone();
        edit.press(&document, Handle::Body, at(2, 4));
        edit.drag_to(at(10, 10), false);
        assert!(edit.cancel_drag().is_some());
        assert_eq!(*edit.pixels(), before);
        assert_eq!(edit.shape(&document).unwrap().points, [(2, 2), (6, 6)]);
        // A press that moves nothing changes nothing.
        edit.press(&document, Handle::Point(0), at(2, 2));
        assert!(!edit.release(&mut document, "Rectangle", None));
        assert_eq!(document.current_step(), 1);
    }

    #[test]
    fn another_colour_redraws_it_in_place_of_its_step() {
        let mut document = document();
        let mut edit = drawn(&mut document, ShapeKind::Line, (0, 0), (5, 0));
        let red = ShapeStyle { color: Rgba([255, 0, 0, 255]), ..STYLE };
        assert!(edit.restyle(&mut document, red));
        assert!(!edit.restyle(&mut document, red), "it is drawn so already");
        assert_eq!(document.current_step(), 1);
        assert_eq!(*document.active().pixels.get_pixel(3, 0), Rgba([255, 0, 0, 255]));
        assert!(edit.shape(&document).is_some(), "it can still be changed");
        assert!(edit.nudge(&mut document, (0, 2), "Line", None));
        assert_eq!(*document.active().pixels.get_pixel(3, 2), Rgba([255, 0, 0, 255]));
    }

    #[test]
    fn a_shape_being_drawn_goes_on_while_nothing_else_changes() {
        let mut document = document();
        let (mut edit, _) = ShapeEdit::begin(&document, ShapeKind::Polygon, (0, 0), FILLED, false, None);
        assert!(edit.is_current(&document));
        document.active_mut().pixels.put_pixel(19, 19, BLACK);
        document.commit("Pencil", None);
        assert!(!edit.is_current(&document));
        edit.cancel();
        assert!(!edit.is_drawing());
    }

    #[test]
    fn only_selected_pixels_change() {
        let mut document = document();
        document.selection = Selection::rect(PixelRect { left: 0, top: 0, right: 3, bottom: 20 });
        drawn(&mut document, ShapeKind::Line, (0, 5), (10, 5));
        assert!(black(&document, 2, 5) && !black(&document, 3, 5));
    }
}

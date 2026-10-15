//! The box around the selection, which moves, resizes and rotates it. With the
//! selection tools it carries the selected pixels along; with Move Selection, the
//! outline alone.
//!
//! A press inside the box moves it, on an edge or corner resizes it, and just outside
//! rotates it about its centre of rotation, as does the right button anywhere with
//! Move Selection. That centre is the box's middle until its handle is dragged
//! elsewhere; it snaps to the middle and to the handles of the edges and corners.
//! Shift keeps the box's proportions while resizing, and turns it in steps of 15°;
//! Ctrl resizes it about its middle. While snapping to the grid, moving the box puts
//! the top left corner of the rectangle around it on a corner of a cell, and resizing
//! an upright box puts its edges on the grid's lines. Between drags, the arrow keys
//! nudge the box by a pixel, or by ten with Shift.
//! A transform lasts from its first drag until the tool or the selection changes some
//! other way: until then every drag starts again from what the box held at first, so
//! nothing degrades.

use std::f64::consts::PI;

use eframe::egui::{self, CursorIcon};
use power_paint_core::{
    Affine, Bounds, Document, FloatingPixels, Frame, Interpolation, Layer, MAX_OPACITY, RgbaImage, Selection,
    composite,
};

use crate::{grid::Cells, resample_preview::ResamplePreview, tiled_texture::TiledTexture, view::View};

/// How far from an edge of the box, in points, a press still grabs that edge.
pub const GRIP_REACH: f64 = 7.0;
/// How far from a handle, in points, a press still grabs it, with the tools that
/// only resize the box by its handles.
pub const HANDLE_REACH: f64 = 9.0;
/// Side of the handle that moves the box, in points, and how far its middle is out
/// from the box's bottom right corner along each side.
pub const MOVE_HANDLE_SIZE: f64 = 14.0;
const MOVE_HANDLE_OFFSET: f64 = 18.0;
/// How far outside the box, in points, a press still rotates it.
pub const ROTATE_REACH: f64 = 30.0;
/// Rotation step while Shift is held.
const ROTATE_SNAP: f64 = PI / 12.0;
/// Radius of the handle of the centre of rotation, in points, which is also how far
/// from it a press grabs it.
pub const PIVOT_RADIUS: f64 = 6.0;
/// Shortest a side of the box is on screen, in points, for the handle of the centre of
/// rotation to show: in a smaller box it would leave no room to grab the box to move it.
const PIVOT_MIN_SIDE: f64 = 40.0;

/// Where the box is: its middle, its size, negative along an axis it has been
/// mirrored on, how far it has turned, in radians clockwise, and the centre it turns
/// about.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxState {
    pub center: (f64, f64),
    pub size: (f64, f64),
    pub angle: f64,
    /// The centre of rotation, from the middle, as a share of the size along each side,
    /// as the box is before it turns: it follows the box as it moves, resizes and turns.
    pub pivot: (f64, f64),
}

impl BoxState {
    pub fn around(bounds: Bounds) -> Self {
        Self { center: bounds.center(), size: (bounds.width(), bounds.height()), angle: 0.0, pivot: (0.0, 0.0) }
    }

    /// The centre of rotation, on the image.
    pub fn pivot_point(&self) -> (f64, f64) {
        add(self.center, rotate((self.pivot.0 * self.size.0, self.pivot.1 * self.size.1), self.angle))
    }

    /// Whether the handle of the centre of rotation shows, and can be grabbed. `reach` is
    /// how many pixels of the image a point of the screen takes.
    pub fn shows_pivot(&self, reach: f64) -> bool {
        self.size.0.abs().min(self.size.1.abs()) >= PIVOT_MIN_SIDE * reach
    }

    fn on_pivot(&self, point: (f64, f64), reach: f64) -> bool {
        let (dx, dy) = sub(point, self.pivot_point());
        self.shows_pivot(reach) && dx.hypot(dy) <= PIVOT_RADIUS * reach
    }

    /// The box of a selection: the turned one it was last left in, or else the one
    /// around it. Nothing if nothing is selected.
    pub fn of(selection: &Selection) -> Option<Self> {
        let bounds = selection.bounds()?;
        Some(selection.frame().map_or_else(|| Self::around(bounds), Self::from))
    }

    /// The middle of each edge and each corner, on the image.
    pub fn handles(&self) -> Vec<(f64, f64)> {
        let half = (self.size.0.abs() / 2.0, self.size.1.abs() / 2.0);
        let mut handles = Vec::with_capacity(8);
        for y in [-1.0, 0.0, 1.0] {
            for x in [-1.0, 0.0, 1.0] {
                if x != 0.0 || y != 0.0 {
                    handles.push(add(self.center, rotate((x * half.0, y * half.1), self.angle)));
                }
            }
        }
        handles
    }

    /// The bottom right corner, as the box is turned, which the handle that moves it
    /// is by.
    pub fn move_corner(&self) -> (f64, f64) {
        let half = (self.size.0.abs() / 2.0, self.size.1.abs() / 2.0);
        add(self.center, rotate(half, self.angle))
    }

    /// The middle of the handle that moves the box, on the image, out from its bottom
    /// right corner. `reach` is how many pixels of the image a point of the screen takes.
    pub fn move_handle(&self, reach: f64) -> (f64, f64) {
        let half = (self.size.0.abs() / 2.0, self.size.1.abs() / 2.0);
        let out = MOVE_HANDLE_OFFSET * reach;
        add(self.center, rotate((half.0 + out, half.1 + out), self.angle))
    }

    /// Whether a point of the image is on the handle that moves the box.
    pub fn on_move_handle(&self, point: (f64, f64), reach: f64) -> bool {
        let (dx, dy) = sub(point, self.move_handle(reach));
        dx.abs().max(dy.abs()) <= MOVE_HANDLE_SIZE / 2.0 * reach
    }

    /// What a press at a point of the image would do to the box. `reach` is how many
    /// pixels of the image a point of the screen takes.
    pub fn drag_at(&self, point: (f64, f64), reach: f64) -> Option<DragKind> {
        if self.on_move_handle(point, reach) {
            return Some(DragKind::Move);
        }
        if self.on_pivot(point, reach) {
            return Some(DragKind::Pivot);
        }
        let local = rotate(sub(point, self.center), -self.angle);
        let outside = (local.0.abs() - self.size.0.abs() / 2.0, local.1.abs() - self.size.1.abs() / 2.0);
        let distance = outside.0.max(outside.1);
        if distance > ROTATE_REACH * reach {
            return None;
        }
        if distance > GRIP_REACH * reach {
            return Some(DragKind::Rotate);
        }
        let grip = self.grip_at(point, GRIP_REACH * reach);
        if grip != (0, 0) {
            return Some(DragKind::Resize(grip));
        }
        Some(if distance <= 0.0 { DragKind::Move } else { DragKind::Rotate })
    }

    /// What a press at a point of the image does with the tools that only work the box
    /// from its handles: on the handle of the centre of rotation, it moves that; on a
    /// handle of an edge or corner, it resizes the box by them; on the handle that moves
    /// it, it moves it; and in the band around it that rotates it with the other tools,
    /// it rotates it. Elsewhere, nothing. `reach` is how many pixels of the image a point
    /// of the screen takes.
    pub fn handle_at(&self, point: (f64, f64), reach: f64) -> Option<DragKind> {
        if self.on_pivot(point, reach) {
            return Some(DragKind::Pivot);
        }
        let on_handle = self.handles().into_iter().any(|handle| {
            let (dx, dy) = sub(point, handle);
            dx.abs().max(dy.abs()) <= HANDLE_REACH * reach
        });
        let grip = self.grip_at(point, HANDLE_REACH * reach);
        if on_handle && grip != (0, 0) {
            return Some(DragKind::Resize(grip));
        }
        if self.on_move_handle(point, reach) {
            return Some(DragKind::Move);
        }
        // Around the box, as with the other tools.
        let local = rotate(sub(point, self.center), -self.angle);
        let outside = (local.0.abs() - self.size.0.abs() / 2.0).max(local.1.abs() - self.size.1.abs() / 2.0);
        (outside > 0.0 && outside <= ROTATE_REACH * reach).then_some(DragKind::Rotate)
    }

    /// Which edges of the box are within `reach` of a point: -1 for the left or top
    /// one, 1 for the right or bottom one, 0 for neither.
    fn grip_at(&self, point: (f64, f64), reach: f64) -> (i32, i32) {
        let local = rotate(sub(point, self.center), -self.angle);
        let half = (self.size.0.abs() / 2.0, self.size.1.abs() / 2.0);
        (edge_at(local.0, half.0, reach), edge_at(local.1, half.1, reach))
    }

    /// Moved by how far the pointer went, in whole pixels, then for the top left
    /// corner of the rectangle around it to be on the nearest corner of a cell of `snap`.
    fn moved(&self, press: (f64, f64), point: (f64, f64), snap: Option<Cells>) -> Self {
        let offset = sub(point, press);
        let center = add(self.center, (offset.0.round(), offset.1.round()));
        let Some(cells) = snap else {
            return Self { center, ..*self };
        };
        let half = (self.size.0.abs() / 2.0, self.size.1.abs() / 2.0);
        let turned = (self.angle.cos().abs(), self.angle.sin().abs());
        let reach = (half.0 * turned.0 + half.1 * turned.1, half.0 * turned.1 + half.1 * turned.0);
        let corner = sub(center, reach);
        Self { center: add(center, sub(cells.snap(corner), corner)), ..*self }
    }

    /// Turned about its centre of rotation by the angle the pointer went round it.
    fn rotated(&self, press: (f64, f64), point: (f64, f64), snap: bool) -> Self {
        let pivot = self.pivot_point();
        let (from, to) = (sub(press, pivot), sub(point, pivot));
        let mut angle = self.angle + to.1.atan2(to.0) - from.1.atan2(from.0);
        if snap {
            angle = (angle / ROTATE_SNAP).round() * ROTATE_SNAP;
        }
        let center = add(pivot, rotate(sub(self.center, pivot), angle - self.angle));
        Self { center, angle, ..*self }
    }

    /// With its centre of rotation moved by how far the pointer went: onto the middle or
    /// a handle of an edge or corner within reach of it, or else onto the nearest corner
    /// of a cell of `snap`, or the nearest corner or middle of a pixel. `reach` is how
    /// many pixels of the image a point of the screen takes.
    fn pivot_moved(&self, press: (f64, f64), point: (f64, f64), reach: f64, snap: Option<Cells>) -> Self {
        let target = add(self.pivot_point(), sub(point, press));
        let distance = |to: (f64, f64)| {
            let (dx, dy) = sub(to, target);
            dx.hypot(dy)
        };
        let anchor = self
            .handles()
            .into_iter()
            .chain([self.center])
            .filter(|&anchor| distance(anchor) <= HANDLE_REACH * reach)
            .min_by(|a, b| distance(*a).total_cmp(&distance(*b)));
        let target = anchor.unwrap_or_else(|| match snap {
            Some(cells) => cells.snap(target),
            None => ((target.0 * 2.0).round() / 2.0, (target.1 * 2.0).round() / 2.0),
        });
        let local = rotate(sub(target, self.center), -self.angle);
        Self { pivot: (local.0 / self.size.0, local.1 / self.size.1), ..*self }
    }

    /// Stretched for the grabbed edges to follow the pointer, the opposite ones staying
    /// put, or with `from_center`, following it as well about the middle. With
    /// `keep_ratio`, it keeps its proportions. Nothing comes of a size under one pixel.
    fn resized(
        &self,
        grip: (i32, i32),
        point: (f64, f64),
        keep_ratio: bool,
        from_center: bool,
        snap: Option<Cells>,
    ) -> Option<Self> {
        // An upright box keeps its edges on whole pixels, or on the lines of `snap`.
        let point = match snap {
            _ if self.angle != 0.0 => point,
            Some(cells) => cells.snap(point),
            None => (point.0.round(), point.1.round()),
        };
        let local = rotate(sub(point, self.center), -self.angle);
        let extent = (self.size.0.abs(), self.size.1.abs());
        // How many times larger the box becomes along each axis; negative once an edge
        // has been dragged past the opposite one.
        let factor_along = |grip: i32, local: f64, extent: f64| match grip {
            0 => 1.0,
            _ if from_center => local * grip as f64 / (extent / 2.0),
            _ => local * grip as f64 / extent + 0.5,
        };
        let mut factor = (factor_along(grip.0, local.0, extent.0), factor_along(grip.1, local.1, extent.1));
        if keep_ratio {
            factor = match grip {
                (0, _) => (factor.1.abs(), factor.1),
                (_, 0) => (factor.0, factor.0.abs()),
                _ => {
                    let larger = factor.0.abs().max(factor.1.abs());
                    (larger.copysign(factor.0), larger.copysign(factor.1))
                }
            };
        }
        if (extent.0 * factor.0).abs() < 1.0 || (extent.1 * factor.1).abs() < 1.0 {
            return None;
        }
        let shift = match from_center {
            true => (0.0, 0.0),
            false => (
                grip.0 as f64 * (factor.0 - 1.0) * extent.0 / 2.0,
                grip.1 as f64 * (factor.1 - 1.0) * extent.1 / 2.0,
            ),
        };
        Some(Self {
            center: add(self.center, rotate(shift, self.angle)),
            size: (self.size.0 * factor.0, self.size.1 * factor.1),
            ..*self
        })
    }
}

impl From<Frame> for BoxState {
    fn from(Frame { center, size, angle }: Frame) -> Self {
        Self { center, size, angle, pivot: (0.0, 0.0) }
    }
}

impl From<BoxState> for Frame {
    fn from(BoxState { center, size, angle, .. }: BoxState) -> Self {
        Self { center, size, angle }
    }
}

/// Which end of a span from `-half` to `half` a value is within `reach` of.
fn edge_at(value: f64, half: f64, reach: f64) -> i32 {
    // Inside a small box the reach shrinks, so its middle can still be grabbed to move it.
    let reach = if value.abs() < half { reach.min(half / 2.0) } else { reach };
    let (to_low, to_high) = ((value + half).abs(), (value - half).abs());
    if to_low.min(to_high) > reach {
        0
    } else if to_low < to_high {
        -1
    } else {
        1
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DragKind {
    Move,
    /// The edges grabbed, as `BoxState::grip_at` gives them.
    Resize((i32, i32)),
    Rotate,
    /// Moves the centre of rotation.
    Pivot,
}

impl DragKind {
    /// The pointer that shows what a drag would do, with the box turned by `angle`.
    pub fn cursor(self, angle: f64) -> CursorIcon {
        match self {
            Self::Move => CursorIcon::Move,
            Self::Pivot => CursorIcon::Crosshair,
            // There is no turning arrow: this stands for one, which the canvas draws.
            Self::Rotate => CursorIcon::Alias,
            Self::Resize((x, y)) => {
                // The arrow that best matches the way the grabbed edge faces on screen.
                let facing = rotate((x as f64, y as f64), angle);
                let eighth = (facing.1.atan2(facing.0) / (PI / 4.0)).round() as i32;
                let arrows = [
                    CursorIcon::ResizeHorizontal,
                    CursorIcon::ResizeNwSe,
                    CursorIcon::ResizeVertical,
                    CursorIcon::ResizeNeSw,
                ];
                arrows[eighth.rem_euclid(4) as usize]
            }
        }
    }
}

struct Drag {
    kind: DragKind,
    /// Where the press was on the image, and the box then.
    press: (f64, f64),
    from: BoxState,
    /// How many pixels of the image a point of the screen took then.
    reach: f64,
    /// Whether the selection moved, which moving the centre of rotation alone does not.
    moved: bool,
}

/// The selected pixels taken out of the layer, and their textures to show them moving
/// among the other layers. Those are blended normally in the preview, whatever their
/// mode: the image is blended as it should be once the pixels are put down.
struct Floating {
    pixels: FloatingPixels,
    preview: ResamplePreview,
    /// The layer without them.
    base_texture: TiledTexture,
    /// How opaque the layer is, or nothing if it is hidden.
    opacity: Option<f32>,
    /// The layers under it and those over it, each blended together, if any show.
    below: Option<TiledTexture>,
    above: Option<TiledTexture>,
}

impl Floating {
    fn new(ctx: &egui::Context, pixels: FloatingPixels, document: &Document) -> Self {
        let preview = ResamplePreview::new(&pixels);
        let base_texture = TiledTexture::new(ctx, "floating base", pixels.base());
        let layer = document.active();
        let opacity = layer.shows().then_some(layer.opacity as f32 / MAX_OPACITY as f32);
        let blended = |name: &str, layers: &[Layer]| {
            layers.iter().any(Layer::shows).then(|| TiledTexture::new(ctx, name, &composite(layers)))
        };
        let (below, above) = document.layers.split_at(document.active_layer);
        let below = blended("layers below", below);
        let above = blended("layers above", &above[1..]);
        Self { pixels, preview, base_texture, opacity, below, above }
    }
}

pub struct Transform {
    /// The selection's box when the transform began, and the selection then.
    start: BoxState,
    selection_start: Selection,
    /// The pixels being moved; none when only the outline is.
    floating: Option<Floating>,
    state: BoxState,
    drag: Option<Drag>,
    /// The step of the document's history that the box was last put down as, if any.
    committed: Option<usize>,
}

impl Transform {
    /// Starts moving the document's selection, and the pixels of the active layer it
    /// holds with `with_pixels`. There is nothing to move without a selection.
    pub fn begin(ctx: &egui::Context, document: &Document, with_pixels: bool) -> Option<Self> {
        let selection = &document.selection;
        let start = BoxState::of(selection)?;
        let floating = with_pixels.then(|| FloatingPixels::lift(&document.active().pixels, selection)).flatten();
        let floating = floating.map(|pixels| Floating::new(ctx, pixels, document));
        Some(Self { start, selection_start: selection.clone(), floating, state: start, drag: None, committed: None })
    }

    /// Starts moving pixels pasted on the document's active layer, selected all around.
    pub fn begin_pasted(ctx: &egui::Context, pixels: FloatingPixels, document: &Document) -> Self {
        let selection = Selection::rect(pixels.area());
        // Pasted pixels are never empty, so neither is their selection.
        let start = BoxState::of(&selection).expect("pasted pixels are selected");
        let floating = Some(Floating::new(ctx, pixels, document));
        Self { start, selection_start: selection, floating, state: start, drag: None, committed: None }
    }

    /// The selection the box holds, which keeps the box if it is turned.
    pub fn selection(&self) -> Selection {
        self.selection_start.transformed(&self.affine()).with_frame(self.state.into())
    }

    pub fn state(&self) -> BoxState {
        self.state
    }

    pub fn moves_pixels(&self) -> bool {
        self.floating.is_some()
    }

    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    pub fn drag_kind(&self) -> Option<DragKind> {
        self.drag.as_ref().map(|drag| drag.kind)
    }

    /// A press at a point of the image. `reach` is how many pixels of the image a point
    /// of the screen takes.
    pub fn press(&mut self, kind: DragKind, point: (f64, f64), reach: f64) {
        self.drag = Some(Drag { kind, press: point, from: self.state, reach, moved: false });
    }

    /// The drag reaches a point of the image. Returns the selection where the box now
    /// is. `shift` keeps the proportions or turns in steps, and `ctrl` resizes about the
    /// middle. Moving, resizing and the centre of rotation snap to the cells of `snap`,
    /// if given.
    pub fn drag_to(&mut self, point: (f64, f64), shift: bool, ctrl: bool, snap: Option<Cells>) -> Option<Selection> {
        let drag = self.drag.as_mut()?;
        drag.moved |= drag.kind != DragKind::Pivot;
        self.state = match drag.kind {
            DragKind::Move => drag.from.moved(drag.press, point, snap),
            DragKind::Rotate => drag.from.rotated(drag.press, point, shift),
            DragKind::Resize(grip) => drag.from.resized(grip, point, shift, ctrl, snap).unwrap_or(self.state),
            DragKind::Pivot => drag.from.pivot_moved(drag.press, point, drag.reach, snap),
        };
        Some(self.selection())
    }

    /// The drag ends. Returns whether it moved the selection, and the layer with the
    /// pixels drawn where they now are, as `interpolation` redraws them, if it moves pixels.
    pub fn release(&mut self, interpolation: Interpolation) -> (bool, Option<RgbaImage>) {
        let Some(drag) = self.drag.take() else {
            return (false, None);
        };
        if !drag.moved {
            return (false, None);
        }
        (true, self.placed(interpolation))
    }

    /// Moves the box by `offset`, in whole pixels, between drags. Returns the selection
    /// where it now is, and the layer with the pixels drawn there, if it moves pixels.
    pub fn nudge(&mut self, offset: (i32, i32), interpolation: Interpolation) -> (Selection, Option<RgbaImage>) {
        self.state.center = add(self.state.center, (offset.0 as f64, offset.1 as f64));
        (self.selection(), self.placed(interpolation))
    }

    /// The box was put down as a step of the document's history, the current one.
    pub fn committed_as(&mut self, step: usize) {
        self.committed = Some(step);
    }

    /// The layer with the pixels drawn again where they were last put down, as
    /// `interpolation` now redraws them, if they were and that is still the document's
    /// `current_step`.
    pub fn placed_again(&self, interpolation: Interpolation, current_step: usize) -> Option<RgbaImage> {
        self.committed.filter(|&step| step == current_step)?;
        self.placed(interpolation)
    }

    fn placed(&self, interpolation: Interpolation) -> Option<RgbaImage> {
        self.floating.as_ref().map(|floating| floating.pixels.place(&self.affine(), interpolation))
    }

    /// Puts the box back where it was before the drag. Returns the selection there.
    pub fn cancel(&mut self) -> Selection {
        if let Some(drag) = self.drag.take() {
            self.state = drag.from;
        }
        self.selection()
    }

    /// Maps the box as it was at first onto the box as it is now.
    fn affine(&self) -> Affine {
        let (start_x, start_y) = self.start.center;
        let scale = (self.state.size.0 / self.start.size.0, self.state.size.1 / self.start.size.1);
        Affine::translation(-start_x, -start_y)
            .then(Affine::rotation(-self.start.angle))
            .then(Affine::scale(scale.0, scale.1))
            .then(Affine::rotation(self.state.angle))
            .then(Affine::translation(self.state.center.0, self.state.center.1))
    }

    /// While pixels are being dragged, draws the image with the layer without them and
    /// them where they now are, as `interpolation` will redraw them, in place of the
    /// image. Returns whether it did.
    pub fn paint_preview(&self, painter: &egui::Painter, view: &View, interpolation: Interpolation) -> bool {
        let Some(floating) = self.floating.as_ref().filter(|_| self.is_dragging()) else {
            return false;
        };
        let rect = view.image_rect();
        if let Some(below) = &floating.below {
            below.paint(painter, rect, 1.0);
        }
        if let Some(opacity) = floating.opacity {
            floating.base_texture.paint(painter, rect, opacity);
            floating.preview.paint(painter, view, &self.affine(), interpolation, opacity);
        }
        if let Some(above) = &floating.above {
            above.paint(painter, rect, 1.0);
        }
        true
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn upright(left: f64, top: f64, right: f64, bottom: f64) -> BoxState {
        BoxState::around(Bounds { left, top, right, bottom })
    }

    #[test]
    fn what_a_press_does_where() {
        let state = upright(0.0, 0.0, 100.0, 50.0);
        assert_eq!(state.drag_at((40.0, 25.0), 1.0), Some(DragKind::Move));
        assert_eq!(state.drag_at((100.0, 25.0), 1.0), Some(DragKind::Resize((1, 0))));
        assert_eq!(state.drag_at((-2.0, -2.0), 1.0), Some(DragKind::Resize((-1, -1))));
        assert_eq!(state.drag_at((50.0, 60.0), 1.0), Some(DragKind::Rotate));
        assert_eq!(state.drag_at((50.0, 90.0), 1.0), None);
        // Zoomed in four times, the reaches cover fewer pixels.
        assert_eq!(state.drag_at((50.0, 60.0), 0.25), None);
    }

    #[test]
    fn handles_alone_work_the_box() {
        let state = upright(0.0, 0.0, 100.0, 50.0);
        assert_eq!(state.handle_at((101.0, 49.0), 1.0), Some(DragKind::Resize((1, 1))));
        assert_eq!(state.handle_at((50.0, -3.0), 1.0), Some(DragKind::Resize((0, -1))));
        // Along an edge, away from its handles, or inside.
        assert_eq!(state.handle_at((100.0, 12.0), 1.0), None);
        assert_eq!(state.handle_at((30.0, 25.0), 1.0), None);
        // The handle out from the bottom right corner moves it, and around it, it rotates.
        assert_eq!(state.move_handle(1.0), (118.0, 68.0));
        assert_eq!(state.handle_at((121.0, 64.0), 1.0), Some(DragKind::Move));
        assert_eq!(state.handle_at((103.0, 68.0), 1.0), Some(DragKind::Rotate));
        // All around the box, it rotates, but no further.
        assert_eq!(state.handle_at((110.0, -10.0), 1.0), Some(DragKind::Rotate));
        assert_eq!(state.handle_at((50.0, -15.0), 1.0), Some(DragKind::Rotate));
        assert_eq!(state.handle_at((140.0, -40.0), 1.0), None);
    }

    #[test]
    fn the_move_handle_moves_the_box() {
        let state = upright(0.0, 0.0, 100.0, 50.0);
        assert_eq!(state.drag_at((118.0, 68.0), 1.0), Some(DragKind::Move));
        // Around it, the box rotates, as all around the box.
        assert_eq!(state.drag_at((128.0, 78.0), 1.0), Some(DragKind::Rotate));
        // Further out when zoomed out, and on a turned box, out from its turned corner.
        assert_eq!(state.move_handle(2.0), (136.0, 86.0));
        let turned = BoxState { angle: PI / 2.0, ..state };
        let (x, y) = turned.move_handle(1.0);
        assert!((x - (50.0 - 43.0)).abs() < 1e-9 && (y - (25.0 + 68.0)).abs() < 1e-9);
    }

    #[test]
    fn a_small_box_can_still_be_moved() {
        let state = upright(0.0, 0.0, 4.0, 4.0);
        assert_eq!(state.drag_at((2.0, 2.0), 1.0), Some(DragKind::Move));
    }

    #[test]
    fn move_by_whole_pixels() {
        let state = upright(0.0, 0.0, 10.0, 10.0).moved((5.0, 5.0), (8.4, 3.6), None);
        assert_eq!(state.center, (8.0, 4.0));
    }

    #[test]
    fn snap_to_the_grid() {
        let cells = Some(Cells { origin: [0, 0], size: [8, 8] });
        // The top left corner goes to the nearest corner of a cell.
        let moved = upright(0.0, 0.0, 10.0, 6.0).moved((5.0, 5.0), (8.0, 15.0), cells);
        assert_eq!(moved.center, (5.0, 11.0));
        // An upright box's edge goes to the nearest line.
        let wider = upright(0.0, 0.0, 10.0, 6.0).resized((1, 0), (19.0, 3.0), false, false, cells).unwrap();
        assert_eq!((wider.center, wider.size), ((8.0, 3.0), (16.0, 6.0)));
    }

    #[test]
    fn resize_keeps_the_opposite_edge() {
        let state = upright(0.0, 0.0, 10.0, 10.0);
        let wider = state.resized((1, 0), (20.0, 3.0), false, false, None).unwrap();
        assert_eq!((wider.center, wider.size), ((10.0, 5.0), (20.0, 10.0)));
        // The corner goes up to y = -10: twice as tall, and as wide with Shift.
        let corner = state.resized((-1, -1), (5.0, -10.0), true, false, None).unwrap();
        assert_eq!((corner.center, corner.size), ((0.0, 0.0), (20.0, 20.0)));
        // Past the opposite edge, it is mirrored.
        let flipped = state.resized((1, 0), (-10.0, 0.0), false, false, None).unwrap();
        assert_eq!((flipped.center, flipped.size), ((-5.0, 5.0), (-10.0, 10.0)));
        assert!(state.resized((1, 0), (0.0, 0.0), false, false, None).is_none());
    }

    #[test]
    fn rotate_about_the_middle() {
        let state = upright(0.0, 0.0, 10.0, 10.0);
        let turned = state.rotated((10.0, 5.0), (5.0, 10.0), false);
        assert!((turned.angle - PI / 2.0).abs() < 1e-9);
        assert_eq!(turned.center, state.center);
        let snapped = state.rotated((10.0, 5.0), (10.0, 6.0), true);
        assert!((snapped.angle - ROTATE_SNAP).abs() < 1e-9);
    }

    #[test]
    fn resize_about_the_middle() {
        let state = upright(0.0, 0.0, 10.0, 10.0);
        let wider = state.resized((1, 0), (15.0, 3.0), false, true, None).unwrap();
        assert_eq!((wider.center, wider.size), ((5.0, 5.0), (20.0, 10.0)));
        let corner = state.resized((1, 1), (12.0, 15.0), true, true, None).unwrap();
        assert_eq!((corner.center, corner.size), ((5.0, 5.0), (20.0, 20.0)));
    }

    #[test]
    fn rotate_about_the_centre_of_rotation() {
        // Turning a quarter about its top left corner.
        let state = BoxState { pivot: (-0.5, -0.5), ..upright(0.0, 0.0, 10.0, 10.0) };
        assert_eq!(state.pivot_point(), (0.0, 0.0));
        let turned = state.rotated((10.0, 0.0), (0.0, 10.0), false);
        assert!((turned.center.0 + 5.0).abs() < 1e-9 && (turned.center.1 - 5.0).abs() < 1e-9);
        let pivot = turned.pivot_point();
        assert!(pivot.0.abs() < 1e-9 && pivot.1.abs() < 1e-9);
        // It follows the box as it moves and resizes.
        let moved = state.moved((5.0, 5.0), (8.0, 5.0), None);
        assert_eq!(moved.pivot_point(), (3.0, 0.0));
        let wider = state.resized((-1, 0), (-10.0, 5.0), false, false, None).unwrap();
        assert_eq!(wider.pivot_point(), (-10.0, 0.0));
    }

    #[test]
    fn move_the_centre_of_rotation() {
        let state = upright(0.0, 0.0, 100.0, 50.0);
        assert_eq!(state.drag_at((51.0, 26.0), 1.0), Some(DragKind::Pivot));
        assert_eq!(state.handle_at((51.0, 26.0), 1.0), Some(DragKind::Pivot));
        // Not on a box too small for it to leave room to move it.
        assert_eq!(upright(0.0, 0.0, 20.0, 20.0).drag_at((10.0, 10.0), 1.0), Some(DragKind::Move));
        // To the nearest corner or middle of a pixel, or onto a handle within reach.
        let moved = state.pivot_moved((50.0, 25.0), (70.2, 30.4), 1.0, None);
        assert_eq!(moved.pivot_point(), (70.0, 30.5));
        let on_corner = state.pivot_moved((50.0, 25.0), (96.0, 47.0), 1.0, None);
        assert_eq!(on_corner.pivot_point(), (100.0, 50.0));
        let back = on_corner.pivot_moved((100.0, 50.0), (52.0, 27.0), 1.0, None);
        assert_eq!(back.pivot, (0.0, 0.0));
    }

        #[test]
    fn resize_cursor_follows_the_turn() {
        assert_eq!(DragKind::Resize((1, 0)).cursor(0.0), CursorIcon::ResizeHorizontal);
        assert_eq!(DragKind::Resize((1, 1)).cursor(0.0), CursorIcon::ResizeNwSe);
        assert_eq!(DragKind::Resize((1, 0)).cursor(PI / 2.0), CursorIcon::ResizeVertical);
    }

    #[test]
    fn transform_moves_the_selection() {
        let mut document = Document::new(RgbaImage::new(20, 20));
        document.selection = Selection::rect(power_paint_core::PixelRect { left: 0, top: 0, right: 10, bottom: 10 });
        let ctx = egui::Context::default();
        let mut transform = Transform::begin(&ctx, &document, false).unwrap();
        transform.press(DragKind::Move, (5.0, 5.0), 1.0);
        let moved = transform.drag_to((8.0, 5.0), false, false, None).unwrap();
        assert_eq!(moved.bounds(), Some(Bounds { left: 3.0, top: 0.0, right: 13.0, bottom: 10.0 }));
        assert_eq!(transform.release(Interpolation::Bilinear), (true, None));
        // The next drag starts from where the last one left the box.
        transform.press(DragKind::Resize((1, 0)), (13.0, 5.0), 1.0);
        let wider = transform.drag_to((23.0, 5.0), false, false, None).unwrap();
        assert_eq!(wider.bounds(), Some(Bounds { left: 3.0, top: 0.0, right: 23.0, bottom: 10.0 }));
        assert_eq!(transform.cancel().bounds(), moved.bounds());
        // Moving the centre of rotation alone moves nothing.
        transform.press(DragKind::Pivot, (8.0, 5.0), 1.0);
        transform.drag_to((10.0, 5.0), false, false, None);
        assert_eq!(transform.release(Interpolation::Bilinear), (false, None));
        let (nudged, placed) = transform.nudge((-1, 2), Interpolation::Bilinear);
        assert_eq!(nudged.bounds(), Some(Bounds { left: 2.0, top: 2.0, right: 12.0, bottom: 12.0 }));
        assert_eq!(placed, None);
    }

    #[test]
    fn pixels_put_down_can_be_redrawn_another_way() {
        let mut document = Document::new(RgbaImage::from_pixel(20, 20, power_paint_core::Rgba([255, 0, 0, 255])));
        document.selection = Selection::rect(power_paint_core::PixelRect { left: 0, top: 0, right: 10, bottom: 10 });
        let ctx = egui::Context::default();
        let mut transform = Transform::begin(&ctx, &document, true).unwrap();
        transform.press(DragKind::Rotate, (15.0, 5.0), 1.0);
        transform.drag_to((10.0, 10.0), false, false, None);
        let (moved, placed) = transform.release(Interpolation::Bilinear);
        assert!(moved && placed.is_some());
        // Not before it is a step, nor once another one follows it.
        assert!(transform.placed_again(Interpolation::Nearest, 1).is_none());
        transform.committed_as(1);
        let again = transform.placed_again(Interpolation::Nearest, 1).unwrap();
        assert_ne!(Some(&again), placed.as_ref());
        assert!(transform.placed_again(Interpolation::Nearest, 2).is_none());
    }

    #[test]
    fn a_turned_box_is_kept_by_the_selection() {
        let mut document = Document::new(RgbaImage::new(40, 40));
        document.selection = Selection::rect(power_paint_core::PixelRect { left: 10, top: 10, right: 30, bottom: 20 });
        let ctx = egui::Context::default();
        let mut transform = Transform::begin(&ctx, &document, false).unwrap();
        transform.press(DragKind::Rotate, (30.0, 15.0), 1.0);
        let turned = transform.drag_to((20.0, 25.0), false, false, None).unwrap();
        transform.release(Interpolation::Bilinear);
        let state = transform.state();
        assert_eq!(BoxState::of(&turned), Some(state));
        // Left and taken up again, by undoing a Deselect, the box carries on as it was.
        document.selection = turned.clone();
        let mut again = Transform::begin(&ctx, &document, false).unwrap();
        assert_eq!(again.state(), state);
        again.press(DragKind::Move, (20.0, 15.0), 1.0);
        let moved = again.drag_to((22.0, 15.0), false, false, None).unwrap();
        let expected = turned.transformed(&Affine::translation(2.0, 0.0));
        for (outline, expected) in moved.outlines().iter().zip(expected.outlines()) {
            for (point, expected) in outline.iter().zip(expected) {
                assert!((point.0 - expected.0).abs() < 1e-9 && (point.1 - expected.1).abs() < 1e-9);
            }
        }
        // Turned back upright, it is in the box around it.
        again.press(DragKind::Rotate, (22.0, 25.0), 1.0);
        let upright = again.drag_to((32.0, 15.0), false, false, None).unwrap();
        assert!(upright.frame().is_none());
    }
}

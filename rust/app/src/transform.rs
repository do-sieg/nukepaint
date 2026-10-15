//! The box around the selection, which moves, resizes and rotates it. With the
//! selection tools it carries the selected pixels along; with Move Selection, the
//! outline alone.
//!
//! A press inside the box moves it, on an edge or corner resizes it, and just outside
//! rotates it about its middle, as does the right button anywhere with Move Selection.
//! Shift keeps the box's proportions while resizing, and turns it in steps of 15°.
//! A transform lasts from its first drag until the tool or the selection changes some
//! other way: until then every drag starts again from what the box held at first, so
//! nothing degrades.

use std::f64::consts::PI;

use eframe::egui::{self, CursorIcon, Pos2};
use power_paint_core::{Affine, Bounds, FloatingPixels, RgbaImage, Selection};

use crate::{tiled_texture::TiledTexture, view::View};

/// How far from an edge of the box, in points, a press still grabs that edge.
pub const GRIP_REACH: f64 = 4.0;
/// How far outside the box, in points, a press still rotates it.
pub const ROTATE_REACH: f64 = 24.0;
/// Rotation step while Shift is held.
const ROTATE_SNAP: f64 = PI / 12.0;

/// Where the box is: its middle, its size, negative along an axis it has been
/// mirrored on, and how far it has turned, in radians clockwise.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxState {
    pub center: (f64, f64),
    pub size: (f64, f64),
    pub angle: f64,
}

impl BoxState {
    pub fn around(bounds: Bounds) -> Self {
        Self { center: bounds.center(), size: (bounds.width(), bounds.height()), angle: 0.0 }
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

    /// What a press at a point of the image would do to the box. `reach` is how many
    /// pixels of the image a point of the screen takes.
    pub fn drag_at(&self, point: (f64, f64), reach: f64) -> Option<DragKind> {
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

    /// Which edges of the box are within `reach` of a point: -1 for the left or top
    /// one, 1 for the right or bottom one, 0 for neither.
    fn grip_at(&self, point: (f64, f64), reach: f64) -> (i32, i32) {
        let local = rotate(sub(point, self.center), -self.angle);
        let half = (self.size.0.abs() / 2.0, self.size.1.abs() / 2.0);
        (edge_at(local.0, half.0, reach), edge_at(local.1, half.1, reach))
    }

    /// Moved by how far the pointer went, in whole pixels.
    fn moved(&self, press: (f64, f64), point: (f64, f64)) -> Self {
        let offset = sub(point, press);
        Self { center: add(self.center, (offset.0.round(), offset.1.round())), ..*self }
    }

    /// Turned about its middle by the angle the pointer went round it.
    fn rotated(&self, press: (f64, f64), point: (f64, f64), snap: bool) -> Self {
        let (from, to) = (sub(press, self.center), sub(point, self.center));
        let mut angle = self.angle + to.1.atan2(to.0) - from.1.atan2(from.0);
        if snap {
            angle = (angle / ROTATE_SNAP).round() * ROTATE_SNAP;
        }
        Self { angle, ..*self }
    }

    /// Stretched for the grabbed edges to follow the pointer, the opposite ones staying
    /// put. With `keep_ratio`, it keeps its proportions. Nothing comes of a size under
    /// one pixel.
    fn resized(&self, grip: (i32, i32), point: (f64, f64), keep_ratio: bool) -> Option<Self> {
        // An upright box keeps its edges on whole pixels.
        let point = if self.angle == 0.0 { (point.0.round(), point.1.round()) } else { point };
        let local = rotate(sub(point, self.center), -self.angle);
        let extent = (self.size.0.abs(), self.size.1.abs());
        // How many times larger the box becomes along each axis; negative once an edge
        // has been dragged past the opposite one.
        let factor_along = |grip: i32, local: f64, extent: f64| {
            if grip == 0 { 1.0 } else { local * grip as f64 / extent + 0.5 }
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
        let shift = (
            grip.0 as f64 * (factor.0 - 1.0) * extent.0 / 2.0,
            grip.1 as f64 * (factor.1 - 1.0) * extent.1 / 2.0,
        );
        Some(Self {
            center: add(self.center, rotate(shift, self.angle)),
            size: (self.size.0 * factor.0, self.size.1 * factor.1),
            angle: self.angle,
        })
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
}

impl DragKind {
    /// The pointer that shows what a drag would do, with the box turned by `angle`.
    pub fn cursor(self, angle: f64) -> CursorIcon {
        match self {
            Self::Move => CursorIcon::Move,
            // The closest there is to a turning arrow.
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
    moved: bool,
}

/// The selected pixels taken out of the layer, and their textures to show them moving.
struct Floating {
    pixels: FloatingPixels,
    texture: TiledTexture,
    /// The layer without them.
    base_texture: TiledTexture,
}

impl Floating {
    fn new(ctx: &egui::Context, pixels: FloatingPixels) -> Self {
        let texture = TiledTexture::new(ctx, "floating pixels", pixels.pixels());
        let base_texture = TiledTexture::new(ctx, "floating base", pixels.base());
        Self { pixels, texture, base_texture }
    }
}

pub struct Transform {
    /// The box around the selection when the transform began, and the selection then.
    start: Bounds,
    selection_start: Selection,
    /// The pixels being moved; none when only the outline is.
    floating: Option<Floating>,
    state: BoxState,
    drag: Option<Drag>,
}

impl Transform {
    /// Starts moving the selection, and the pixels of `layer` it holds if given. There
    /// is nothing to move without a selection.
    pub fn begin(ctx: &egui::Context, selection: &Selection, layer: Option<&RgbaImage>) -> Option<Self> {
        let start = selection.bounds()?;
        let floating = layer.and_then(|layer| FloatingPixels::lift(layer, selection));
        let floating = floating.map(|pixels| Floating::new(ctx, pixels));
        Some(Self { start, selection_start: selection.clone(), floating, state: BoxState::around(start), drag: None })
    }

    /// Starts moving pasted pixels, selected all around.
    pub fn begin_pasted(ctx: &egui::Context, pixels: FloatingPixels) -> Self {
        let selection = Selection::rect(pixels.area());
        // Pasted pixels are never empty, so neither is their selection.
        let start = selection.bounds().expect("pasted pixels are selected");
        let floating = Some(Floating::new(ctx, pixels));
        Self { start, selection_start: selection, floating, state: BoxState::around(start), drag: None }
    }

    /// The selection the box holds.
    pub fn selection(&self) -> Selection {
        self.selection_start.transformed(&self.affine())
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

    pub fn press(&mut self, kind: DragKind, point: (f64, f64)) {
        self.drag = Some(Drag { kind, press: point, from: self.state, moved: false });
    }

    /// The drag reaches a point of the image. Returns the selection where the box now is.
    pub fn drag_to(&mut self, point: (f64, f64), shift: bool) -> Option<Selection> {
        let drag = self.drag.as_mut()?;
        drag.moved = true;
        self.state = match drag.kind {
            DragKind::Move => drag.from.moved(drag.press, point),
            DragKind::Rotate => drag.from.rotated(drag.press, point, shift),
            DragKind::Resize(grip) => drag.from.resized(grip, point, shift).unwrap_or(self.state),
        };
        Some(self.selection())
    }

    /// The drag ends. Returns whether it moved anything, and the layer with the pixels
    /// drawn where they now are, if it moves pixels.
    pub fn release(&mut self) -> (bool, Option<RgbaImage>) {
        let Some(drag) = self.drag.take() else {
            return (false, None);
        };
        if !drag.moved {
            return (false, None);
        }
        (true, self.floating.as_ref().map(|floating| floating.pixels.place(&self.affine())))
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
        let (start_x, start_y) = self.start.center();
        let scale = (self.state.size.0 / self.start.width(), self.state.size.1 / self.start.height());
        Affine::translation(-start_x, -start_y)
            .then(Affine::scale(scale.0, scale.1))
            .then(Affine::rotation(self.state.angle))
            .then(Affine::translation(self.state.center.0, self.state.center.1))
    }

    /// While pixels are being dragged, draws the layer without them and them where
    /// they now are, in place of the layer. Returns whether it did.
    pub fn paint_preview(&self, painter: &egui::Painter, view: &View) -> bool {
        let Some(floating) = self.floating.as_ref().filter(|_| self.is_dragging()) else {
            return false;
        };
        floating.base_texture.paint(painter, view.image_rect());
        let area = floating.pixels.area();
        let map = self.affine();
        let to_screen = |point: Pos2| {
            let (x, y) = map.apply((point.x as f64 + area.left as f64, point.y as f64 + area.top as f64));
            view.to_screen(x as f32, y as f32)
        };
        // About how many screen pixels each of the pixels takes, to draw from the copy that suits.
        let scale = (self.state.size.0.abs() / self.start.width()).max(self.state.size.1.abs() / self.start.height());
        floating.texture.paint_mapped(painter, to_screen, scale as f32 * view.zoom);
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
        assert_eq!(state.drag_at((50.0, 25.0), 1.0), Some(DragKind::Move));
        assert_eq!(state.drag_at((100.0, 25.0), 1.0), Some(DragKind::Resize((1, 0))));
        assert_eq!(state.drag_at((-2.0, -2.0), 1.0), Some(DragKind::Resize((-1, -1))));
        assert_eq!(state.drag_at((50.0, 60.0), 1.0), Some(DragKind::Rotate));
        assert_eq!(state.drag_at((50.0, 90.0), 1.0), None);
        // Zoomed in four times, the reaches cover fewer pixels.
        assert_eq!(state.drag_at((50.0, 60.0), 0.25), None);
    }

    #[test]
    fn a_small_box_can_still_be_moved() {
        let state = upright(0.0, 0.0, 4.0, 4.0);
        assert_eq!(state.drag_at((2.0, 2.0), 1.0), Some(DragKind::Move));
    }

    #[test]
    fn move_by_whole_pixels() {
        let state = upright(0.0, 0.0, 10.0, 10.0).moved((5.0, 5.0), (8.4, 3.6));
        assert_eq!(state.center, (8.0, 4.0));
    }

    #[test]
    fn resize_keeps_the_opposite_edge() {
        let state = upright(0.0, 0.0, 10.0, 10.0);
        let wider = state.resized((1, 0), (20.0, 3.0), false).unwrap();
        assert_eq!((wider.center, wider.size), ((10.0, 5.0), (20.0, 10.0)));
        // The corner goes up to y = -10: twice as tall, and as wide with Shift.
        let corner = state.resized((-1, -1), (5.0, -10.0), true).unwrap();
        assert_eq!((corner.center, corner.size), ((0.0, 0.0), (20.0, 20.0)));
        // Past the opposite edge, it is mirrored.
        let flipped = state.resized((1, 0), (-10.0, 0.0), false).unwrap();
        assert_eq!((flipped.center, flipped.size), ((-5.0, 5.0), (-10.0, 10.0)));
        assert!(state.resized((1, 0), (0.0, 0.0), false).is_none());
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
    fn resize_cursor_follows_the_turn() {
        assert_eq!(DragKind::Resize((1, 0)).cursor(0.0), CursorIcon::ResizeHorizontal);
        assert_eq!(DragKind::Resize((1, 1)).cursor(0.0), CursorIcon::ResizeNwSe);
        assert_eq!(DragKind::Resize((1, 0)).cursor(PI / 2.0), CursorIcon::ResizeVertical);
    }

    #[test]
    fn transform_moves_the_selection() {
        let selection = Selection::rect(power_paint_core::PixelRect { left: 0, top: 0, right: 10, bottom: 10 });
        let ctx = egui::Context::default();
        let mut transform = Transform::begin(&ctx, &selection, None).unwrap();
        transform.press(DragKind::Move, (5.0, 5.0));
        let moved = transform.drag_to((8.0, 5.0), false).unwrap();
        assert_eq!(moved.bounds(), Some(Bounds { left: 3.0, top: 0.0, right: 13.0, bottom: 10.0 }));
        assert_eq!(transform.release(), (true, None));
        // The next drag starts from where the last one left the box.
        transform.press(DragKind::Resize((1, 0)), (13.0, 5.0));
        let wider = transform.drag_to((23.0, 5.0), false).unwrap();
        assert_eq!(wider.bounds(), Some(Bounds { left: 3.0, top: 0.0, right: 23.0, bottom: 10.0 }));
        assert_eq!(transform.cancel().bounds(), moved.bounds());
    }
}

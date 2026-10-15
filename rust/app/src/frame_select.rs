//! Rectangle Select and Ellipse Select: a drag selects the rectangle it frames, or the
//! ellipse that fits in it, between the corners of pixels nearest to where it begins
//! and ends, a square or a circle while Shift is held. The selection mode says what it does
//! to the selection there already is, and holding Ctrl when the drag begins adds to
//! it whatever the mode. A click that frames nothing selects
//! nothing, unless it lands on the selection in a mode other than Replace. Escape
//! cancels a drag.

use power_paint_core::{PixelRect, Selection, SelectionMode};

/// What a drag selects in the rectangle it frames.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Shape {
    #[default]
    Rectangle,
    Ellipse,
}

#[derive(Default)]
pub struct FrameSelect {
    drag: Option<Drag>,
}

struct Drag {
    shape: Shape,
    /// The corner between pixels the drag began on, and the pixel it was pressed on.
    from: (i32, i32),
    pressed: (i64, i64),
    /// The selection before the drag, which what is framed replaces or changes.
    base: Selection,
    mode: SelectionMode,
    moved: bool,
}

impl FrameSelect {
    /// A drag begins. A selection that what is framed replaces is cleared at once.
    pub fn press(
        &mut self,
        selection: &mut Selection,
        shape: Shape,
        from: (i32, i32),
        pressed: (i64, i64),
        mode: SelectionMode,
    ) {
        let base = std::mem::take(selection);
        if mode != SelectionMode::Replace {
            *selection = base.clone();
        }
        self.drag = Some(Drag { shape, from, pressed, base, mode, moved: false });
    }

    /// The drag reaches the corner `to`. With `even`, it frames a square, as large as
    /// fits between the two corners.
    pub fn drag_to(&mut self, selection: &mut Selection, to: (i32, i32), even: bool) {
        let Some(drag) = &mut self.drag else {
            return;
        };
        drag.moved = true;
        let (from, mut to) = (drag.from, to);
        if even {
            let side = (to.0 - from.0).abs().min((to.1 - from.1).abs());
            to = (from.0 + side * (to.0 - from.0).signum(), from.1 + side * (to.1 - from.1).signum());
        }
        let rect = PixelRect::from_corners(from, to);
        let drawn = match drag.shape {
            Shape::Rectangle => Selection::rect(rect),
            Shape::Ellipse => Selection::ellipse(rect),
        };
        *selection = drag.base.combine(&drawn, drag.mode);
    }

    pub fn release(&mut self, selection: &mut Selection) {
        let Some(drag) = self.drag.take() else {
            return;
        };
        let (x, y) = drag.pressed;
        if !drag.moved && (drag.mode == SelectionMode::Replace || !drag.base.contains(x, y)) {
            *selection = Selection::default();
        }
    }

    /// Puts the selection back the way it was before the drag.
    pub fn cancel(&mut self, selection: &mut Selection) {
        if let Some(drag) = self.drag.take() {
            *selection = drag.base;
        }
    }

    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn select(selection: &mut Selection, from: (i32, i32), to: (i32, i32), mode: SelectionMode) {
        let mut tool = FrameSelect::default();
        tool.press(selection, Shape::Rectangle, from, (from.0 as i64, from.1 as i64), mode);
        tool.drag_to(selection, to, false);
        tool.release(selection);
    }

    fn click(selection: &mut Selection, at: (i32, i32), mode: SelectionMode) {
        let mut tool = FrameSelect::default();
        tool.press(selection, Shape::Rectangle, at, (at.0 as i64, at.1 as i64), mode);
        tool.release(selection);
    }

    #[test]
    fn replace_and_add() {
        let mut selection = Selection::default();
        select(&mut selection, (5, 5), (0, 0), SelectionMode::Replace);
        assert_eq!(selection, Selection::rect(PixelRect { left: 0, top: 0, right: 5, bottom: 5 }));
        select(&mut selection, (10, 0), (12, 2), SelectionMode::Add);
        assert_eq!(selection.outlines().len(), 2);
        select(&mut selection, (20, 20), (22, 22), SelectionMode::Replace);
        assert_eq!(selection, Selection::rect(PixelRect { left: 20, top: 20, right: 22, bottom: 22 }));
    }

    #[test]
    fn a_click_deselects_unless_it_adds_on_the_selection() {
        let square = Selection::rect(PixelRect { left: 0, top: 0, right: 4, bottom: 4 });
        let mut selection = square.clone();
        click(&mut selection, (1, 1), SelectionMode::Add);
        assert_eq!(selection, square);
        click(&mut selection, (8, 8), SelectionMode::Add);
        assert!(selection.is_empty());
        let mut selection = square.clone();
        click(&mut selection, (1, 1), SelectionMode::Replace);
        assert!(selection.is_empty());
    }

    #[test]
    fn ellipse_and_circle() {
        let mut selection = Selection::default();
        let mut tool = FrameSelect::default();
        tool.press(&mut selection, Shape::Ellipse, (10, 10), (10, 10), SelectionMode::Replace);
        tool.drag_to(&mut selection, (0, 4), false);
        assert_eq!(selection, Selection::ellipse(PixelRect { left: 0, top: 4, right: 10, bottom: 10 }));
        // With Shift, the shorter side wins, on the side the pointer is.
        tool.drag_to(&mut selection, (0, 4), true);
        assert_eq!(selection, Selection::ellipse(PixelRect { left: 4, top: 4, right: 10, bottom: 10 }));
        tool.release(&mut selection);
    }

    #[test]
    fn cancel_restores_the_selection() {
        let square = Selection::rect(PixelRect { left: 0, top: 0, right: 4, bottom: 4 });
        let mut selection = square.clone();
        let mut tool = FrameSelect::default();
        tool.press(&mut selection, Shape::Rectangle, (6, 6), (6, 6), SelectionMode::Replace);
        tool.drag_to(&mut selection, (9, 9), false);
        tool.cancel(&mut selection);
        assert_eq!(selection, square);
    }
}

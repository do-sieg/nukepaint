//! Lasso Select and Polygon Select: an outline through corners between pixels, drawn
//! by dragging around what to select, or put down a corner at a click, selects what
//! it goes around once it is closed. Until then the selection shows what it would be
//! if it closed now. The selection mode says what it does to the selection there
//! already is, and holding Ctrl when it begins adds to it whatever the mode. An
//! outline that goes around no pixel selects nothing, unless it begins on the
//! selection in a mode other than Replace.
//!
//! Polygon Select's corners can then still be moved, added to by a click on an edge,
//! and removed, each a step of its own, for as long as the selection is the one they
//! left: undoing and redoing those steps brings them back as they were then.

use power_paint_core::{Document, Overlaps, Selection, SelectionMode};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LassoKind {
    /// Lasso Select, which draws the outline as it is dragged.
    Freehand,
    /// Polygon Select, which puts down a corner at each click.
    Polygon,
}

/// An outline being drawn, which is not a selection until it is closed.
pub struct Lasso {
    pub kind: LassoKind,
    corners: Vec<(i32, i32)>,
    /// Where Polygon Select's outline goes on to, the pointer's corner, once it closes.
    to: Option<(i32, i32)>,
    /// The pixel it began on.
    pressed: (i64, i64),
    /// The selection before it began, which giving it up puts back.
    before: Selection,
    /// What the outline changes once closed, as `mode` says, and how it shuts in where it
    /// crosses itself.
    base: Selection,
    mode: SelectionMode,
    overlaps: Overlaps,
    /// The selection it last showed, and the step of the history then: it goes on only
    /// while nothing else changes them.
    shown: Selection,
    step: usize,
}

/// A closed outline.
pub struct Closed {
    /// The selection before it began, if it changed anything.
    pub before: Option<Selection>,
    /// Polygon Select's outline, if it went around any pixel, to go on changing once
    /// its corners are recorded with the step the outline is.
    pub polygon: Option<PolygonEdit>,
    pub corners: Vec<(i32, i32)>,
}

impl Lasso {
    /// Begins an outline on the corner `corner` of the document, pressed on the pixel
    /// `pressed`. A selection that it replaces is cleared at once.
    pub fn begin(
        document: &mut Document,
        kind: LassoKind,
        corner: (i32, i32),
        pressed: (i64, i64),
        mode: SelectionMode,
        overlaps: Overlaps,
    ) -> Self {
        let before = document.selection.clone();
        let base = if mode == SelectionMode::Replace { Selection::default() } else { before.clone() };
        let step = document.current_step();
        let mut lasso = Self {
            kind,
            corners: vec![corner],
            to: None,
            pressed,
            before,
            base,
            mode,
            overlaps,
            shown: Selection::default(),
            step,
        };
        lasso.show(document);
        lasso
    }

    /// Whether the document is still as the outline left it, for it to go on.
    pub fn is_current(&self, document: &Document) -> bool {
        document.current_step() == self.step && document.selection == self.shown
    }

    pub fn corners(&self) -> &[(i32, i32)] {
        &self.corners
    }

    /// The corners and where the outline goes on to, as it would close now.
    pub fn outline(&self) -> Vec<(i32, i32)> {
        let mut outline = self.corners.clone();
        outline.extend(self.to.filter(|&to| Some(&to) != self.corners.last()));
        outline
    }

    /// Adds a corner, unless the outline already ends there.
    pub fn add(&mut self, document: &mut Document, corner: (i32, i32)) {
        if self.corners.last() != Some(&corner) {
            self.corners.push(corner);
            self.show(document);
        }
    }

    /// Takes back the last corner. Returns whether any is left.
    pub fn remove_last(&mut self, document: &mut Document) -> bool {
        self.corners.pop();
        self.show(document);
        !self.corners.is_empty()
    }

    /// Has Polygon Select's outline go on to the corner the pointer is on, or nowhere.
    pub fn reach(&mut self, document: &mut Document, to: Option<(i32, i32)>) {
        if self.to != to {
            self.to = to;
            self.show(document);
        }
    }

    /// Shows what the outline would select if it closed now.
    fn show(&mut self, document: &mut Document) {
        let drawn = Selection::polygon(&self.outline(), self.overlaps);
        self.shown = self.base.combine(&drawn, self.mode);
        document.selection = self.shown.clone();
    }

    /// Closes the outline: what it goes around is selected as the mode says, or, if it
    /// goes around no pixel, nothing, unless it began on the selection in a mode other
    /// than Replace.
    pub fn close(self, document: &mut Document) -> Closed {
        let drawn = Selection::polygon(&self.corners, self.overlaps);
        let (x, y) = self.pressed;
        document.selection = if !drawn.is_empty() {
            self.base.combine(&drawn, self.mode)
        } else if self.mode == SelectionMode::Replace || !self.base.contains(x, y) {
            Selection::default()
        } else {
            self.base.clone()
        };
        let polygon = (self.kind == LassoKind::Polygon && !drawn.is_empty()).then(|| PolygonEdit {
            base: self.base,
            mode: self.mode,
            overlaps: self.overlaps,
            versions: Vec::new(),
            drag: None,
        });
        let before = (document.selection != self.before).then_some(self.before);
        Closed { before, polygon, corners: self.corners }
    }

    /// Puts the selection back as it was before the outline began.
    pub fn cancel(self, document: &mut Document) {
        document.selection = self.before;
    }
}

/// Polygon Select's corners once its outline is closed, to go on changing.
pub struct PolygonEdit {
    base: Selection,
    mode: SelectionMode,
    overlaps: Overlaps,
    /// The corners as of each step that left them, with the selection they left.
    versions: Vec<Version>,
    drag: Option<CornerDrag>,
}

struct Version {
    step: usize,
    corners: Vec<(i32, i32)>,
    selection: Selection,
}

/// A corner being moved.
struct CornerDrag {
    corners: Vec<(i32, i32)>,
    index: usize,
    /// The corners and the selection before, which giving up puts back.
    before: Vec<(i32, i32)>,
    selection: Selection,
}

impl PolygonEdit {
    /// The corners are as the document's current step left them.
    pub fn record(&mut self, document: &Document, corners: Vec<(i32, i32)>) {
        let step = document.current_step();
        self.versions.push(Version { step, corners, selection: document.selection.clone() });
    }

    /// The corners as the document now is, if it is as they left it, or as they are
    /// being moved.
    pub fn corners(&self, document: &Document) -> Option<&[(i32, i32)]> {
        if let Some(drag) = &self.drag {
            return Some(&drag.corners);
        }
        let step = document.current_step();
        let version = self.versions.iter().rev().find(|version| version.step == step);
        version.filter(|version| version.selection == document.selection).map(|version| &version.corners[..])
    }

    fn selection(&self, corners: &[(i32, i32)]) -> Selection {
        self.base.combine(&Selection::polygon(corners, self.overlaps), self.mode)
    }

    /// Begins moving the corner at `index`.
    pub fn press_corner(&mut self, document: &Document, index: usize) {
        let Some(corners) = self.corners(document).map(<[_]>::to_vec) else {
            return;
        };
        let selection = document.selection.clone();
        self.drag = Some(CornerDrag { before: corners.clone(), corners, index, selection });
    }

    /// Adds a corner at `corner` on the edge that goes from the corner at `index` to the
    /// next one, and begins moving it.
    pub fn press_edge(&mut self, document: &mut Document, index: usize, corner: (i32, i32)) {
        let Some(mut corners) = self.corners(document).map(<[_]>::to_vec) else {
            return;
        };
        let before = corners.clone();
        corners.insert(index + 1, corner);
        let selection = std::mem::replace(&mut document.selection, self.selection(&corners));
        self.drag = Some(CornerDrag { corners, index: index + 1, before, selection });
    }

    /// The corner being moved reaches `corner`.
    pub fn drag_to(&mut self, document: &mut Document, corner: (i32, i32)) {
        let Some(drag) = &mut self.drag else {
            return;
        };
        if drag.corners[drag.index] == corner {
            return;
        }
        drag.corners[drag.index] = corner;
        let corners = drag.corners.clone();
        document.selection = self.selection(&corners);
    }

    /// The corner is let go. Returns the selection before and the corners now, if the
    /// corners changed, to record as a step.
    pub fn release(&mut self) -> Option<(Selection, Vec<(i32, i32)>)> {
        let drag = self.drag.take()?;
        (drag.corners != drag.before).then_some((drag.selection, drag.corners))
    }

    /// Puts the corners and the selection back as they were before the corner moved.
    pub fn cancel(&mut self, document: &mut Document) {
        if let Some(drag) = self.drag.take() {
            document.selection = drag.selection;
        }
    }

    /// Removes the corner at `index`, if three are left without it. Returns the corners
    /// now, if it did, to record as a step.
    pub fn remove(&mut self, document: &mut Document, index: usize) -> Option<Vec<(i32, i32)>> {
        let mut corners = self.corners(document)?.to_vec();
        if corners.len() <= 3 {
            return None;
        }
        corners.remove(index);
        document.selection = self.selection(&corners);
        Some(corners)
    }

    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }
}

#[cfg(test)]
mod tests {
    use power_paint_core::{PixelRect, RgbaImage};

    use super::*;

    fn document() -> Document {
        Document::pasted(RgbaImage::new(10, 10))
    }

    fn square(left: i32, top: i32, right: i32, bottom: i32) -> Selection {
        Selection::rect(PixelRect { left, top, right, bottom })
    }

    fn begin(document: &mut Document, kind: LassoKind, corner: (i32, i32), mode: SelectionMode) -> Lasso {
        let pressed = (corner.0 as i64, corner.1 as i64);
        Lasso::begin(document, kind, corner, pressed, mode, Overlaps::Add)
    }

    /// The pixels a selection holds, to compare whatever corner its outlines start on.
    fn pixels(selection: &Selection) -> Vec<PixelRect> {
        selection.spans(PixelRect { left: -100, top: -100, right: 100, bottom: 100 })
    }

    #[test]
    fn the_lasso_selects_what_it_goes_around() {
        let mut document = document();
        let mut lasso = begin(&mut document, LassoKind::Freehand, (1, 1), SelectionMode::Replace);
        for corner in [(1, 1), (5, 1), (5, 5), (1, 5)] {
            lasso.add(&mut document, corner);
        }
        assert_eq!(lasso.corners().len(), 4, "the same corner again is left out");
        assert!(lasso.is_current(&document));
        let closed = lasso.close(&mut document);
        assert_eq!(pixels(&document.selection), pixels(&square(1, 1, 5, 5)));
        assert!(closed.before.is_some() && closed.polygon.is_none());
    }

    #[test]
    fn a_click_deselects_unless_it_adds_on_the_selection() {
        let mut document = document();
        document.selection = square(0, 0, 4, 4);
        let closed = begin(&mut document, LassoKind::Freehand, (2, 2), SelectionMode::Add).close(&mut document);
        assert_eq!(document.selection, square(0, 0, 4, 4));
        assert!(closed.before.is_none());
        begin(&mut document, LassoKind::Freehand, (8, 8), SelectionMode::Add).close(&mut document);
        assert!(document.selection.is_empty());
    }

    #[test]
    fn polygon_select_shows_the_outline_to_the_pointer_and_can_be_given_up() {
        let mut document = document();
        document.selection = square(6, 6, 8, 8);
        let mut lasso = begin(&mut document, LassoKind::Polygon, (0, 0), SelectionMode::Replace);
        assert!(document.selection.is_empty(), "what it replaces is cleared at once");
        lasso.add(&mut document, (4, 0));
        lasso.reach(&mut document, Some((4, 4)));
        assert_eq!(lasso.outline(), vec![(0, 0), (4, 0), (4, 4)]);
        assert!(document.selection.contains(3, 1) && !document.selection.contains(1, 3));
        assert!(lasso.remove_last(&mut document));
        lasso.cancel(&mut document);
        assert_eq!(document.selection, square(6, 6, 8, 8));
    }

    #[test]
    fn an_outline_goes_on_only_while_nothing_else_changes_the_selection() {
        let mut document = document();
        let lasso = begin(&mut document, LassoKind::Polygon, (0, 0), SelectionMode::Replace);
        document.selection = square(0, 0, 2, 2);
        assert!(!lasso.is_current(&document));
    }

    /// A square drawn with Polygon Select, as a step, and its corners.
    fn polygon(document: &mut Document) -> PolygonEdit {
        let mut lasso = begin(document, LassoKind::Polygon, (0, 0), SelectionMode::Replace);
        for corner in [(4, 0), (4, 4), (0, 4)] {
            lasso.add(document, corner);
        }
        let closed = lasso.close(document);
        document.commit("Polygon Select", None);
        let mut polygon = closed.polygon.expect("it goes around pixels");
        polygon.record(document, closed.corners);
        polygon
    }

    #[test]
    fn corners_move_and_come_back_with_undo() {
        let mut document = document();
        let mut polygon = polygon(&mut document);
        assert_eq!(polygon.corners(&document).unwrap().len(), 4);
        polygon.press_corner(&document, 2);
        polygon.drag_to(&mut document, (8, 8));
        assert!(document.selection.contains(6, 6));
        let (before, corners) = polygon.release().expect("it moved");
        assert_eq!(pixels(&before), pixels(&square(0, 0, 4, 4)));
        document.commit("Polygon Select", None);
        polygon.record(&document, corners);
        assert_eq!(polygon.corners(&document).unwrap()[2], (8, 8));
        document.undo();
        assert_eq!(polygon.corners(&document).unwrap()[2], (4, 4));
        document.redo();
        assert_eq!(polygon.corners(&document).unwrap()[2], (8, 8));
        // Another selection leaves the corners behind.
        document.selection = square(0, 0, 1, 1);
        assert!(polygon.corners(&document).is_none());
    }

    #[test]
    fn a_press_on_an_edge_adds_a_corner_and_corners_can_go() {
        let mut document = document();
        let mut polygon = polygon(&mut document);
        polygon.press_edge(&mut document, 0, (2, 0));
        polygon.drag_to(&mut document, (2, -2));
        let (_, corners) = polygon.release().unwrap();
        assert_eq!(corners, vec![(0, 0), (2, -2), (4, 0), (4, 4), (0, 4)]);
        document.commit("Polygon Select", None);
        polygon.record(&document, corners);
        let corners = polygon.remove(&mut document, 1).unwrap();
        assert_eq!(corners, vec![(0, 0), (4, 0), (4, 4), (0, 4)]);
        document.commit("Polygon Select", None);
        polygon.record(&document, corners);
        assert!(polygon.remove(&mut document, 0).is_some(), "four corners can lose one");
        // A press that does not move anything changes nothing.
        let mut polygon = self::polygon(&mut document);
        polygon.press_corner(&document, 0);
        assert!(polygon.release().is_none());
    }

    #[test]
    fn giving_up_a_corner_drag_puts_it_back() {
        let mut document = document();
        let mut polygon = polygon(&mut document);
        let before = document.selection.clone();
        polygon.press_corner(&document, 1);
        polygon.drag_to(&mut document, (9, 0));
        polygon.cancel(&mut document);
        assert_eq!(document.selection, before);
        assert_eq!(polygon.corners(&document).unwrap()[1], (4, 0));
    }
}

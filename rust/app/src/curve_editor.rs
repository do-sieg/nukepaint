//! Shows curves that map each level of a channel, across, to the level it becomes, up,
//! and lets the points one of them goes through be moved: dragging a point moves it,
//! clicking elsewhere adds one there and right-clicking one takes it away. A curve
//! always keeps its first and last points.

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Shape, Stroke, Vec2};
use power_paint_core::sample_curve;

/// How far from a point, in points, a click still takes hold of it.
const GRAB_REACH: f32 = 8.0;
const POINT_RADIUS: f32 = 4.0;
/// Closest two points of a curve can be across, so each level maps to one alone.
const MIN_GAP: f32 = 1.0 / 255.0;
pub const SIZE: f32 = 320.0;
const BACKGROUND: Color32 = Color32::from_black_alpha(90);
const GRID_COLOR: Color32 = Color32::from_rgba_premultiplied(38, 38, 38, 38);
/// How much of their colour the curves that cannot be changed keep.
const FADED_OPACITY: f32 = 0.4;

#[derive(Default)]
pub struct CurveEditor {
    /// Which point of the active curve is being dragged.
    dragged: Option<usize>,
}

impl CurveEditor {
    /// Shows the curves, each in its colour, the active one over the others with its
    /// points. Returns whether a point of it was moved, added or taken away.
    pub fn show(&mut self, ui: &mut egui::Ui, curves: &mut [Vec<[f32; 2]>], colors: &[Color32], active: usize) -> bool {
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(SIZE), Sense::click_and_drag());
        let response = response.on_hover_text("Drag a point to move it, click elsewhere to add one and right-click one to take it away.");
        let mut changed = false;
        let pointer = ui.input(|input| input.pointer.interact_pos());
        let (pressed, released) = ui.input(|input| (input.pointer.primary_pressed(), input.pointer.primary_released()));
        if pressed
            && response.hovered()
            && let Some(at) = pointer
        {
            let points = &mut curves[active];
            self.dragged = Some(find_point(rect, points, at).unwrap_or_else(|| {
                changed = true;
                add_point(points, to_curve(rect, at))
            }));
        }
        // The point may be gone, with the curve shown changing under it.
        if let (Some(index), Some(at)) = (self.dragged.filter(|&index| index < curves[active].len()), pointer) {
            changed |= move_point(&mut curves[active], index, to_curve(rect, at));
        }
        if released {
            self.dragged = None;
        }
        if response.secondary_clicked()
            && let Some(at) = pointer
            && let Some(index) = find_point(rect, &curves[active], at)
            && index > 0
            && index < curves[active].len() - 1
        {
            curves[active].remove(index);
            changed = true;
        }

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, BACKGROUND);
        for quarter in 1..4 {
            let along = quarter as f32 / 4.0;
            let x = rect.left() + along * rect.width();
            let y = rect.top() + along * rect.height();
            painter.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], Stroke::new(1.0, GRID_COLOR));
            painter.line_segment([Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)], Stroke::new(1.0, GRID_COLOR));
        }
        painter.line_segment([rect.left_bottom(), rect.right_top()], Stroke::new(1.0, GRID_COLOR));
        for (index, points) in curves.iter().enumerate().filter(|&(index, _)| index != active) {
            draw_curve(&painter, rect, points, colors[index].gamma_multiply(FADED_OPACITY));
        }
        let color = colors[active];
        draw_curve(&painter, rect, &curves[active], color);
        for &point in &curves[active] {
            let at = to_view(rect, point);
            painter.circle(at, POINT_RADIUS, color, Stroke::new(1.0, Color32::BLACK));
        }
        changed
    }
}

fn draw_curve(painter: &egui::Painter, rect: Rect, points: &[[f32; 2]], color: Color32) {
    let levels = sample_curve(points);
    let line = levels.iter().enumerate().map(|(level, &y)| to_view(rect, [level as f32 / 255.0, y])).collect();
    painter.add(Shape::line(line, Stroke::new(2.0, color)));
}

/// Which point is under this place of the view, if any.
fn find_point(rect: Rect, points: &[[f32; 2]], at: Pos2) -> Option<usize> {
    let distances = points.iter().map(|&point| to_view(rect, point).distance(at)).enumerate();
    distances.filter(|&(_, distance)| distance <= GRAB_REACH).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(index, _)| index)
}

/// Adds a point, unless it would be too close across to one there already is, and
/// returns which it is, or which that other one is.
fn add_point(points: &mut Vec<[f32; 2]>, point: [f32; 2]) -> usize {
    let index = points.iter().position(|other| other[0] >= point[0]).unwrap_or(points.len());
    for neighbour in [index.checked_sub(1), Some(index)].into_iter().flatten() {
        if points.get(neighbour).is_some_and(|other| (other[0] - point[0]).abs() < MIN_GAP) {
            return neighbour;
        }
    }
    points.insert(index, point);
    index
}

/// Moves a point as close to `point` as it can go without passing its neighbours.
/// Returns whether it moved.
fn move_point(points: &mut [[f32; 2]], index: usize, [x, y]: [f32; 2]) -> bool {
    let low = if index == 0 { 0.0 } else { points[index - 1][0] + MIN_GAP };
    let high = if index == points.len() - 1 { 1.0 } else { points[index + 1][0] - MIN_GAP };
    let moved = [x.clamp(low, high.max(low)), y];
    if moved == points[index] {
        return false;
    }
    points[index] = moved;
    true
}

fn to_view(rect: Rect, [x, y]: [f32; 2]) -> Pos2 {
    Pos2::new(rect.left() + x * rect.width(), rect.top() + (1.0 - y) * rect.height())
}

fn to_curve(rect: Rect, at: Pos2) -> [f32; 2] {
    let x = (at.x - rect.left()) / rect.width();
    let y = 1.0 - (at.y - rect.top()) / rect.height();
    [x.clamp(0.0, 1.0), y.clamp(0.0, 1.0)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_keep_their_order_and_gaps() {
        let mut points = vec![[0.0, 0.0], [1.0, 1.0]];
        assert_eq!(add_point(&mut points, [0.5, 0.8]), 1);
        assert_eq!(points.len(), 3);
        // Too close to the one there is: that one is taken hold of instead.
        assert_eq!(add_point(&mut points, [0.501, 0.2]), 1);
        assert_eq!(points.len(), 3);
        assert!(move_point(&mut points, 1, [2.0, 0.3]));
        assert_eq!(points[1], [1.0 - MIN_GAP, 0.3], "stops short of the last point");
        assert!(!move_point(&mut points, 1, [2.0, 0.3]));
    }
}

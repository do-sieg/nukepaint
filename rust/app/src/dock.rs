//! The dock: a panel along one edge of the window that holds sections, each under a
//! header that folds it away. Down the left or right side they go one above the
//! other; along the top or bottom, one beside the other, each with its own scroll.
//! The grip at its start drags it to another edge: it goes to whichever edge of the
//! window it is let go nearest to. It can be made wider or narrower from its inner edge.

use eframe::egui::{
    self, Color32, CursorIcon, Id, LayerId, Order, PanelState, Rect, Sense, Stroke, StrokeKind, Vec2,
};
use serde::{Deserialize, Serialize};

/// The same for both sides, and both ends, so the dock keeps its size when it moves
/// to the opposite edge.
const SIDE_ID: &str = "dock";
const END_ID: &str = "dock_end";
const DEFAULT_WIDTH: f32 = 240.0;
const MIN_WIDTH: f32 = 180.0;
const DEFAULT_HEIGHT: f32 = 240.0;
const MIN_HEIGHT: f32 = 120.0;
/// How wide each section is when they go one beside the other.
const SECTION_WIDTH: f32 = 240.0;
const GRIP_THICKNESS: f32 = 12.0;
/// The dots of the grip, along and across it, and their room.
const GRIP_DOTS: (usize, usize) = (8, 2);
const GRIP_DOT_SPACING: f32 = 4.0;
const GRIP_DOT_RADIUS: f32 = 1.0;

/// Which edge of the window the dock is on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DockSide {
    #[default]
    Left,
    Right,
    Top,
    Bottom,
}

impl DockSide {
    pub const ALL: [Self; 4] = [Self::Left, Self::Right, Self::Top, Self::Bottom];

    pub fn name(self) -> &'static str {
        match self {
            Self::Left => "Left",
            Self::Right => "Right",
            Self::Top => "Top",
            Self::Bottom => "Bottom",
        }
    }

    /// Whether the dock goes down a side, rather than along an end.
    fn is_side(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }

    fn panel_id(self) -> &'static str {
        if self.is_side() { SIDE_ID } else { END_ID }
    }

    /// How deep the dock is from its edge: as it was last there, or as it starts out.
    fn depth(self, ctx: &egui::Context) -> f32 {
        let state = PanelState::load(ctx, Id::new(self.panel_id()));
        match (self.is_side(), state) {
            (true, Some(state)) => state.size().x,
            (false, Some(state)) => state.size().y,
            (true, None) => DEFAULT_WIDTH,
            (false, None) => DEFAULT_HEIGHT,
        }
    }
}

/// Shows the dock on its edge, with what `add_contents` puts in it. Returns the edge
/// its grip was dragged to and let go on, if it is another one.
pub fn show(ui: &mut egui::Ui, side: DockSide, add_contents: impl FnOnce(&mut egui::Ui)) -> Option<DockSide> {
    // Where the dock can go: the room the panels before it have left.
    let room = ui.available_rect_before_wrap();
    let id = side.panel_id();
    let panel = match side {
        DockSide::Left => egui::Panel::left(id),
        DockSide::Right => egui::Panel::right(id),
        DockSide::Top => egui::Panel::top(id),
        DockSide::Bottom => egui::Panel::bottom(id),
    };
    let (default_size, min_size) =
        if side.is_side() { (DEFAULT_WIDTH, MIN_WIDTH) } else { (DEFAULT_HEIGHT, MIN_HEIGHT) };
    let output = panel.resizable(true).default_size(default_size).min_size(min_size).show(ui, |ui| {
        if side.is_side() {
            let dropped = grip(ui, room, side);
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, add_contents);
            dropped
        } else {
            ui.horizontal_top(|ui| {
                let dropped = grip(ui, room, side);
                egui::ScrollArea::horizontal().auto_shrink(false).show(ui, |ui| {
                    ui.horizontal_top(add_contents);
                });
                dropped
            })
            .inner
        }
    });
    output.inner.filter(|&dropped| dropped != side)
}

/// A section of the dock, open until its header is clicked.
pub fn section(ui: &mut egui::Ui, title: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    let header = |ui: &mut egui::Ui| {
        egui::CollapsingHeader::new(title).default_open(true).show(ui, add_contents);
    };
    if ui.layout().is_vertical() {
        header(ui);
        return;
    }
    // Beside the others: a column of its own, which scrolls on its own.
    ui.vertical(|ui| {
        ui.set_width(SECTION_WIDTH);
        egui::ScrollArea::vertical().id_salt(title).auto_shrink([false, false]).show(ui, header);
    });
}

/// The grip that moves the dock, across its start. While it is dragged, the edge the
/// dock would go to is marked. Returns that edge once it is let go.
fn grip(ui: &mut egui::Ui, room: Rect, side: DockSide) -> Option<DockSide> {
    let size = if side.is_side() {
        Vec2::new(ui.available_width(), GRIP_THICKNESS)
    } else {
        Vec2::new(GRIP_THICKNESS, ui.available_height())
    };
    let (rect, response) = ui.allocate_exact_size(size, Sense::drag());
    let response = response.on_hover_text("Drag to another edge of the window to move the dock");
    let visuals = ui.style().interact(&response);
    if response.hovered() || response.dragged() {
        ui.painter().rect_filled(rect, 2.0, visuals.weak_bg_fill);
    }
    let (along, across) = GRIP_DOTS;
    let (columns, rows) = if side.is_side() { (along, across) } else { (across, along) };
    let span = Vec2::new((columns - 1) as f32, (rows - 1) as f32) * GRIP_DOT_SPACING;
    let first = rect.center() - span / 2.0;
    for x in 0..columns {
        for y in 0..rows {
            let center = first + Vec2::new(x as f32, y as f32) * GRIP_DOT_SPACING;
            ui.painter().circle_filled(center, GRIP_DOT_RADIUS, visuals.fg_stroke.color);
        }
    }

    if response.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    } else if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::Grab);
    }
    let pointer = ui.ctx().pointer_latest_pos()?;
    let target = nearest_edge(room, pointer);
    if response.dragged() {
        // As deep as the dock would be there, along the edge it would go to.
        let depth = target.depth(ui.ctx());
        let mark = match target {
            DockSide::Left => Rect::from_min_size(room.min, Vec2::new(depth, room.height())),
            DockSide::Right => Rect::from_min_max(room.right_top() - Vec2::new(depth, 0.0), room.max),
            DockSide::Top => Rect::from_min_size(room.min, Vec2::new(room.width(), depth)),
            DockSide::Bottom => Rect::from_min_max(room.left_bottom() - Vec2::new(0.0, depth), room.max),
        };
        let painter = ui.ctx().layer_painter(LayerId::new(Order::Foreground, Id::new("dock_drop_mark")));
        let selection = ui.visuals().selection;
        let fill = Color32::from_rgba_unmultiplied(selection.bg_fill.r(), selection.bg_fill.g(), selection.bg_fill.b(), 64);
        painter.rect(mark, 4.0, fill, Stroke::new(2.0, selection.bg_fill), StrokeKind::Inside);
    }
    response.drag_stopped().then_some(target)
}

/// The edge of `room` nearest to `pointer`, as a share of the room's size, so the
/// room's diagonals divide it between them.
fn nearest_edge(room: Rect, pointer: egui::Pos2) -> DockSide {
    let x = (pointer.x - room.left()) / room.width().max(1.0);
    let y = (pointer.y - room.top()) / room.height().max(1.0);
    [(DockSide::Left, x), (DockSide::Right, 1.0 - x), (DockSide::Top, y), (DockSide::Bottom, 1.0 - y)]
        .into_iter()
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(DockSide::Left, |(side, _)| side)
}

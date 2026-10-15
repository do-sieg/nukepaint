//! The dock: a panel down one side of the window, left or right, that holds sections
//! one above the other, each under a header that folds it away. The grip at its top
//! drags it to the other side: it goes to whichever half of the window it is let go
//! over. It can be made wider or narrower from its inner edge.

use eframe::egui::{self, Color32, CursorIcon, Id, LayerId, Order, Rect, Sense, Stroke, StrokeKind, Vec2};
use serde::{Deserialize, Serialize};

/// The same for both sides, so the dock keeps its width when it moves.
const ID: &str = "dock";
const DEFAULT_WIDTH: f32 = 240.0;
const MIN_WIDTH: f32 = 180.0;
const GRIP_HEIGHT: f32 = 12.0;
/// The dots of the grip, along and across it, and their room.
const GRIP_DOTS: (usize, usize) = (8, 2);
const GRIP_DOT_SPACING: f32 = 4.0;
const GRIP_DOT_RADIUS: f32 = 1.0;

/// Which side of the window the dock is on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DockSide {
    #[default]
    Left,
    Right,
}

impl DockSide {
    pub const ALL: [Self; 2] = [Self::Left, Self::Right];

    pub fn name(self) -> &'static str {
        match self {
            Self::Left => "Left",
            Self::Right => "Right",
        }
    }
}

/// Shows the dock on its side, with what `add_contents` puts in it. Returns the side
/// its grip was dragged to and let go on, if it is the other one.
pub fn show(ui: &mut egui::Ui, side: DockSide, add_contents: impl FnOnce(&mut egui::Ui)) -> Option<DockSide> {
    // Where the dock can go: the room the panels before it have left.
    let room = ui.available_rect_before_wrap();
    let panel = match side {
        DockSide::Left => egui::Panel::left(ID),
        DockSide::Right => egui::Panel::right(ID),
    };
    let output = panel.resizable(true).default_size(DEFAULT_WIDTH).min_size(MIN_WIDTH).show(ui, |ui| {
        let dropped = grip(ui, room);
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, add_contents);
        dropped
    });
    output.inner.filter(|&dropped| dropped != side)
}

/// A section of the dock, open until its header is clicked.
pub fn section(ui: &mut egui::Ui, title: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::CollapsingHeader::new(title).default_open(true).show(ui, add_contents);
}

/// The grip that moves the dock. While it is dragged, the side the dock would go to
/// is marked. Returns that side once it is let go.
fn grip(ui: &mut egui::Ui, room: Rect) -> Option<DockSide> {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, GRIP_HEIGHT), Sense::drag());
    let response = response.on_hover_text("Drag to the other side of the window to move the dock");
    let visuals = ui.style().interact(&response);
    if response.hovered() || response.dragged() {
        ui.painter().rect_filled(rect, 2.0, visuals.weak_bg_fill);
    }
    let (along, across) = GRIP_DOTS;
    let span = Vec2::new((along - 1) as f32, (across - 1) as f32) * GRIP_DOT_SPACING;
    let first = rect.center() - span / 2.0;
    for x in 0..along {
        for y in 0..across {
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
    let target = if pointer.x < room.center().x { DockSide::Left } else { DockSide::Right };
    if response.dragged() {
        // As wide as the dock is, along the side it would go to.
        let dock_width = ui.max_rect().width();
        let mark = match target {
            DockSide::Left => Rect::from_min_size(room.min, Vec2::new(dock_width, room.height())),
            DockSide::Right => Rect::from_min_max(room.right_top() - Vec2::new(dock_width, 0.0), room.max),
        };
        let painter = ui.ctx().layer_painter(LayerId::new(Order::Foreground, Id::new("dock_drop_mark")));
        let selection = ui.visuals().selection;
        let fill = Color32::from_rgba_unmultiplied(selection.bg_fill.r(), selection.bg_fill.g(), selection.bg_fill.b(), 64);
        painter.rect(mark, 4.0, fill, Stroke::new(2.0, selection.bg_fill), StrokeKind::Inside);
    }
    response.drag_stopped().then_some(target)
}

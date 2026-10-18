//! The Palette section of the dock: the current image's palette, its entries in
//! order, as square cells that wrap to the section's width. A click on one sets the
//! primary colour, a right click the secondary one, and corner marks show the entries
//! that are either. The transparent entry shows as checkers. Hovering one says its
//! index and code. Edit opens the palette editor.

use eframe::egui::{self, Color32, Pos2, Rect, Response, Sense, Shape, Stroke, Vec2};
use nukepaint_core::{Palette, Rgba};

use crate::colors_panel::{Colors, hex_code, paint_swatch};

/// Smallest a cell gets before fewer go on a row, in points.
const MIN_CELL_SIZE: f32 = 18.0;
const GAP: f32 = 2.0;
/// Tallest the cells get before they scroll, in points.
const MAX_HEIGHT: f32 = 200.0;
/// How much of a cell's side the marks of the primary and secondary colours take.
const MARK_RATIO: f32 = 0.4;

/// Where the cells of a palette's entries are, a row after another.
pub struct Cells {
    rect: Rect,
    columns: usize,
    rows: usize,
    size: f32,
    len: usize,
}

impl Cells {
    /// Takes the width there is for the entries of `palette`, as cells no smaller than
    /// `min_size`, and paints them. The palette must not be empty.
    pub fn show(ui: &mut egui::Ui, palette: &Palette, min_size: f32, sense: Sense) -> (Self, Response) {
        let width = ui.available_width();
        let len = palette.len();
        let columns = (((width + GAP) / (min_size + GAP)).floor() as usize).clamp(1, len);
        let size = (width + GAP) / columns as f32 - GAP;
        let rows = len.div_ceil(columns);
        let height = rows as f32 * (size + GAP) - GAP;
        let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), sense);
        let cells = Self { rect, columns, rows, size, len };
        for index in 0..len {
            paint_swatch(ui.painter(), cells.rect(index), palette.color(index));
        }
        (cells, response)
    }

    pub fn size(&self) -> f32 {
        self.size
    }

    pub fn rect(&self, index: usize) -> Rect {
        let (column, row) = (index % self.columns, index / self.columns);
        let at = Vec2::new(column as f32, row as f32) * (self.size + GAP);
        Rect::from_min_size(self.rect.min + at, Vec2::splat(self.size))
    }

    /// The entry at `at`, if it is on one.
    pub fn at(&self, at: Pos2) -> Option<usize> {
        (0..self.len).find(|&index| self.rect(index).contains(at))
    }

    /// The entry at `at`, or the nearest one, between cells or past them.
    pub fn nearest(&self, at: Pos2) -> usize {
        let offset = (at - self.rect.min) / (self.size + GAP);
        let column = (offset.x.max(0.0) as usize).min(self.columns - 1);
        let row = (offset.y.max(0.0) as usize).min(self.rows - 1);
        (row * self.columns + column).min(self.len - 1)
    }
}

/// What hovering an entry says: its index and code, and whether it is the transparent one.
pub fn entry_tip(palette: &Palette, index: usize) -> String {
    let transparent = if palette.transparent == Some(index) { " (transparent)" } else { "" };
    format!("{index}: #{}{transparent}", hex_code(palette.colors[index]))
}

/// Shows the entries of `palette`, no taller than `tallest`, setting `colors` from the
/// one clicked, and the button to edit them, if `enabled`. Returns whether it was clicked.
pub fn show(ui: &mut egui::Ui, tallest: f32, palette: &Palette, colors: &mut Colors, enabled: bool) -> bool {
    if palette.is_empty() {
        ui.weak("The palette is empty");
    } else {
        let button = ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
        egui::ScrollArea::vertical()
            .id_salt("palette")
            .max_height(MAX_HEIGHT.min(tallest - button).max(MIN_CELL_SIZE))
            .auto_shrink([false, true])
            .show(ui, |ui| cells(ui, palette, colors));
    }
    let edit = ui.add_enabled(enabled, egui::Button::new("Edit…"));
    edit.on_hover_text("Edit the palette's colors, or load another").clicked()
}

/// The entries as cells, setting `colors` from the one clicked.
fn cells(ui: &mut egui::Ui, palette: &Palette, colors: &mut Colors) {
    let (cells, response) = Cells::show(ui, palette, MIN_CELL_SIZE, Sense::click());
    // The primary colour in the top left corner and the secondary one in the bottom right, as on the Colors section's swatches.
    let entry_of = |color: Rgba<u8>| (0..palette.len()).find(|&index| palette.color(index) == color);
    let mark = cells.size() * MARK_RATIO;
    if let Some(index) = entry_of(colors.primary) {
        let corner = cells.rect(index).left_top();
        paint_mark(ui.painter(), [corner, corner + Vec2::X * mark, corner + Vec2::Y * mark]);
    }
    if let Some(index) = entry_of(colors.secondary) {
        let corner = cells.rect(index).right_bottom();
        paint_mark(ui.painter(), [corner, corner - Vec2::X * mark, corner - Vec2::Y * mark]);
    }

    let clicked = (response.clicked(), response.secondary_clicked());
    if (clicked.0 || clicked.1)
        && let Some(index) = response.interact_pointer_pos().and_then(|at| cells.at(at))
    {
        if clicked.1 {
            colors.secondary = palette.color(index);
        } else {
            colors.primary = palette.color(index);
        }
    }
    if let Some(index) = response.hover_pos().and_then(|at| cells.at(at)) {
        let tip = format!("{}\nClick to set the primary color, right click the secondary", entry_tip(palette, index));
        response.on_hover_text_at_pointer(tip);
    }
}

/// A corner mark of a cell, black edged with white to show on any colour.
fn paint_mark(painter: &egui::Painter, corners: [Pos2; 3]) {
    painter.add(Shape::convex_polygon(corners.to_vec(), Color32::BLACK, Stroke::new(1.0, Color32::WHITE)));
}

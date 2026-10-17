//! The Palette section of the dock: the current image's palette, its entries in
//! order, as square cells that wrap to the section's width. A click on one sets the
//! primary colour, a right click the secondary one, and the entries that are either
//! are outlined. Hovering one says its index and code.

use eframe::egui::{self, Rect, Sense, Stroke, StrokeKind, Vec2};
use power_paint_core::Rgba;

use crate::colors_panel::{Colors, hex_code, paint_swatch};

/// Smallest a cell gets before fewer go on a row, in points.
const MIN_CELL_SIZE: f32 = 18.0;
const GAP: f32 = 2.0;
/// Tallest the cells get before they scroll, in points.
const MAX_HEIGHT: f32 = 200.0;
/// How far the outlines of the primary and secondary colours' entries go past them.
const OUTLINE_ROOM: f32 = 3.0;

/// Shows the entries of `palette`, setting `colors` from the one clicked.
pub fn show(ui: &mut egui::Ui, palette: &[Rgba<u8>], colors: &mut Colors) {
    if palette.is_empty() {
        ui.weak("The palette is empty");
        return;
    }
    egui::ScrollArea::vertical()
        .id_salt("palette")
        .max_height(MAX_HEIGHT)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            // Room for the outlines of the entries along the edges, which the scrolling would cut off.
            egui::Frame::NONE.inner_margin(OUTLINE_ROOM).show(ui, |ui| cells(ui, palette, colors));
        });
}

/// The entries as cells, setting `colors` from the one clicked.
fn cells(ui: &mut egui::Ui, palette: &[Rgba<u8>], colors: &mut Colors) {
    let width = ui.available_width();
    let columns = (((width + GAP) / (MIN_CELL_SIZE + GAP)).floor() as usize).clamp(1, palette.len());
    let size = (width + GAP) / columns as f32 - GAP;
    let rows = palette.len().div_ceil(columns);
    let height = rows as f32 * (size + GAP) - GAP;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    let cell_rect = |index: usize| {
        let (column, row) = (index % columns, index / columns);
        let at = Vec2::new(column as f32, row as f32) * (size + GAP);
        Rect::from_min_size(rect.min + at, Vec2::splat(size))
    };
    let at_pointer = |at| (0..palette.len()).find(|&index| cell_rect(index).contains(at));

    let painter = ui.painter();
    for (index, &color) in palette.iter().enumerate() {
        paint_swatch(painter, cell_rect(index), color);
    }
    // The secondary colour's outline first, so the primary one's shows over it when both are the same entry.
    let selection = ui.visuals().selection.stroke.color;
    let text = ui.visuals().text_color();
    for (color, stroke) in [(colors.secondary, Stroke::new(1.0, text)), (colors.primary, Stroke::new(2.0, selection))] {
        if let Some(index) = palette.iter().position(|&entry| entry == color) {
            painter.rect_stroke(cell_rect(index).expand(1.0), 0.0, stroke, StrokeKind::Outside);
        }
    }

    let clicked = (response.clicked(), response.secondary_clicked());
    if (clicked.0 || clicked.1)
        && let Some(index) = response.interact_pointer_pos().and_then(at_pointer)
    {
        if clicked.1 {
            colors.secondary = palette[index];
        } else {
            colors.primary = palette[index];
        }
    }
    let hovered = response.hover_pos().and_then(at_pointer);
    if let Some(index) = hovered {
        let tip = format!("{index}: #{}\nClick to set the primary color, right click the secondary", hex_code(palette[index]));
        response.on_hover_text_at_pointer(tip);
    }
}

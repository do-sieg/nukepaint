//! Asks where the grid starts, how large its cells are and what colour its lines are.
//! The image shows the grid as it is tried out; Cancel, or Escape, leaves it as it was.

use eframe::egui::{self, Color32, Key};

use crate::grid::Grid;

/// Largest a cell can be, and furthest from the image's corner one can start, in pixels.
const MAX_SIDE: i32 = 32768;

pub enum Outcome {
    Accepted,
    Canceled,
}

pub struct Dialog {
    /// The grid as it is being tried out, which shows while it is.
    pub grid: Grid,
    /// Set until the width field has been focused.
    focus_width: bool,
}

impl Dialog {
    pub fn new(grid: Grid) -> Self {
        Self { grid: Grid { visible: true, ..grid }, focus_width: true }
    }

    /// Shows the dialog. Returns what the user did, if anything.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<Outcome> {
        // The image stays in plain view, to see the grid on it.
        let modal = egui::Modal::new(egui::Id::new("grid_settings")).backdrop_color(Color32::TRANSPARENT);
        let response = modal.show(ctx, |ui| {
            ui.heading("Grid Settings");
            ui.add_space(8.0);
            egui::Grid::new("grid_settings_grid").num_columns(2).spacing([8.0, 8.0]).show(ui, |ui| {
                let offsets = [
                    ("X offset:", "left", 0),
                    ("Y offset:", "top", 1),
                ];
                for (label, edge, axis) in offsets {
                    ui.label(label);
                    let offset = egui::DragValue::new(&mut self.grid.origin[axis]).range(-MAX_SIDE..=MAX_SIDE).suffix(" px");
                    ui.add(offset).on_hover_text(format!(
                        "How far from the image's {edge} edge a cell starts, to line the grid up with a tile sheet's margin."
                    ));
                    ui.end_row();
                }
                for (label, axis) in [("Width:", 0), ("Height:", 1)] {
                    ui.label(label);
                    let side = ui.add(egui::DragValue::new(&mut self.grid.cell[axis]).range(1..=MAX_SIDE).suffix(" px"));
                    if axis == 0 && std::mem::take(&mut self.focus_width) {
                        side.request_focus();
                    }
                    ui.end_row();
                }

                ui.label("Line color:");
                ui.color_edit_button_srgba_unmultiplied(&mut self.grid.color);
                ui.end_row();
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let enter = ui.input(|input| input.key_pressed(Key::Enter));
                if ui.button("OK").clicked() || enter {
                    return Some(Outcome::Accepted);
                }
                ui.button("Cancel").clicked().then_some(Outcome::Canceled)
            })
            .inner
        });
        if response.should_close() {
            return Some(Outcome::Canceled);
        }
        response.inner
    }
}

//! The checkerboard that shows through the image's transparent pixels, in squares of
//! a set size on screen, and the dialog that asks for that size. The image shows the
//! size as it is tried out; Cancel, or Escape, leaves it as it was.

use eframe::egui::{self, Color32};
use serde::{Deserialize, Serialize};

use crate::dialog;

/// Largest a square can be, in screen pixels.
const MAX_SIZE: u32 = 64;

/// How the checkerboard is set, as the settings keep it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Checkerboard {
    /// Side of each square, in screen pixels.
    pub size: u32,
}

impl Default for Checkerboard {
    fn default() -> Self {
        Self { size: 8 }
    }
}

pub enum Outcome {
    Accepted,
    Canceled,
}

pub struct Dialog {
    /// The checkerboard as it is being tried out, which shows while it is.
    pub checkerboard: Checkerboard,
    /// Set until the size field has been focused.
    focus_size: bool,
}

impl Dialog {
    pub fn new(checkerboard: Checkerboard) -> Self {
        Self { checkerboard, focus_size: true }
    }

    /// Shows the dialog. Returns what the user did, if anything.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<Outcome> {
        // The image stays in plain view, to see the checkerboard behind it.
        let response = dialog::show(ctx, "checkerboard", "Checkerboard", Color32::TRANSPARENT, |ui| {
            egui::Grid::new("checkerboard_grid").num_columns(2).spacing([8.0, 8.0]).show(ui, |ui| {
                ui.label("Square size:");
                let size = egui::DragValue::new(&mut self.checkerboard.size).range(1..=MAX_SIZE).suffix(" px");
                let size = ui.add(size).on_hover_text("Side of each square on screen, whatever the zoom.");
                if std::mem::take(&mut self.focus_size) {
                    size.request_focus();
                }
                ui.end_row();
            });
            dialog::ok_cancel(ui).map(|ok| if ok { Outcome::Accepted } else { Outcome::Canceled })
        });
        if response.should_close() {
            return Some(Outcome::Canceled);
        }
        response.inner
    }
}

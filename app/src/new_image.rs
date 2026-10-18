//! Asks for the size of a new image and what it is filled with.

use eframe::egui;
use nukepaint_core::{Rgba, RgbaImage};

use crate::dialog;

/// Widest and tallest a new image can be, in pixels, as for resizing one.
const MAX_SIDE: u32 = 32768;
/// The size a new image starts with when there is nothing to take one from.
pub const DEFAULT_SIZE: [u32; 2] = [800, 600];

/// What a new image's pixels are.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Background {
    #[default]
    White,
    Transparent,
    Primary,
    Secondary,
}

impl Background {
    const ALL: [Background; 4] = [Background::White, Background::Transparent, Background::Primary, Background::Secondary];

    fn name(self) -> &'static str {
        match self {
            Background::White => "White",
            Background::Transparent => "Transparent",
            Background::Primary => "Primary Color",
            Background::Secondary => "Secondary Color",
        }
    }
}

pub enum Outcome {
    Accepted,
    Canceled,
}

pub struct Dialog {
    pub size: [u32; 2],
    pub background: Background,
    /// Set until the width field has been focused.
    focus_width: bool,
}

impl Dialog {
    pub fn new(size: [u32; 2], background: Background) -> Self {
        Self { size, background, focus_width: true }
    }

    /// Shows the dialog. Returns what the user did, if anything.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<Outcome> {
        let response = dialog::show(ctx, "new_image", "New Image", dialog::DIM, |ui| {
            egui::Grid::new("new_image_grid").num_columns(2).spacing([8.0, 8.0]).show(ui, |ui| {
                for (axis, label) in ["Width:", "Height:"].into_iter().enumerate() {
                    ui.label(label);
                    let side = ui.add(egui::DragValue::new(&mut self.size[axis]).range(1..=MAX_SIDE).suffix(" px"));
                    if axis == 0 && std::mem::take(&mut self.focus_width) {
                        side.request_focus();
                    }
                    ui.end_row();
                }
                ui.label("Background:");
                egui::ComboBox::from_id_salt("new_image_background").selected_text(self.background.name()).show_ui(ui, |ui| {
                    for background in Background::ALL {
                        ui.selectable_value(&mut self.background, background, background.name());
                    }
                });
                ui.end_row();
            });
            dialog::ok_cancel(ui).map(|ok| if ok { Outcome::Accepted } else { Outcome::Canceled })
        });
        if response.should_close() {
            return Some(Outcome::Canceled);
        }
        response.inner
    }

    /// The new image's pixels, given the primary and the secondary colours.
    pub fn pixels(&self, [primary, secondary]: [Rgba<u8>; 2]) -> RgbaImage {
        let fill = match self.background {
            Background::White => Rgba([255, 255, 255, 255]),
            Background::Transparent => Rgba([0, 0, 0, 0]),
            Background::Primary => primary,
            Background::Secondary => secondary,
        };
        RgbaImage::from_pixel(self.size[0], self.size[1], fill)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixels_are_the_size_and_background_asked_for() {
        let red = Rgba([255, 0, 0, 255]);
        let blue = Rgba([0, 0, 255, 128]);
        let mut dialog = Dialog::new([3, 2], Background::Secondary);
        let pixels = dialog.pixels([red, blue]);
        assert_eq!(pixels.dimensions(), (3, 2));
        assert!(pixels.pixels().all(|&pixel| pixel == blue));
        dialog.background = Background::Transparent;
        assert!(dialog.pixels([red, blue]).pixels().all(|pixel| pixel.0[3] == 0));
    }
}

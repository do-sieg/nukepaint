//! Asks for a new size of the image: one to stretch it to, with how its pixels are
//! redrawn, or one for the canvas around it, with where the image stays on it.

use eframe::egui;
use power_paint_core::Interpolation;

use crate::dialog;

/// Widest and tallest the image can be made, in pixels.
const MAX_SIDE: u32 = 32768;
const ANCHOR_BUTTON_SIZE: f32 = 24.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The image is stretched to the new size.
    Resize,
    /// The canvas around the image gets the new size.
    CanvasSize,
}

impl Kind {
    pub fn title(self) -> &'static str {
        match self {
            Kind::Resize => "Resize",
            Kind::CanvasSize => "Canvas Size",
        }
    }
}

pub enum Outcome {
    Accepted,
    Canceled,
}

/// One of the fields a side is given in.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Pixels(usize),
    Percent(usize),
}

pub struct Dialog {
    /// The image to resize.
    pub image: usize,
    pub kind: Kind,
    /// The size the image has now, which the percentages and the aspect ratio are of.
    original: [u32; 2],
    /// The new width and height.
    pub size: [u32; 2],
    /// The new width and height, as percentages of what they are now.
    percents: [f64; 2],
    keep_ratio: bool,
    pub interpolation: Interpolation,
    /// Where the image stays on the canvas: 0, 1 or 2 along each axis, for against the
    /// left or top edge, in the middle, or against the right or bottom edge.
    pub anchor: (u8, u8),
    /// Set until the width field has been focused.
    focus_width: bool,
}

impl Dialog {
    pub fn new(image: usize, kind: Kind, (width, height): (u32, u32)) -> Self {
        Self {
            image,
            kind,
            original: [width, height],
            size: [width, height],
            percents: [100.0; 2],
            // Stretching an image usually keeps its proportions; making room around it usually does not.
            keep_ratio: kind == Kind::Resize,
            interpolation: Interpolation::default(),
            anchor: (1, 1),
            focus_width: true,
        }
    }

    /// Shows the dialog. Returns what the user did, if anything.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<Outcome> {
        let response = dialog::show(ctx, "image_size", self.kind.title(), dialog::DIM, |ui| {
            egui::Grid::new("image_size_grid").num_columns(3).spacing([8.0, 8.0]).show(ui, |ui| {
                if self.kind == Kind::Resize {
                    ui.label("Resampling:");
                    egui::ComboBox::from_id_salt("interpolation").selected_text(self.interpolation.name()).show_ui(
                        ui,
                        |ui| {
                            for interpolation in Interpolation::ALL {
                                ui.selectable_value(&mut self.interpolation, interpolation, interpolation.name());
                            }
                        },
                    );
                    ui.end_row();
                }

                for (axis, label) in ["Width:", "Height:"].into_iter().enumerate() {
                    ui.label(label);
                    let pixels = egui::DragValue::new(&mut self.size[axis]).range(1..=MAX_SIDE).suffix(" px");
                    let pixels = ui.add(pixels);
                    if axis == 0 && std::mem::take(&mut self.focus_width) {
                        pixels.request_focus();
                    }
                    if pixels.changed() {
                        let ratio = self.size[axis] as f64 / self.original[axis] as f64;
                        self.set_ratio(axis, ratio, Some(Field::Pixels(axis)));
                    }
                    let percent = egui::DragValue::new(&mut self.percents[axis])
                        .range(0.01..=f64::from(MAX_SIDE) * 100.0)
                        .speed(0.1)
                        .max_decimals(2)
                        .suffix(" %");
                    if ui.add(percent).changed() {
                        self.set_ratio(axis, self.percents[axis] / 100.0, Some(Field::Percent(axis)));
                    }
                    ui.end_row();
                }

                ui.label("");
                if ui.checkbox(&mut self.keep_ratio, "Maintain aspect ratio").changed() && self.keep_ratio {
                    self.set_ratio(0, self.size[0] as f64 / self.original[0] as f64, None);
                }
                ui.end_row();

                if self.kind == Kind::CanvasSize {
                    ui.label("Anchor:");
                    egui::Grid::new("anchor_grid").spacing([2.0, 2.0]).show(ui, |ui| {
                        for y in 0..3 {
                            for x in 0..3 {
                                let selected = self.anchor == (x, y);
                                let button = egui::Button::selectable(selected, if selected { "●" } else { "" });
                                if ui.add_sized([ANCHOR_BUTTON_SIZE; 2], button).clicked() {
                                    self.anchor = (x, y);
                                }
                            }
                            ui.end_row();
                        }
                    })
                    .response
                    .on_hover_text("Where the image stays on the canvas.");
                    ui.end_row();
                }
            });
            dialog::ok_cancel(ui).map(|ok| if ok { Outcome::Accepted } else { Outcome::Canceled })
        });
        if response.should_close() {
            return Some(Outcome::Canceled);
        }
        response.inner
    }

    /// Sets a side to so many times what it is now, and the other side as well while
    /// the aspect ratio is kept. `edited` is the field this comes from, which is left
    /// as it was typed.
    fn set_ratio(&mut self, axis: usize, ratio: f64, edited: Option<Field>) {
        let axes = if self.keep_ratio { 0..2 } else { axis..axis + 1 };
        for changed in axes {
            if edited != Some(Field::Pixels(changed)) {
                let pixels = (self.original[changed] as f64 * ratio).round();
                self.size[changed] = (pixels as u32).clamp(1, MAX_SIDE);
            }
            if edited != Some(Field::Percent(changed)) {
                self.percents[changed] = ratio * 100.0;
            }
        }
    }
}

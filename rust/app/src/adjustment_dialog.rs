//! Asks for the settings of an adjustment: each a number on a slider, as its kind
//! lists them, or the curves of Curves. The image shows each change
//! as it is made; Cancel, or Escape, puts the layer back as it was.

use eframe::egui::{self, Color32, Key};
use power_paint_core::{Curves, RgbaImage, SelectionMask};

use crate::{adjustments::AdjustmentKind, curve_editor::CurveEditor};

/// Width of the sliders, in points.
const SLIDER_WIDTH: f32 = 220.0;
/// Width of the field next to each slider, in points: wide enough for any of the
/// numbers, so the dialog keeps its width as they change.
const VALUE_WIDTH: f32 = 72.0;
const CHANNEL_NAMES: [&str; 3] = ["Red", "Green", "Blue"];
const CHANNEL_COLORS: [Color32; 3] =
    [Color32::from_rgb(255, 77, 77), Color32::from_rgb(77, 255, 77), Color32::from_rgb(102, 128, 255)];

/// A number a dialog asks for on a slider, from `min` to `max`, going up and down by
/// `step`, with `suffix` shown after it.
pub struct Setting {
    pub label: &'static str,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub default: f64,
    pub suffix: &'static str,
}

pub enum Outcome {
    /// The settings were changed, and are being tried out.
    Edited,
    Accepted,
    Canceled,
}

pub struct Dialog {
    /// The image and the layer being adjusted.
    pub image: usize,
    pub layer: usize,
    pub kind: AdjustmentKind,
    /// The settings, in the order the kind lists them.
    pub values: Vec<f64>,
    pub curves: Curves,
    /// Which of red, green and blue the curve being changed is, in RGB mode.
    channel: usize,
    editor: CurveEditor,
    /// The pixels the adjustment may change, if not all of them.
    pub mask: Option<SelectionMask>,
    /// The layer's pixels as the settings adjust them, once worked out.
    pub preview: Option<RgbaImage>,
}

impl Dialog {
    pub fn new(image: usize, layer: usize, kind: AdjustmentKind, values: Vec<f64>, curves: Curves, mask: Option<SelectionMask>) -> Self {
        Self { image, layer, kind, values, curves, channel: 0, editor: CurveEditor::default(), mask, preview: None }
    }

    /// Shows the dialog. Returns what the user did, if anything.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<Outcome> {
        let mut edited = false;
        // The image stays in plain view, to see the changes on it.
        let modal = egui::Modal::new(egui::Id::new("adjustment")).backdrop_color(Color32::TRANSPARENT);
        let response = modal.show(ctx, |ui| {
            ui.heading(self.kind.name());
            ui.add_space(8.0);
            if self.kind == AdjustmentKind::Curves {
                edited |= self.curves_ui(ui);
            } else {
                edited |= self.settings_ui(ui);
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let enter = ui.input(|input| input.key_pressed(Key::Enter));
                if ui.button("OK").clicked() || enter {
                    return Some(Outcome::Accepted);
                }
                if ui.button("Cancel").clicked() {
                    return Some(Outcome::Canceled);
                }
                if ui.button("Reset").clicked() {
                    self.reset();
                    edited = true;
                }
                None
            })
            .inner
        });
        if response.should_close() {
            return Some(Outcome::Canceled);
        }
        response.inner.or_else(|| edited.then_some(Outcome::Edited))
    }

    /// Returns whether a setting changed.
    fn settings_ui(&mut self, ui: &mut egui::Ui) -> bool {
        let mut edited = false;
        egui::Grid::new("adjustment_grid").num_columns(2).spacing([8.0, 8.0]).show(ui, |ui| {
            for (setting, value) in self.kind.settings().iter().zip(&mut self.values) {
                ui.label(setting.label);
                ui.horizontal(|ui| {
                    ui.spacing_mut().slider_width = SLIDER_WIDTH;
                    let range = setting.min..=setting.max;
                    let slider = egui::Slider::new(value, range.clone()).step_by(setting.step).show_value(false);
                    edited |= ui.add(slider).changed();
                    let decimals = (-setting.step.log10()).ceil().max(0.0) as usize;
                    let field = egui::DragValue::new(value)
                        .range(range)
                        .speed(setting.step)
                        .fixed_decimals(decimals)
                        .suffix(setting.suffix);
                    let height = ui.spacing().interact_size.y;
                    edited |= ui.add_sized([VALUE_WIDTH, height], field).changed();
                });
                ui.end_row();
            }
        });
        edited
    }

    /// Returns whether the curves changed.
    fn curves_ui(&mut self, ui: &mut egui::Ui) -> bool {
        let mut edited = false;
        ui.horizontal(|ui| {
            let mode = if self.curves.luminosity { "Luminosity" } else { "RGB" };
            egui::ComboBox::from_id_salt("curves_mode")
                .selected_text(mode)
                .show_ui(ui, |ui| {
                    edited |= ui.selectable_value(&mut self.curves.luminosity, true, "Luminosity").changed();
                    edited |= ui.selectable_value(&mut self.curves.luminosity, false, "RGB").changed();
                })
                .response
                .on_hover_text(
                    "Luminosity changes how light each pixel is and keeps its colour.\n\
                     RGB changes red, green and blue each along a curve of its own.",
                );
            if !self.curves.luminosity {
                egui::ComboBox::from_id_salt("curves_channel")
                    .selected_text(CHANNEL_NAMES[self.channel])
                    .show_ui(ui, |ui| {
                        for (index, name) in CHANNEL_NAMES.into_iter().enumerate() {
                            ui.selectable_value(&mut self.channel, index, name);
                        }
                    })
                    .response
                    .on_hover_text("Which curve is changed. The others are shown faded.");
            }
        });
        if self.curves.luminosity {
            edited |= self.editor.show(ui, std::slice::from_mut(&mut self.curves.light), &[Color32::WHITE], 0);
        } else {
            edited |= self.editor.show(ui, &mut self.curves.channels, &CHANNEL_COLORS, self.channel);
        }
        edited
    }

    /// Sets every setting back to where it starts. The curves are made straight, in
    /// the mode they are in.
    fn reset(&mut self) {
        self.values = self.kind.settings().iter().map(|setting| setting.default).collect();
        self.curves = Curves { luminosity: self.curves.luminosity, ..Curves::default() };
    }
}

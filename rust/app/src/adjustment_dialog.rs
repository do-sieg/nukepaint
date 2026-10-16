//! Asks for the settings of an adjustment or an effect: each a number on a slider, or
//! a box to check, as its kind lists them, or the curves of Curves. The image shows
//! each change as it is made; Cancel, or Escape, puts the layer back as it was.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};

use eframe::egui::{self, Color32, Key};
use power_paint_core::{Curves, Effect, OffsetEdges, RgbaImage, SelectionMask, apply_effect};

use crate::{adjustments::AdjustmentKind, curve_editor::CurveEditor, dialog, effects::EffectKind};

/// Width of the sliders, in points.
const SLIDER_WIDTH: f32 = 220.0;
/// Width of the field next to each slider, in points: wide enough for any of the
/// numbers, so the dialog keeps its width as they change.
const VALUE_WIDTH: f32 = 72.0;
const CHANNEL_NAMES: [&str; 3] = ["Red", "Green", "Blue"];
const CHANNEL_COLORS: [Color32; 3] =
    [Color32::from_rgb(255, 77, 77), Color32::from_rgb(77, 255, 77), Color32::from_rgb(102, 128, 255)];

/// A number a dialog asks for on a slider, from `min` to `max`, going up and down by
/// `step`, with `suffix` shown after it, or a box to check, which is 1 when checked
/// and 0 when not.
pub struct Setting {
    pub label: &'static str,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub default: f64,
    pub suffix: &'static str,
    pub check: bool,
}

pub const fn slider(label: &'static str, min: f64, max: f64, step: f64, default: f64, suffix: &'static str) -> Setting {
    Setting { label, min, max, step, default, suffix, check: false }
}

pub const fn check(label: &'static str, default: bool) -> Setting {
    Setting { label, min: 0.0, max: 1.0, step: 1.0, default: if default { 1.0 } else { 0.0 }, suffix: "", check: true }
}

/// What a dialog asks the settings of: an adjustment of the Colors menu or an effect
/// of the Filters menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FilterKind {
    Adjustment(AdjustmentKind),
    Effect(EffectKind),
}

impl FilterKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Adjustment(kind) => kind.name(),
            Self::Effect(kind) => kind.name(),
        }
    }

    pub fn settings(self) -> &'static [Setting] {
        match self {
            Self::Adjustment(kind) => kind.settings(),
            Self::Effect(kind) => kind.settings(),
        }
    }
}

/// An effect being worked out on a thread of its own, as it may take a while, which
/// gives up once dropped.
pub struct Job {
    stop: Arc<AtomicBool>,
    result: mpsc::Receiver<RgbaImage>,
}

impl Job {
    pub fn start(pixels: RgbaImage, effect: Effect, mask: Option<SelectionMask>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let (sender, result) = mpsc::channel();
        let stopped = Arc::clone(&stop);
        thread::spawn(move || {
            if let Some(pixels) = apply_effect(&pixels, &effect, mask.as_ref(), &stopped) {
                // The dialog may be gone.
                let _ = sender.send(pixels);
            }
        });
        Self { stop, result }
    }

    /// The pixels, if they are worked out.
    pub fn poll(&self) -> Option<RgbaImage> {
        self.result.try_recv().ok()
    }

    /// Waits for the pixels.
    pub fn wait(self) -> Option<RgbaImage> {
        self.result.recv().ok()
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
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
    pub kind: FilterKind,
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
    /// The effect being worked out with the latest settings, if it is not yet.
    pub job: Option<Job>,
    /// Size of what the Offset moves the pixels within, in pixels.
    offset_area: (u32, u32),
}

impl Dialog {
    /// The dialog for `kind` on a layer of an image `size` pixels across and down,
    /// starting from `values`, or for the Offset, from half of what it moves the pixels
    /// within: the rectangle around those `mask` holds, if there is one, or the image.
    pub fn new(
        image: usize,
        layer: usize,
        kind: FilterKind,
        values: Vec<f64>,
        curves: Curves,
        mask: Option<SelectionMask>,
        size: (u32, u32),
    ) -> Self {
        let offset_area = mask.as_ref().and_then(SelectionMask::bounds).map_or(size, |area| {
            (area.width() as u32, area.height() as u32)
        });
        let editor = CurveEditor::default();
        let mut dialog =
            Self { image, layer, kind, values, curves, channel: 0, editor, mask, preview: None, job: None, offset_area };
        if dialog.is_offset() {
            dialog.offset_by_half();
        }
        dialog
    }

    fn is_offset(&self) -> bool {
        self.kind == FilterKind::Effect(EffectKind::Offset)
    }

    /// Moves by half the area each way, which brings its edges to the middle.
    fn offset_by_half(&mut self) {
        let (width, height) = self.offset_area;
        self.values[0] = (width / 2) as f64;
        self.values[1] = (height / 2) as f64;
    }

    /// Shows the dialog. Returns what the user did, if anything.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<Outcome> {
        let mut edited = false;
        // The image stays in plain view, to see the changes on it.
        let response = dialog::show(ctx, "adjustment", self.kind.name(), Color32::TRANSPARENT, |ui| {
            if self.kind == FilterKind::Adjustment(AdjustmentKind::Curves) {
                edited |= self.curves_ui(ui);
            } else if self.is_offset() {
                edited |= self.offset_ui(ui);
            } else {
                edited |= self.settings_ui(ui);
            }
            dialog::buttons(ui, |ui| {
                let enter = ui.input(|input| input.key_pressed(Key::Enter));
                if ui.button("Cancel").clicked() {
                    return Some(Outcome::Canceled);
                }
                if ui.button("OK").clicked() || enter {
                    return Some(Outcome::Accepted);
                }
                if ui.button("Reset").clicked() {
                    self.reset();
                    edited = true;
                }
                if self.job.is_some() {
                    ui.spinner().on_hover_text("Working out the preview");
                }
                None
            })
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
                if setting.check {
                    ui.label("");
                    let mut checked = *value != 0.0;
                    if ui.checkbox(&mut checked, setting.label).changed() {
                        *value = if checked { 1.0 } else { 0.0 };
                        edited = true;
                    }
                    ui.end_row();
                    continue;
                }
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

    /// The moves across and down, each as far as the area either way, a button to move
    /// by half of it, and what fills the edges. Returns whether a setting changed.
    fn offset_ui(&mut self, ui: &mut egui::Ui) -> bool {
        let mut edited = false;
        let (width, height) = self.offset_area;
        egui::Grid::new("offset_grid").num_columns(2).spacing([8.0, 8.0]).show(ui, |ui| {
            for (index, (label, size)) in [("Horizontal:", width), ("Vertical:", height)].into_iter().enumerate() {
                ui.label(label);
                ui.horizontal(|ui| {
                    ui.spacing_mut().slider_width = SLIDER_WIDTH;
                    let value = &mut self.values[index];
                    let range = -(size as f64)..=size as f64;
                    edited |= ui.add(egui::Slider::new(value, range.clone()).step_by(1.0).show_value(false)).changed();
                    let field = egui::DragValue::new(value).range(range).speed(1.0).fixed_decimals(0).suffix(" px");
                    let height = ui.spacing().interact_size.y;
                    edited |= ui.add_sized([VALUE_WIDTH, height], field).changed();
                });
                ui.end_row();
            }
            ui.label("");
            if ui.button("Half").on_hover_text("Moves by half the width and height, bringing the edges to the middle").clicked() {
                self.offset_by_half();
                edited = true;
            }
            ui.end_row();
            ui.label("Edges:");
            let edges = &mut self.values[2];
            let current = OffsetEdges::ALL[(*edges as usize).min(OffsetEdges::ALL.len() - 1)];
            egui::ComboBox::from_id_salt("offset_edges").selected_text(current.name()).show_ui(ui, |ui| {
                for (index, choice) in OffsetEdges::ALL.into_iter().enumerate() {
                    edited |= ui.selectable_value(edges, index as f64, choice.name()).changed();
                }
            });
            ui.end_row();
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
        if self.is_offset() {
            self.offset_by_half();
        }
    }
}

//! Asks for the settings of an adjustment or an effect: each a number on a slider, a
//! box to check, a choice from a list, one of the image's layers or a colour, as its
//! kind lists them, or the curves of Curves. The image shows
//! each change as it is made; Cancel, or Escape, puts the layer back as it was.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};

use eframe::egui::{self, Color32};
use power_paint_core::{Curves, Effect, OffsetEdges, Rgba, RgbaImage, SelectionMask, apply_effect};

use crate::{adjustments::AdjustmentKind, curve_editor::CurveEditor, dialog, effects::EffectKind, slider_field::SliderField};

/// Width of the sliders, in points.
const SLIDER_WIDTH: f32 = 220.0;
const CHANNEL_NAMES: [&str; 3] = ["Red", "Green", "Blue"];
const CHANNEL_COLORS: [Color32; 3] =
    [Color32::from_rgb(255, 77, 77), Color32::from_rgb(77, 255, 77), Color32::from_rgb(102, 128, 255)];

/// What a colour setting starts as: the primary colour or the secondary one, as the
/// dialog opens, rather than one of its own.
pub const PRIMARY: f64 = -1.0;
pub const SECONDARY: f64 = -2.0;

/// A number a dialog asks for, as `input` says, on a slider from `min` to `max`,
/// going up and down by `step`, logarithmic or not, with `suffix` shown after it,
/// which can be changed only while the setting at `enabled_by`, if any, is
/// `enabled_value`, or is not `disabled_value`, or for neither, is a checked box.
pub struct Setting {
    pub label: &'static str,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub default: f64,
    pub suffix: &'static str,
    pub input: Input,
    pub enabled_by: Option<usize>,
    pub enabled_value: Option<f64>,
    pub disabled_value: Option<f64>,
    pub logarithmic: bool,
}

impl Setting {
    const fn new(label: &'static str, min: f64, max: f64, default: f64, input: Input) -> Self {
        Self { label, min, max, step: 1.0, default, suffix: "", input, enabled_by: None, enabled_value: None, disabled_value: None, logarithmic: false }
    }

    /// The same, which can be changed only while the box to check at `index` is checked.
    pub const fn enabled_by(self, index: usize) -> Self {
        Self { enabled_by: Some(index), ..self }
    }

    /// The same, which can be changed only while the setting at `index` is `value`.
    pub const fn enabled_when(self, index: usize, value: f64) -> Self {
        Self { enabled_by: Some(index), enabled_value: Some(value), ..self }
    }

    /// The same, which can be changed only while the setting at `index` is not `value`.
    pub const fn enabled_unless(self, index: usize, value: f64) -> Self {
        Self { enabled_by: Some(index), disabled_value: Some(value), ..self }
    }

    /// The same on a logarithmic slider.
    pub const fn logarithmic(self) -> Self {
        Self { logarithmic: true, ..self }
    }
}

/// How a dialog asks for a setting.
#[derive(Clone, Copy, PartialEq)]
pub enum Input {
    Slider,
    /// A box to check, which is 1 when checked and 0 when not.
    Check,
    /// One of these, by its index.
    Choice(&'static [&'static str]),
    /// One of the image's layers, by its index, bottom first. Below zero, it is the one
    /// above the layer being changed, or that one if it is the top one.
    Layer,
    /// A colour, as [`pack_color`] makes it a number, or [`PRIMARY`] or [`SECONDARY`].
    Color,
}

pub const fn slider(label: &'static str, min: f64, max: f64, step: f64, default: f64, suffix: &'static str) -> Setting {
    Setting { step, suffix, ..Setting::new(label, min, max, default, Input::Slider) }
}

pub const fn check(label: &'static str, default: bool) -> Setting {
    Setting::new(label, 0.0, 1.0, if default { 1.0 } else { 0.0 }, Input::Check)
}

pub const fn choice(label: &'static str, choices: &'static [&'static str], default: usize) -> Setting {
    Setting::new(label, 0.0, choices.len() as f64 - 1.0, default as f64, Input::Choice(choices))
}

pub const fn layer(label: &'static str) -> Setting {
    Setting::new(label, 0.0, 0.0, -1.0, Input::Layer)
}

/// A colour, starting as `default`: [`PRIMARY`], [`SECONDARY`] or one [`pack_color`]
/// makes.
pub const fn color(label: &'static str, default: f64) -> Setting {
    Setting::new(label, 0.0, 0.0, default, Input::Color)
}

/// A straight RGBA colour as a number, which holds it exactly.
pub const fn pack_color(color: [u8; 4]) -> f64 {
    u32::from_be_bytes(color) as f64
}

/// The colour a colour setting holds, or black for the primary colour and white for the
/// secondary one, which the dialog sets it to as it opens.
pub fn unpack_color(value: f64) -> [u8; 4] {
    match value {
        PRIMARY => [0, 0, 0, 255],
        SECONDARY => [255, 255, 255, 255],
        _ => (value as u32).to_be_bytes(),
    }
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
    /// The names of the image's layers, bottom first, to pick one from.
    layer_names: Vec<String>,
    /// The primary and secondary colours, which colour settings start as.
    palette: [Rgba<u8>; 2],
}

impl Dialog {
    /// The dialog for `kind` on a layer of an image `size` pixels across and down, with
    /// layers named as `layer_names` says, bottom first, starting from `values`, or for
    /// the Offset, from half of what it moves the pixels within: the rectangle around
    /// those `mask` holds, if there is one, or the image. The colours start as `palette`,
    /// the primary and secondary colours, says, unless they have been set.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        image: usize,
        layer: usize,
        kind: FilterKind,
        values: Vec<f64>,
        curves: Curves,
        mask: Option<SelectionMask>,
        size: (u32, u32),
        layer_names: Vec<String>,
        palette: [Rgba<u8>; 2],
    ) -> Self {
        let offset_area = mask.as_ref().and_then(SelectionMask::bounds).map_or(size, |area| {
            (area.width() as u32, area.height() as u32)
        });
        let editor = CurveEditor::default();
        let mut dialog = Self {
            image,
            layer,
            kind,
            values,
            curves,
            channel: 0,
            editor,
            mask,
            preview: None,
            job: None,
            offset_area,
            layer_names,
            palette,
        };
        if dialog.is_offset() {
            dialog.offset_by_half();
        }
        dialog.pick_layers();
        dialog.pick_colors();
        dialog
    }

    /// Sets each colour setting that starts as the primary or the secondary colour to it.
    fn pick_colors(&mut self) {
        for (setting, value) in self.kind.settings().iter().zip(&mut self.values) {
            if setting.input == Input::Color && *value < 0.0 {
                let [primary, secondary] = self.palette;
                *value = pack_color(if *value == PRIMARY { primary.0 } else { secondary.0 });
            }
        }
    }

    /// Picks the layer above the one being changed, or that one if it is the top one,
    /// for each layer setting that has none, or one the image does not have.
    fn pick_layers(&mut self) {
        let above = (self.layer + 1).min(self.layer_names.len() - 1) as f64;
        for (setting, value) in self.kind.settings().iter().zip(&mut self.values) {
            if setting.input == Input::Layer && (*value < 0.0 || *value as usize >= self.layer_names.len()) {
                *value = above;
            }
        }
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
                let enter = dialog::enter(ui);
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
        let settings = self.kind.settings();
        let enabled: Vec<bool> = settings
            .iter()
            .map(|setting| {
                setting.enabled_by.is_none_or(|index| {
                    let value = self.values[index];
                    match (setting.enabled_value, setting.disabled_value) {
                        (Some(enabling), _) => value == enabling,
                        (None, Some(disabling)) => value != disabling,
                        (None, None) => value != 0.0,
                    }
                })
            })
            .collect();
        egui::Grid::new("adjustment_grid").num_columns(2).spacing([8.0, 8.0]).show(ui, |ui| {
            for ((setting, value), enabled) in settings.iter().zip(&mut self.values).zip(enabled) {
                // A box to check shows its label beside it instead.
                let label = if setting.input == Input::Check { "" } else { setting.label };
                ui.add_enabled_ui(enabled, |ui| ui.label(label));
                match setting.input {
                    Input::Slider => {}
                    Input::Check => {
                        let mut checked = *value != 0.0;
                        if ui.add_enabled(enabled, egui::Checkbox::new(&mut checked, setting.label)).changed() {
                            *value = if checked { 1.0 } else { 0.0 };
                            edited = true;
                        }
                        ui.end_row();
                        continue;
                    }
                    Input::Choice(choices) => {
                        edited |= ui.add_enabled_ui(enabled, |ui| pick(ui, setting.label, value, choices)).inner;
                        ui.end_row();
                        continue;
                    }
                    Input::Layer => {
                        let names: Vec<&str> = self.layer_names.iter().map(String::as_str).collect();
                        edited |= ui.add_enabled_ui(enabled, |ui| pick(ui, setting.label, value, &names)).inner;
                        ui.end_row();
                        continue;
                    }
                    Input::Color => {
                        let mut color = unpack_color(*value);
                        if ui.add_enabled_ui(enabled, |ui| ui.color_edit_button_srgba_unmultiplied(&mut color)).inner.changed() {
                            *value = pack_color(color);
                            edited = true;
                        }
                        ui.end_row();
                        continue;
                    }
                }
                let mut field = SliderField::new(value, setting.min..=setting.max).step(setting.step).suffix(setting.suffix);
                if setting.logarithmic {
                    field = field.logarithmic();
                }
                edited |= ui.add_enabled(enabled, field.slider_width(SLIDER_WIDTH)).changed();
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
                let field = SliderField::new(&mut self.values[index], -(size as f64)..=size as f64).suffix(" px");
                edited |= ui.add(field.slider_width(SLIDER_WIDTH)).changed();
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
        self.pick_layers();
        self.pick_colors();
    }
}

/// A list to pick one of `choices` from, `value` being the index of the one picked.
/// Returns whether it changed.
fn pick(ui: &mut egui::Ui, id: &str, value: &mut f64, choices: &[&str]) -> bool {
    let mut edited = false;
    let current = choices.get(*value as usize).copied().unwrap_or_default();
    egui::ComboBox::from_id_salt(id).selected_text(current).truncate().show_ui(ui, |ui| {
        for (index, choice) in choices.iter().enumerate() {
            edited |= ui.selectable_value(value, index as f64, *choice).changed();
        }
    });
    edited
}

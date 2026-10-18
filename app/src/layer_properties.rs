//! Asks for a layer's name, whether it shows, how it is blended with the layers under
//! it and how opaque it is. The image shows each change as it is made; Cancel, or
//! Escape, puts the layer back as it was.

use eframe::egui::{
    self, Color32,
    text::{CCursor, CCursorRange},
};
use nukepaint_core::{BlendMode, Layer, MAX_OPACITY};

use crate::dialog;

/// All there is to a layer but its pixels.
#[derive(Clone, PartialEq)]
pub struct Properties {
    pub name: String,
    pub visible: bool,
    pub opacity: u8,
    pub blend_mode: BlendMode,
}

impl Properties {
    pub fn of(layer: &Layer) -> Self {
        Self { name: layer.name.clone(), visible: layer.visible, opacity: layer.opacity, blend_mode: layer.blend_mode }
    }

    pub fn apply(&self, layer: &mut Layer) {
        layer.name.clone_from(&self.name);
        layer.visible = self.visible;
        layer.opacity = self.opacity;
        layer.blend_mode = self.blend_mode;
    }
}

pub enum Outcome {
    /// The properties were changed, and are being tried out.
    Edited,
    Accepted,
    Canceled,
}

pub struct Dialog {
    /// The image and the layer whose properties these are.
    pub image: usize,
    pub layer: usize,
    /// As they were when the dialog opened, to go back to.
    pub before: Properties,
    pub properties: Properties,
    /// Set until the name field has been focused, with all of it selected.
    focus_name: bool,
}

impl Dialog {
    pub fn new(image: usize, layer: usize, properties: Properties) -> Self {
        Self { image, layer, before: properties.clone(), properties, focus_name: true }
    }

    /// Shows the dialog. Returns what the user did, if anything.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<Outcome> {
        let before = self.properties.clone();
        // The image stays in plain view, to see the changes on it.
        let response = dialog::show(ctx, "layer_properties", "Layer Properties", Color32::TRANSPARENT, |ui| {
            egui::Grid::new("layer_properties_grid").num_columns(2).spacing([8.0, 8.0]).show(ui, |ui| {
                ui.label("Name:");
                let mut name = egui::TextEdit::singleline(&mut self.properties.name).desired_width(220.0).show(ui);
                if std::mem::take(&mut self.focus_name) {
                    name.response.request_focus();
                    let all = CCursorRange::two(CCursor::new(0), CCursor::new(self.properties.name.chars().count()));
                    name.state.cursor.set_char_range(Some(all));
                    name.state.store(ui.ctx(), name.response.id);
                }
                ui.end_row();

                ui.label("");
                ui.checkbox(&mut self.properties.visible, "Visible");
                ui.end_row();

                ui.label("Blending mode:");
                egui::ComboBox::from_id_salt("blend_mode")
                    .selected_text(self.properties.blend_mode.name())
                    .show_ui(ui, |ui| {
                        for mode in BlendMode::ALL {
                            ui.selectable_value(&mut self.properties.blend_mode, mode, mode.name());
                        }
                    });
                ui.end_row();

                ui.label("Opacity:");
                ui.add(egui::Slider::new(&mut self.properties.opacity, 0..=MAX_OPACITY));
                ui.end_row();
            });
            dialog::ok_cancel(ui).map(|ok| if ok { Outcome::Accepted } else { Outcome::Canceled })
        });
        if response.should_close() {
            return Some(Outcome::Canceled);
        }
        response.inner.or_else(|| (self.properties != before).then_some(Outcome::Edited))
    }
}

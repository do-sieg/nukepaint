//! The tools, as a button each, which wrap onto as many rows as the dock's width
//! needs. The current tool's stands out.

use eframe::egui;

use crate::icons::{ICON_SIZE, Icons};
use crate::tools::Tool;

/// Shows the buttons, with `current` as the tool in use. Returns the tool clicked.
pub fn show(ui: &mut egui::Ui, icons: &mut Icons, current: Tool) -> Option<Tool> {
    let mut clicked = None;
    ui.horizontal_wrapped(|ui| {
        for tool in Tool::ALL {
            let texture = icons.get(ui.ctx(), tool.icon());
            // The icons are light gray lines, made the colour of the text to suit either theme.
            let tint = ui.visuals().strong_text_color();
            let icon = egui::Image::new(texture).fit_to_exact_size(ICON_SIZE).tint(tint);
            let button = egui::Button::selectable(current == tool, icon);
            let key = ui.ctx().format_shortcut(&tool.key());
            let tooltip = format!("{} ({key})\n{}", tool.name(), tool.help());
            if ui.add(button).on_hover_text(tooltip).clicked() {
                clicked = Some(tool);
            }
        }
    });
    clicked
}

//! The steps the current image went through, oldest first, each with its icon and
//! name. The step the image is as of stands out, and those undone after it are dimmed.
//! A click on a step undoes or redoes as many steps as it takes to get back to it.

use eframe::egui::{self, Pos2, Rect, Sense, TextStyle, TextWrapMode, Vec2, WidgetText};
use nukepaint_core::Document;

use crate::icons::{ICON_SIZE, Icon, Icons};

/// Room around the parts of a row, in points.
const ROW_MARGIN: f32 = 4.0;
const ROW_HEIGHT: f32 = ICON_SIZE.y + ROW_MARGIN * 2.0;
/// Tallest the list gets before it scrolls, in points.
const LIST_MAX_HEIGHT: f32 = 200.0;
/// How much of their colour the undone steps keep.
const UNDONE_OPACITY: f32 = 0.4;

/// Shows the list, no taller than `tallest`, which does nothing unless `enabled`. The
/// current step is brought into view when it changes, and with `image_changed`.
/// Returns the step clicked.
pub fn show(
    ui: &mut egui::Ui,
    tallest: f32,
    document: &Document,
    icons: &mut Icons,
    enabled: bool,
    image_changed: bool,
) -> Option<usize> {
    let mut clicked = None;
    egui::ScrollArea::vertical()
        .id_salt("history")
        .max_height(LIST_MAX_HEIGHT.min(tallest))
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.add_enabled_ui(enabled, |ui| clicked = list(ui, document, icons, image_changed));
        });
    clicked
}

/// Returns the step clicked.
fn list(ui: &mut egui::Ui, document: &Document, icons: &mut Icons, image_changed: bool) -> Option<usize> {
    let current = document.current_step();
    let mut clicked = None;
    ui.spacing_mut().item_spacing.y = 0.0;
    let shown_id = ui.id().with("shown_step");
    let shown = (current, document.steps().len());
    let scroll = image_changed || ui.data(|data| data.get_temp::<(usize, usize)>(shown_id)) != Some(shown);
    ui.data_mut(|data| data.insert_temp(shown_id, shown));

    for (index, step) in document.steps().enumerate() {
        let width = ui.available_width();
        let (rect, response) = ui.allocate_exact_size(Vec2::new(width, ROW_HEIGHT), Sense::click());
        let visuals = ui.visuals().clone();
        let is_current = index == current;
        if is_current {
            ui.painter().rect_filled(rect, 3.0, visuals.selection.bg_fill);
            if scroll {
                ui.scroll_to_rect(rect, None);
            }
        } else if response.hovered() && ui.is_enabled() {
            ui.painter().rect_filled(rect, 3.0, visuals.widgets.hovered.weak_bg_fill);
        }
        let (color, icon_tint) = match () {
            _ if is_current => (visuals.selection.stroke.color, visuals.selection.stroke.color),
            _ if index > current => (
                visuals.text_color().gamma_multiply(UNDONE_OPACITY),
                visuals.strong_text_color().gamma_multiply(UNDONE_OPACITY),
            ),
            _ => (visuals.text_color(), visuals.strong_text_color()),
        };

        let icon_rect = Rect::from_min_size(rect.min + Vec2::splat(ROW_MARGIN), ICON_SIZE);
        if let Some(icon) = step.icon.and_then(Icon::of_step) {
            let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
            // The icons are light gray lines, made the colour of the text to suit either theme.
            ui.painter().image(icons.get(ui.ctx(), icon).id(), icon_rect, uv, icon_tint);
        }
        let text_left = icon_rect.right() + ROW_MARGIN * 2.0;
        let text_width = (rect.right() - ROW_MARGIN - text_left).max(0.0);
        let name = WidgetText::from(step.name.as_str());
        let galley = name.into_galley(ui, Some(TextWrapMode::Truncate), text_width, TextStyle::Body);
        let top = rect.center().y - galley.size().y / 2.0;
        let truncated = galley.elided;
        ui.painter().galley(Pos2::new(text_left, top), galley, color);

        let response = if truncated { response.on_hover_text(&step.name) } else { response };
        if response.clicked() && !is_current {
            clicked = Some(index);
        }
    }
    clicked
}

//! The layers of the current image, top one first, each with a box that shows or
//! hides it, a small picture of it and its name. A click on a layer makes it the
//! active one, a double click asks for its properties, a right click makes it active
//! and shows a menu of what can be done with it, and dragging it moves it up or down
//! the list. Buttons under the list, or beside it in a short dock, run the layer
//! commands that have an icon.

use eframe::egui::{
    self, Color32, CursorIcon, Pos2, Rect, Sense, Stroke, StrokeKind, TextStyle, TextWrapMode, TextureHandle, Vec2,
    WidgetText,
};
use nukepaint_core::{BlendMode, Layer, MAX_OPACITY};

use crate::{
    OpenImage, canvas,
    dock::Flow,
    icons::{ICON_SIZE, Icons},
    layer_commands::LayerCommand,
};

/// Side, in points, of the square each layer's picture is fitted into.
pub const THUMBNAIL_SIZE: f32 = 32.0;
/// Room around the parts of a row, in points.
const ROW_MARGIN: f32 = 4.0;
const ROW_HEIGHT: f32 = THUMBNAIL_SIZE + ROW_MARGIN * 2.0;
const VISIBILITY_WIDTH: f32 = 24.0;
const CHECKER_CELL: f32 = 4.0;
/// Tallest the list gets before it scrolls, in points.
const LIST_MAX_HEIGHT: f32 = 320.0;

pub enum Action {
    Select(usize),
    SetVisible(usize, bool),
    /// A layer was dragged to another place among the layers.
    Move { from: usize, to: usize },
    /// A layer was double clicked.
    Properties(usize),
    Run(LayerCommand),
}

/// Shows the list and the buttons after it. `enabled` says which commands can run.
pub fn show(
    flow: &mut Flow,
    image: &OpenImage,
    checker: &TextureHandle,
    icons: &mut Icons,
    enabled: impl Fn(LayerCommand) -> bool,
) -> Option<Action> {
    let mut action = flow.part(|ui, tallest| {
        let scroll = egui::ScrollArea::vertical().max_height(LIST_MAX_HEIGHT.min(tallest)).auto_shrink([false, true]);
        let action = scroll.show(ui, |ui| list(ui, image, checker, &enabled)).inner;
        ui.add_space(ROW_MARGIN);
        action
    });
    flow.part(|ui, _| ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let tint = ui.visuals().strong_text_color();
        for command in LayerCommand::BUTTONS {
            let Some(icon) = command.icon() else {
                continue;
            };
            let icon = egui::Image::new(icons.get(ui.ctx(), icon)).fit_to_exact_size(ICON_SIZE).tint(tint);
            let label = command.label().trim_end_matches('…');
            let tooltip = match command.shortcut() {
                Some(shortcut) => format!("{label} ({})", ui.ctx().format_shortcut(&shortcut)),
                None => label.to_owned(),
            };
            if ui.add_enabled(enabled(command), egui::Button::image(icon)).on_hover_text(tooltip).clicked() {
                action = Some(Action::Run(command));
            }
        }
    }));
    action
}

fn list(
    ui: &mut egui::Ui,
    image: &OpenImage,
    checker: &TextureHandle,
    enabled: &impl Fn(LayerCommand) -> bool,
) -> Option<Action> {
    let document = &image.document;
    let count = document.layers.len();
    let mut action = None;
    ui.spacing_mut().item_spacing.y = 2.0;
    // Each row, top layer first, with the layer it shows.
    let mut rows = Vec::with_capacity(count);
    let mut dragged = None;
    for index in (0..count).rev() {
        let layer = &document.layers[index];
        let width = ui.available_width();
        let (rect, response) = ui.allocate_exact_size(Vec2::new(width, ROW_HEIGHT), Sense::click_and_drag());
        rows.push(rect);
        let active = index == document.active_layer;
        let visuals = ui.visuals().clone();
        if active {
            ui.painter().rect_filled(rect, 3.0, visuals.selection.bg_fill);
        } else if response.hovered() {
            ui.painter().rect_filled(rect, 3.0, visuals.widgets.hovered.weak_bg_fill);
        }

        // Added after the row, to be clicked rather than it.
        let visibility_rect = Rect::from_min_size(rect.min, Vec2::new(VISIBILITY_WIDTH, ROW_HEIGHT));
        let mut visible = layer.visible;
        let visibility = ui.put(visibility_rect, egui::Checkbox::without_text(&mut visible));
        if visibility.on_hover_text("Layer visibility").changed() {
            action = Some(Action::SetVisible(index, visible));
        }

        let picture_room = Rect::from_min_size(
            Pos2::new(visibility_rect.right(), rect.top() + ROW_MARGIN),
            Vec2::splat(THUMBNAIL_SIZE),
        );
        let picture = canvas::fit(image.texture.size(), picture_room);
        let painter = ui.painter();
        canvas::paint_checker(painter, picture, CHECKER_CELL, checker);
        if let Some(thumbnail) = image.layer_thumbnails.get(index) {
            let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
            painter.image(thumbnail.id(), picture, uv, Color32::WHITE);
        }
        painter.rect_stroke(picture, 0.0, Stroke::new(1.0, Color32::from_black_alpha(128)), StrokeKind::Outside);

        let text_left = picture_room.right() + ROW_MARGIN * 2.0;
        let text_width = (rect.right() - ROW_MARGIN - text_left).max(0.0);
        let color = match () {
            _ if active => visuals.selection.stroke.color,
            _ if !layer.visible => visuals.weak_text_color(),
            _ => visuals.text_color(),
        };
        let truncated = paint_name(ui, layer, Pos2::new(text_left, rect.center().y), text_width, color);

        let response = if truncated { response.on_hover_text(&layer.name) } else { response };
        if response.double_clicked() {
            action = Some(Action::Properties(index));
        } else if response.clicked() || response.drag_started() || response.secondary_clicked() {
            action = action.or(Some(Action::Select(index)));
        }
        response.context_menu(|ui| {
            if let Some(command) = context_menu(ui, enabled) {
                action = Some(Action::Run(command));
            }
        });
        if response.dragged() || response.drag_stopped() {
            dragged = Some((index, response));
        }
    }

    if let Some((index, response)) = dragged {
        let from_row = count - 1 - index;
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        let pointer = response.interact_pointer_pos()?;
        // How many rows would be above the layer dropped now.
        let slot = rows.iter().filter(|row| pointer.y > row.center().y).count();
        let line = match slot {
            0 => rows[0].top(),
            _ => rows[slot - 1].bottom() + ui.spacing().item_spacing.y / 2.0,
        };
        let stroke = Stroke::new(2.0, ui.visuals().strong_text_color());
        ui.painter().hline(rows[0].x_range(), line, stroke);
        // Taking the layer out of the list moves the rows under it up by one.
        let to_row = if slot <= from_row { slot } else { slot - 1 };
        if response.drag_stopped() && to_row != from_row {
            action = Some(Action::Move { from: index, to: count - 1 - to_row });
        }
    }
    action
}

/// The commands that act on the active layer, which the layer right clicked has become.
/// Returns the one clicked.
fn context_menu(ui: &mut egui::Ui, enabled: &impl Fn(LayerCommand) -> bool) -> Option<LayerCommand> {
    let mut clicked = None;
    for (group, commands) in LayerCommand::CONTEXT_MENU.iter().enumerate() {
        if group > 0 {
            ui.separator();
        }
        for &command in *commands {
            let mut button = egui::Button::new(command.context_label());
            if let Some(shortcut) = command.shortcut() {
                button = button.shortcut_text(ui.ctx().format_shortcut(&shortcut));
            }
            if ui.add_enabled(enabled(command), button).clicked() {
                clicked = Some(command);
            }
        }
    }
    clicked
}

/// Writes the layer's name, centered on `left_center`, cut short to `width`, with how
/// it is blended under it when that is not plain. Returns whether the name was cut short.
fn paint_name(ui: &egui::Ui, layer: &Layer, left_center: Pos2, width: f32, color: Color32) -> bool {
    let galley = |text: WidgetText, style: TextStyle| text.into_galley(ui, Some(TextWrapMode::Truncate), width, style);
    let name = galley(WidgetText::from(layer.name.as_str()), TextStyle::Body);
    let mut blending = Vec::new();
    if layer.blend_mode != BlendMode::Normal {
        blending.push(layer.blend_mode.name().to_owned());
    }
    if layer.opacity != MAX_OPACITY {
        blending.push(format!("{}%", (layer.opacity as f32 / MAX_OPACITY as f32 * 100.0).round()));
    }
    let details = (!blending.is_empty()).then(|| galley(WidgetText::from(blending.join(" · ")), TextStyle::Small));
    let height = name.size().y + details.as_ref().map_or(0.0, |details| details.size().y);
    let top = left_center.y - height / 2.0;
    let truncated = name.elided;
    let name_height = name.size().y;
    ui.painter().galley(Pos2::new(left_center.x, top), name, color);
    if let Some(details) = details {
        ui.painter().galley(Pos2::new(left_center.x, top + name_height), details, color.gamma_multiply(0.7));
    }
    truncated
}

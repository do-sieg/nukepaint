//! The open images, side by side as thumbnails in a row that scrolls. A click on
//! one makes it the current image. Its close button, or a middle click, closes it.
//! When they do not all fit, arrows at both ends scroll the row. A dot marks the
//! images with unsaved changes.

use eframe::egui::{self, Align, Color32, Layout, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, TextureHandle, Vec2};

use crate::{OpenImage, canvas};

/// Side, in points, of the square each image is fitted into.
pub const THUMBNAIL_SIZE: f32 = 48.0;
/// Room around that square, in points.
const THUMBNAIL_MARGIN: f32 = 4.0;
const CHECKER_CELL: f32 = 4.0;
const CLOSE_SIZE: f32 = 16.0;
const UNSAVED_DOT_RADIUS: f32 = 3.0;
const UNSAVED_COLOR: Color32 = Color32::from_rgb(255, 191, 51);
const ARROW_WIDTH: f32 = 16.0;
/// How long, in seconds, an arrow is held before the row scrolls on its own.
const ARROW_REPEAT_DELAY: f64 = 0.4;
/// Points per second the row scrolls while an arrow is held.
const ARROW_SCROLL_SPEED: f32 = 600.0;

pub enum Action {
    Select(usize),
    Close(usize),
}

/// Shows the strip, right-aligned in what is left of the row.
pub fn show(
    ui: &mut egui::Ui,
    images: &[OpenImage],
    current: Option<usize>,
    scroll_to_current: bool,
    checker: &TextureHandle,
) -> Option<Action> {
    let mut action = None;
    let side = THUMBNAIL_SIZE + THUMBNAIL_MARGIN * 2.0;
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let row_width = images.len() as f32 * side;
        let overflows = row_width > ui.available_width();
        let arrow_size = Vec2::new(ARROW_WIDTH, side);
        let right_arrow = overflows.then(|| ui.allocate_exact_size(arrow_size, Sense::click()));

        // All the room left in the row, the thumbnails keeping to its right end.
        let room = Vec2::new(ui.available_width() - if overflows { ARROW_WIDTH } else { 0.0 }, side);
        let output = ui
            .allocate_ui_with_layout(room, Layout::left_to_right(Align::Center), |ui| {
                egui::ScrollArea::horizontal().auto_shrink(false).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.add_space((ui.available_width() - row_width).max(0.0));
                    for (index, image) in images.iter().enumerate() {
                        let selected = current == Some(index);
                        let (response, close) = thumbnail(ui, image, selected, checker);
                        if close {
                            action = Some(Action::Close(index));
                        } else if response.clicked() {
                            action = Some(Action::Select(index));
                        }
                        if selected && scroll_to_current {
                            response.scroll_to_me(None);
                        }
                    }
                })
            })
            .inner;

        let Some((right_rect, right_response)) = right_arrow else {
            return;
        };
        let (left_rect, left_response) = ui.allocate_exact_size(arrow_size, Sense::click());
        let mut state = output.state;
        let offset = state.offset.x;
        let max_offset = (output.content_size.x - output.inner_rect.width()).max(0.0);
        let can_go_left = offset > 0.0;
        let can_go_right = offset < max_offset;
        arrow(ui, left_rect, &left_response, false, can_go_left);
        arrow(ui, right_rect, &right_response, true, can_go_right);

        let target = if can_go_left && let Some(held) = arrow_held(ui, &left_response) {
            match held {
                // One thumbnail at a time, lining the row up with a thumbnail's edge.
                Held::Pressed => ((offset / side).ceil() - 1.0) * side,
                Held::Repeating(distance) => offset - distance,
            }
        } else if can_go_right && let Some(held) = arrow_held(ui, &right_response) {
            match held {
                Held::Pressed => ((offset / side).floor() + 1.0) * side,
                Held::Repeating(distance) => offset + distance,
            }
        } else {
            return;
        };
        state.offset.x = target.clamp(0.0, max_offset);
        state.store(ui.ctx(), output.id);
        ui.ctx().request_repaint();
    });
    action
}

/// Draws one thumbnail. Returns its response, and whether it was asked to close.
fn thumbnail(ui: &mut egui::Ui, image: &OpenImage, selected: bool, checker: &TextureHandle) -> (egui::Response, bool) {
    let side = THUMBNAIL_SIZE + THUMBNAIL_MARGIN * 2.0;
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(side), Sense::click());
    let close_rect = Rect::from_min_size(rect.right_top() - Vec2::new(CLOSE_SIZE, 0.0), Vec2::splat(CLOSE_SIZE));
    let close = ui.interact(close_rect, response.id.with("close"), Sense::click());
    let hovered = response.hovered() || close.hovered();

    let visuals = ui.visuals();
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, 3.0, visuals.selection.bg_fill);
    } else if hovered {
        painter.rect_filled(rect, 3.0, visuals.widgets.hovered.weak_bg_fill);
    }

    // Fitted to the image's own proportions, which the shrunk copy only rounds.
    let picture = canvas::fit(image.texture.size(), rect.shrink(THUMBNAIL_MARGIN));
    canvas::paint_checker(painter, picture, CHECKER_CELL, checker);
    painter.image(image.thumbnail.id(), picture, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    painter.rect_stroke(picture, 0.0, Stroke::new(1.0, Color32::from_black_alpha(128)), StrokeKind::Outside);

    if image.document.unsaved {
        // In the bottom left corner, as long as there are changes to save.
        let center = rect.left_bottom() + Vec2::new(THUMBNAIL_MARGIN + 4.0, -THUMBNAIL_MARGIN - 4.0);
        painter.circle(center, UNSAVED_DOT_RADIUS, UNSAVED_COLOR, Stroke::new(1.0, Color32::BLACK));
    }

    // Only the current image, and the one under the pointer, show their close button.
    if selected || hovered {
        let background = if close.hovered() { Color32::from_black_alpha(200) } else { Color32::from_black_alpha(120) };
        painter.circle_filled(close_rect.center(), CLOSE_SIZE / 2.0 - 1.0, background);
        painter.text(close_rect.center(), egui::Align2::CENTER_CENTER, "×", egui::FontId::proportional(14.0), Color32::WHITE);
    }

    let document = &image.document;
    let mut tooltip = match &document.path {
        Some(path) => format!("{}\n{}", document.name(), path.display()),
        None => document.name(),
    };
    if document.unsaved {
        tooltip.push_str("\nUnsaved changes");
    }
    let response = response.on_hover_text(tooltip);
    let close_requested = close.on_hover_text("Close").clicked() || response.middle_clicked();
    (response, close_requested)
}

enum Held {
    /// Just pressed.
    Pressed,
    /// Held long enough to scroll on its own, by this distance this frame.
    Repeating(f32),
}

/// Whether the arrow is being pressed, and how.
fn arrow_held(ui: &egui::Ui, response: &Response) -> Option<Held> {
    if !response.is_pointer_button_down_on() {
        return None;
    }
    ui.input(|input| {
        if input.pointer.any_pressed() {
            return Some(Held::Pressed);
        }
        let held_for = input.time - input.pointer.press_start_time()?;
        (held_for > ARROW_REPEAT_DELAY).then_some(Held::Repeating(ARROW_SCROLL_SPEED * input.stable_dt))
    })
}

/// Draws an arrow button pointing left or right, dimmed when there is nothing more that way.
fn arrow(ui: &egui::Ui, rect: Rect, response: &Response, points_right: bool, enabled: bool) {
    let visuals = ui.visuals();
    let painter = ui.painter();
    let color = if !enabled {
        visuals.weak_text_color()
    } else if response.hovered() {
        painter.rect_filled(rect, 3.0, visuals.widgets.hovered.weak_bg_fill);
        visuals.widgets.hovered.fg_stroke.color
    } else {
        visuals.widgets.inactive.fg_stroke.color
    };
    let center = rect.center();
    let (half_width, half_height) = (3.0, 6.0);
    let direction = if points_right { 1.0 } else { -1.0 };
    let tip = Pos2::new(center.x + half_width * direction, center.y);
    let top = Pos2::new(center.x - half_width * direction, center.y - half_height);
    let bottom = Pos2::new(center.x - half_width * direction, center.y + half_height);
    // Clockwise, for the edges to be smoothed.
    let points = if points_right { vec![top, tip, bottom] } else { vec![bottom, tip, top] };
    painter.add(Shape::convex_polygon(points, color, Stroke::NONE));
}

//! The open images, side by side as thumbnails in a row that scrolls. A click on
//! one makes it the current image, and a drag moves it along the row. Its close
//! button, or a middle click, closes it. When they do not all fit, arrows at both ends
//! scroll the row, as does dragging a thumbnail near them. A dot marks the images with
//! unsaved changes.

use eframe::egui::{
    self, Align, Color32, CursorIcon, Id, LayerId, Layout, Order, Painter, PointerButton, Pos2, Rect, Response, Sense, Shape,
    Stroke, StrokeKind, TextureHandle, Vec2,
};

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
/// Opacity of the copy of a thumbnail that follows the pointer while it is dragged.
const DRAGGED_OPACITY: f32 = 0.6;
const DROP_MARKER_WIDTH: f32 = 2.0;

pub enum Action {
    Select(usize),
    Close(usize),
    /// The image at the first index was dragged to the second.
    Move(usize, usize),
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
    // Where the pointer is while a thumbnail is dragged.
    let mut drag_pointer = None;
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
                    let mut rects = Vec::with_capacity(images.len());
                    let mut dragged = None;
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
                        if response.dragged_by(PointerButton::Primary) {
                            dragged = Some((index, false));
                        } else if response.drag_stopped_by(PointerButton::Primary) {
                            dragged = Some((index, true));
                        }
                        rects.push(response.rect);
                    }
                    if let Some((from, dropped)) = dragged
                        && let Some(pointer) = ui.ctx().pointer_interact_pos()
                    {
                        let to = drop_index(&rects, from, pointer.x);
                        if dropped {
                            if to != from {
                                action = Some(Action::Move(from, to));
                            }
                        } else {
                            drag_pointer = Some(pointer);
                            show_drag(ui, &rects, (from, to), pointer, &images[from], checker);
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

        let target = if let Some(pointer) = drag_pointer {
            // Over the arrows, or half a thumbnail from them, to drop it out of view.
            let distance = ARROW_SCROLL_SPEED * ui.input(|input| input.stable_dt);
            if pointer.x < output.inner_rect.left() + side / 2.0 {
                offset - distance
            } else if pointer.x > output.inner_rect.right() - side / 2.0 {
                offset + distance
            } else {
                return;
            }
        } else if can_go_left && let Some(held) = arrow_held(ui, &left_response) {
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

/// Where a thumbnail dragged from `from` goes when dropped with the pointer at `x`:
/// before the first thumbnail whose middle is past the pointer.
fn drop_index(rects: &[Rect], from: usize, x: f32) -> usize {
    let gap = rects.iter().filter(|rect| rect.center().x < x).count();
    // Taking it out first shifts those after it back by one.
    if gap > from { gap - 1 } else { gap }
}

/// Marks where the dragged thumbnail would go, and draws a copy of it under the pointer.
fn show_drag(
    ui: &egui::Ui,
    rects: &[Rect],
    (from, to): (usize, usize),
    pointer: Pos2,
    image: &OpenImage,
    checker: &TextureHandle,
) {
    ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    if to != from {
        // Between two thumbnails, kept inside the row at its ends.
        let gap = if to > from { to + 1 } else { to };
        let x = if gap == 0 { rects[0].left() } else { rects[gap - 1].right() };
        let half = DROP_MARKER_WIDTH / 2.0;
        let x = x.clamp(rects[0].left() + half, rects[rects.len() - 1].right() - half);
        let stroke = Stroke::new(DROP_MARKER_WIDTH, ui.visuals().selection.stroke.color);
        ui.painter().vline(x, rects[from].y_range(), stroke);
    }

    // Along the row only, keeping where in the thumbnail it was picked up.
    let press_x = ui.input(|input| input.pointer.press_origin()).map_or(pointer.x, |origin| origin.x);
    let rect = rects[from].translate(Vec2::new(pointer.x - press_x, 0.0));
    let mut painter = ui.ctx().layer_painter(LayerId::new(Order::Tooltip, Id::new("image_strip_drag")));
    painter.multiply_opacity(DRAGGED_OPACITY);
    paint_picture(&painter, rect, image, checker);
}

/// Draws one thumbnail. Returns its response, and whether it was asked to close.
fn thumbnail(ui: &mut egui::Ui, image: &OpenImage, selected: bool, checker: &TextureHandle) -> (egui::Response, bool) {
    let side = THUMBNAIL_SIZE + THUMBNAIL_MARGIN * 2.0;
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(side), Sense::click_and_drag());
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

    paint_picture(painter, rect, image, checker);

    if image.document.is_unsaved() {
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
    if document.is_unsaved() {
        tooltip.push_str("\nUnsaved changes");
    }
    let response = response.on_hover_text(tooltip);
    let close_requested = close.on_hover_text("Close").clicked() || response.middle_clicked();
    (response, close_requested)
}

/// Draws the image fitted into the thumbnail's square, over a checkerboard.
fn paint_picture(painter: &Painter, rect: Rect, image: &OpenImage, checker: &TextureHandle) {
    // Fitted to the image's own proportions, which the shrunk copy only rounds.
    let picture = canvas::fit(image.texture.size(), rect.shrink(THUMBNAIL_MARGIN));
    canvas::paint_checker(painter, picture, CHECKER_CELL, checker);
    image.texture.paint(painter, picture);
    painter.rect_stroke(picture, 0.0, Stroke::new(1.0, Color32::from_black_alpha(128)), StrokeKind::Outside);
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

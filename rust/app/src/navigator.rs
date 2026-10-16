//! The whole of the current image, small, with a frame around the part in view and
//! the rest dimmed. A click on it scrolls the view to have that point in the middle,
//! and dragging keeps it there. While the pointer is over it, a fainter frame shows
//! the part that a click would bring into view. Tiled, the frames wrap around the
//! edges of the tile, as the view goes from it onto its copies, and what is dimmed is
//! the rest of the image, which is not shown.
//!
//! Under it, a slider sets the zoom, with buttons that go a level in and out on
//! either side. The wheel over any of it, or a pinch, zooms too, about the middle
//! of the view.

use eframe::egui::{self, Color32, CursorIcon, Pos2, Rect, Sense, Stroke, StrokeKind, TextureHandle, Vec2};

use crate::{
    OpenImage, canvas,
    view::{ZOOM_LEVELS, format_zoom},
};

/// Tallest the picture gets, in points: a tall image is narrower than the section.
const MAX_HEIGHT: f32 = 200.0;
const CHECKER_CELL: f32 = 4.0;
const DIM: Color32 = Color32::from_black_alpha(110);
/// The frame around the part in view: a light line in a dark one, seen on any image.
const FRAME_WIDTH: f32 = 1.5;
/// How strong the frame of the part a click would bring into view is, next to it.
const HOVER_ALPHA: f32 = 0.5;
/// Room for the zoom after the slider, as wide as the widest level.
const ZOOM_TEXT_WIDTH: f32 = 44.0;
const MIN_SLIDER_WIDTH: f32 = 32.0;

pub enum Action {
    /// Scroll the view to have this point of the image, in pixels, in the middle.
    CenterOn(Pos2),
    /// Go this many zoom levels in, or out if below zero.
    StepZoom(i32),
    ZoomTo(f32),
}

/// Shows the picture and the zoom under it.
pub fn show(ui: &mut egui::Ui, image: &OpenImage, checker: &TextureHandle) -> Option<Action> {
    let top = ui.cursor().min;
    let centered = picture(ui, image, checker).map(Action::CenterOn);
    let zoomed = zoom_row(ui, image.view.zoom);
    // Rather than scroll the dock.
    let area = Rect::from_min_max(top, ui.min_rect().max);
    let wheeled = (ui.rect_contains_pointer(area) && ui.ctx().dragged_id().is_none()).then(|| {
        ui.ctx().input_mut(|input| input.smooth_scroll_delta = Vec2::ZERO);
        match canvas::wheel_zoom(ui, ui.id().with("navigator_zoom_scroll"), false) {
            (0, 1.0) => None,
            (0, pinch) => Some(Action::ZoomTo(image.view.zoom * pinch)),
            (steps, _) => Some(Action::StepZoom(steps)),
        }
    });
    centered.or(zoomed).or(wheeled.flatten())
}

/// The slider, and the buttons on either side.
fn zoom_row(ui: &mut egui::Ui, zoom: f32) -> Option<Action> {
    let (min, max) = (ZOOM_LEVELS[0], ZOOM_LEVELS[ZOOM_LEVELS.len() - 1]);
    ui.horizontal(|ui| {
        let button_size = Vec2::splat(ui.spacing().interact_size.y);
        let button = |text| egui::Button::new(text).min_size(button_size);
        let mut action = None;
        if ui.add_enabled(zoom > min, button("−")).on_hover_text("Zoom Out").clicked() {
            action = Some(Action::StepZoom(-1));
        }
        let spacing = ui.spacing().item_spacing.x;
        let room = ui.available_width() - button_size.x - ZOOM_TEXT_WIDTH - spacing * 2.0;
        ui.spacing_mut().slider_width = room.max(MIN_SLIDER_WIDTH);
        let mut value = zoom;
        let slider = egui::Slider::new(&mut value, min..=max).logarithmic(true).show_value(false);
        if ui.add(slider).on_hover_text("Zoom").changed() {
            action = Some(Action::ZoomTo(value));
        }
        ui.add_sized([ZOOM_TEXT_WIDTH, button_size.y], egui::Label::new(format_zoom(zoom)));
        if ui.add_enabled(zoom < max, button("+")).on_hover_text("Zoom In").clicked() {
            action = Some(Action::StepZoom(1));
        }
        action
    })
    .inner
}

/// The whole image, with the part in view framed. Returns the point of the image,
/// in pixels, to scroll the view to while it is being clicked or dragged.
fn picture(ui: &mut egui::Ui, image: &OpenImage, checker: &TextureHandle) -> Option<Pos2> {
    let image_size = image.texture.size();
    let width = ui.available_width();
    let scale = (width / image_size.x).min(MAX_HEIGHT / image_size.y);
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, (image_size.y * scale).round()), Sense::click_and_drag());
    let picture = canvas::fit(image_size, rect);
    let to_image = |pos: Pos2| {
        let point = (pos - picture.min) / picture.size() * image_size;
        point.clamp(Vec2::ZERO, image_size).to_pos2()
    };
    let to_picture = |area: Rect| {
        let scale = picture.size() / image_size;
        Rect::from_min_max(picture.min + area.min.to_vec2() * scale, picture.min + area.max.to_vec2() * scale)
    };

    let painter = ui.painter_at(rect);
    canvas::paint_checker(&painter, picture, CHECKER_CELL, checker);
    image.texture.paint(&painter, picture);
    painter.rect_stroke(picture, 0.0, Stroke::new(1.0, Color32::from_black_alpha(128)), StrokeKind::Outside);

    let tile = image.view.tile().map(|tile| {
        let corners = |x: i32, y: i32| Pos2::new(x as f32, y as f32);
        to_picture(Rect::from_min_max(corners(tile.left, tile.top), corners(tile.right, tile.bottom)))
    });
    // Tiled, a frame off one edge of the tile comes back in at the other.
    let frame = |rect: Rect, alpha: f32| {
        let Some(tile) = tile else {
            paint_frame(&painter, rect, alpha);
            return;
        };
        let painter = painter.with_clip_rect(tile);
        for y in -1..=1 {
            for x in -1..=1 {
                paint_frame(&painter, rect.translate(Vec2::new(x as f32, y as f32) * tile.size()), alpha);
            }
        }
    };
    // Dimmed around the part in view, or tiled, around the tile.
    let visible = to_picture(image.view.visible_area());
    let lit = tile.unwrap_or(visible);
    let dims = [
        Rect::from_min_max(picture.min, Pos2::new(picture.max.x, lit.min.y)),
        Rect::from_min_max(Pos2::new(picture.min.x, lit.max.y), picture.max),
        Rect::from_min_max(Pos2::new(picture.min.x, lit.min.y), Pos2::new(lit.min.x, lit.max.y)),
        Rect::from_min_max(Pos2::new(lit.max.x, lit.min.y), Pos2::new(picture.max.x, lit.max.y)),
    ];
    for dimmed in dims {
        if dimmed.is_positive() {
            painter.rect_filled(dimmed, 0.0, DIM);
        }
    }
    frame(visible, 1.0);

    let pressed = response.is_pointer_button_down_on();
    if pressed {
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    } else if let Some(pointer) = response.hover_pos() {
        ui.ctx().set_cursor_icon(CursorIcon::Grab);
        frame(to_picture(image.view.area_centered_on(to_image(pointer))), HOVER_ALPHA);
    }
    response.on_hover_text("Click or drag to show that part of the image");

    let primary = ui.input(|input| input.pointer.primary_down());
    let pointer = ui.ctx().pointer_interact_pos();
    pointer.filter(|_| pressed && primary).map(to_image)
}

fn paint_frame(painter: &egui::Painter, rect: Rect, alpha: f32) {
    painter.rect_stroke(rect, 0.0, Stroke::new(FRAME_WIDTH + 2.0, Color32::BLACK.gamma_multiply(alpha)), StrokeKind::Inside);
    let inner = rect.shrink(1.0);
    painter.rect_stroke(inner, 0.0, Stroke::new(FRAME_WIDTH, Color32::WHITE.gamma_multiply(alpha)), StrokeKind::Inside);
}

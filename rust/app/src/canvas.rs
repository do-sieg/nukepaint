//! The current image, zoomed and scrolled, over a checkerboard. Ctrl and the wheel,
//! or a pinch, zoom on the pointer, and the wheel alone scrolls. The middle button,
//! or the left one while Space is held, drags the image around.

use eframe::egui::{
    self, Color32, ColorImage, CursorIcon, Event, Key, Painter, Pos2, Rect, Sense, TextureHandle, TextureOptions,
    Vec2, scroll_area::ScrollSource,
};

use crate::{tiled_texture::TiledTexture, view::View};

/// Side, in pixels, of each checkerboard square behind the image.
const CHECKER_CELL: f32 = 8.0;
const CHECKER_LIGHT: Color32 = Color32::from_gray(204);
const CHECKER_DARK: Color32 = Color32::from_gray(153);
/// How far, in points, a touchpad scrolls with Ctrl held to go one zoom level.
const POINTS_PER_ZOOM_STEP: f32 = 50.0;

/// A 2 × 2 checkerboard that repeats, to stretch over transparent areas.
pub fn checker_texture(ctx: &egui::Context) -> TextureHandle {
    let image = ColorImage::new([2, 2], vec![CHECKER_DARK, CHECKER_LIGHT, CHECKER_LIGHT, CHECKER_DARK]);
    let options = TextureOptions {
        magnification: egui::TextureFilter::Nearest,
        minification: egui::TextureFilter::Nearest,
        wrap_mode: egui::TextureWrapMode::Repeat,
        mipmap_mode: None,
    };
    ctx.load_texture("checker", image, options)
}

/// Fills the rectangle with the checkerboard, in squares of `cell` points.
pub fn paint_checker(painter: &Painter, rect: Rect, cell: f32, checker: &TextureHandle) {
    let uv = Rect::from_min_size(Pos2::ZERO, rect.size() / (cell * 2.0));
    painter.image(checker.id(), rect, uv, Color32::WHITE);
}

/// Shows the image. Returns the pixel under the pointer, while it is over the view.
pub fn show(ui: &mut egui::Ui, view: &mut View, texture: &TiledTexture, checker: &TextureHandle) -> Option<(i64, i64)> {
    let pixels_per_point = ui.ctx().pixels_per_point();
    let viewport = ui.available_rect_before_wrap();
    view.place(viewport, pixels_per_point);
    let pointer = ui.ctx().pointer_hover_pos().filter(|pos| viewport.contains(*pos));
    if let Some(pointer) = pointer {
        zoom_with_wheel(ui, view, pointer);
    }

    let image_size = view.image_size_on_screen();
    let output = egui::ScrollArea::both()
        .auto_shrink(false)
        // Dragging is left to the tools, and to panning.
        .scroll_source(ScrollSource { scroll_bar: true, mouse_wheel: true, ..ScrollSource::NONE })
        .scroll_offset(view.offset)
        .show(ui, |ui| {
            let (_, response) = ui.allocate_exact_size(image_size.max(ui.available_size()), Sense::drag());
            let rect = view.image_rect();
            let painter = ui.painter();
            paint_checker(painter, rect, CHECKER_CELL / pixels_per_point, checker);
            texture.paint(painter, rect);
            response
        });
    view.offset = output.state.offset;

    let response = output.inner;
    let space = ui.input(|input| input.key_down(Key::Space));
    let panning = response.dragged_by(egui::PointerButton::Middle)
        || (space && response.dragged_by(egui::PointerButton::Primary));
    if panning {
        view.pan(response.drag_delta());
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    } else if space && response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::Grab);
    }

    pointer.map(|pointer| view.pixel_at(pointer))
}

/// Zooms a level in or out at each notch of the wheel turned with Ctrl held, and
/// along with a pinch.
fn zoom_with_wheel(ui: &egui::Ui, view: &mut View, pointer: Pos2) {
    let id = ui.id().with("zoom_scroll");
    // What a touchpad scrolled short of a whole step, kept for the next frames.
    let mut leftover: f32 = ui.data(|data| data.get_temp(id).unwrap_or(0.0));
    let mut steps = 0;
    let mut pinch = 1.0;
    ui.input(|input| {
        for event in &input.events {
            match event {
                Event::MouseWheel { unit, delta, modifiers, .. } if modifiers.command => match unit {
                    egui::MouseWheelUnit::Point => {
                        leftover += delta.y;
                        let whole = (leftover / POINTS_PER_ZOOM_STEP).trunc();
                        steps += whole as i32;
                        leftover -= whole * POINTS_PER_ZOOM_STEP;
                    }
                    _ => steps += delta.y.signum() as i32,
                },
                Event::Zoom(factor) => pinch *= factor,
                _ => {}
            }
        }
    });
    ui.data_mut(|data| data.insert_temp(id, leftover));
    for _ in 0..steps.unsigned_abs() {
        view.step_zoom(steps.signum(), pointer);
    }
    if pinch != 1.0 {
        view.set_zoom(view.zoom * pinch, pointer);
    }
}

/// The largest rectangle of the image's proportions that fits in `room`, centered.
pub fn fit(image_size: Vec2, room: Rect) -> Rect {
    let scale = (room.width() / image_size.x).min(room.height() / image_size.y);
    let size = (image_size * scale).max(Vec2::splat(1.0)).round();
    Rect::from_center_size(room.center(), size)
}

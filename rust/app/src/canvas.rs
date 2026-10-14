//! The current image, shown at actual size over a checkerboard.

use eframe::egui::{self, Color32, ColorImage, Painter, Pos2, Rect, Sense, TextureHandle, TextureOptions, Vec2};

/// Side, in pixels, of each checkerboard square behind the image.
const CHECKER_CELL: f32 = 8.0;
const CHECKER_LIGHT: Color32 = Color32::from_gray(204);
const CHECKER_DARK: Color32 = Color32::from_gray(153);

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

pub fn show(ui: &mut egui::Ui, texture: &TextureHandle, checker: &TextureHandle) {
    let pixels_per_point = ui.ctx().pixels_per_point();
    // One image pixel to one screen pixel.
    let size = texture.size_vec2() / pixels_per_point;
    egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
        let (room, _) = ui.allocate_exact_size(size.max(ui.available_size()), Sense::hover());
        let corner = ((room.center() - size / 2.0) * pixels_per_point).round() / pixels_per_point;
        let rect = Rect::from_min_size(corner, size);
        let painter = ui.painter();
        paint_checker(painter, rect, CHECKER_CELL / pixels_per_point, checker);
        painter.image(texture.id(), rect, full_uv(), Color32::WHITE);
    });
}

pub fn full_uv() -> Rect {
    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0))
}

/// The largest rectangle of the image's proportions that fits in `room`, centered.
pub fn fit(image_size: Vec2, room: Rect) -> Rect {
    let scale = (room.width() / image_size.x).min(room.height() / image_size.y);
    let size = (image_size * scale).max(Vec2::splat(1.0)).round();
    Rect::from_center_size(room.center(), size)
}

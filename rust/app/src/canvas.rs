//! The current image, zoomed and scrolled, over a checkerboard. Ctrl and the wheel,
//! or a pinch, zoom on the pointer, and the wheel alone scrolls. The middle button,
//! or the left one while Space is held, drags the image around. The left one alone
//! selects a rectangle or an ellipse, and the selection shows as a tinted area in a dashed outline.

use eframe::egui::{
    self, Color32, ColorImage, CursorIcon, Event, Key, Painter, PointerButton, Pos2, Rect, Sense, Stroke,
    TextureHandle, TextureOptions, Vec2, scroll_area::ScrollSource,
};
use power_paint_core::{Selection, SelectionMode};

use crate::{
    OpenImage,
    frame_select::{FrameSelect, Shape},
    view::View,
};

/// Side, in pixels, of each checkerboard square behind the image.
const CHECKER_CELL: f32 = 8.0;
const CHECKER_LIGHT: Color32 = Color32::from_gray(204);
const CHECKER_DARK: Color32 = Color32::from_gray(153);
/// How far, in points, a touchpad scrolls with Ctrl held to go one zoom level.
const POINTS_PER_ZOOM_STEP: f32 = 50.0;
/// Length, in screen pixels, of the dashes of an outline and of the gaps between them.
const OUTLINE_DASH: f32 = 4.0;
/// What the selection is tinted with, which its outline alone does not always tell from its holes.
const SELECTION_TINT: Color32 = Color32::from_rgba_unmultiplied_const(51, 128, 255, 31);

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

/// Shows the image, and selects a shape with the left button, the way the selection
/// mode says. Returns the pixel under the pointer, while it is over the view.
pub fn show(
    ui: &mut egui::Ui,
    image: &mut OpenImage,
    tool: &mut FrameSelect,
    shape: Shape,
    mode: SelectionMode,
    checker: &TextureHandle,
) -> Option<(i64, i64)> {
    let view = &mut image.view;
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
            image.texture.paint(painter, rect);
            response
        });
    view.offset = output.state.offset;

    let response = output.inner;
    let space = ui.input(|input| input.key_down(Key::Space));
    let panning = response.dragged_by(PointerButton::Middle)
        || (space && response.dragged_by(PointerButton::Primary) && !tool.is_dragging());
    if panning {
        view.pan(response.drag_delta());
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    } else if space && response.hovered() && !tool.is_dragging() {
        ui.ctx().set_cursor_icon(CursorIcon::Grab);
    } else {
        select(ui, &response, view, tool, shape, mode, &mut image.document.selection, space);
        if response.hovered() || tool.is_dragging() {
            ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
        }
    }

    paint_selection(&ui.painter_at(viewport), view, &image.document.selection);
    pointer.map(|pointer| view.pixel_at(pointer))
}

/// Selects the shape the left button frames, a square or a circle while Shift is held.
/// Holding Ctrl when it is pressed adds to the selection, whatever the mode. Alt is
/// left alone, as window managers on Linux move the window with Alt and a drag.
#[allow(clippy::too_many_arguments)]
fn select(
    ui: &egui::Ui,
    response: &egui::Response,
    view: &View,
    tool: &mut FrameSelect,
    shape: Shape,
    mode: SelectionMode,
    selection: &mut Selection,
    space: bool,
) {
    if tool.is_dragging() && ui.input(|input| input.key_pressed(Key::Escape)) {
        tool.cancel(selection);
        return;
    }
    if response.drag_started_by(PointerButton::Primary) && !space {
        let (press, command) = ui.input(|input| (input.pointer.press_origin(), input.modifiers.command));
        if let Some(press) = press {
            let mode = if command { SelectionMode::Add } else { mode };
            tool.press(selection, shape, view.corner_at(press), view.pixel_at(press), mode);
        }
    }
    if response.dragged_by(PointerButton::Primary)
        && response.drag_delta() != Vec2::ZERO
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let shift = ui.input(|input| input.modifiers.shift);
        tool.drag_to(selection, view.corner_at(pointer), shift);
    }
    if response.drag_stopped_by(PointerButton::Primary) {
        tool.release(selection);
    }
}

/// Tints the selection and draws its outline, in lines one screen pixel wide.
fn paint_selection(painter: &Painter, view: &View, selection: &Selection) {
    for span in selection.spans() {
        let min = view.to_screen(span.left as f32, span.top as f32);
        let max = view.to_screen(span.right as f32, span.bottom as f32);
        painter.rect_filled(Rect::from_min_max(min, max), 0.0, SELECTION_TINT);
    }
    for outline in selection.outlines() {
        let points: Vec<Pos2> = outline
            .iter()
            .map(|&(x, y)| view.to_screen(x as f32, y as f32))
            .map(|point| Pos2::new(painter.round_to_pixel_center(point.x), painter.round_to_pixel_center(point.y)))
            .collect();
        paint_outline(painter, &points);
    }
}

/// Draws a closed outline. Black dashes over a white line stay visible on any image.
fn paint_outline(painter: &Painter, points: &[Pos2]) {
    let pixel = 1.0 / painter.pixels_per_point();
    let dash = OUTLINE_DASH * pixel;
    let period = dash * 2.0;
    let white = Stroke::new(pixel, Color32::WHITE);
    let black = Stroke::new(pixel, Color32::BLACK);
    // An edge can run far out of view at a high zoom: only what is in view is drawn.
    let clip = painter.clip_rect().expand(period);
    // How far along the outline each edge starts, so that the dashes carry on from
    // one edge to the next instead of starting again on each.
    let mut travelled = 0.0;
    for (i, &from) in points.iter().enumerate() {
        let to = points[(i + 1) % points.len()];
        let length = from.distance(to);
        if let Some((start, end)) = clip_segment(from, to, clip) {
            let along = |t: f32| from + (to - from) * t;
            painter.line_segment([along(start), along(end)], white);
            let (start, end) = (travelled + start * length, travelled + end * length);
            let mut dash_start = (start / period).floor() * period;
            while dash_start < end {
                let (a, b) = (dash_start.max(start), (dash_start + dash).min(end));
                if a < b {
                    let at = |distance: f32| along((distance - travelled) / length);
                    painter.line_segment([at(a), at(b)], black);
                }
                dash_start += period;
            }
        }
        travelled += length;
    }
}

/// The part of the segment from `from` to `to` inside `clip`, as fractions of the way
/// along it, or nothing if it is all outside.
fn clip_segment(from: Pos2, to: Pos2, clip: Rect) -> Option<(f32, f32)> {
    let delta = to - from;
    let (mut start, mut end) = (0.0_f32, 1.0_f32);
    let sides = [
        (-delta.x, from.x - clip.min.x),
        (delta.x, clip.max.x - from.x),
        (-delta.y, from.y - clip.min.y),
        (delta.y, clip.max.y - from.y),
    ];
    for (towards, room) in sides {
        if towards == 0.0 {
            if room < 0.0 {
                return None;
            }
        } else if towards < 0.0 {
            start = start.max(room / towards);
        } else {
            end = end.min(room / towards);
        }
    }
    (start < end).then_some((start, end))
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

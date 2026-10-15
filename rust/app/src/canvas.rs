//! The current image, zoomed and scrolled, over a checkerboard. Ctrl and the wheel,
//! or a pinch, zoom on the pointer, and the wheel alone scrolls. The middle button,
//! or the left one while Space is held, drags the image around. The left button
//! otherwise works the current tool: the selection tools select a rectangle or an
//! ellipse, or move, resize and rotate the selected pixels from the selection's box,
//! and Move Selection does the same to the selection alone. The selection shows as a
//! tinted area in a dashed outline, with a button by it to go from one to the other.

use eframe::egui::{
    self, Color32, ColorImage, CursorIcon, Event, Key, Painter, PointerButton, Pos2, Rect, Sense, Stroke,
    TextureHandle, TextureOptions, Vec2, emath::GuiRounding, scroll_area::ScrollSource,
};
use power_paint_core::{Selection, SelectionMode};

use crate::{
    OpenImage,
    frame_select::Shape,
    icons::Icons,
    tools::{Tool, Tools},
    transform::{BoxState, DragKind, Transform},
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
/// What the selection is tinted with, which its outline alone does not always tell
/// from its holes. Move Selection tints it more, as a sign that only the outline moves.
const SELECTION_TINT: Color32 = Color32::from_rgba_unmultiplied_const(51, 128, 255, 31);
const MOVE_SELECTION_TINT: Color32 = Color32::from_rgba_unmultiplied_const(51, 128, 255, 64);
/// Side of the handles on the selection's box, in points.
const HANDLE_SIZE: f32 = 6.0;
/// Room between the selection and the button by it, in points.
const BUTTON_GAP: f32 = 4.0;
/// Size of the icon on the button by the selection, and the room around it, in points:
/// larger than the toolbar's, to be easy to find over any image.
const BUTTON_ICON_SIZE: Vec2 = Vec2::splat(24.0);
const BUTTON_PADDING: Vec2 = Vec2::splat(6.0);

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

pub struct Output {
    /// The pixel under the pointer, while it is over the view.
    pub pointer_pixel: Option<(i64, i64)>,
    /// The tool the button by the selection asks for.
    pub tool_requested: Option<Tool>,
}

/// Shows the image, and works the current tool on it.
pub fn show(
    ui: &mut egui::Ui,
    image: &mut OpenImage,
    tools: &mut Tools,
    icons: &mut Icons,
    checker: &TextureHandle,
) -> Output {
    let pixels_per_point = ui.ctx().pixels_per_point();
    let viewport = ui.available_rect_before_wrap();
    image.view.place(viewport, pixels_per_point);
    let pointer = ui.ctx().pointer_hover_pos().filter(|pos| viewport.contains(*pos));
    if let Some(pointer) = pointer {
        zoom_with_wheel(ui, &mut image.view, pointer);
    }

    let image_size = image.view.image_size_on_screen();
    let output = egui::ScrollArea::both()
        .auto_shrink(false)
        // Dragging is left to the tools, and to panning.
        .scroll_source(ScrollSource { scroll_bar: true, mouse_wheel: true, ..ScrollSource::NONE })
        .scroll_offset(image.view.offset)
        .show(ui, |ui| {
            let (_, response) = ui.allocate_exact_size(image_size.max(ui.available_size()), Sense::drag());
            let rect = image.view.image_rect();
            let painter = ui.painter();
            paint_checker(painter, rect, CHECKER_CELL / pixels_per_point, checker);
            let transform = image.transform.as_ref();
            if !transform.is_some_and(|transform| transform.paint_preview(painter, &image.view)) {
                image.texture.paint(painter, rect, 1.0);
            }
            response
        });
    image.view.offset = output.state.offset;

    let response = output.inner;
    let space = ui.input(|input| input.key_down(Key::Space));
    let busy = tools.frame_select.is_dragging() || image.transform.as_ref().is_some_and(Transform::is_dragging);
    let panning = response.dragged_by(PointerButton::Middle)
        || (space && response.dragged_by(PointerButton::Primary) && !busy);
    if panning {
        image.view.pan(response.drag_delta());
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    } else if space && response.hovered() && !busy {
        ui.ctx().set_cursor_icon(CursorIcon::Grab);
    } else {
        use_tool(ui, &response, image, tools, pointer, space);
    }

    let painter = ui.painter_at(viewport);
    let tint = if tools.tool == Tool::MoveSelection { MOVE_SELECTION_TINT } else { SELECTION_TINT };
    paint_selection(&painter, &image.view, &image.document.selection, tint);
    let mut tool_requested = None;
    if let Some(state) = box_state(image, tools) {
        paint_handles(&painter, &image.view, state);
        let idle = !tools.frame_select.is_dragging() && !image.transform.as_ref().is_some_and(Transform::is_dragging);
        if idle {
            tool_requested = switch_button(ui, image, tools, icons, viewport);
        }
    }
    Output { pointer_pixel: pointer.map(|pointer| image.view.pixel_at(pointer)), tool_requested }
}

/// The selection's box, if the tool shows one: Move Selection's moves the outline
/// alone, and the selection tools' moves the selected pixels, except while they draw
/// a new selection.
fn box_state(image: &OpenImage, tools: &Tools) -> Option<BoxState> {
    let shown = tools.tool == Tool::MoveSelection || !tools.frame_select.is_dragging();
    if !shown {
        return None;
    }
    match &image.transform {
        Some(transform) => Some(transform.state()),
        None => image.document.selection.bounds().map(BoxState::around),
    }
}

/// Works the tool with the left button, and with the right one, which rotates the box.
fn use_tool(
    ui: &egui::Ui,
    response: &egui::Response,
    image: &mut OpenImage,
    tools: &mut Tools,
    pointer: Option<Pos2>,
    space: bool,
) {
    let (command, shift) = ui.input(|input| (input.modifiers.command, input.modifiers.shift));
    if ui.input(|input| input.key_pressed(Key::Escape)) {
        if tools.frame_select.is_dragging() {
            tools.frame_select.cancel(&mut image.document.selection);
        }
        if let Some(transform) = image.transform.as_mut().filter(|transform| transform.is_dragging()) {
            image.document.selection = transform.cancel();
        }
    }
    let reach = image.view.pixels_per_screen_point();
    let shape = tools.tool.select_shape();
    // The selection tools draw a new selection with Ctrl, or in a mode other than Replace,
    // wherever they are pressed.
    let draws_anyway = shape.is_some() && (command || tools.selection_mode != SelectionMode::Replace);
    let box_drag_at = |image: &OpenImage, tools: &Tools, point: Pos2| {
        box_state(image, tools)?.drag_at(image.view.point_at(point), reach)
    };

    let started = [PointerButton::Primary, PointerButton::Secondary]
        .into_iter()
        .find(|&button| response.drag_started_by(button));
    if let Some(button) = started
        && !space
        && let Some(press) = ui.input(|input| input.pointer.press_origin())
    {
        let on_box = box_drag_at(image, tools, press);
        let kind = match button {
            PointerButton::Primary if !draws_anyway => on_box,
            // The right button rotates the box: anywhere with Move Selection, which has nothing else to do.
            PointerButton::Secondary if on_box.is_some() || tools.tool == Tool::MoveSelection => {
                box_state(image, tools).map(|_| DragKind::Rotate)
            }
            _ => None,
        };
        if let Some(kind) = kind {
            if image.transform.is_none() {
                image.transform = Transform::begin(ui.ctx(), &image.document, shape.is_some());
            }
            if let Some(transform) = &mut image.transform {
                transform.press(kind, image.view.point_at(press));
            }
        } else if button == PointerButton::Primary
            && let Some(shape) = shape
        {
            start_selecting(image, tools, shape, press, command);
        }
    }

    let dragged = response.dragged() && response.drag_delta() != Vec2::ZERO;
    if dragged && let Some(at) = response.interact_pointer_pos() {
        if tools.frame_select.is_dragging() {
            tools.frame_select.drag_to(&mut image.document.selection, image.view.corner_at(at), shift);
        }
        if let Some(transform) = image.transform.as_mut()
            && let Some(selection) = transform.drag_to(image.view.point_at(at), shift)
        {
            image.document.selection = selection;
        }
    }

    if response.drag_stopped() {
        tools.frame_select.release(&mut image.document.selection);
        if let Some(transform) = image.transform.as_mut().filter(|transform| transform.is_dragging()) {
            let clicked = transform.drag_kind().is_some() && response.drag_stopped_by(PointerButton::Primary);
            let (moved, placed) = transform.release();
            if let Some(pixels) = placed {
                image.document.active_mut().pixels = pixels;
                image.document.unsaved = true;
                image.refresh(ui.ctx());
            }
            // A click on the box of a selection tool does what a click with the tool does.
            if !moved
                && clicked
                && let Some(shape) = shape
                && let Some(press) = ui.input(|input| input.pointer.press_origin())
            {
                start_selecting(image, tools, shape, press, command);
                tools.frame_select.release(&mut image.document.selection);
            }
        }
    }

    // The pointer shows what a press would do.
    let dragging_box = image.transform.as_ref().and_then(Transform::drag_kind);
    let angle = box_state(image, tools).map_or(0.0, |state| state.angle);
    let hovered_box = pointer.filter(|_| !draws_anyway && !tools.frame_select.is_dragging());
    if let Some(kind) = dragging_box.or_else(|| hovered_box.and_then(|at| box_drag_at(image, tools, at))) {
        ui.ctx().set_cursor_icon(kind.cursor(angle));
    } else if shape.is_some() && (response.hovered() || tools.frame_select.is_dragging()) {
        ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
    }
}

/// A selection tool starts drawing a new selection, which ends any transform. Holding
/// Ctrl when it is pressed adds to the selection, whatever the mode. Alt is left
/// alone, as window managers on Linux move the window with Alt and a drag.
fn start_selecting(image: &mut OpenImage, tools: &mut Tools, shape: Shape, press: Pos2, command: bool) {
    image.transform = None;
    let mode = if command { SelectionMode::Add } else { tools.selection_mode };
    let view = &image.view;
    tools.frame_select.press(&mut image.document.selection, shape, view.corner_at(press), view.pixel_at(press), mode);
}

/// Tints the selection and draws its outline, in lines one screen pixel wide.
fn paint_selection(painter: &Painter, view: &View, selection: &Selection, tint: Color32) {
    for span in selection.spans(view.visible_pixels()) {
        let min = view.to_screen(span.left as f32, span.top as f32);
        let max = view.to_screen(span.right as f32, span.bottom as f32);
        painter.rect_filled(Rect::from_min_max(min, max), 0.0, tint);
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

/// A handle on each corner of the box and in the middle of each edge.
fn paint_handles(painter: &Painter, view: &View, state: BoxState) {
    let pixel = 1.0 / painter.pixels_per_point();
    for (x, y) in state.handles() {
        let center = view.to_screen(x as f32, y as f32).round_to_pixels(painter.pixels_per_point());
        let handle = Rect::from_center_size(center, Vec2::splat(HANDLE_SIZE));
        painter.rect_filled(handle, 0.0, Color32::WHITE);
        painter.rect_stroke(handle, 0.0, Stroke::new(pixel, Color32::BLACK), egui::StrokeKind::Inside);
    }
}

/// Shows a button just right of the selection's top right corner, or just inside it
/// where the view ends there: Move Selection with the selection tools, and back to
/// the selection tool used last with Move Selection. Returns the tool it asks for.
fn switch_button(ui: &egui::Ui, image: &OpenImage, tools: &Tools, icons: &mut Icons, viewport: Rect) -> Option<Tool> {
    let bounds = image.document.selection.bounds()?;
    let target = if tools.tool == Tool::MoveSelection { tools.last_select_tool } else { Tool::MoveSelection };
    let tooltip = match target {
        Tool::MoveSelection => format!("{} ({})", target.name(), ui.ctx().format_shortcut(&target.key())),
        _ => format!("Back to {} ({})", target.name(), ui.ctx().format_shortcut(&target.key())),
    };
    let corner = image.view.to_screen(bounds.right as f32, bounds.top as f32);
    let size = BUTTON_ICON_SIZE + BUTTON_PADDING * 2.0;
    let mut place = Pos2::new(corner.x + BUTTON_GAP, corner.y);
    if place.x + size.x > viewport.right() {
        place = Pos2::new(corner.x - size.x - BUTTON_GAP, corner.y + BUTTON_GAP);
    }
    let place = place.clamp(viewport.min, (viewport.max - size).max(viewport.min));
    let texture = icons.get_sized(ui.ctx(), target.icon(), BUTTON_ICON_SIZE.x).clone();
    let tint = ui.visuals().strong_text_color();
    let area = egui::Area::new(ui.id().with("switch_selection_tool")).fixed_pos(place).show(ui.ctx(), |ui| {
        ui.spacing_mut().button_padding = BUTTON_PADDING;
        let icon = egui::Image::new(&texture).fit_to_exact_size(BUTTON_ICON_SIZE).tint(tint);
        ui.add(egui::Button::image(icon)).on_hover_text(tooltip).on_hover_cursor(CursorIcon::PointingHand).clicked()
    });
    area.inner.then_some(target)
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

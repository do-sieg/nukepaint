//! The current image, zoomed and scrolled, over a checkerboard. Ctrl and the wheel,
//! or a pinch, zoom on the pointer, and the wheel alone scrolls. The middle button,
//! or the left one with the Pan tool or while Space is held, drags the image around.
//! The left button otherwise works the current tool: the selection tools select a
//! rectangle or an ellipse, or move, resize and rotate the selected pixels from the
//! selection's box, and Move Selection does the same to the selection alone. The Zoom
//! tool zooms in on a left click or on the area a left drag frames, and out on a right
//! click. The Paintbrush, the Pencil and the Eraser draw a line along a left drag, the
//! Blur, the Sharpen and the Jumble retouch the pixels along it, and the Spray leaves
//! dots for as long as the left button is held, on the selected pixels alone if any are. Unless their selection controls are off, they resize or move the selection from a handle
//! of its box, or the one out from its bottom right corner, or rotate it from around
//! the box. The shape tools draw their shape, which their handles then change until it
//! is finished. The selection shows as a tinted area in a dashed outline, with a button by it: with the
//! selection tools and Move Selection, to go from one to the other, and with the other
//! tools, to deselect. The grid shows over the image; while snapping to it, the
//! selection tools put their corners on its cells' corners.

use eframe::egui::{
    self, Color32, ColorImage, CursorIcon, Event, Key, Painter, PointerButton, Pos2, Rect, Sense, Stroke,
    TextureHandle, TextureOptions, Vec2, emath::GuiRounding, scroll_area::ScrollSource,
};
use power_paint_core::{
    BrushStroke, FloodFill, PixelRect, RetouchStroke, Rgba, RgbaImage, Selection, SelectionMask, SelectionMode,
    ShapeKind, Spray, hard_tip,
};

use crate::{
    OpenImage,
    grid::{self, Cells, Grid},
    icons::{self, Icon, Icons},
    lasso::{Lasso, LassoKind},
    shapes::{self, Handle, ShapeEdit},
    tools::{RightClick, SampleSource, Tool, Tools, ZoomDrag},
    transform::{BoxState, DragKind, MOVE_HANDLE_SIZE, PIVOT_RADIUS, ROTATE_REACH, Transform},
    view::View,
};

const CHECKER_LIGHT: Color32 = Color32::from_gray(204);
const CHECKER_DARK: Color32 = Color32::from_gray(153);
/// How far, in points, a touchpad scrolls with Ctrl held to go one zoom level.
const POINTS_PER_ZOOM_STEP: f32 = 50.0;
/// Length, in screen pixels, of the dashes of an outline and of the gaps between them.
const OUTLINE_DASH: f32 = 4.0;
/// How far, in points, the Zoom tool must be dragged to frame an area rather than click.
const ZOOM_DRAG_THRESHOLD: f32 = 4.0;
/// What the selection is tinted with, which its outline alone does not always tell
/// from its holes. Move Selection tints it more, as a sign that only the outline moves.
const SELECTION_TINT: Color32 = Color32::from_rgba_unmultiplied_const(51, 128, 255, 31);
const MOVE_SELECTION_TINT: Color32 = Color32::from_rgba_unmultiplied_const(51, 128, 255, 64);
/// Side of the handles on the selection's box, in points, and of the arrows on the
/// one that moves it.
const HANDLE_SIZE: f32 = 8.0;
const MOVE_HANDLE_ARROWS: f32 = 10.0;
/// Radius of the handles on Polygon Select's corners, in points, and how close to one,
/// or to an edge between two, a press must be to grab it. A click that close to the
/// first corner closes the outline.
const CORNER_RADIUS: f32 = 4.0;
const CORNER_REACH: f32 = 6.0;
const EDGE_REACH: f32 = 4.0;
/// Room between the selection and the button by it, in points.
const BUTTON_GAP: f32 = 4.0;
/// Size of the icon on the button by the selection, and the room around it, in points:
/// larger than the toolbar's, to be easy to find over any image.
const BUTTON_ICON_SIZE: Vec2 = Vec2::splat(24.0);
const BUTTON_PADDING: Vec2 = Vec2::splat(6.0);
/// Smallest a tip is on screen, in screen pixels, to be outlined along its edge rather
/// than ringed from outside.
const OUTLINED_TIP: f32 = 8.0;
/// Size of the cursors of the drawing tools, in points, as their files are drawn for,
/// and the point of theirs that draws, from its top left.
const CURSOR_SIZE: Vec2 = Vec2::splat(24.0);
const DRAWING_HOTSPOT: Vec2 = Vec2::new(1.0, 23.0);
const SPRAY_HOTSPOT: Vec2 = Vec2::new(6.0, 6.0);
const RETOUCH_HOTSPOT: Vec2 = Vec2::new(12.0, 3.0);
const JUMBLE_HOTSPOT: Vec2 = Vec2::new(4.0, 4.0);
const COLOR_PICKER_HOTSPOT: Vec2 = Vec2::new(5.0, 19.0);
const PAINT_BUCKET_HOTSPOT: Vec2 = Vec2::new(20.0, 21.0);
const MAGIC_WAND_HOTSPOT: Vec2 = Vec2::new(8.0, 8.0);
/// Longest a frame counts for while spraying, in seconds, so that a stall does not
/// leave a burst of dots.
const MAX_SPRAY_FRAME: f64 = 0.1;

/// What a drawing tool is drawing on the active layer, until the button is let go.
pub enum Drawing {
    Line(BrushStroke),
    Spray(Spray),
    Retouch(RetouchStroke),
}

impl Drawing {
    /// Whether it changed any pixel yet.
    pub fn changed(&self) -> bool {
        match self {
            Self::Line(stroke) => stroke.changed(),
            Self::Spray(spray) => spray.changed(),
            Self::Retouch(stroke) => stroke.changed(),
        }
    }

    /// Takes it on to the point `at` of the image, after `seconds` went by. Returns the
    /// area it changed, if any of it is on the image.
    fn go_to(&mut self, image: &mut RgbaImage, at: (f64, f64), seconds: f64) -> Option<PixelRect> {
        match self {
            Self::Line(stroke) => stroke.draw_to(image, at),
            Self::Spray(spray) => spray.spray(image, at, seconds),
            Self::Retouch(stroke) => stroke.draw_to(image, at),
        }
    }
}
/// The point of the rotate cursor that is on the pointer, its middle.
const ROTATE_HOTSPOT: Vec2 = Vec2::new(12.0, 12.0);

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
    /// Set when the Deselect button by the selection is clicked.
    pub deselect_requested: bool,
}

/// Shows the image, and works the current tool on it.
pub fn show(
    ui: &mut egui::Ui,
    image: &mut OpenImage,
    tools: &mut Tools,
    icons: &mut Icons,
    checker: &TextureHandle,
    checker_size: u32,
    grid: &Grid,
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
            paint_checker(painter, rect, checker_size as f32 / pixels_per_point, checker);
            let transform = image.transform.as_ref();
            let previewed = transform.is_some_and(|transform| transform.paint_preview(painter, &image.view, tools.resampling));
            if !previewed {
                image.texture.paint(painter, rect);
            }
            grid::paint_lines(painter, &image.view, grid);
            response
        });
    image.view.offset = output.state.offset;

    let response = output.inner;
    let space = ui.input(|input| input.key_down(Key::Space));
    let busy = tools.frame_select.is_dragging() || tools.zoom_drag.is_some() || image.is_dragging();
    let hand = space || tools.tool == Tool::Pan;
    // Where the pointer is over the image while it works a drawing tool, or the Color Picker.
    let (mut drawing_at, mut picking_at, mut filling_at, mut wand_at) = (None, None, None, None);
    let panning = response.dragged_by(PointerButton::Middle)
        || (hand && response.dragged_by(PointerButton::Primary) && !busy);
    if panning {
        image.view.pan(response.drag_delta());
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    } else if hand && response.hovered() && !busy {
        ui.ctx().set_cursor_icon(CursorIcon::Grab);
    } else if tools.tool == Tool::Zoom {
        use_zoom(ui, &response, &mut image.view, tools, space);
    } else if tools.tool.draws() {
        drawing_at = pointer.filter(|_| use_drawing_tool(ui, &response, image, tools, space));
    } else if tools.tool == Tool::ColorPicker {
        picking_at = pointer.filter(|_| use_color_picker(&response, image, tools, space));
    } else if tools.tool == Tool::PaintBucket {
        filling_at = pointer.filter(|_| use_paint_bucket(ui, &response, image, tools, space));
    } else if tools.tool == Tool::MagicWand {
        wand_at = pointer.filter(|_| use_magic_wand(ui, &response, image, tools, space));
    } else if tools.tool.shape().is_some() {
        use_shape_tool(ui, &response, image, tools, pointer, space);
    } else if tools.tool != Tool::Pan {
        use_tool(ui, &response, image, tools, pointer, space);
    }

    let painter = ui.painter_at(viewport);
    let tint = if tools.tool == Tool::MoveSelection { MOVE_SELECTION_TINT } else { SELECTION_TINT };
    paint_selection(&painter, &image.view, &image.document.selection, tint);
    paint_lasso(&painter, image, tools);
    paint_shape(&painter, image, tools);
    if let Some(drag) = tools.zoom_drag.as_ref().filter(|drag| drag.framing) {
        paint_zoom_frame(&painter, &image.view, drag);
    }
    if let Some(at) = drawing_at {
        paint_tip(&painter, &image.view, at, tools);
        let (cursor, hotspot) = match tools.tool {
            Tool::Paintbrush => (icons::CURSOR_PAINTBRUSH, DRAWING_HOTSPOT),
            Tool::Eraser => (icons::CURSOR_ERASER, DRAWING_HOTSPOT),
            Tool::Spray => (icons::CURSOR_SPRAY, SPRAY_HOTSPOT),
            Tool::Blur => (icons::CURSOR_BLUR, RETOUCH_HOTSPOT),
            Tool::Sharpen => (icons::CURSOR_SHARPEN, RETOUCH_HOTSPOT),
            Tool::Jumble => (icons::CURSOR_JUMBLE, JUMBLE_HOTSPOT),
            _ => (icons::CURSOR_PENCIL, DRAWING_HOTSPOT),
        };
        paint_cursor(ui, icons, cursor, hotspot, at);
    }
    if let Some(at) = picking_at {
        paint_cursor(ui, icons, icons::CURSOR_COLOR_PICKER, COLOR_PICKER_HOTSPOT, at);
    }
    if let Some(at) = filling_at {
        paint_cursor(ui, icons, icons::CURSOR_PAINT_BUCKET, PAINT_BUCKET_HOTSPOT, at);
    }
    if let Some(at) = wand_at {
        paint_cursor(ui, icons, icons::CURSOR_MAGIC_WAND, MAGIC_WAND_HOTSPOT, at);
    }
    let box_shown = box_state(image, tools);
    if let Some(state) = box_shown {
        paint_handles(&painter, &image.view, state, icons);
    }
    let (mut tool_requested, mut deselect_requested) = (None, false);
    let idle = !tools.frame_select.is_dragging() && tools.zoom_drag.is_none() && !image.is_dragging();
    let button_shown = !tools.tool.draws() || tools.selection_controls;
    if idle && button_shown && let Some(clicked) = selection_button(ui, image, tools, icons, viewport, box_shown) {
        match clicked {
            SelectionButton::Deselect => deselect_requested = true,
            SelectionButton::Switch(tool) => tool_requested = Some(tool),
        }
    }
    // The pointer egui has to turn a box is drawn here as a turning arrow.
    if ui.ctx().output(|output| output.cursor_icon) == CursorIcon::Alias
        && let Some(at) = ui.ctx().pointer_latest_pos()
    {
        paint_cursor(ui, icons, icons::CURSOR_ROTATE, ROTATE_HOTSPOT, at);
    }
    let pointer_pixel = pointer.map(|pointer| image.view.pixel_at(pointer));
    Output { pointer_pixel, tool_requested, deselect_requested }
}

/// The selection's box, if the tool shows one: Move Selection's moves the outline
/// alone, and the selection tools' moves the selected pixels, except while they draw
/// a new selection. The drawing tools' resizes the outline by its handles, moves it by
/// the one out from its bottom right corner, and rotates it from around it.
fn box_state(image: &OpenImage, tools: &Tools) -> Option<BoxState> {
    let shown = tools.tool == Tool::MoveSelection
        || (tools.tool.draws() && tools.selection_controls)
        || (tools.tool.has_box() && !tools.frame_select.is_dragging() && image.lasso.is_none());
    if !shown {
        return None;
    }
    match &image.transform {
        Some(transform) => Some(transform.state()),
        None => BoxState::of(&image.document.selection),
    }
}

/// Works the tool with the left button, and with the right one, which rotates the box.
/// Polygon Select puts down a corner at each click instead, and once its outline is
/// closed, a press on a corner moves it, one on an edge adds a corner there, and a
/// right click on a corner removes it, before the box has the press.
fn use_tool(
    ui: &egui::Ui,
    response: &egui::Response,
    image: &mut OpenImage,
    tools: &mut Tools,
    pointer: Option<Pos2>,
    space: bool,
) {
    let (command, shift) = ui.input(|input| (input.modifiers.command, input.modifiers.shift));
    // An outline being drawn is given up once anything else changes the selection.
    if image.lasso.as_ref().is_some_and(|lasso| !lasso.is_current(&image.document)) {
        image.lasso = None;
    }
    if ui.input(|input| input.key_pressed(Key::Escape)) {
        if tools.frame_select.is_dragging() {
            tools.frame_select.cancel(&mut image.document.selection);
        }
        if let Some(transform) = image.transform.as_mut().filter(|transform| transform.is_dragging()) {
            image.document.selection = transform.cancel();
        }
        if let Some(polygon) = &mut image.polygon {
            polygon.cancel(&mut image.document);
        }
        image.cancel_lasso();
    }
    let drawing_polygon = image.lasso.as_ref().is_some_and(|lasso| lasso.kind == LassoKind::Polygon);
    if drawing_polygon {
        if ui.input(|input| input.key_pressed(Key::Enter)) {
            close_lasso(image, tools);
        } else if ui.input(|input| input.key_pressed(Key::Backspace)) {
            let lasso = image.lasso.as_mut().expect("drawing");
            if lasso.corners().len() == 1 {
                image.cancel_lasso();
            } else {
                lasso.remove_last(&mut image.document);
            }
        }
    }
    if let Some(lasso) = image.lasso.as_mut().filter(|lasso| lasso.kind == LassoKind::Polygon) {
        lasso.reach(&mut image.document, pointer.map(|at| corner_at(&image.view, at, tools.snap)));
    }
    let drawing_polygon = image.lasso.as_ref().is_some_and(|lasso| lasso.kind == LassoKind::Polygon);
    let reach = image.view.pixels_per_screen_point();
    let has_box = tools.tool.has_box();
    // The selection tools draw a new selection with Ctrl, or in a mode other than Replace,
    // wherever they are pressed.
    let draws_anyway = has_box && (command || tools.selection_mode != SelectionMode::Replace);
    let box_drag_at = |image: &OpenImage, tools: &Tools, point: Pos2| {
        box_state(image, tools)?.drag_at(image.view.point_at(point), reach)
    };
    // Ctrl on an edge or corner resizes the box about its middle, rather than drawing.
    let takes_box = |kind: &DragKind| !draws_anyway || (command && matches!(kind, DragKind::Resize(_)));
    // Polygon Select's corners and edges, unless Ctrl draws a new outline.
    let polygon_hit = |image: &OpenImage, tools: &Tools, at: Pos2| {
        let corners = polygon_corners(image, tools).filter(|_| !command)?;
        polygon_hit_at(&image.view, corners, at)
    };

    let started = [PointerButton::Primary, PointerButton::Secondary]
        .into_iter()
        .find(|&button| response.drag_started_by(button));
    if let Some(button) = started
        && !space
        && let Some(press) = ui.input(|input| input.pointer.press_origin())
    {
        let on_box = box_drag_at(image, tools, press).filter(takes_box);
        let on_handle = on_box.is_some_and(|kind| kind != DragKind::Move);
        let hit = polygon_hit(image, tools, press);
        let corner = corner_at(&image.view, press, tools.snap);
        let kind = match button {
            PointerButton::Primary => on_box,
            // The right button rotates the box: anywhere with Move Selection, which has nothing else to do.
            PointerButton::Secondary if on_box.is_some() || tools.tool == Tool::MoveSelection => {
                box_state(image, tools).map(|_| DragKind::Rotate)
            }
            _ => None,
        };
        match (button, hit) {
            (PointerButton::Primary, _) if drawing_polygon => put_corner(image, tools, press),
            (PointerButton::Primary, Some(PolygonHit::Corner(index))) => {
                image.transform = None;
                image.polygon.as_mut().expect("hit").press_corner(&image.document, index);
            }
            (PointerButton::Primary, Some(PolygonHit::Edge(index))) if !on_handle => {
                image.transform = None;
                image.polygon.as_mut().expect("hit").press_edge(&mut image.document, index, corner);
            }
            (PointerButton::Secondary, Some(PolygonHit::Corner(index))) => {
                image.transform = None;
                let polygon = image.polygon.as_mut().expect("hit");
                if let Some(corners) = polygon.remove(&mut image.document, index) {
                    crate::commit_selection(&mut image.document, tools.tool.name(), tools.tool.icon());
                    polygon.record(&image.document, corners);
                }
            }
            _ if drawing_polygon => {}
            _ => {
                if let Some(kind) = kind {
                    if image.transform.is_none() {
                        image.transform = Transform::begin(ui.ctx(), &image.document, has_box);
                    }
                    if let Some(transform) = &mut image.transform {
                        transform.press(kind, image.view.point_at(press), reach);
                    }
                } else if button == PointerButton::Primary && has_box {
                    start_selecting(image, tools, press, command);
                }
            }
        }
    }

    let dragged = response.dragged() && response.drag_delta() != Vec2::ZERO;
    if dragged && let Some(at) = response.interact_pointer_pos() {
        if tools.frame_select.is_dragging() {
            let corner = corner_at(&image.view, at, tools.snap);
            tools.frame_select.drag_to(&mut image.document.selection, corner, shift);
        }
        if let Some(lasso) = image.lasso.as_mut().filter(|lasso| lasso.kind == LassoKind::Freehand) {
            lasso.add(&mut image.document, image.view.corner_at(at));
        }
        if let Some(polygon) = image.polygon.as_mut().filter(|polygon| polygon.is_dragging()) {
            polygon.drag_to(&mut image.document, corner_at(&image.view, at, tools.snap));
        }
        if let Some(transform) = image.transform.as_mut()
            && let Some(selection) = transform.drag_to(image.view.point_at(at), shift, command, tools.snap)
        {
            image.document.selection = selection;
        }
    }

    if response.drag_stopped() {
        release_selecting(image, tools);
        if let Some(polygon) = &mut image.polygon
            && let Some((_, corners)) = polygon.release()
        {
            crate::commit_selection(&mut image.document, tools.tool.name(), tools.tool.icon());
            polygon.record(&image.document, corners);
        }
        // A double click closes Polygon Select's outline.
        if drawing_polygon && ui.input(|input| input.pointer.button_double_clicked(PointerButton::Primary)) {
            close_lasso(image, tools);
        }
        if let Some(transform) = image.transform.as_mut().filter(|transform| transform.is_dragging()) {
            // Not of the centre of rotation, which is left where it is let go.
            let clicked = transform.drag_kind().is_some_and(|kind| kind != DragKind::Pivot)
                && response.drag_stopped_by(PointerButton::Primary);
            let (moved, placed) = transform.release(tools.resampling);
            if moved {
                commit_transform(ui.ctx(), image, placed);
            }
            // A click on the box of a selection tool does what a click with the tool does.
            if !moved
                && clicked
                && has_box
                && let Some(press) = ui.input(|input| input.pointer.press_origin())
            {
                start_selecting(image, tools, press, command);
                release_selecting(image, tools);
            }
        }
    }

    // The pointer shows what a press would do, in the order a press goes by.
    let dragging_box = image.transform.as_ref().and_then(Transform::drag_kind);
    let angle = box_state(image, tools).map_or(0.0, |state| state.angle);
    let hovered = pointer.filter(|_| !tools.frame_select.is_dragging() && !image.is_dragging());
    let hovered_kind = hovered.and_then(|at| box_drag_at(image, tools, at)).filter(takes_box);
    let hovered_hit = hovered.and_then(|at| polygon_hit(image, tools, at));
    let cursor = if let Some(kind) = dragging_box {
        Some(kind.cursor(angle))
    } else if image.polygon.as_ref().is_some_and(|polygon| polygon.is_dragging()) {
        Some(CursorIcon::Grabbing)
    } else if let Some(PolygonHit::Corner(_)) = hovered_hit {
        Some(CursorIcon::Grab)
    } else if let Some(kind) = hovered_kind.filter(|&kind| kind != DragKind::Move) {
        Some(kind.cursor(angle))
    } else if let Some(PolygonHit::Edge(_)) = hovered_hit {
        Some(CursorIcon::Copy)
    } else if let Some(kind) = hovered_kind {
        Some(kind.cursor(angle))
    } else {
        let selecting = tools.frame_select.is_dragging() || image.lasso.is_some();
        (has_box && (response.hovered() || selecting)).then_some(CursorIcon::Crosshair)
    };
    if let Some(cursor) = cursor {
        ui.ctx().set_cursor_icon(cursor);
    }
}

/// What of Polygon Select's corners and edges a press is on.
#[derive(Clone, Copy)]
enum PolygonHit {
    Corner(usize),
    /// The edge from the corner at that index to the next one.
    Edge(usize),
}

/// Polygon Select's corners, if they can be changed: with the tool, while no outline is
/// being drawn, and the selection is as they left it.
fn polygon_corners<'a>(image: &'a OpenImage, tools: &Tools) -> Option<&'a [(i32, i32)]> {
    if tools.tool != Tool::PolygonSelect || image.lasso.is_some() {
        return None;
    }
    image.polygon.as_ref()?.corners(&image.document)
}

/// The corner nearest to a point of the screen, or else the edge, if any is close enough.
fn polygon_hit_at(view: &View, corners: &[(i32, i32)], at: Pos2) -> Option<PolygonHit> {
    let screen: Vec<Pos2> = corners.iter().map(|&(x, y)| view.to_screen(x as f32, y as f32)).collect();
    let nearest = |distances: &mut dyn Iterator<Item = (usize, f32)>, reach: f32| {
        distances.filter(|&(_, distance)| distance <= reach).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(index, _)| index)
    };
    let mut to_corners = screen.iter().map(|corner| corner.distance(at)).enumerate();
    if let Some(index) = nearest(&mut to_corners, CORNER_REACH) {
        return Some(PolygonHit::Corner(index));
    }
    let mut to_edges = (0..screen.len()).map(|index| {
        (index, distance_to_segment(at, screen[index], screen[(index + 1) % screen.len()]))
    });
    nearest(&mut to_edges, EDGE_REACH).map(PolygonHit::Edge)
}

/// How far a point of the screen is from the nearest point of the segment from `a` to `b`.
fn distance_to_segment(at: Pos2, a: Pos2, b: Pos2) -> f32 {
    let along = (at - a).dot(b - a) / (b - a).length_sq().max(f32::EPSILON);
    (a + (b - a) * along.clamp(0.0, 1.0)).distance(at)
}

/// Polygon Select puts down a corner where it is pressed, or closes its outline on its
/// first corner once it has three.
fn put_corner(image: &mut OpenImage, tools: &Tools, press: Pos2) {
    let corner = corner_at(&image.view, press, tools.snap);
    let Some(lasso) = &mut image.lasso else {
        return;
    };
    let &first = lasso.corners().first().expect("an outline has a corner");
    let first_on_screen = image.view.to_screen(first.0 as f32, first.1 as f32);
    let on_first = corner == first || first_on_screen.distance(press) <= CORNER_REACH;
    if lasso.corners().len() >= 3 && on_first {
        close_lasso(image, tools);
    } else {
        lasso.add(&mut image.document, corner);
    }
}

/// Closes the outline a lasso has drawn, which is a step if it changed the selection.
/// Polygon Select's corners can then be changed.
fn close_lasso(image: &mut OpenImage, tools: &Tools) {
    let Some(lasso) = image.lasso.take() else {
        return;
    };
    let closed = lasso.close(&mut image.document);
    if closed.before.is_some() {
        crate::commit_selection(&mut image.document, tools.tool.name(), tools.tool.icon());
    }
    image.polygon = closed.polygon.map(|mut polygon| {
        polygon.record(&image.document, closed.corners);
        polygon
    });
}

/// What of a shape a press is on.
#[derive(Clone, Copy)]
enum ShapeHit {
    Handle(Handle),
    /// The Polygon's edge from the corner at that index to the next one.
    Edge(usize),
}

/// Works a shape tool: a drag draws its shape, and the Polygon puts down a corner at
/// each click, in the primary colour, or in the secondary one with the right button
/// unless that picks a colour as the Color Picker does. Once drawn, a press on one of
/// the shape's handles, on the Polygon's edges, on the shape itself or around it
/// changes it instead, unless Ctrl is held, and a right click on a Polygon's corner
/// removes it.
/// Enter and Escape finish it. Escape gives up what is being drawn or dragged instead,
/// and Backspace takes back the Polygon's last corner.
fn use_shape_tool(
    ui: &egui::Ui,
    response: &egui::Response,
    image: &mut OpenImage,
    tools: &mut Tools,
    pointer: Option<Pos2>,
    space: bool,
) {
    let Some(kind) = tools.tool.shape() else {
        return;
    };
    let ctx = ui.ctx();
    let (name, icon) = (tools.tool.name(), Some(tools.tool.icon().name()));
    let (command, shift) = ui.input(|input| (input.modifiers.command, input.modifiers.shift));
    // A shape being drawn is given up once anything else changes the layers.
    if image.shape.as_ref().is_some_and(|shape| shape.is_drawing() && !shape.is_current(&image.document)) {
        image.drop_shape();
    }
    let drawing_polygon = |image: &OpenImage| {
        image.shape.as_ref().is_some_and(|shape| shape.is_drawing() && shape.kind() == ShapeKind::Polygon)
    };
    let (escape, enter, backspace) = ui.input(|input| {
        (input.key_pressed(Key::Escape), input.key_pressed(Key::Enter), input.key_pressed(Key::Backspace))
    });
    if escape {
        if image.shape.as_ref().is_some_and(|shape| shape.dragged_handle().is_some()) {
            let area = image.shape.as_mut().and_then(ShapeEdit::cancel_drag);
            image.show_shape(area);
        } else {
            image.drop_shape();
        }
    }
    if enter {
        if drawing_polygon(image) {
            finish_shape(ctx, image, tools);
        } else if !image.is_dragging() {
            image.drop_shape();
        }
    }
    if backspace && drawing_polygon(image) {
        let shape = image.shape.as_mut().expect("drawing");
        if shape.corners().len() == 1 {
            image.drop_shape();
        } else {
            let area = shape.remove_last();
            image.show_shape(area);
        }
    }
    // Another colour, size or antialiasing draws the shape again that way.
    if !image.is_dragging()
        && let Some(shape) = image.shape.as_mut()
    {
        let style = tools.shape_style(shape.secondary);
        if shape.is_drawing() {
            let area = shape.restyle_draft(style);
            image.show_shape(area);
        } else if shape.kind() == kind && shape.restyle(&mut image.document, style) {
            image.refresh(ctx);
        }
    }
    if drawing_polygon(image) {
        let to = pointer.map(|at| shape_pixel(&image.view, at));
        let area = image.shape.as_mut().and_then(|shape| shape.reach(to));
        image.show_shape(area);
    }
    // Ctrl draws another shape, wherever it is pressed.
    let hit_at = |image: &OpenImage, at: Pos2| if command { None } else { shape_hit_at(image, kind, at) };
    let picking = tools.right_click == RightClick::PickColor;

    let started = [PointerButton::Primary, PointerButton::Secondary]
        .into_iter()
        .find(|&button| response.drag_started_by(button));
    if let Some(button) = started
        && !space
        && let Some(press) = ui.input(|input| input.pointer.press_origin())
    {
        let (at, point) = (shape_pixel(&image.view, press), image.view.point_at(press));
        let polygon_open = drawing_polygon(image);
        let hit = if polygon_open { None } else { hit_at(image, press) };
        match (button, hit) {
            (PointerButton::Primary, _) if polygon_open => put_shape_corner(ctx, image, tools, press),
            _ if polygon_open => {}
            (PointerButton::Primary, Some(ShapeHit::Edge(index))) => {
                let area = image.shape.as_mut().and_then(|shape| shape.press_edge(&image.document, index, point));
                image.show_shape(area);
            }
            (PointerButton::Primary, Some(ShapeHit::Handle(handle))) => {
                if let Some(shape) = &mut image.shape {
                    shape.press(&image.document, handle, point);
                }
            }
            (PointerButton::Secondary, Some(ShapeHit::Handle(Handle::Point(index)))) if kind == ShapeKind::Polygon => {
                if let Some(shape) = &mut image.shape
                    && shape.remove_corner(&mut image.document, index, name, icon)
                {
                    image.refresh(ctx);
                }
            }
            (PointerButton::Secondary, _) if picking => {}
            _ => {
                // Which leaves the shape drawn before as it is.
                image.drop_shape();
                let secondary = button == PointerButton::Secondary;
                let style = tools.shape_style(secondary);
                let (shape, area) = ShapeEdit::begin(&image.document, kind, at, style, secondary);
                image.shape = Some(shape);
                image.show_shape(area);
            }
        }
    }
    if picking
        && !space
        && response.dragged_by(PointerButton::Secondary)
        && let Some(at) = response.interact_pointer_pos()
        && let Some(color) = color_at(image, at)
    {
        tools.colors.pick(color, false);
    }

    let dragged = response.dragged() && response.drag_delta() != Vec2::ZERO;
    if dragged && let Some(at) = response.interact_pointer_pos() {
        let (pixel, point) = (shape_pixel(&image.view, at), image.view.point_at(at));
        let area = image.shape.as_mut().and_then(|shape| match shape.is_drawing() {
            true => shape.draw_to(pixel, shift),
            false => shape.drag_to(point, shift),
        });
        image.show_shape(area);
    }

    if response.drag_stopped() {
        let drawing = image.shape.as_ref().filter(|shape| shape.is_drawing()).map(ShapeEdit::kind);
        // A double click finishes the Polygon.
        let double_click = ui.input(|input| input.pointer.button_double_clicked(PointerButton::Primary));
        if drawing.is_some_and(|drawing| drawing != ShapeKind::Polygon || double_click) {
            finish_shape(ctx, image, tools);
        } else if let Some(shape) = &mut image.shape
            && shape.release(&mut image.document, name, icon)
        {
            image.refresh(ctx);
        }
    }

    // The pointer shows what a press would do.
    let dragged_handle = image.shape.as_ref().and_then(ShapeEdit::dragged_handle);
    let hovered = pointer.filter(|_| !image.is_dragging());
    let hit = dragged_handle.map(ShapeHit::Handle).or_else(|| hovered.and_then(|at| hit_at(image, at)));
    let angle = image.shape.as_ref().and_then(|shape| shape.shape(&image.document)).map_or(0.0, |shape| shape.angle);
    let cursor = match hit {
        Some(ShapeHit::Handle(Handle::Box(x, y))) => DragKind::Resize((x, y)).cursor(angle),
        Some(ShapeHit::Handle(Handle::Rotate)) => DragKind::Rotate.cursor(angle),
        Some(ShapeHit::Handle(Handle::Body)) => CursorIcon::Move,
        Some(ShapeHit::Handle(Handle::Point(_))) if dragged_handle.is_some() => CursorIcon::Grabbing,
        Some(ShapeHit::Handle(Handle::Point(_))) => CursorIcon::Grab,
        Some(ShapeHit::Edge(_)) => CursorIcon::Copy,
        None => CursorIcon::Crosshair,
    };
    if response.hovered() || image.is_dragging() {
        ui.ctx().set_cursor_icon(cursor);
    }
}

/// The pixel at a point of the screen, which the shape tools put their points on.
fn shape_pixel(view: &View, at: Pos2) -> (i32, i32) {
    let (x, y) = view.pixel_at(at);
    (x as i32, y as i32)
}

/// What of the shape the tool can change a point of the screen is on, if any: the
/// nearest of its handles, or else of the Polygon's edges, or else the shape itself,
/// its line or what it goes around, or else the band around its box that turns it.
fn shape_hit_at(image: &OpenImage, kind: ShapeKind, at: Pos2) -> Option<ShapeHit> {
    let edit = image.shape.as_ref().filter(|edit| edit.kind() == kind)?;
    let shape = edit.shape(&image.document)?;
    let view = &image.view;
    let screen = |(x, y): (f64, f64)| view.to_screen(x as f32, y as f32);
    let nearest = |distances: &mut dyn Iterator<Item = (usize, f32)>, reach: f32| {
        distances.filter(|&(_, distance)| distance <= reach).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(index, _)| index)
    };
    let handles = shapes::handles(shape);
    let mut to_handles = handles.iter().map(|&(_, point)| screen(point).distance(at)).enumerate();
    if let Some(index) = nearest(&mut to_handles, CORNER_REACH) {
        return Some(ShapeHit::Handle(handles[index].0));
    }
    let outline: Vec<Pos2> = shape.outline().into_iter().map(screen).collect();
    let segment = |index: usize| distance_to_segment(at, outline[index], outline[(index + 1) % outline.len()]);
    if kind == ShapeKind::Polygon {
        let mut to_edges = (0..outline.len()).map(|index| (index, segment(index)));
        if let Some(index) = nearest(&mut to_edges, EDGE_REACH) {
            return Some(ShapeHit::Edge(index));
        }
    }
    // On its line, as wide as it is drawn, or within reach of it if that is wider.
    let half_width = edit.size(&image.document).unwrap_or(1) as f32 / 2.0 / view.pixels_per_screen_point() as f32;
    let segments = if kind.is_closed() { outline.len() } else { outline.len().saturating_sub(1) };
    let on_line = (0..segments).any(|index| segment(index) <= half_width.max(EDGE_REACH));
    if on_line || (kind.is_closed() && encloses(&outline, at)) {
        return Some(ShapeHit::Handle(Handle::Body));
    }
    let (center, half, angle) = shapes::frame(shape);
    let (x, y) = view.point_at(at);
    let (sin, cos) = (-angle).sin_cos();
    let (dx, dy) = (x - center.0, y - center.1);
    let local = (dx * cos - dy * sin, dx * sin + dy * cos);
    let outside = (local.0.abs() - half.0).max(local.1.abs() - half.1) / view.pixels_per_screen_point();
    (outside > 0.0 && outside <= ROTATE_REACH).then_some(ShapeHit::Handle(Handle::Rotate))
}

/// Whether a point is inside an outline, going around it an odd number of times.
fn encloses(outline: &[Pos2], at: Pos2) -> bool {
    let mut inside = false;
    for (i, &a) in outline.iter().enumerate() {
        let b = outline[(i + 1) % outline.len()];
        if (a.y > at.y) != (b.y > at.y) && at.x < a.x + (at.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

/// The Polygon puts down a corner where it is pressed, or is finished on its first
/// corner once it has three.
fn put_shape_corner(ctx: &egui::Context, image: &mut OpenImage, tools: &Tools, press: Pos2) {
    let at = shape_pixel(&image.view, press);
    let Some(shape) = &mut image.shape else {
        return;
    };
    let &first = shape.corners().first().expect("a polygon being drawn has a corner");
    let first_on_screen = image.view.to_screen(first.0 as f32 + 0.5, first.1 as f32 + 0.5);
    let on_first = at == first || first_on_screen.distance(press) <= CORNER_REACH;
    if shape.corners().len() >= 3 && on_first {
        finish_shape(ctx, image, tools);
    } else {
        let area = shape.add_corner(at);
        image.show_shape(area);
    }
}

/// Finishes the shape being drawn, which becomes part of the layer as a step, to go on
/// changing, if it changes any pixel, and is given up otherwise.
fn finish_shape(ctx: &egui::Context, image: &mut OpenImage, tools: &Tools) {
    let Some(shape) = &mut image.shape else {
        return;
    };
    if !shape.finish(&mut image.document, tools.tool.name(), Some(tools.tool.icon().name())) {
        image.shape = None;
    }
    // The layer shows as it is, in place of what was drawn over it.
    image.refresh(ctx);
}

/// The selection's box moved, as a step: with `placed`, the active layer with the
/// selected pixels drawn where they now are.
fn commit_transform(ctx: &egui::Context, image: &mut OpenImage, placed: Option<RgbaImage>) {
    let document = &mut image.document;
    match placed {
        Some(pixels) => {
            document.active_mut().pixels = pixels;
            document.commit("Move Selected Pixels", Some(icons::MOVE_PIXELS.name()));
            image.refresh(ctx);
        }
        None => document.commit(Tool::MoveSelection.name(), Some(Tool::MoveSelection.icon().name())),
    }
    let step = image.document.current_step();
    if let Some(transform) = &mut image.transform {
        transform.committed_as(step);
    }
}

/// Nudges the selection's box by `offset`, in pixels, with the selected pixels with the
/// selection tools, as a step, or the shape a shape tool can change. Nothing happens
/// while the tool shows no box, or drags it.
pub fn nudge(ctx: &egui::Context, image: &mut OpenImage, tools: &Tools, offset: (i32, i32)) {
    if image.is_dragging() || tools.frame_select.is_dragging() {
        return;
    }
    if let Some(shape) = image.shape.as_mut().filter(|shape| tools.tool.shape() == Some(shape.kind())) {
        if shape.nudge(&mut image.document, offset, tools.tool.name(), Some(tools.tool.icon().name())) {
            image.refresh(ctx);
        }
        return;
    }
    if box_state(image, tools).is_none() {
        return;
    }
    if image.transform.is_none() {
        image.transform = Transform::begin(ctx, &image.document, tools.tool.has_box());
    }
    let Some(transform) = &mut image.transform else {
        return;
    };
    let (selection, placed) = transform.nudge(offset, tools.resampling);
    image.document.selection = selection;
    commit_transform(ctx, image, placed);
}

/// Works the Zoom tool: a click zooms a level in on the pointer, or out with the right
/// button, and a drag with the left one frames the area to show. Escape cancels a drag.
fn use_zoom(ui: &egui::Ui, response: &egui::Response, view: &mut View, tools: &mut Tools, space: bool) {
    if ui.input(|input| input.key_pressed(Key::Escape)) {
        tools.zoom_drag = None;
    }
    let started = [PointerButton::Primary, PointerButton::Secondary]
        .into_iter()
        .find(|&button| response.drag_started_by(button));
    if let Some(button) = started
        && !space
        && let Some(press) = ui.input(|input| input.pointer.press_origin())
    {
        let from = view.point_at(press);
        tools.zoom_drag = Some(ZoomDrag { button, from, to: from, framing: false });
    }
    if let Some(drag) = &mut tools.zoom_drag
        && response.dragged()
        && let Some(at) = response.interact_pointer_pos()
    {
        drag.to = view.point_at(at);
        let press = ui.input(|input| input.pointer.press_origin()).unwrap_or(at);
        drag.framing |= drag.button == PointerButton::Primary && press.distance(at) > ZOOM_DRAG_THRESHOLD;
    }
    if response.drag_stopped()
        && let Some(drag) = tools.zoom_drag.take()
    {
        if drag.framing {
            view.zoom_to_rect(frame_rect(&drag));
        } else {
            let at = view.to_screen(drag.from.0 as f32, drag.from.1 as f32);
            view.step_zoom(if drag.button == PointerButton::Secondary { -1 } else { 1 }, at);
        }
    }
    let zooming_out = tools.zoom_drag.as_ref().map_or_else(
        || ui.input(|input| input.pointer.secondary_down()),
        |drag| drag.button == PointerButton::Secondary,
    );
    if response.hovered() || tools.zoom_drag.is_some() {
        ui.ctx().set_cursor_icon(if zooming_out { CursorIcon::ZoomOut } else { CursorIcon::ZoomIn });
    }
}

/// Works the Color Picker: the left button picks the colour of the active layer under
/// the pointer as the colour the Colors section is editing, for as long as it is held,
/// and the right one as the other colour. Returns whether the pointer is the tool's, over the image or picking.
fn use_color_picker(response: &egui::Response, image: &OpenImage, tools: &mut Tools, space: bool) -> bool {
    if !space
        && let Some(at) = response.interact_pointer_pos()
        && let Some(color) = color_at(image, at)
    {
        if response.dragged_by(PointerButton::Primary) {
            tools.colors.pick(color, false);
        } else if response.dragged_by(PointerButton::Secondary) {
            tools.colors.pick(color, true);
        }
    }
    response.hovered() || response.dragged()
}

/// Works the Paint Bucket: a click fills the pixels of similar colour around the one
/// clicked, or all those in the image as the contiguous setting says, or the other way
/// while Shift is held, with the primary colour. With a selection, only the selected
/// pixels are filled, from a click on one of them. The right button fills with the
/// secondary colour, or picks a colour as the Color Picker does, as the right click
/// option says. Returns whether the pointer is the tool's, over the image.
fn use_paint_bucket(
    ui: &egui::Ui,
    response: &egui::Response,
    image: &mut OpenImage,
    tools: &mut Tools,
    space: bool,
) -> bool {
    let started = [PointerButton::Primary, PointerButton::Secondary]
        .into_iter()
        .find(|&button| response.drag_started_by(button));
    let picking = tools.right_click == RightClick::PickColor;
    if picking
        && !space
        && response.dragged_by(PointerButton::Secondary)
        && let Some(at) = response.interact_pointer_pos()
        && let Some(color) = color_at(image, at)
    {
        tools.colors.pick(color, false);
    }
    let Some(button) = started.filter(|&button| !space && (button == PointerButton::Primary || !picking)) else {
        return response.hovered();
    };
    let Some(press) = ui.input(|input| input.pointer.press_origin()) else {
        return response.hovered();
    };
    let (x, y) = image.view.pixel_at(press);
    let (width, height) = image.document.size();
    let mask = image.document.selection_mask();
    let on_image = (0..width as i64).contains(&x) && (0..height as i64).contains(&y);
    if !on_image || mask.as_ref().is_some_and(|mask| !mask.contains(x as u32, y as u32)) {
        return response.hovered();
    }
    let fill = flood_at(ui, image, tools, (x as u32, y as u32), mask.as_ref());
    let color = if button == PointerButton::Primary { tools.colors.primary } else { tools.colors.secondary };
    // A transform would go on from the pixels as they were before.
    image.transform = None;
    if fill.paint(&mut image.document.active_mut().pixels, color, tools.smooth()).is_some() {
        image.document.commit(Tool::PaintBucket.name(), Some(Tool::PaintBucket.icon().name()));
        image.refresh(ui.ctx());
    }
    response.hovered()
}

/// Works the Magic Wand: a click on the image selects the pixels whose colour is close
/// to that of the one clicked, changing the selection as the selection mode says, or
/// adding to it with Ctrl, which is a step if it changed anything. Returns whether
/// the pointer is the tool's, over the image.
fn use_magic_wand(
    ui: &egui::Ui,
    response: &egui::Response,
    image: &mut OpenImage,
    tools: &Tools,
    space: bool,
) -> bool {
    if space || !response.drag_started_by(PointerButton::Primary) {
        return response.hovered();
    }
    let Some(press) = ui.input(|input| input.pointer.press_origin()) else {
        return response.hovered();
    };
    let (x, y) = image.view.pixel_at(press);
    let (width, height) = image.document.size();
    if !(0..width as i64).contains(&x) || !(0..height as i64).contains(&y) {
        return response.hovered();
    }
    let found = flood_at(ui, image, tools, (x as u32, y as u32), None).selection();
    let mode = if ui.input(|input| input.modifiers.command) { SelectionMode::Add } else { tools.selection_mode };
    // A transform would go on from the selection it began with.
    image.transform = None;
    let document = &mut image.document;
    let selection = document.selection.combine(&found, mode);
    if selection != document.selection {
        document.selection = selection;
        crate::commit_selection(document, Tool::MagicWand.name(), Tool::MagicWand.icon());
    }
    response.hovered()
}

/// The pixels the Magic Wand or the Paint Bucket takes in from a click on the pixel
/// `seed`, among the selected ones of `mask`: those of a colour close to its own, in
/// the layer or the whole image as they sample, around it as Contiguous says, or the
/// other way while Shift is held.
fn flood_at(
    ui: &egui::Ui,
    image: &OpenImage,
    tools: &Tools,
    seed: (u32, u32),
    mask: Option<&SelectionMask>,
) -> FloodFill {
    let sample = match tools.sample_source {
        SampleSource::Layer => None,
        SampleSource::Image => Some(image.document.flattened()),
    };
    let sample = sample.as_ref().unwrap_or(&image.document.active().pixels);
    let contiguous = tools.contiguous != ui.input(|input| input.modifiers.shift);
    let tolerance = tools.tolerance as f32 / 100.0;
    FloodFill::new(sample, seed, tolerance, contiguous, mask)
}

/// The colour of the active layer at a point of the screen, if it is over the image.
fn color_at(image: &OpenImage, point: Pos2) -> Option<Rgba<u8>> {
    let (x, y) = image.view.pixel_at(point);
    let pixels = &image.document.active().pixels;
    let on_image = (0..pixels.width() as i64).contains(&x) && (0..pixels.height() as i64).contains(&y);
    on_image.then(|| *pixels.get_pixel(x as u32, y as u32))
}

/// Works a drawing tool: a left drag draws a line on the active layer, or sprays dots
/// for as long as it is held, which is a step once let go if it changed anything. The
/// tools that draw in a colour do it in the secondary one with a right drag, or pick
/// the colour under the pointer as the one the Colors section is editing, as the right
/// click option says. A left drag from a handle of the selection's
/// box resizes the selection instead, and one from the handle out from its bottom
/// right corner moves it, and one from around the box rotates it, without its pixels,
/// as Move Selection does.
/// Returns whether the pointer is the tool's, over the image or drawing.
fn use_drawing_tool(
    ui: &egui::Ui,
    response: &egui::Response,
    image: &mut OpenImage,
    tools: &mut Tools,
    space: bool,
) -> bool {
    let reach = image.view.pixels_per_screen_point();
    let handle_at = |image: &OpenImage, tools: &Tools, point: Pos2| {
        box_state(image, tools)?.handle_at(image.view.point_at(point), reach)
    };
    if ui.input(|input| input.key_pressed(Key::Escape))
        && let Some(transform) = image.transform.as_mut().filter(|transform| transform.is_dragging())
    {
        image.document.selection = transform.cancel();
    }
    let picking = tools.tool.draws_in_color() && tools.right_click == RightClick::PickColor;
    let started = [PointerButton::Primary, PointerButton::Secondary]
        .into_iter()
        .find(|&button| response.drag_started_by(button));
    if let Some(button) = started
        && !space
        && let Some(press) = ui.input(|input| input.pointer.press_origin())
    {
        let primary = button == PointerButton::Primary;
        // The right button does not draw with the Eraser, nor while it picks colours.
        let draws = primary || (tools.tool.draws_in_color() && !picking);
        if let Some(kind) = handle_at(image, tools, press).filter(|_| primary) {
            if image.transform.is_none() {
                image.transform = Transform::begin(ui.ctx(), &image.document, false);
            }
            if let Some(transform) = &mut image.transform {
                transform.press(kind, image.view.point_at(press), reach);
            }
        } else if draws {
            // A transform would go on from the selection as it was before.
            image.transform = None;
            let mask = image.document.selection_mask();
            let at = image.view.point_at(press);
            let layer = &mut image.document.active_mut().pixels;
            let color = if primary { tools.colors.primary } else { tools.colors.secondary };
            let (drawing, area) = match (tools.tool.retouch(), tools.ink(color)) {
                (Some(retouch), _) => {
                    let (size, strength) = (tools.tip_size(), tools.strength());
                    let (stroke, area) = RetouchStroke::begin(layer, at, retouch, size, strength, mask, seed());
                    (Drawing::Retouch(stroke), area)
                }
                (None, Some(ink)) => {
                    let (stroke, area) = BrushStroke::begin(layer, at, ink, tools.tip_size(), tools.smooth(), mask);
                    (Drawing::Line(stroke), area)
                }
                (None, None) => {
                    let (size, speed, smooth) = (tools.spray_size, tools.spray_speed, tools.smooth());
                    let (spray, area) = Spray::begin(layer, at, color, size, speed, smooth, mask, seed());
                    (Drawing::Spray(spray), area)
                }
            };
            // Nothing to show again while all it went over was left as it was.
            if let Some(area) = area.filter(|_| drawing.changed()) {
                image.refresh_area(area);
            }
            image.stroke = Some(drawing);
        }
    }
    if picking
        && !space
        && response.dragged_by(PointerButton::Secondary)
        && let Some(at) = response.interact_pointer_pos()
        && let Some(color) = color_at(image, at)
    {
        tools.colors.pick(color, false);
    }
    let held = response.dragged() && !response.drag_started();
    let moved = held && response.drag_delta() != Vec2::ZERO;
    // The Spray leaves dots as time goes by, whether the pointer moves or not.
    let spraying = held && matches!(image.stroke, Some(Drawing::Spray(_)));
    if spraying {
        ui.ctx().request_repaint();
    }
    if (moved || spraying) && let Some(at) = response.interact_pointer_pos() {
        let seconds = ui.input(|input| input.stable_dt as f64).min(MAX_SPRAY_FRAME);
        if let Some(drawing) = &mut image.stroke
            && let Some(area) = drawing.go_to(&mut image.document.active_mut().pixels, image.view.point_at(at), seconds)
            && drawing.changed()
        {
            image.refresh_area(area);
        }
        let (shift, command) = ui.input(|input| (input.modifiers.shift, input.modifiers.command));
        if let Some(transform) = image.transform.as_mut()
            && let Some(selection) = transform.drag_to(image.view.point_at(at), shift, command, tools.snap)
        {
            image.document.selection = selection;
        }
    }
    if response.drag_stopped() {
        if let Some(stroke) = image.stroke.take()
            && stroke.changed()
        {
            image.document.commit(tools.tool.name(), Some(tools.tool.icon().name()));
            image.refresh(ui.ctx());
        }
        if let Some(transform) = image.transform.as_mut()
            && transform.release(tools.resampling).0
        {
            commit_transform(ui.ctx(), image, None);
        }
    }
    // A handle shows what dragging it would do, in place of the tool.
    let angle = box_state(image, tools).map_or(0.0, |state| state.angle);
    let resizing = image.transform.as_ref().and_then(Transform::drag_kind);
    let on_handle = || ui.ctx().pointer_hover_pos().and_then(|at| handle_at(image, tools, at));
    if let Some(kind) = resizing.or_else(|| if image.stroke.is_none() { on_handle() } else { None }) {
        ui.ctx().set_cursor_icon(kind.cursor(angle));
        return false;
    }
    response.hovered() || image.stroke.is_some()
}

/// Shows what the drawing tool would cover with the pointer at `at`, in the colour it
/// would leave, if it leaves one. A tip large enough on screen is outlined along its
/// edge. That would hide a smaller one, down to a single pixel, which is ringed from
/// outside instead.
fn paint_tip(painter: &Painter, view: &View, at: Pos2, tools: &Tools) {
    let retouching = tools.tool.retouch().is_some();
    let fill = (tools.tool != Tool::Eraser && !retouching).then(|| {
        let [red, green, blue, alpha] = tools.colors.primary.0;
        Color32::from_rgba_unmultiplied(red, green, blue, alpha)
    });
    let pixel = 1.0 / painter.pixels_per_point();
    let tip_side = tools.tip_size() as f32 * view.zoom;
    if tools.smooth() || tools.tool == Tool::Spray || retouching {
        // A smooth tip is round about the point itself, not about a pixel, and so are
        // the areas the retouch tools go over, and the area the Spray covers, which is
        // left empty: it only fills up little by little.
        let radius = tools.tip_size() as f32 / 2.0 * view.zoom * pixel;
        if let Some(fill) = fill.filter(|_| tools.tool != Tool::Spray) {
            painter.circle_filled(at, radius, fill);
        }
        let ring = if tip_side >= OUTLINED_TIP { radius } else { radius + 1.5 * pixel };
        painter.circle_stroke(at, ring, Stroke::new(3.0 * pixel, Color32::WHITE));
        painter.circle_stroke(at, ring, Stroke::new(pixel, Color32::BLACK));
        return;
    }
    let at = view.pixel_at(at);
    let rows: Vec<(i64, i64, i64)> = hard_tip(at, tools.tip_size()).collect();
    let screen = |x: i64, y: i64| view.to_screen(x as f32, y as f32);
    if let Some(fill) = fill {
        for &(y, from, to) in &rows {
            painter.rect_filled(Rect::from_min_max(screen(from, y), screen(to, y + 1)), 0.0, fill);
        }
    }
    if tip_side >= OUTLINED_TIP {
        // The two ends of each row, and the parts of its top and bottom that the rows
        // above and below do not cover.
        let mut edges = Vec::new();
        for (i, &(y, from, to)) in rows.iter().enumerate() {
            edges.push((screen(from, y), screen(from, y + 1)));
            edges.push((screen(to, y), screen(to, y + 1)));
            for (neighbour, edge_y) in [(i.checked_sub(1), y), (Some(i + 1), y + 1)] {
                let (covered_from, covered_to) = match neighbour.and_then(|n| rows.get(n)) {
                    Some(&(_, other_from, other_to)) => (other_from, other_to),
                    None => (from, from),
                };
                if covered_from > from {
                    edges.push((screen(from, edge_y), screen(covered_from.min(to), edge_y)));
                }
                if covered_to < to {
                    edges.push((screen(covered_to.max(from), edge_y), screen(to, edge_y)));
                }
            }
        }
        for stroke in [Stroke::new(3.0 * pixel, Color32::WHITE), Stroke::new(pixel, Color32::BLACK)] {
            for &(from, to) in &edges {
                painter.line_segment([from, to], stroke);
            }
        }
        return;
    }
    let (top, left) = (rows[0].0, rows.iter().map(|row| row.1).min().unwrap_or(at.0));
    let corner = screen(left, top).round_to_pixels(painter.pixels_per_point());
    let tip = Rect::from_min_size(corner, Vec2::splat(tip_side.ceil() * pixel));
    for (grow, color) in [(2.0, Color32::WHITE), (1.0, Color32::BLACK)] {
        // Four bars one pixel thick, which land on whole pixels where an outlined
        // rectangle does not.
        let outer = tip.expand(grow * pixel);
        let bars = [
            Rect::from_min_max(outer.min, Pos2::new(outer.max.x, outer.min.y + pixel)),
            Rect::from_min_max(Pos2::new(outer.min.x, outer.max.y - pixel), outer.max),
            Rect::from_min_max(outer.min, Pos2::new(outer.min.x + pixel, outer.max.y)),
            Rect::from_min_max(Pos2::new(outer.max.x - pixel, outer.min.y), outer.max),
        ];
        for bar in bars {
            painter.rect_filled(bar, 0.0, color);
        }
    }
}

/// A different seed each time, for the Spray's dots to fall differently.
fn seed() -> u64 {
    let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    since.map_or(0, |since| since.as_nanos() as u64)
}

/// Shows a cursor drawn from `cursor` in place of the pointer, `hotspot` on it, over
/// all else.
fn paint_cursor(ui: &egui::Ui, icons: &mut Icons, cursor: Icon, hotspot: Vec2, at: Pos2) {
    ui.ctx().set_cursor_icon(CursorIcon::None);
    let texture = icons.get_sized(ui.ctx(), cursor, CURSOR_SIZE.x);
    let layer = egui::LayerId::new(egui::Order::Tooltip, ui.id().with("tool_cursor"));
    let corner = (at - hotspot).round_to_pixels(ui.ctx().pixels_per_point());
    let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
    ui.ctx().layer_painter(layer).image(texture.id(), Rect::from_min_size(corner, CURSOR_SIZE), uv, Color32::WHITE);
}

/// The area a drag of the Zoom tool frames, on the image, in pixels.
fn frame_rect(drag: &ZoomDrag) -> Rect {
    let point = |(x, y): (f64, f64)| Pos2::new(x as f32, y as f32);
    Rect::from_two_pos(point(drag.from), point(drag.to))
}

/// Draws the area the Zoom tool frames: a black line in a white one stays visible on any image.
fn paint_zoom_frame(painter: &Painter, view: &View, drag: &ZoomDrag) {
    let rect = frame_rect(drag);
    let pixel = 1.0 / painter.pixels_per_point();
    let min = view.to_screen(rect.min.x, rect.min.y).round_to_pixel_center(painter.pixels_per_point());
    let max = view.to_screen(rect.max.x, rect.max.y).round_to_pixel_center(painter.pixels_per_point());
    let frame = Rect::from_min_max(min, max);
    painter.rect_stroke(frame.expand(pixel), 0.0, Stroke::new(pixel, Color32::WHITE), egui::StrokeKind::Middle);
    painter.rect_stroke(frame, 0.0, Stroke::new(pixel, Color32::BLACK), egui::StrokeKind::Middle);
}

/// A selection tool starts drawing a new selection, which ends any transform. Holding
/// Ctrl when it is pressed adds to the selection, whatever the mode. Alt is left
/// alone, as window managers on Linux move the window with Alt and a drag. Lasso
/// Select's outline keeps to the pointer, not to the grid.
fn start_selecting(image: &mut OpenImage, tools: &mut Tools, press: Pos2, command: bool) {
    image.transform = None;
    let mode = if command { SelectionMode::Add } else { tools.selection_mode };
    let view = &image.view;
    let corner = corner_at(view, press, tools.snap);
    let pressed = view.pixel_at(press);
    if let Some(kind) = tools.tool.lasso() {
        let corner = if kind == LassoKind::Freehand { view.corner_at(press) } else { corner };
        image.lasso = Some(Lasso::begin(&mut image.document, kind, corner, pressed, mode, tools.overlaps));
    } else if let Some(shape) = tools.tool.select_shape() {
        tools.frame_select.press(&mut image.document.selection, shape, corner, pressed, mode);
    }
}

/// The corner between pixels where the selection tools put a corner of the selection,
/// for a point of the screen: the nearest one, or the nearest corner of a cell of `snap`.
fn corner_at(view: &View, at: Pos2, snap: Option<Cells>) -> (i32, i32) {
    match snap {
        Some(cells) => view.nearest_corner(cells.snap(view.point_at(at))),
        None => view.corner_at(at),
    }
}

/// A selection tool ends drawing the selection, which is a step if it changed anything.
/// Polygon Select's outline goes on until it is closed.
fn release_selecting(image: &mut OpenImage, tools: &mut Tools) {
    let document = &mut image.document;
    if let Some(before) = tools.frame_select.release(&mut document.selection)
        && document.selection != before
    {
        crate::commit_selection(document, tools.tool.name(), tools.tool.icon());
    }
    if image.lasso.as_ref().is_some_and(|lasso| lasso.kind == LassoKind::Freehand) {
        close_lasso(image, tools);
    }
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

/// The outline a lasso is drawing, closed as it would be if it ended now, with a handle
/// on Polygon Select's first corner, the one a click on which closes it. Or Polygon
/// Select's corners, which can then be moved, each with a handle.
fn paint_lasso(painter: &Painter, image: &OpenImage, tools: &Tools) {
    let view = &image.view;
    let on_screen = |&(x, y): &(i32, i32)| {
        let point = view.to_screen(x as f32, y as f32);
        Pos2::new(painter.round_to_pixel_center(point.x), painter.round_to_pixel_center(point.y))
    };
    let handles: Vec<Pos2> = if let Some(lasso) = &image.lasso {
        let outline = lasso.outline();
        paint_outline(painter, &outline.iter().map(on_screen).collect::<Vec<_>>());
        match lasso.kind {
            LassoKind::Polygon => lasso.corners().first().map(on_screen).into_iter().collect(),
            LassoKind::Freehand => Vec::new(),
        }
    } else {
        polygon_corners(image, tools).unwrap_or_default().iter().map(on_screen).collect()
    };
    for center in handles {
        paint_corner_handle(painter, center);
    }
}

/// A round handle, on a corner or another point that can be moved.
fn paint_corner_handle(painter: &Painter, center: Pos2) {
    let pixel = 1.0 / painter.pixels_per_point();
    painter.circle_filled(center, CORNER_RADIUS, Color32::WHITE);
    painter.circle_stroke(center, CORNER_RADIUS, Stroke::new(pixel, Color32::BLACK));
}

/// The handles of the shape the tool can change: round on the points that place a
/// Line, a Curve or a Polygon, with lines from a Curve's ends to the points it bends
/// towards, and square on the box around a Rectangle, an Ellipse or a Contour, which
/// shows as well. Or, while the Polygon is being drawn, a handle on its first corner,
/// a click on which finishes it.
fn paint_shape(painter: &Painter, image: &OpenImage, tools: &Tools) {
    let Some(edit) = image.shape.as_ref().filter(|edit| tools.tool.shape() == Some(edit.kind())) else {
        return;
    };
    let pixel = 1.0 / painter.pixels_per_point();
    let on_screen = |(x, y): (f64, f64)| {
        let point = image.view.to_screen(x as f32, y as f32);
        Pos2::new(painter.round_to_pixel_center(point.x), painter.round_to_pixel_center(point.y))
    };
    let middle = |(x, y): (i32, i32)| (x as f64 + 0.5, y as f64 + 0.5);
    if edit.is_drawing() {
        if edit.kind() == ShapeKind::Polygon
            && let Some(&first) = edit.corners().first()
        {
            paint_corner_handle(painter, on_screen(middle(first)));
        }
        return;
    }
    let Some(shape) = edit.shape(&image.document) else {
        return;
    };
    if shape.kind == ShapeKind::Curve {
        for (end, bend) in [(0, 2), (1, 3)] {
            let line = [on_screen(middle(shape.points[end])), on_screen(middle(shape.points[bend]))];
            painter.line_segment(line, Stroke::new(3.0 * pixel, Color32::WHITE));
            painter.line_segment(line, Stroke::new(pixel, Color32::BLACK));
        }
    }
    let handles = shapes::handles(shape);
    let corner = |x, y| handles.iter().find(|(handle, _)| *handle == Handle::Box(x, y)).map(|&(_, point)| on_screen(point));
    let frame: Vec<Pos2> = [(-1, -1), (1, -1), (1, 1), (-1, 1)].into_iter().filter_map(|(x, y)| corner(x, y)).collect();
    if !frame.is_empty() {
        paint_outline(painter, &frame);
    }
    for (handle, point) in handles {
        if let Handle::Box(..) = handle {
            let square = Rect::from_center_size(on_screen(point), Vec2::splat(HANDLE_SIZE));
            painter.rect_filled(square, 0.0, Color32::WHITE);
            painter.rect_stroke(square, 0.0, Stroke::new(pixel, Color32::BLACK), egui::StrokeKind::Inside);
        } else {
            paint_corner_handle(painter, on_screen(point));
        }
    }
}

/// A handle on each corner of the box and in the middle of each edge, out from its
/// bottom right corner, the one that moves it, with arrows on it, and on its centre of
/// rotation, a ring with a cross, if the box is large enough on screen.
fn paint_handles(painter: &Painter, view: &View, state: BoxState, icons: &mut Icons) {
    let pixel = 1.0 / painter.pixels_per_point();
    let on_screen = |(x, y): (f64, f64)| view.to_screen(x as f32, y as f32).round_to_pixels(painter.pixels_per_point());
    for point in state.handles() {
        let handle = Rect::from_center_size(on_screen(point), Vec2::splat(HANDLE_SIZE));
        painter.rect_filled(handle, 0.0, Color32::WHITE);
        painter.rect_stroke(handle, 0.0, Stroke::new(pixel, Color32::BLACK), egui::StrokeKind::Inside);
    }
    let center = on_screen(state.move_handle(view.pixels_per_screen_point()));
    let handle = Rect::from_center_size(center, Vec2::splat(MOVE_HANDLE_SIZE as f32));
    painter.rect_filled(handle, 2.0, Color32::WHITE);
    painter.rect_stroke(handle, 2.0, Stroke::new(pixel, Color32::BLACK), egui::StrokeKind::Inside);
    let arrows = Rect::from_center_size(center, Vec2::splat(MOVE_HANDLE_ARROWS));
    let texture = icons.get_sized(painter.ctx(), icons::MOVE, MOVE_HANDLE_ARROWS);
    let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
    // The icons are light gray lines, which the tint makes black.
    painter.image(texture.id(), arrows, uv, Color32::BLACK);
    if state.shows_pivot(view.pixels_per_screen_point()) {
        let center = on_screen(state.pivot_point());
        let radius = PIVOT_RADIUS as f32 - 1.0;
        for stroke in [Stroke::new(3.0 * pixel, Color32::WHITE), Stroke::new(pixel, Color32::BLACK)] {
            painter.circle_stroke(center, radius, stroke);
            painter.line_segment([center - Vec2::X * radius, center + Vec2::X * radius], stroke);
            painter.line_segment([center - Vec2::Y * radius, center + Vec2::Y * radius], stroke);
        }
    }
}

/// A button by the selection.
#[derive(Clone, Copy)]
enum SelectionButton {
    Deselect,
    /// Move Selection with the selection tools, and back to the selection tool used
    /// last with Move Selection.
    Switch(Tool),
}

/// Shows a button by the selection: with the selection tools and Move Selection, one
/// showing what the box moves, the selected pixels or the selection alone, which goes
/// to the tool that moves the other; and with the other tools Deselect. It goes just
/// right of where the selection ends, level with its rightmost handle but the bottom
/// right corner, or else point of the outline, the highest of those as far right, and
/// above the handle that moves the box if that is in the way. Where the view ends
/// there, it goes just inside that handle instead. Returns whether it was clicked, as
/// what.
fn selection_button(
    ui: &egui::Ui,
    image: &OpenImage,
    tools: &Tools,
    icons: &mut Icons,
    viewport: Rect,
    box_state: Option<BoxState>,
) -> Option<SelectionButton> {
    let points: Vec<(f64, f64)> = match box_state {
        Some(state) => state.handles(),
        None => image.document.selection.outlines().iter().flatten().copied().collect(),
    };
    let to_screen = |(x, y): (f64, f64)| image.view.to_screen(x as f32, y as f32);
    let screen: Vec<Pos2> = points.into_iter().map(to_screen).collect();
    // Not the corner the handle that moves the box is by, which the button would cover.
    let move_corner = box_state.map(|state| to_screen(state.move_corner()));
    let anchors = screen.iter().filter(|&&point| Some(point) != move_corner);
    let corner = *anchors.max_by(|a, b| match a.x - b.x {
        apart if apart.abs() > 0.5 => apart.total_cmp(&0.0),
        _ => b.y.total_cmp(&a.y),
    })?;
    // Where the selection ends on the right, which a turned box can reach further than
    // that corner.
    let right = screen.iter().map(|point| point.x).fold(corner.x, f32::max);

    let shortcut = |key| ui.ctx().format_shortcut(&key);
    let selecting = tools.tool == Tool::MoveSelection || tools.tool.has_box();
    // It shows what the box moves now, and switches to moving the other.
    let (button, icon, tooltip) = if tools.tool == Tool::MoveSelection {
        let target = tools.box_select_tool();
        let tooltip = format!(
            "Moving the selection alone.\nClick to move the selected pixels instead, with {} ({}).",
            target.name(),
            shortcut(target.key())
        );
        (SelectionButton::Switch(target), icons::MOVE_SELECTION, tooltip)
    } else if selecting {
        let target = Tool::MoveSelection;
        let tooltip = format!(
            "Moving the selected pixels.\nClick to move the selection alone, with {} ({}).",
            target.name(),
            shortcut(target.key())
        );
        (SelectionButton::Switch(target), icons::MOVE_PIXELS, tooltip)
    } else {
        (SelectionButton::Deselect, icons::DESELECT, format!("Deselect ({})", shortcut(crate::DESELECT)))
    };

    let size = BUTTON_ICON_SIZE + BUTTON_PADDING * 2.0;
    // Clear of the handle on the corner.
    let gap = BUTTON_GAP + HANDLE_SIZE / 2.0;
    let mut place = Pos2::new(right + gap, corner.y);
    // Above the handle that moves the box, if it is in the way.
    let reach = image.view.pixels_per_screen_point();
    let move_handle = box_state.map(|state| {
        Rect::from_center_size(to_screen(state.move_handle(reach)), Vec2::splat(MOVE_HANDLE_SIZE as f32))
    });
    if let Some(handle) = move_handle.map(|handle| handle.expand(BUTTON_GAP))
        && handle.intersects(Rect::from_min_size(place, size))
    {
        place.y = handle.top() - size.y;
    }
    if place.x + size.x > viewport.right() {
        place = Pos2::new(corner.x - size.x - gap, corner.y + gap);
    }
    let place = place.clamp(viewport.min, (viewport.max - size).max(viewport.min));
    let texture = icons.get_sized(ui.ctx(), icon, BUTTON_ICON_SIZE.x).clone();
    let tint = ui.visuals().strong_text_color();
    let area = egui::Area::new(ui.id().with("selection_button")).fixed_pos(place).show(ui.ctx(), |ui| {
        ui.spacing_mut().button_padding = BUTTON_PADDING;
        let icon = egui::Image::new(&texture).fit_to_exact_size(BUTTON_ICON_SIZE).tint(tint);
        ui.add(egui::Button::image(icon)).on_hover_text(tooltip).on_hover_cursor(CursorIcon::PointingHand).clicked()
    });
    area.inner.then_some(button)
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

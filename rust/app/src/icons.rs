//! The icons of the buttons, from the SVG files in `icons/`, built into the app. Each
//! is drawn at the screen's scale and the size it shows at the first time it shows,
//! and again when the scale changes.

use std::collections::HashMap;

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions, Vec2};
use power_paint_core::render_svg_to_width;

/// Size the icons show at, in points, which their files are scaled to.
pub const ICON_SIZE: Vec2 = Vec2::splat(20.0);

#[derive(Clone, Copy)]
pub struct Icon {
    name: &'static str,
    svg: &'static [u8],
}

macro_rules! icon {
    ($name:literal) => {
        Icon { name: $name, svg: include_bytes!(concat!("../../../icons/", $name, ".svg")) }
    };
}

pub const RECT_SELECT: Icon = icon!("rect_select");
pub const LASSO_SELECT: Icon = icon!("lasso_select");
pub const ELLIPSE_SELECT: Icon = icon!("ellipse_select");
pub const POLYGON_SELECT: Icon = icon!("polygon_lasso");
pub const MAGIC_WAND: Icon = icon!("magic_wand");
pub const MOVE_SELECTION: Icon = icon!("move_selection");
pub const ZOOM: Icon = icon!("zoom");
pub const PAN: Icon = icon!("pan");
pub const PAINTBRUSH: Icon = icon!("paintbrush");
pub const PENCIL: Icon = icon!("pencil");
pub const ERASER: Icon = icon!("eraser");
pub const SPRAY: Icon = icon!("spray");
pub const COLOR_PICKER: Icon = icon!("color_picker");
pub const PAINT_BUCKET: Icon = icon!("paint_bucket");
pub const LINE: Icon = icon!("line");
pub const CURVE: Icon = icon!("curve");
pub const RECTANGLE: Icon = icon!("rectangle");
pub const FILLED_RECTANGLE: Icon = icon!("filled_rectangle");
pub const ELLIPSE: Icon = icon!("ellipse");
pub const FILLED_ELLIPSE: Icon = icon!("filled_ellipse");
pub const CONTOUR: Icon = icon!("contour");
pub const POLYGON: Icon = icon!("polygon");
pub const CURSOR_PAINTBRUSH: Icon = icon!("cursor_paintbrush");
pub const CURSOR_PENCIL: Icon = icon!("cursor_pencil");
pub const CURSOR_ERASER: Icon = icon!("cursor_eraser");
pub const CURSOR_SPRAY: Icon = icon!("cursor_spray");
pub const CURSOR_COLOR_PICKER: Icon = icon!("cursor_color_picker");
pub const CURSOR_PAINT_BUCKET: Icon = icon!("cursor_paint_bucket");
pub const CURSOR_MAGIC_WAND: Icon = icon!("cursor_magic_wand");
pub const CURSOR_ROTATE: Icon = icon!("cursor_rotate");
pub const SELECT_ALL: Icon = icon!("select_all");
pub const DESELECT: Icon = icon!("deselect");
pub const INVERT_SELECTION: Icon = icon!("invert_selection");
pub const LAYER_ADD: Icon = icon!("layer_add");
pub const LAYER_DELETE: Icon = icon!("layer_delete");
pub const LAYER_DUPLICATE: Icon = icon!("layer_duplicate");
pub const LAYER_MERGE_DOWN: Icon = icon!("layer_merge_down");
pub const LAYER_UP: Icon = icon!("layer_up");
pub const LAYER_DOWN: Icon = icon!("layer_down");
pub const LAYER_PROPERTIES: Icon = icon!("layer_properties");
pub const LAYERS: Icon = icon!("layers");
pub const MOVE_PIXELS: Icon = icon!("move_pixels");
pub const MOVE: Icon = icon!("move");
pub const OPEN: Icon = icon!("open");
pub const CROP: Icon = icon!("crop");
pub const CUT: Icon = icon!("cut");
pub const PASTE: Icon = icon!("paste");
pub const ERASE: Icon = icon!("erase");
pub const SAVE: Icon = icon!("save");
pub const SAVE_AS: Icon = icon!("save_as");
pub const COPY: Icon = icon!("copy");
pub const GRID: Icon = icon!("grid");
pub const GRID_SNAP: Icon = icon!("grid_snap");
pub const GRID_SETTINGS: Icon = icon!("grid_settings");
pub const UNDO: Icon = icon!("undo");
pub const REDO: Icon = icon!("redo");

/// Those the history can list steps with, which it keeps by name.
const STEP_ICONS: [Icon; 36] = [
    RECT_SELECT,
    LASSO_SELECT,
    ELLIPSE_SELECT,
    POLYGON_SELECT,
    MAGIC_WAND,
    MOVE_SELECTION,
    SELECT_ALL,
    DESELECT,
    INVERT_SELECTION,
    LAYER_ADD,
    LAYER_DELETE,
    LAYER_DUPLICATE,
    LAYER_MERGE_DOWN,
    LAYER_UP,
    LAYER_DOWN,
    LAYER_PROPERTIES,
    LAYERS,
    MOVE_PIXELS,
    OPEN,
    CROP,
    CUT,
    PASTE,
    ERASE,
    PAINTBRUSH,
    PENCIL,
    ERASER,
    SPRAY,
    PAINT_BUCKET,
    LINE,
    CURVE,
    RECTANGLE,
    FILLED_RECTANGLE,
    ELLIPSE,
    FILLED_ELLIPSE,
    CONTOUR,
    POLYGON,
];

impl Icon {
    pub fn name(self) -> &'static str {
        self.name
    }

    /// The icon of a step of the history, from the name it keeps.
    pub fn of_step(name: &str) -> Option<Self> {
        STEP_ICONS.into_iter().find(|icon| icon.name == name)
    }
}

#[derive(Default)]
pub struct Icons {
    /// Each icon at each size it was asked for, in points.
    textures: HashMap<(&'static str, u32), TextureHandle>,
    /// The screen's scale the textures were drawn at.
    pixels_per_point: f32,
}

impl Icons {
    /// The icon to show [`ICON_SIZE`] wide.
    pub fn get(&mut self, ctx: &egui::Context, icon: Icon) -> &TextureHandle {
        self.get_sized(ctx, icon, ICON_SIZE.x)
    }

    /// The icon to show `side` points wide, drawn that large rather than stretched.
    pub fn get_sized(&mut self, ctx: &egui::Context, icon: Icon, side: f32) -> &TextureHandle {
        let pixels_per_point = ctx.pixels_per_point();
        if pixels_per_point != self.pixels_per_point {
            self.textures.clear();
            self.pixels_per_point = pixels_per_point;
        }
        self.textures.entry((icon.name, side.to_bits())).or_insert_with(|| {
            // The icons are part of the app, so one that cannot be drawn is a bug.
            let image = render_svg_to_width(icon.svg, pixels_per_point * side).expect("icons are valid SVG");
            let size = [image.width() as usize, image.height() as usize];
            let pixels = ColorImage::from_rgba_unmultiplied(size, image.as_raw());
            ctx.load_texture(format!("{} icon", icon.name), pixels, TextureOptions::LINEAR)
        })
    }
}

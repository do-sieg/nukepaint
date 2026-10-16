mod adjustment_dialog;
mod adjustments;
mod canvas;
mod checkerboard;
mod clipboard;
mod colors_panel;
mod composite;
mod curve_editor;
mod dialog;
mod dock;
mod effects;
mod image_strip;
mod frame_select;
mod grid;
mod grid_settings;
mod history_panel;
mod icons;
mod image_size;
mod lasso;
mod layer_commands;
mod layer_properties;
mod layers_panel;
mod navigator;
mod resample_preview;
mod settings;
mod shapes;
mod tiled_texture;
mod tools;
mod tools_panel;
mod transform;
mod view;

use std::path::{Path, PathBuf};

use adjustment_dialog::FilterKind;
use adjustments::{AdjustmentKind, LastSettings};
use canvas::{Drawing, GradientDrag};
use dock::{DockLayout, DockSide, Section};
use effects::EffectKind;
use eframe::egui::{self, ColorImage, Event, Key, KeyboardShortcut, Modifiers, TextureHandle, TextureOptions};
use checkerboard::Checkerboard;
use clipboard::Clipboard;
use composite::{Composite, Gpu};
use grid::{Cells, Grid};
use power_paint_core::{
    Affine, Document, FloatingPixels, GradientKind, GradientMode, adjust, Interpolation, MAX_SPRAY_SIZE, MAX_TIP_SIZE, OPEN_EXTENSIONS, Overlaps, PixelRect,
    RgbaImage, SAVE_EXTENSIONS, SelectionMode, is_savable, thumbnail,
};
use icons::{ICON_SIZE, Icon, Icons};
use lasso::{Lasso, LassoKind, PolygonEdit};
use layer_commands::LayerCommand;
use layer_properties::Properties;
use settings::Settings;
use shapes::ShapeEdit;
use tools::{SampleSource, Tool, Tools};
use transform::Transform;
use view::{View, ZOOM_LEVELS, format_zoom};

const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
const SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
const SAVE_AS: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::S);
const CLOSE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::W);
const QUIT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Q);
const UNDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
const REDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Y);
/// The other keys Redo goes by, in other applications.
const REDO_SHIFT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Z);
// Cut and the copies come as events of their own rather than as keys: these are for the menu.
const CUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::X);
const COPY: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::C);
const COPY_MERGED: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::C);
const PASTE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::V);
const PASTE_INTO_NEW_LAYER: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::V);
const PASTE_INTO_NEW_IMAGE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::ALT), Key::V);
const ERASE_SELECTION: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::Delete);
const SELECT_ALL: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::A);
const DESELECT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::D);
/// How many pixels the arrow keys nudge the selection's box by with Shift held.
const NUDGE_SHIFT_STEP: i32 = 10;
const INVERT_SELECTION: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::I);
const ZOOM_IN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Plus);
/// Plus is a shifted key on most keyboards, so the key it shares counts as well.
const ZOOM_IN_EQUALS: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Equals);
const ZOOM_OUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Minus);
const ZOOM_TO_WINDOW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::B);
const ACTUAL_SIZE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Num0);
const SHOW_GRID: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Quote);
const SNAP_TO_GRID: KeyboardShortcut = KeyboardShortcut::new(Modifiers::SHIFT, Key::S);

const APP_NAME: &str = "Power Paint";

/// Widest the list of recent files gets, in points.
const WELCOME_WIDTH: f32 = 600.0;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_NAME)
            .with_inner_size([1280.0, 800.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        APP_NAME,
        options,
        Box::new(|cc| {
            let render_state = cc.wgpu_render_state.as_ref().expect("eframe draws with wgpu");
            grid::init(render_state);
            resample_preview::init(render_state);
            Ok(Box::new(App::new(&cc.egui_ctx, Gpu::new(render_state))))
        }),
    )
}

/// A document with the textures that show it.
pub struct OpenImage {
    pub document: Document,
    /// The layers on the GPU, blended there.
    pub texture: Composite,
    /// Each layer shrunk for the layers list, in the order of the layers.
    pub layer_thumbnails: Vec<TextureHandle>,
    pub view: View,
    /// The selection being moved, resized or rotated, and its pixels with the selection tools.
    pub transform: Option<Transform>,
    /// What is being drawn on the active layer, until the button is let go.
    pub stroke: Option<Drawing>,
    /// The outline a lasso is drawing, until it is closed.
    pub lasso: Option<Lasso>,
    /// Polygon Select's corners, to go on changing.
    pub polygon: Option<PolygonEdit>,
    /// The shape a shape tool is drawing, or drew, to go on changing.
    pub shape: Option<ShapeEdit>,
    /// The gradient the Gradient is drawing on the active layer, until the button is let go.
    pub gradient: Option<GradientDrag>,
}

impl OpenImage {
    fn new(ctx: &egui::Context, gpu: &Gpu, document: Document) -> Self {
        let texture = Composite::new(gpu, &document.layers);
        let view = View::new(texture.size());
        let mut image = Self {
            document,
            texture,
            layer_thumbnails: Vec::new(),
            view,
            transform: None,
            stroke: None,
            lasso: None,
            polygon: None,
            shape: None,
            gradient: None,
        };
        image.refresh_layer_thumbnails(ctx);
        image
    }

    /// Shows the image again after the pixels of the active layer changed.
    pub fn refresh(&mut self, ctx: &egui::Context) {
        let active = self.document.active_layer;
        self.texture.update_layer(&self.document.layers, active, None);
        self.layer_thumbnails[active] = self.make_layer_thumbnail(ctx, active);
    }

    /// Shows an area of the image again after the pixels of the active layer changed
    /// there, while it is being drawn on. The thumbnails wait for [`OpenImage::refresh`].
    pub fn refresh_area(&mut self, area: PixelRect) {
        self.texture.update_layer(&self.document.layers, self.document.active_layer, Some(area));
    }

    /// Shows the image and its layers again after layers were added, removed, moved or
    /// changed.
    pub fn refresh_layers(&mut self, ctx: &egui::Context) {
        self.texture.replace_layers(&self.document.layers);
        // Cropping, resizing, and undoing either, change the size.
        self.view.set_image_size(self.texture.size());
        self.refresh_layer_thumbnails(ctx);
    }

    /// Shows the image again after how its layers are blended changed, not their pixels.
    pub fn refresh_image(&mut self) {
        self.texture.reblend(&self.document.layers);
    }

    fn refresh_layer_thumbnails(&mut self, ctx: &egui::Context) {
        self.layer_thumbnails = (0..self.document.layers.len()).map(|index| self.make_layer_thumbnail(ctx, index)).collect();
    }

    fn make_layer_thumbnail(&self, ctx: &egui::Context, index: usize) -> TextureHandle {
        let name = format!("{} layer {index}", self.document.name());
        make_thumbnail(ctx, &name, &self.document.layers[index].pixels, layers_panel::THUMBNAIL_SIZE)
    }

    fn is_dragging(&self) -> bool {
        self.transform.as_ref().is_some_and(Transform::is_dragging)
            || self.stroke.is_some()
            || self.gradient.is_some()
            || self.lasso.as_ref().is_some_and(|lasso| lasso.kind == LassoKind::Freehand)
            || self.polygon.as_ref().is_some_and(PolygonEdit::is_dragging)
            || self.shape.as_ref().is_some_and(ShapeEdit::is_dragging)
    }

    /// Shows an area of the layer the shape is on as the shape tool draws it there,
    /// before it is part of the layer.
    pub fn show_shape(&mut self, area: Option<PixelRect>) {
        if let Some(shape) = self.shape.as_ref().filter(|shape| shape.layer() < self.document.layers.len())
            && area.is_some()
        {
            self.texture.show_layer_as(&self.document.layers, shape.layer(), shape.pixels(), area);
        }
    }

    /// Leaves the shape a shape tool drew as it is, to change no more, or gives up the
    /// one it is drawing or changing, showing its layer as it is again.
    pub fn drop_shape(&mut self) {
        if let Some(shape) = self.shape.take()
            && (shape.is_drawing() || shape.is_dragging())
            && shape.layer() < self.document.layers.len()
        {
            self.texture.update_layer(&self.document.layers, shape.layer(), None);
        }
    }

    /// Gives up the outline a lasso is drawing, putting the selection back as it was.
    pub fn cancel_lasso(&mut self) {
        if let Some(lasso) = self.lasso.take()
            && lasso.is_current(&self.document)
        {
            lasso.cancel(&mut self.document);
        }
    }
}

/// What the View menu and its shortcuts do.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ViewCommand {
    ZoomIn,
    ZoomOut,
    ZoomToWindow,
    ActualSize,
    ShowGrid,
    SnapToGrid,
    GridSettings,
    SelectionAsGrid,
    Tiled,
    TileSelection,
    Checkerboard,
}

impl ViewCommand {
    /// The View menu, in groups that separators keep apart.
    const MENU: [&[Self]; 3] = [
        &[Self::ZoomIn, Self::ZoomOut, Self::ZoomToWindow, Self::ActualSize],
        &[Self::ShowGrid, Self::SnapToGrid, Self::GridSettings, Self::SelectionAsGrid],
        &[Self::Tiled, Self::TileSelection, Self::Checkerboard],
    ];

    fn label(self) -> &'static str {
        match self {
            Self::ZoomIn => "Zoom In",
            Self::ZoomOut => "Zoom Out",
            Self::ZoomToWindow => "Zoom to Window",
            Self::ActualSize => "Actual Size",
            Self::ShowGrid => "Show Grid",
            Self::SnapToGrid => "Snap to Grid",
            Self::GridSettings => "Grid Settings…",
            Self::SelectionAsGrid => "Selection as Grid",
            Self::Tiled => "Tiled",
            Self::TileSelection => "Tile Selection",
            Self::Checkerboard => "Checkerboard…",
        }
    }

    fn shortcut(self) -> Option<KeyboardShortcut> {
        match self {
            Self::ZoomIn => Some(ZOOM_IN),
            Self::ZoomOut => Some(ZOOM_OUT),
            Self::ZoomToWindow => Some(ZOOM_TO_WINDOW),
            Self::ActualSize => Some(ACTUAL_SIZE),
            Self::ShowGrid => Some(SHOW_GRID),
            Self::SnapToGrid => Some(SNAP_TO_GRID),
            Self::GridSettings | Self::SelectionAsGrid | Self::Tiled | Self::TileSelection | Self::Checkerboard => None,
        }
    }

    fn icon(self) -> Option<Icon> {
        match self {
            Self::ShowGrid => Some(icons::GRID),
            Self::SnapToGrid => Some(icons::GRID_SNAP),
            Self::GridSettings => Some(icons::GRID_SETTINGS),
            Self::Tiled => Some(icons::TILED),
            _ => None,
        }
    }
}

/// What the Edit menu and its shortcuts do.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EditCommand {
    Undo,
    Redo,
    Cut,
    Copy,
    CopyMerged,
    Paste,
    PasteIntoNewLayer,
    PasteIntoNewImage,
    EraseSelection,
    SelectAll,
    Deselect,
    InvertSelection,
}

impl EditCommand {
    /// The Edit menu, in groups that separators keep apart.
    const MENU: [&[Self]; 3] = [
        &[Self::Undo, Self::Redo],
        &[
            Self::Cut,
            Self::Copy,
            Self::CopyMerged,
            Self::Paste,
            Self::PasteIntoNewLayer,
            Self::PasteIntoNewImage,
            Self::EraseSelection,
        ],
        &[Self::SelectAll, Self::Deselect, Self::InvertSelection],
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Undo => "Undo",
            Self::Redo => "Redo",
            Self::Cut => "Cut",
            Self::Copy => "Copy",
            Self::CopyMerged => "Copy Merged",
            Self::Paste => "Paste",
            Self::PasteIntoNewLayer => "Paste in to New Layer",
            Self::PasteIntoNewImage => "Paste in to New Image",
            Self::EraseSelection => "Erase Selection",
            Self::SelectAll => "Select All",
            Self::Deselect => "Deselect",
            Self::InvertSelection => "Invert Selection",
        }
    }

    fn shortcut(self) -> KeyboardShortcut {
        match self {
            Self::Undo => UNDO,
            Self::Redo => REDO,
            Self::Cut => CUT,
            Self::Copy => COPY,
            Self::CopyMerged => COPY_MERGED,
            Self::Paste => PASTE,
            Self::PasteIntoNewLayer => PASTE_INTO_NEW_LAYER,
            Self::PasteIntoNewImage => PASTE_INTO_NEW_IMAGE,
            Self::EraseSelection => ERASE_SELECTION,
            Self::SelectAll => SELECT_ALL,
            Self::Deselect => DESELECT,
            Self::InvertSelection => INVERT_SELECTION,
        }
    }

    fn icon(self) -> Option<Icon> {
        match self {
            Self::Undo => Some(icons::UNDO),
            Self::Redo => Some(icons::REDO),
            Self::Cut => Some(icons::CUT),
            Self::Copy => Some(icons::COPY),
            Self::Paste => Some(icons::PASTE),
            Self::EraseSelection => Some(icons::ERASE),
            Self::SelectAll => Some(icons::SELECT_ALL),
            Self::Deselect => Some(icons::DESELECT),
            Self::InvertSelection => Some(icons::INVERT_SELECTION),
            Self::CopyMerged | Self::PasteIntoNewLayer | Self::PasteIntoNewImage => None,
        }
    }
}

/// What a button of the toolbar does.
#[derive(Clone, Copy)]
enum ToolbarButton {
    Open,
    Save,
    SaveAs,
    Edit(EditCommand),
    Layer(LayerCommand),
    View(ViewCommand),
}

/// The toolbar's buttons, in groups that separators keep apart.
const TOOLBAR: [&[ToolbarButton]; 7] = {
    use ToolbarButton::*;
    [
        &[Open, Save, SaveAs],
        &[Edit(EditCommand::Undo), Edit(EditCommand::Redo)],
        &[Edit(EditCommand::Cut), Edit(EditCommand::Copy), Edit(EditCommand::Paste), Edit(EditCommand::EraseSelection)],
        &[Edit(EditCommand::SelectAll), Edit(EditCommand::Deselect), Edit(EditCommand::InvertSelection)],
        &[Layer(LayerCommand::CropToSelection)],
        &[
            View(ViewCommand::ShowGrid),
            View(ViewCommand::SnapToGrid),
            View(ViewCommand::GridSettings),
        ],
        &[View(ViewCommand::Tiled)],
    ]
};

/// What is being closed, waiting on the user to say what becomes of unsaved changes.
enum Closing {
    Image(usize),
    /// The whole app, once every image has been saved or given up on, as these ones were.
    Quit { discarded: Vec<usize> },
}

/// A save waiting on the user to agree to flatten the image, which the file cannot keep
/// the layers of.
struct FlattenPrompt {
    image: usize,
    path: PathBuf,
}

/// How a save went.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Saving {
    Saved,
    /// Given up on, or failed.
    NotSaved,
    /// Waiting on the user to agree to flatten the image.
    AskingToFlatten,
}

/// What the user chose to do with unsaved changes.
#[derive(Clone, Copy)]
enum UnsavedChoice {
    Save,
    Discard,
    Cancel,
}

struct App {
    gpu: Gpu,
    images: Vec<OpenImage>,
    current: Option<usize>,
    /// Set when the current image changes, so the strip brings it into view.
    scroll_to_current: bool,
    /// Files given on the command line, opened on the first frame.
    pending_paths: Vec<PathBuf>,
    /// Files that could not be opened, with why, until the user closes the message.
    open_errors: Vec<String>,
    /// Files that could not be saved, with why, until the user closes the message.
    save_errors: Vec<String>,
    /// Set while asking whether to save the changes of what is being closed.
    closing: Option<Closing>,
    /// Set while asking whether to flatten an image to save it.
    flatten_prompt: Option<FlattenPrompt>,
    /// Set once the user has said what to do with every unsaved change, for the window to close.
    may_quit: bool,
    /// What the window's title was last set to.
    title: String,
    settings: Settings,
    checker: TextureHandle,
    /// The pixel under the pointer, while it is over the image's view.
    pointer_pixel: Option<(i64, i64)>,
    icons: Icons,
    tools: Tools,
    /// The pixels to paste, from the app or other applications.
    clipboard: Clipboard,
    /// Set while the properties of a layer are being edited.
    layer_dialog: Option<layer_properties::Dialog>,
    /// Set while asking for a new size of an image or its canvas.
    size_dialog: Option<image_size::Dialog>,
    /// Set while the grid's settings are being changed.
    grid_dialog: Option<grid_settings::Dialog>,
    /// Set while the checkerboard behind the image is being changed.
    checkerboard_dialog: Option<checkerboard::Dialog>,
    /// Set while the settings of an adjustment or an effect are being tried out.
    adjustment_dialog: Option<adjustment_dialog::Dialog>,
    /// What each adjustment's and each effect's dialog opens with.
    last_adjustments: LastSettings,
}

impl App {
    fn new(ctx: &egui::Context, gpu: Gpu) -> Self {
        // Ctrl with plus, minus and 0 zoom the image, not the whole interface.
        ctx.options_mut(|options| options.zoom_with_keyboard = false);
        let settings = Settings::load();
        let tools = Tools {
            right_click: settings.right_click(),
            selection_controls: settings.selection_controls(),
            ..Tools::default()
        };
        Self {
            gpu,
            images: Vec::new(),
            current: None,
            scroll_to_current: false,
            pending_paths: std::env::args_os().skip(1).map(PathBuf::from).collect(),
            open_errors: Vec::new(),
            save_errors: Vec::new(),
            closing: None,
            flatten_prompt: None,
            may_quit: false,
            title: APP_NAME.to_owned(),
            settings,
            checker: canvas::checker_texture(ctx),
            pointer_pixel: None,
            icons: Icons::default(),
            tools,
            clipboard: Clipboard::new(),
            layer_dialog: None,
            size_dialog: None,
            grid_dialog: None,
            checkerboard_dialog: None,
            adjustment_dialog: None,
            last_adjustments: LastSettings::default(),
        }
    }

    fn show_open_dialog(&mut self, ctx: &egui::Context) {
        let mut dialog = rfd::FileDialog::new().add_filter("Images", OPEN_EXTENSIONS);
        if let Some(dir) = self.settings.last_dir() {
            dialog = dialog.set_directory(dir);
        }
        if let Some(paths) = dialog.pick_files() {
            self.open_paths(ctx, paths);
        }
    }

    /// Opens each file as an image of its own, the last one becoming the current
    /// image. A file already open is made current instead.
    fn open_paths(&mut self, ctx: &egui::Context, paths: Vec<PathBuf>) {
        for path in paths {
            // Kept whole in the recent files, whatever folder the app was started from.
            let path = path.canonicalize().unwrap_or(path);
            if let Some(index) = self.find_image(&path) {
                self.select(index);
                continue;
            }
            let document = match Document::open(&path) {
                Ok(document) => document,
                Err(error) => {
                    self.open_errors.push(format!("{}: {error}", path.display()));
                    self.settings.remove_recent_file(&path);
                    continue;
                }
            };
            if let Some(dir) = path.parent() {
                self.settings.set_last_dir(dir);
            }
            self.settings.add_recent_file(&path);
            self.add_image(ctx, document);
        }
    }

    /// Shows the document as the current image, after the others.
    fn add_image(&mut self, ctx: &egui::Context, document: Document) {
        self.images.push(OpenImage::new(ctx, &self.gpu, document));
        self.select(self.images.len() - 1);
    }

    fn find_image(&self, path: &Path) -> Option<usize> {
        let path = path.canonicalize().ok()?;
        self.images.iter().position(|image| {
            image.document.path.as_deref().and_then(|own| own.canonicalize().ok()).as_deref() == Some(path.as_path())
        })
    }

    fn select(&mut self, index: usize) {
        for image in &mut self.images {
            image.cancel_lasso();
            if image.shape.as_ref().is_some_and(ShapeEdit::is_drawing) {
                image.drop_shape();
            }
        }
        self.current = Some(index);
        self.scroll_to_current = true;
    }

    /// Saves the image to its own file, or to one the user picks when `ask_path` is set
    /// or its own file cannot be saved to. None of the formats keep layers yet, so an
    /// image of more than one plain layer is only saved once the user agrees to flatten it.
    fn save(&mut self, index: usize, ask_path: bool) -> Saving {
        let document = &self.images[index].document;
        let path = match &document.path {
            Some(path) if !ask_path && is_savable(path) => path.clone(),
            _ => match self.pick_save_path(document) {
                Some(path) => path,
                None => return Saving::NotSaved,
            },
        };
        if !document.is_flat() {
            self.flatten_prompt = Some(FlattenPrompt { image: index, path });
            return Saving::AskingToFlatten;
        }
        if self.write(index, &path, false) { Saving::Saved } else { Saving::NotSaved }
    }

    /// Writes the image to the file, then blends its layers into one with `flatten`, as
    /// the file holds them. Returns whether it was written.
    fn write(&mut self, index: usize, path: &Path, flatten: bool) -> bool {
        if let Err(error) = self.images[index].document.save(path, flatten) {
            self.save_errors.push(format!("{}: {error}", path.display()));
            return false;
        }
        if let Some(dir) = path.parent() {
            self.settings.set_last_dir(dir);
        }
        self.settings.add_recent_file(path);
        true
    }

    /// Asks where to save the document. A name without an extension that can be saved
    /// to is saved as PNG.
    fn pick_save_path(&self, document: &Document) -> Option<PathBuf> {
        let own = document.path.as_deref();
        let name = match own {
            Some(path) if is_savable(path) => document.name(),
            _ => {
                let name = document.name();
                let stem = Path::new(&name).file_stem().map(|stem| stem.to_string_lossy().into_owned());
                format!("{}.{}", stem.unwrap_or(name), SAVE_EXTENSIONS[0])
            }
        };
        let mut dialog = rfd::FileDialog::new()
            .set_file_name(name)
            .add_filter("Images", SAVE_EXTENSIONS)
            .add_filter("PNG", &["png"])
            .add_filter("JPEG", &["jpg", "jpeg"])
            .add_filter("WebP", &["webp"])
            .add_filter("BMP", &["bmp"])
            .add_filter("TGA", &["tga"]);
        if let Some(dir) = own.and_then(Path::parent).or(self.settings.last_dir()) {
            dialog = dialog.set_directory(dir);
        }
        let path = dialog.save_file()?;
        if is_savable(&path) {
            return Some(path);
        }
        let mut path = path.into_os_string();
        path.push(".");
        path.push(SAVE_EXTENSIONS[0]);
        Some(PathBuf::from(path))
    }

    /// Closes the image, first asking whether to save its changes if it has any.
    fn request_close(&mut self, index: usize) {
        if self.images[index].document.is_unsaved() {
            self.closing = Some(Closing::Image(index));
        } else {
            self.close(index);
        }
    }

    /// Closes the image. The one after it becomes current, or the last one if it was last.
    fn close(&mut self, index: usize) {
        self.images.remove(index);
        self.current = match self.current {
            _ if self.images.is_empty() => None,
            Some(current) if current > index => Some(current - 1),
            Some(current) if current == index => Some(index.min(self.images.len() - 1)),
            other => other,
        };
    }

    fn file_menu(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.menu_button("File", |ui| {
            if ui.add(egui::Button::new("Open…").shortcut_text(ctx.format_shortcut(&OPEN))).clicked() {
                self.show_open_dialog(&ctx);
            }
            ui.menu_button("Open Recent", |ui| {
                let mut opened = None;
                for path in self.settings.recent_files() {
                    if ui.button(path.display().to_string()).clicked() {
                        opened = Some(path.clone());
                    }
                }
                if !self.settings.recent_files().is_empty() {
                    ui.separator();
                }
                let clear = egui::Button::new("Clear Recent Files");
                if ui.add_enabled(!self.settings.recent_files().is_empty(), clear).clicked() {
                    self.settings.clear_recent_files();
                }
                if let Some(path) = opened {
                    self.open_paths(&ctx, vec![path]);
                }
            });
            ui.separator();
            let save = egui::Button::new("Save").shortcut_text(ctx.format_shortcut(&SAVE));
            if ui.add_enabled(self.current.is_some(), save).clicked() {
                self.save(self.current.unwrap(), false);
            }
            let save_as = egui::Button::new("Save As…").shortcut_text(ctx.format_shortcut(&SAVE_AS));
            if ui.add_enabled(self.current.is_some(), save_as).clicked() {
                self.save(self.current.unwrap(), true);
            }
            ui.separator();
            let close = egui::Button::new("Close").shortcut_text(ctx.format_shortcut(&CLOSE));
            if ui.add_enabled(self.current.is_some(), close).clicked() {
                self.request_close(self.current.unwrap());
            }
            if ui.add(egui::Button::new("Quit").shortcut_text(ctx.format_shortcut(&QUIT))).clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }

    fn current_view(&mut self) -> Option<&mut View> {
        self.current.map(|current| &mut self.images[current].view)
    }

    fn can_run(&self, command: ViewCommand) -> bool {
        match command {
            ViewCommand::ShowGrid | ViewCommand::SnapToGrid => true,
            ViewCommand::GridSettings | ViewCommand::Checkerboard => !self.has_dialog(),
            ViewCommand::SelectionAsGrid => self.selection_rect().is_some(),
            ViewCommand::Tiled => self.current.is_some(),
            // Or to turn it off again, once the selection is gone.
            ViewCommand::TileSelection => self.selection_rect().is_some() || self.is_on(command) == Some(true),
            ViewCommand::ZoomIn | ViewCommand::ZoomOut | ViewCommand::ZoomToWindow | ViewCommand::ActualSize => {
                let Some(current) = self.current else {
                    return false;
                };
                let view = &self.images[current].view;
                match command {
                    ViewCommand::ZoomIn => view.can_zoom_in(),
                    ViewCommand::ZoomOut => view.can_zoom_out(),
                    _ => true,
                }
            }
        }
    }

    /// What a View button of the toolbar runs, and whether it shows as on. Tiled
    /// tiles the selection if there is one, and stands for either way of tiling, which
    /// it turns off.
    fn toolbar_view_command(&self, command: ViewCommand) -> (ViewCommand, Option<bool>) {
        if command != ViewCommand::Tiled {
            return (command, self.is_on(command));
        }
        let selection_tiled = self.is_on(ViewCommand::TileSelection) == Some(true);
        let tiled = self.current.is_some_and(|current| self.images[current].view.tiled());
        let tile_selection = selection_tiled || (!tiled && self.selection_rect().is_some());
        (if tile_selection { ViewCommand::TileSelection } else { command }, Some(tiled))
    }

    fn run(&mut self, command: ViewCommand) {
        if !self.can_run(command) {
            return;
        }
        let mut grid = self.settings.grid().clone();
        match command {
            ViewCommand::ShowGrid => grid.visible = !grid.visible,
            ViewCommand::SnapToGrid => grid.snap = !grid.snap,
            ViewCommand::GridSettings => {
                self.grid_dialog = Some(grid_settings::Dialog::new(grid));
                return;
            }
            ViewCommand::Checkerboard => {
                self.checkerboard_dialog = Some(checkerboard::Dialog::new(self.settings.checkerboard().clone()));
                return;
            }
            ViewCommand::SelectionAsGrid => {
                // One cell is the rectangle around the selection, and the others line up with it.
                let cells = Cells::around(self.selection_rect().unwrap());
                (grid.origin, grid.cell, grid.visible) = (cells.origin, cells.size, true);
            }
            ViewCommand::Tiled | ViewCommand::TileSelection => {
                let on = self.is_on(command) == Some(true);
                let tile = self.selection_rect().filter(|_| command == ViewCommand::TileSelection);
                let view = self.current_view().unwrap();
                let tile = if on { None } else { Some(tile.unwrap_or(view.whole())) };
                view.set_tile(tile);
                // The selection that made the tile would only get in the way on it.
                if command == ViewCommand::TileSelection && !on {
                    self.change_selection(EditCommand::Deselect);
                }
                return;
            }
            ViewCommand::ZoomIn | ViewCommand::ZoomOut | ViewCommand::ZoomToWindow | ViewCommand::ActualSize => {
                let view = self.current_view().unwrap();
                match command {
                    ViewCommand::ZoomIn => view.zoom_in(),
                    ViewCommand::ZoomOut => view.zoom_out(),
                    ViewCommand::ZoomToWindow => view.zoom_to_window(),
                    _ => view.zoom_to(1.0),
                }
                return;
            }
        }
        self.settings.set_grid(grid);
    }

    /// Whether what the command turns on is on, for the commands that turn something on and off.
    fn is_on(&self, command: ViewCommand) -> Option<bool> {
        let grid = self.settings.grid();
        match command {
            ViewCommand::ShowGrid => Some(grid.visible),
            ViewCommand::SnapToGrid => Some(grid.snap),
            ViewCommand::Tiled | ViewCommand::TileSelection => {
                let view = &self.images[self.current?].view;
                let whole = view.tile() == Some(view.whole());
                Some(view.tiled() && whole == (command == ViewCommand::Tiled))
            }
            _ => None,
        }
    }

    /// The pixels the current image's selection covers, whole or in part, if anything is selected.
    fn selection_rect(&self) -> Option<PixelRect> {
        let bounds = self.images[self.current?].document.selection.bounds()?;
        // Rotating leaves edges a hair off whole pixels; that must not cost an extra pixel.
        let slack = 0.001;
        Some(PixelRect {
            left: (bounds.left + slack).floor() as i32,
            top: (bounds.top + slack).floor() as i32,
            right: (bounds.right - slack).ceil() as i32,
            bottom: (bounds.bottom - slack).ceil() as i32,
        })
    }

    /// The grid as it shows: as it is being tried out in its settings, or as the settings say.
    fn shown_grid(&self) -> Grid {
        self.grid_dialog.as_ref().map_or_else(|| self.settings.grid().clone(), |dialog| dialog.grid.clone())
    }

    /// The checkerboard as it shows: as it is being tried out in its dialog, or as the settings say.
    fn shown_checkerboard(&self) -> Checkerboard {
        let dialog = self.checkerboard_dialog.as_ref();
        dialog.map_or_else(|| self.settings.checkerboard().clone(), |dialog| dialog.checkerboard.clone())
    }

    /// Shows the checkerboard dialog while it is open, and keeps the checkerboard once the user agrees.
    fn checkerboard_dialog(&mut self, ctx: &egui::Context) {
        let Some(outcome) = self.checkerboard_dialog.as_mut().and_then(|dialog| dialog.show(ctx)) else {
            return;
        };
        let dialog = self.checkerboard_dialog.take().unwrap();
        if let checkerboard::Outcome::Accepted = outcome {
            self.settings.set_checkerboard(dialog.checkerboard);
        }
    }

    /// Shows the grid settings dialog while it is open, and keeps the grid once the user agrees.
    fn grid_dialog(&mut self, ctx: &egui::Context) {
        let Some(outcome) = self.grid_dialog.as_mut().and_then(|dialog| dialog.show(ctx)) else {
            return;
        };
        let dialog = self.grid_dialog.take().unwrap();
        if let grid_settings::Outcome::Accepted = outcome {
            self.settings.set_grid(dialog.grid);
        }
    }

    /// The row of buttons under the menus, for the commands with an icon. The ones that
    /// turn something on stay pressed while it is.
    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let mut clicked = None;
        ui.horizontal(|ui| {
            for (group, buttons) in TOOLBAR.iter().enumerate() {
                if group > 0 {
                    ui.separator();
                }
                for &button in *buttons {
                    let (icon, label, shortcut, enabled, on) = match button {
                        ToolbarButton::Open => (Some(icons::OPEN), "Open", Some(OPEN), true, None),
                        ToolbarButton::Save => (Some(icons::SAVE), "Save", Some(SAVE), self.current.is_some(), None),
                        ToolbarButton::SaveAs => {
                            (Some(icons::SAVE_AS), "Save As", Some(SAVE_AS), self.current.is_some(), None)
                        }
                        ToolbarButton::Edit(command) => {
                            (command.icon(), command.label(), Some(command.shortcut()), self.can_edit(command), None)
                        }
                        ToolbarButton::Layer(command) => {
                            (command.icon(), command.label(), command.shortcut(), self.can_run_layer(command), None)
                        }
                        ToolbarButton::View(command) => {
                            let (run, on) = self.toolbar_view_command(command);
                            (command.icon(), command.label(), command.shortcut(), self.can_run(run), on)
                        }
                    };
                    let texture = self.icons.get(&ctx, icon.expect("the toolbar's commands have an icon"));
                    // The icons are light gray lines, made the colour of the text to suit either theme.
                    let tint = ui.visuals().strong_text_color();
                    let image = egui::Image::new(texture).fit_to_exact_size(ICON_SIZE).tint(tint);
                    let widget = match on {
                        Some(on) => egui::Button::selectable(on, image),
                        None => egui::Button::image(image),
                    };
                    let label = label.trim_end_matches('…');
                    let tooltip = match shortcut {
                        Some(shortcut) => format!("{label} ({})", ctx.format_shortcut(&shortcut)),
                        None => label.to_owned(),
                    };
                    if ui.add_enabled(enabled, widget).on_hover_text(tooltip).clicked() {
                        clicked = Some(button);
                    }
                }
            }
        });
        match clicked {
            Some(ToolbarButton::Open) => self.show_open_dialog(&ctx),
            Some(ToolbarButton::Save | ToolbarButton::SaveAs) if let Some(current) = self.current => {
                self.save(current, matches!(clicked, Some(ToolbarButton::SaveAs)));
            }
            Some(ToolbarButton::Edit(command)) => self.edit(&ctx, command),
            Some(ToolbarButton::Layer(command)) => self.run_layer(&ctx, command),
            Some(ToolbarButton::View(command)) => self.run(self.toolbar_view_command(command).0),
            _ => {}
        }
    }

    fn view_menu(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.menu_button("View", |ui| {
            for commands in ViewCommand::MENU {
                for &command in commands {
                    // Those that turn something on stand out while it is.
                    let mut button = match self.is_on(command) {
                        Some(on) => egui::Button::selectable(on, command.label()),
                        None => egui::Button::new(command.label()),
                    };
                    if let Some(shortcut) = command.shortcut() {
                        button = button.shortcut_text(ctx.format_shortcut(&shortcut));
                    }
                    if ui.add_enabled(self.can_run(command), button).clicked() {
                        self.run(command);
                    }
                }
                ui.separator();
            }
            ui.menu_button("Dock", |ui| {
                for side in DockSide::ALL {
                    if ui.button(format!("All Sections on the {}", side.name())).clicked() {
                        let mut dock = self.settings.dock().clone();
                        dock.move_all(side);
                        self.settings.set_dock(dock);
                    }
                }
                ui.separator();
                if ui.button("Reset").clicked() {
                    self.settings.set_dock(DockLayout::default());
                }
            });
        });
    }

    fn can_edit(&self, command: EditCommand) -> bool {
        if command == EditCommand::PasteIntoNewImage {
            return self.clipboard.image().is_some();
        }
        let Some(current) = self.current else {
            return false;
        };
        let document = &self.images[current].document;
        match command {
            // Nor while a drag or the layer properties dialog would carry on with layers that are no longer there.
            EditCommand::Undo => self.can_go_to_step() && document.can_undo(),
            EditCommand::Redo => self.can_go_to_step() && document.can_redo(),
            // A drag would carry on with pixels that are no longer there, or not yet.
            EditCommand::Paste | EditCommand::PasteIntoNewLayer => self.clipboard.image().is_some() && !self.is_dragging(),
            // A drag would carry on with the selection it began with.
            EditCommand::Cut | EditCommand::EraseSelection | EditCommand::Deselect if self.is_dragging() => false,
            EditCommand::SelectAll | EditCommand::InvertSelection => !self.is_dragging(),
            _ => !document.selection.is_empty(),
        }
    }

    fn edit(&mut self, ctx: &egui::Context, command: EditCommand) {
        if !self.can_edit(command) {
            return;
        }
        match command {
            EditCommand::Undo | EditCommand::Redo => {
                let step = self.images[self.current.unwrap()].document.current_step();
                self.go_to_step(ctx, if command == EditCommand::Undo { step - 1 } else { step + 1 });
            }
            EditCommand::Paste => self.paste(ctx, false),
            EditCommand::PasteIntoNewLayer => self.paste(ctx, true),
            EditCommand::PasteIntoNewImage => self.add_image(ctx, Document::pasted(self.clipboard.image().unwrap().clone())),
            EditCommand::Cut | EditCommand::Copy | EditCommand::CopyMerged | EditCommand::EraseSelection => {
                self.copy_or_erase(ctx, command);
            }
            EditCommand::SelectAll | EditCommand::Deselect | EditCommand::InvertSelection => {
                self.change_selection(command);
            }
        }
    }

    fn change_selection(&mut self, command: EditCommand) {
        let image = &mut self.images[self.current.unwrap()];
        let document = &mut image.document;
        let (changed, name, icon) = match command {
            EditCommand::SelectAll => (document.select_all(), "Select All", icons::SELECT_ALL),
            EditCommand::Deselect => (document.deselect(), "Deselect", icons::DESELECT),
            _ => (document.invert_selection(), "Invert Selection", icons::INVERT_SELECTION),
        };
        if changed {
            commit_selection(document, name, icon);
            // A transform would go on with the selection as it was.
            image.transform = None;
        }
    }

    fn copy_or_erase(&mut self, ctx: &egui::Context, command: EditCommand) {
        let image = &mut self.images[self.current.unwrap()];
        let document = &mut image.document;
        let copied = match command {
            EditCommand::Cut | EditCommand::Copy => document.copy(),
            EditCommand::CopyMerged => document.copy_merged(),
            _ => None,
        };
        // Nothing replaces what was copied before when the selection is off the image.
        if let Some(copied) = copied {
            self.clipboard.set(copied);
        }
        if matches!(command, EditCommand::Cut | EditCommand::EraseSelection) && document.erase_selection() {
            let (name, icon) = if command == EditCommand::Cut { ("Cut", icons::CUT) } else { ("Erase Selection", icons::ERASE) };
            document.commit(name, Some(icon.name()));
            // A transform would go on from the pixels as they were before.
            image.transform = None;
            image.refresh(ctx);
        }
    }

    /// Pastes into the active layer, or a new one over it with `new_layer`, at the
    /// selection's top left corner or the image's without one. The pixels can then be
    /// moved, resized and rotated with the selection's box, over what is under them,
    /// until the selection changes some other way.
    fn paste(&mut self, ctx: &egui::Context, new_layer: bool) {
        let Some(current) = self.current else {
            return;
        };
        // Move Selection would move the outline of the pasted pixels, without them, and
        // the Magic Wand has no box to move them with.
        let tool = match self.tools.tool {
            Tool::MoveSelection | Tool::MagicWand => self.tools.box_select_tool(),
            tool => tool,
        };
        // Which ends any transform, putting down the pixels it held.
        self.set_tool(tool);
        let image = &mut self.images[current];
        let document = &mut image.document;
        if new_layer {
            document.add_layer();
        }
        let bounds = document.selection.bounds();
        let at = bounds.map_or((0, 0), |bounds| (bounds.left.floor() as i32, bounds.top.floor() as i32));
        let layer = &mut document.active_mut().pixels;
        let floating = FloatingPixels::pasted(layer, self.clipboard.image().unwrap().clone(), at);
        *layer = floating.place(&Affine::IDENTITY, Interpolation::Nearest);
        let transform = Transform::begin_pasted(ctx, floating, document);
        document.selection = transform.selection();
        let name = if new_layer { "Paste in to New Layer" } else { "Paste" };
        document.commit(name, Some(icons::PASTE.name()));
        image.transform = Some(transform);
        if new_layer {
            image.refresh_layers(ctx);
        } else {
            image.refresh(ctx);
        }
    }

    fn edit_menu(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.menu_button("Edit", |ui| {
            for (group, commands) in EditCommand::MENU.into_iter().enumerate() {
                if group > 0 {
                    ui.separator();
                }
                for &command in commands {
                    let button = egui::Button::new(command.label()).shortcut_text(ctx.format_shortcut(&command.shortcut()));
                    if ui.add_enabled(self.can_edit(command), button).clicked() {
                        self.edit(&ctx, command);
                    }
                }
            }
        });
    }

    /// Whether the current image can be put back as it was after another of its steps.
    fn can_go_to_step(&self) -> bool {
        self.current.is_some() && !self.is_dragging() && !self.has_dialog()
    }

    /// Puts the current image back as it was after one of its steps, undoing or redoing
    /// as many as that takes.
    fn go_to_step(&mut self, ctx: &egui::Context, index: usize) {
        if !self.can_go_to_step() {
            return;
        }
        let image = &mut self.images[self.current.unwrap()];
        if image.document.go_to_step(index) {
            // A transform would go on with pixels that are no longer there.
            image.transform = None;
            image.refresh_layers(ctx);
        }
    }

    /// A menu of layer commands, in groups that separators keep apart.
    fn layer_command_menu(&mut self, ui: &mut egui::Ui, title: &str, groups: &[&[LayerCommand]]) {
        let ctx = ui.ctx().clone();
        ui.menu_button(title, |ui| {
            for (group, commands) in groups.iter().enumerate() {
                if group > 0 {
                    ui.separator();
                }
                for &command in *commands {
                    let mut button = egui::Button::new(command.label());
                    if let Some(shortcut) = command.shortcut() {
                        button = button.shortcut_text(ctx.format_shortcut(&shortcut));
                    }
                    if ui.add_enabled(self.can_run_layer(command), button).clicked() {
                        self.run_layer(&ctx, command);
                    }
                }
            }
        });
    }

    fn can_run_layer(&self, command: LayerCommand) -> bool {
        let Some(current) = self.current else {
            return false;
        };
        // A drag would carry on with a layer that is no longer the active one.
        !self.is_dragging() && !self.has_dialog() && command.can_run(&self.images[current].document)
    }

    fn run_layer(&mut self, ctx: &egui::Context, command: LayerCommand) {
        if !self.can_run_layer(command) {
            return;
        }
        let current = self.current.unwrap();
        let kind = match command {
            LayerCommand::Properties => {
                self.edit_layer_properties(current, self.images[current].document.active_layer);
                return;
            }
            LayerCommand::Resize => Some(image_size::Kind::Resize),
            LayerCommand::CanvasSize => Some(image_size::Kind::CanvasSize),
            _ => None,
        };
        if let Some(kind) = kind {
            let size = self.images[current].document.size();
            self.size_dialog = Some(image_size::Dialog::new(current, kind, size));
            return;
        }
        let image = &mut self.images[current];
        // A transform would go on with the pixels of the layer it began on.
        image.transform = None;
        if command.run(&mut image.document) {
            image.document.commit(command.step_name(), command.icon().map(Icon::name));
            image.refresh_layers(ctx);
        }
    }

    /// Does what was done in the layers list.
    fn layers_panel_action(&mut self, ctx: &egui::Context, action: layers_panel::Action) {
        let Some(current) = self.current.filter(|_| !self.is_dragging() && !self.has_dialog()) else {
            return;
        };
        let image = &mut self.images[current];
        let document = &mut image.document;
        match action {
            layers_panel::Action::Select(index) => {
                if document.set_active_layer(index) {
                    image.transform = None;
                }
            }
            layers_panel::Action::SetVisible(index, visible) => {
                // The transform shows the layers as they were when it began.
                image.transform = None;
                document.layers[index].visible = visible;
                document.commit(if visible { "Layer Shown" } else { "Layer Hidden" }, Some(icons::LAYERS.name()));
                image.refresh_image();
            }
            layers_panel::Action::Move { from, to } => {
                image.transform = None;
                if document.move_layer(from, to) {
                    let icon = if to > from { icons::LAYER_UP } else { icons::LAYER_DOWN };
                    document.commit("Move Layer", Some(icon.name()));
                    image.refresh_layers(ctx);
                }
            }
            layers_panel::Action::Properties(index) => {
                if document.set_active_layer(index) {
                    image.transform = None;
                }
                self.edit_layer_properties(current, index);
            }
            layers_panel::Action::Run(command) => self.run_layer(ctx, command),
        }
    }

    fn edit_layer_properties(&mut self, image: usize, layer: usize) {
        // The transform shows the layers as they were when it began.
        self.images[image].transform = None;
        let properties = Properties::of(&self.images[image].document.layers[layer]);
        self.layer_dialog = Some(layer_properties::Dialog::new(image, layer, properties));
    }

    /// Shows the layer properties dialog while it is open, and the layer as it says.
    fn layer_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.layer_dialog else {
            return;
        };
        let Some(outcome) = dialog.show(ctx) else {
            return;
        };
        let image = &mut self.images[dialog.image];
        let layer = &mut image.document.layers[dialog.layer];
        match outcome {
            layer_properties::Outcome::Edited => dialog.properties.apply(layer),
            layer_properties::Outcome::Accepted => {
                dialog.properties.apply(layer);
                if dialog.properties != dialog.before {
                    image.document.commit("Layer Properties", Some(icons::LAYER_PROPERTIES.name()));
                }
                self.layer_dialog = None;
            }
            layer_properties::Outcome::Canceled => {
                dialog.before.apply(layer);
                self.layer_dialog = None;
            }
        }
        image.refresh_image();
    }

    /// Shows the dialog asking for a new size while it is open, and resizes the image
    /// once the user agrees.
    fn size_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.size_dialog else {
            return;
        };
        let Some(outcome) = dialog.show(ctx) else {
            return;
        };
        let dialog = self.size_dialog.take().unwrap();
        if let image_size::Outcome::Canceled = outcome {
            return;
        }
        let image = &mut self.images[dialog.image];
        let [width, height] = dialog.size;
        let document = &mut image.document;
        let resized = match dialog.kind {
            image_size::Kind::Resize => document.resize(width, height, dialog.interpolation),
            image_size::Kind::CanvasSize => document.resize_canvas(width, height, dialog.anchor),
        };
        if resized {
            document.commit(dialog.kind.title(), None);
            // A transform would go on with pixels that are no longer there.
            image.transform = None;
            image.refresh_layers(ctx);
        }
    }

    /// Whether a dialog is open, which nothing else may change the images under.
    fn has_dialog(&self) -> bool {
        self.layer_dialog.is_some()
            || self.size_dialog.is_some()
            || self.grid_dialog.is_some()
            || self.checkerboard_dialog.is_some()
            || self.adjustment_dialog.is_some()
    }

    /// Puts the layer back as it was and closes the layer properties dialog, if it is open.
    fn cancel_layer_dialog(&mut self) {
        if let Some(dialog) = self.layer_dialog.take() {
            let image = &mut self.images[dialog.image];
            dialog.before.apply(&mut image.document.layers[dialog.layer]);
            image.refresh_image();
        }
    }

    fn adjustments_menu(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.menu_button("Colors", |ui| {
            for kind in AdjustmentKind::MENU {
                let mut button = egui::Button::new(kind.label());
                if let Some(shortcut) = kind.shortcut() {
                    button = button.shortcut_text(ctx.format_shortcut(&shortcut));
                }
                if ui.add_enabled(self.can_adjust(), button).clicked() {
                    self.adjust(&ctx, FilterKind::Adjustment(kind));
                }
            }
        });
    }

    fn filters_menu(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.menu_button("Filters", |ui| {
            for (name, kinds) in EffectKind::MENUS {
                ui.menu_button(name, |ui| {
                    for &kind in kinds {
                        if ui.add_enabled(self.can_adjust(), egui::Button::new(format!("{}…", kind.name()))).clicked() {
                            self.adjust(&ctx, FilterKind::Effect(kind));
                        }
                    }
                });
            }
        });
    }

    fn can_adjust(&self) -> bool {
        self.current.is_some() && !self.is_dragging() && !self.has_dialog()
    }

    /// Adjusts the active layer of the current image, or applies an effect to it, inside
    /// the selection if there is one, at once for an adjustment without settings, or
    /// else once its dialog says so.
    fn adjust(&mut self, ctx: &egui::Context, kind: FilterKind) {
        if !self.can_adjust() {
            return;
        }
        let current = self.current.unwrap();
        let image = &mut self.images[current];
        // A transform would go on with the pixels of the layer as they were.
        image.transform = None;
        image.cancel_lasso();
        image.drop_shape();
        let document = &mut image.document;
        if let FilterKind::Adjustment(kind) = kind
            && !kind.has_dialog()
        {
            if document.adjust(&kind.adjustment(&[], &self.last_adjustments.curves)) {
                document.commit(kind.name(), None);
                image.refresh(ctx);
            }
            return;
        }
        let values = self.last_adjustments.values(kind);
        let curves = self.last_adjustments.curves.clone();
        let dialog = adjustment_dialog::Dialog::new(current, document.active_layer, kind, values, curves, document.selection_mask());
        self.adjustment_dialog = Some(dialog);
        self.preview_adjustment();
    }

    /// Shows the layer as the adjustment dialog's settings make it, at once for an
    /// adjustment, or once worked out for an effect.
    fn preview_adjustment(&mut self) {
        let Some(dialog) = &mut self.adjustment_dialog else {
            return;
        };
        let image = &mut self.images[dialog.image];
        let layers = &image.document.layers;
        let pixels = &layers[dialog.layer].pixels;
        match dialog.kind {
            FilterKind::Adjustment(kind) => {
                let preview = adjust(pixels, &kind.adjustment(&dialog.values, &dialog.curves), dialog.mask.as_ref());
                image.texture.show_layer_as(layers, dialog.layer, &preview, None);
                dialog.preview = Some(preview);
            }
            // The one working out the settings before, if any, gives up.
            FilterKind::Effect(kind) => {
                let effect = kind.effect(&dialog.values);
                dialog.job = Some(adjustment_dialog::Job::start(pixels.clone(), effect, dialog.mask.clone()));
            }
        }
    }

    /// Shows the layer as the effect being worked out makes it, once it is.
    fn poll_effect(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.adjustment_dialog else {
            return;
        };
        let Some(job) = &dialog.job else {
            return;
        };
        let Some(preview) = job.poll() else {
            ctx.request_repaint_after(std::time::Duration::from_millis(15));
            return;
        };
        dialog.job = None;
        let image = &mut self.images[dialog.image];
        image.texture.show_layer_as(&image.document.layers, dialog.layer, &preview, None);
        dialog.preview = Some(preview);
    }

    /// Shows the adjustment dialog while it is open, and the layer as it says.
    fn adjustment_dialog(&mut self, ctx: &egui::Context) {
        self.poll_effect(ctx);
        let Some(dialog) = &mut self.adjustment_dialog else {
            return;
        };
        match dialog.show(ctx) {
            None => {}
            Some(adjustment_dialog::Outcome::Edited) => self.preview_adjustment(),
            Some(adjustment_dialog::Outcome::Accepted) => {
                let mut dialog = self.adjustment_dialog.take().unwrap();
                if let Some(job) = dialog.job.take() {
                    dialog.preview = job.wait();
                }
                let image = &mut self.images[dialog.image];
                let layer = &mut image.document.layers[dialog.layer];
                if let Some(preview) = dialog.preview
                    && preview != layer.pixels
                {
                    layer.pixels = preview;
                    image.document.commit(dialog.kind.name(), None);
                }
                image.refresh(ctx);
                self.last_adjustments.set_values(dialog.kind, dialog.values);
                if dialog.kind == FilterKind::Adjustment(AdjustmentKind::Curves) {
                    self.last_adjustments.curves = dialog.curves;
                }
            }
            Some(adjustment_dialog::Outcome::Canceled) => self.cancel_adjustment_dialog(),
        }
    }

    /// Shows the layer as it is again and closes the adjustment dialog, if it is open.
    fn cancel_adjustment_dialog(&mut self) {
        if let Some(dialog) = self.adjustment_dialog.take() {
            let image = &mut self.images[dialog.image];
            image.texture.update_layer(&image.document.layers, dialog.layer, None);
        }
    }

    /// The row under the toolbar: the current tool, and its settings.
    fn tool_options(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(self.tools.tool.name()).strong());
            if self.tools.tool.draws() {
                self.drawing_options(ui);
            } else if self.tools.tool.floods() {
                self.flood_options(ui);
            } else if self.tools.tool.shape().is_some() {
                self.shape_options(ui);
            } else if self.tools.tool == Tool::Gradient {
                self.gradient_options(ui);
            }
            if self.tools.tool.selects() {
                self.selection_options(ui);
            }
        });
    }

    /// The selection tools' settings: what selecting does, and how the pixels they move
    /// are redrawn.
    fn selection_options(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.label("Mode:");
        let name = |mode| match mode {
            SelectionMode::Replace => "Replace",
            SelectionMode::Add => "Add",
            SelectionMode::Subtract => "Subtract",
            SelectionMode::Intersect => "Intersect",
            SelectionMode::Invert => "Invert",
        };
        egui::ComboBox::from_id_salt("selection_mode")
            .selected_text(name(self.tools.selection_mode))
            .show_ui(ui, |ui| {
                for mode in SelectionMode::ALL {
                    ui.selectable_value(&mut self.tools.selection_mode, mode, name(mode));
                }
            })
            .response
            .on_hover_text(
                "What selecting does to what is already selected.\nWhatever this says, holding Ctrl adds to it.",
            );
        if self.tools.tool.lasso().is_some() {
            self.overlaps_option(
                ui,
                "What is selected where the outline crosses itself and goes around an area more than once. Add \
                 takes in all it goes around. Subtract leaves out what it goes around an even number of times.",
            );
        }
        // The Magic Wand moves no pixels.
        if !self.tools.tool.has_box() {
            return;
        }
        ui.separator();
        ui.label("Resampling:");
        let before = self.tools.resampling;
        egui::ComboBox::from_id_salt("resampling")
            .selected_text(self.tools.resampling.name())
            .show_ui(ui, |ui| {
                for interpolation in Interpolation::ALL {
                    ui.selectable_value(&mut self.tools.resampling, interpolation, interpolation.name());
                }
            })
            .response
            .on_hover_text(
                "How the selected pixels are redrawn once they are resized, rotated or moved by part of a pixel. \
                 Nearest Neighbor keeps their edges hard; Bicubic and Lanczos keep them sharper than Bilinear. \
                 Moving them by whole pixels always copies them as they are.",
            );
        if self.tools.resampling != before {
            self.resample_transforms(ui.ctx());
        }
    }

    /// Draws the pixels each transform last put down again, with the resampling now
    /// chosen, in place of the step that put them down.
    fn resample_transforms(&mut self, ctx: &egui::Context) {
        for image in &mut self.images {
            let document = &mut image.document;
            let Some(transform) = &image.transform else {
                continue;
            };
            if let Some(pixels) = transform.placed_again(self.tools.resampling, document.current_step()) {
                document.active_mut().pixels = pixels;
                document.amend();
                image.refresh(ctx);
            }
        }
    }

    /// What the lassos' outlines shut in, and the Contour and the Polygon fill, where they
    /// cross themselves.
    fn overlaps_option(&mut self, ui: &mut egui::Ui, help: &str) {
        ui.separator();
        ui.label("Overlaps:");
        egui::ComboBox::from_id_salt("overlaps")
            .selected_text(self.tools.overlaps.name())
            .show_ui(ui, |ui| {
                for overlaps in Overlaps::ALL {
                    ui.selectable_value(&mut self.tools.overlaps, overlaps, overlaps.name());
                }
            })
            .response
            .on_hover_text(help);
    }

    /// The shape tools' settings: how wide they draw, whether they smooth their edges,
    /// and what the Contour and the Polygon fill where they cross themselves.
    fn shape_options(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.label("Size:");
        let size = egui::DragValue::new(&mut self.tools.shape_size).range(1..=MAX_TIP_SIZE).suffix(" px");
        ui.add(size).on_hover_text("How wide the shapes' lines are.");
        self.antialiasing_option(ui);
        if matches!(self.tools.tool, Tool::Contour | Tool::Polygon) {
            self.overlaps_option(
                ui,
                "What is filled where the outline crosses itself and goes around an area more than once. Add \
                 fills all it goes around. Subtract leaves out what it goes around an even number of times.",
            );
        }
    }

    /// The Gradient's settings: how its colours spread out, what it does to the pixels,
    /// and whether the selection's controls show.
    fn gradient_options(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.label("Shape:");
        egui::ComboBox::from_id_salt("gradient_kind")
            .selected_text(self.tools.gradient_kind.name())
            .show_ui(ui, |ui| {
                for kind in GradientKind::ALL {
                    ui.selectable_value(&mut self.tools.gradient_kind, kind, kind.name());
                }
            })
            .response
            .on_hover_text(
                "How the colours spread out from where the drag starts. Linear goes across the drag, and Reflected \
                 goes both ways from its start. Radial makes circles, and Diamond squares on a corner, around its \
                 start, out to its end. Conical goes around its start, from the end's side to the other.",
            );
        ui.separator();
        ui.label("Mode:");
        egui::ComboBox::from_id_salt("gradient_mode")
            .selected_text(self.tools.gradient_mode.name())
            .show_ui(ui, |ui| {
                for mode in GradientMode::ALL {
                    ui.selectable_value(&mut self.tools.gradient_mode, mode, mode.name());
                }
            })
            .response
            .on_hover_text(
                "Color blends the gradient's colours over the layer. Transparency fades the layer out instead, \
                 from as it is where the drag starts to clear where it ends.",
            );
        self.selection_controls_option(ui);
    }

    /// The drawing tool's width and whether it smooths its edges, as far as it has them,
    /// and whether the selection's controls show.
    fn drawing_options(&mut self, ui: &mut egui::Ui) {
        let tool = self.tools.tool;
        if let Some(size) = self.tools.tip_size_mut() {
            ui.separator();
            ui.label("Size:");
            let max = if tool == Tool::Spray { MAX_SPRAY_SIZE } else { MAX_TIP_SIZE };
            let size = egui::DragValue::new(size).range(1..=max).suffix(" px");
            let verb = if tool.retouch().is_some() { "goes over" } else { "draws" };
            ui.add(size).on_hover_text(format!("How wide the {} {verb}.", tool.name()));
        }
        if tool == Tool::Spray {
            ui.separator();
            ui.label("Speed:");
            let speed = egui::DragValue::new(&mut self.tools.spray_speed).range(1..=100).suffix(" %");
            ui.add(speed).on_hover_text("How fast the Spray leaves dots, as a share of the fastest it can.");
        }
        if let Some(strength) = self.tools.strength_mut() {
            ui.separator();
            ui.label("Strength:");
            let help = match tool {
                Tool::Jumble => "The share of the pixels gone over that each pass swaps for one nearby.",
                Tool::Sharpen => "How much each pass sharpens. Going over the same place again sharpens it more.",
                _ => "How much each pass blurs. Going over the same place again blurs it more.",
            };
            ui.add(egui::DragValue::new(strength).range(1..=100).suffix(" %")).on_hover_text(help);
        }
        if tool.can_smooth() {
            self.antialiasing_option(ui);
        }
        self.selection_controls_option(ui);
    }

    /// Whether the selection's controls show, while there is a selection.
    fn selection_controls_option(&mut self, ui: &mut egui::Ui) {
        // Without a selection, there is nothing for them to control.
        let selected = self.current.is_some_and(|current| !self.images[current].document.selection.is_empty());
        if !selected {
            return;
        }
        ui.separator();
        let controls = ui.checkbox(&mut self.tools.selection_controls, "Selection controls").on_hover_text(
            "On, the selection's box shows while drawing or filling, to resize, move and rotate the selection from its \
             handles, with a button by it to deselect. Off, only its outline shows, and drawing by its edges \
             never grabs a handle.",
        );
        if controls.changed() {
            self.settings.set_selection_controls(self.tools.selection_controls);
        }
    }

    /// The Magic Wand's and the Paint Bucket's settings, which they share: which pixels
    /// they take in, and how the Paint Bucket fills them.
    fn flood_options(&mut self, ui: &mut egui::Ui) {
        let (taken, take) = if self.tools.tool == Tool::MagicWand { ("selected", "selects") } else { ("filled", "fills") };
        ui.separator();
        ui.checkbox(&mut self.tools.contiguous, "Contiguous").on_hover_text(format!(
            "On, only the pixels of similar colour that touch the one clicked through others like it are {taken}. \
             Off, all those in the image are. Holding Shift does the other.",
        ));
        ui.separator();
        ui.label("Tolerance:");
        let tolerance = egui::DragValue::new(&mut self.tools.tolerance).range(0..=100).suffix(" %");
        ui.add(tolerance).on_hover_text(format!(
            "How different a colour can be from the one clicked to be {taken}. 0% {take} only the same colour, \
             100% any.",
        ));
        ui.separator();
        ui.label("Sampling:");
        egui::ComboBox::from_id_salt("sample_source")
            .selected_text(self.tools.sample_source.name())
            .show_ui(ui, |ui| {
                for source in SampleSource::ALL {
                    ui.selectable_value(&mut self.tools.sample_source, source, source.name());
                }
            })
            .response
            .on_hover_text(
                "Whose colours are looked at: the active layer's, or those of all the visible layers blended together.",
            );
        if self.tools.tool == Tool::PaintBucket {
            self.antialiasing_option(ui);
            self.selection_controls_option(ui);
        }
    }

    fn antialiasing_option(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.checkbox(&mut self.tools.antialiasing, "Antialiasing").on_hover_text(
            "On, the Paintbrush, the Eraser and the shapes fade the edges of what they draw out over a pixel, the \
             Spray's dots can land between pixels, and the Paint Bucket fades the edge of what it fills over the \
             colours just too different. Off, their edges are hard, as the Pencil's always are.",
        );
    }

    /// Changes the tool, which ends any transform: the next one starts from the
    /// selection as it then is.
    fn set_tool(&mut self, tool: Tool) {
        if self.is_dragging() {
            return;
        }
        self.tools.set_tool(tool);
        for image in &mut self.images {
            image.transform = None;
            image.cancel_lasso();
            image.drop_shape();
        }
    }

    /// Whether a tool is in the middle of a drag, which must not change tools or images under it.
    fn is_dragging(&self) -> bool {
        self.tools.frame_select.is_dragging()
            || self.tools.zoom_drag.is_some()
            || self.images.iter().any(OpenImage::is_dragging)
    }

    /// The image's size, the pixel under the pointer, and a picker for the zoom.
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let Some(current) = self.current else {
            return;
        };
        let pointer_pixel = self.pointer_pixel;
        let image = &mut self.images[current];
        let (width, height) = image.document.size();
        let view = &mut image.view;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            egui::ComboBox::from_id_salt("zoom")
                .selected_text(format_zoom(view.zoom))
                .show_ui(ui, |ui| {
                    // Largest first, then a last row that fits the image in the window.
                    for &level in ZOOM_LEVELS.iter().rev() {
                        let selected = (view.zoom - level).abs() < 0.0001;
                        if ui.selectable_label(selected, format_zoom(level)).clicked() {
                            view.zoom_to(level);
                        }
                    }
                    ui.separator();
                    if ui.selectable_label(false, "Window").clicked() {
                        view.zoom_to_window();
                    }
                })
                .response
                .on_hover_text("Zoom");
            ui.separator();
            ui.label(format!("{width} × {height}"));
            if let Some((x, y)) = pointer_pixel {
                ui.separator();
                ui.label(format!("{x}, {y}"));
            }
        });
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        // Nothing else happens while the user is asked about unsaved changes, flattening or a layer's properties.
        if self.closing.is_some() || self.flatten_prompt.is_some() || self.has_dialog() {
            return;
        }
        let typing = ctx.memory(|memory| memory.focused().is_some());
        let view_shortcuts = ViewCommand::MENU
            .into_iter()
            .flatten()
            .filter_map(|&command| Some((command.shortcut()?, command)))
            .chain([(ZOOM_IN_EQUALS, ViewCommand::ZoomIn)]);
        for (shortcut, command) in view_shortcuts {
            // Snap to Grid takes S with Shift, which would otherwise go to the selection
            // tools' key, so it goes first, but not while typing.
            if (command != ViewCommand::SnapToGrid || !typing) && ctx.input_mut(|input| input.consume_shortcut(&shortcut)) {
                self.run(command);
            }
        }
        // Not while typing, nor while a drag would carry on with another tool than it began with.
        let busy = typing || self.is_dragging();
        if !busy && ctx.input_mut(|input| input.consume_shortcut(&tools::SELECT_KEY)) {
            self.set_tool(self.tools.next_select_tool());
        }
        // Tools that share a key, as the selection tools do, go from one to the next.
        let mut keys: Vec<KeyboardShortcut> = Vec::new();
        for tool in Tool::ALL.into_iter().filter(|tool| !tool.selects()) {
            if !keys.contains(&tool.key()) {
                keys.push(tool.key());
            }
        }
        for key in keys {
            if !busy && ctx.input_mut(|input| input.consume_shortcut(&key)) {
                self.set_tool(self.tools.next_tool(key));
            }
        }
        // The arrow keys nudge the selection's box, by ten pixels with Shift.
        if !busy && let Some(current) = self.current {
            let shift = ctx.input(|input| input.modifiers.shift);
            let step = if shift { NUDGE_SHIFT_STEP } else { 1 };
            let arrows = [
                (Key::ArrowLeft, (-1, 0)),
                (Key::ArrowRight, (1, 0)),
                (Key::ArrowUp, (0, -1)),
                (Key::ArrowDown, (0, 1)),
            ];
            for (key, (x, y)) in arrows {
                let presses = ctx.input_mut(|input| input.count_and_consume_key(Modifiers::NONE, key));
                for _ in 0..presses {
                    canvas::nudge(ctx, &mut self.images[current], &self.tools, (x * step, y * step));
                }
            }
        }
        if !busy && ctx.input_mut(|input| input.consume_shortcut(&colors_panel::SWAP_KEY)) {
            self.tools.colors.swap();
        }
        if !busy && ctx.input_mut(|input| input.consume_shortcut(&colors_panel::RESET_KEY)) {
            self.tools.colors.reset();
        }
        if !typing {
            // Undo would take Redo's other keys as well, so they go first.
            if ctx.input_mut(|input| input.consume_shortcut(&REDO_SHIFT) || input.consume_shortcut(&REDO)) {
                self.edit(ctx, EditCommand::Redo);
            }
            if ctx.input_mut(|input| input.consume_shortcut(&UNDO)) {
                self.edit(ctx, EditCommand::Undo);
            }
            let (cut, copy, shift, command) = ctx.input(|input| {
                let has = |wanted: &Event| input.events.iter().any(|event| event == wanted);
                (has(&Event::Cut), has(&Event::Copy), input.modifiers.shift, input.modifiers.command)
            });
            // Ctrl+Shift+X comes as a Cut event as well, rather than as keys, as does
            // Shift+Delete on Windows.
            if cut && shift && command {
                self.run_layer(ctx, LayerCommand::CropToSelection);
            } else if cut {
                self.edit(ctx, EditCommand::Cut);
            }
            if copy {
                self.edit(ctx, if shift { EditCommand::CopyMerged } else { EditCommand::Copy });
            }
            // Pressing Ctrl+V gives no event unless the system clipboard holds text, so
            // pasting waits for the key to be let go.
            let pasted = ctx.input(|input| {
                input.events.iter().find_map(|event| match event {
                    Event::Key { key: Key::V, pressed: false, modifiers, .. } if modifiers.command => Some(*modifiers),
                    _ => None,
                })
            });
            if let Some(modifiers) = pasted {
                let command = match () {
                    _ if modifiers.alt => EditCommand::PasteIntoNewImage,
                    _ if modifiers.shift => EditCommand::PasteIntoNewLayer,
                    _ => EditCommand::Paste,
                };
                self.edit(ctx, command);
            }
            // Before the layer commands, as Merge Layer Down takes M with Ctrl, which Curves takes with Shift as well.
            for kind in AdjustmentKind::MENU {
                if let Some(shortcut) = kind.shortcut()
                    && ctx.input_mut(|input| input.consume_shortcut(&shortcut))
                {
                    self.adjust(ctx, FilterKind::Adjustment(kind));
                }
            }
            for commands in LayerCommand::MENU.into_iter().chain(LayerCommand::IMAGE_MENU) {
                for &command in commands {
                    if let Some(shortcut) = command.shortcut()
                        && ctx.input_mut(|input| input.consume_shortcut(&shortcut))
                    {
                        self.run_layer(ctx, command);
                    }
                }
            }
            // After the layer commands, as Delete Layer takes Delete with Ctrl and Shift,
            // and Duplicate Layer D.
            let edit_shortcuts = [
                (ERASE_SELECTION, EditCommand::EraseSelection),
                (SELECT_ALL, EditCommand::SelectAll),
                (DESELECT, EditCommand::Deselect),
                (INVERT_SELECTION, EditCommand::InvertSelection),
            ];
            for (shortcut, command) in edit_shortcuts {
                if ctx.input_mut(|input| input.consume_shortcut(&shortcut)) {
                    self.edit(ctx, command);
                }
            }
        }
        if ctx.input_mut(|input| input.consume_shortcut(&OPEN)) {
            self.show_open_dialog(ctx);
        }
        // Not while a drag has yet to put the pixels where it takes them.
        if let Some(current) = self.current.filter(|_| !self.is_dragging()) {
            // Save would take Save As's keys as well, so it goes first.
            if ctx.input_mut(|input| input.consume_shortcut(&SAVE_AS)) {
                self.save(current, true);
            } else if ctx.input_mut(|input| input.consume_shortcut(&SAVE)) {
                self.save(current, false);
            }
        }
        if ctx.input_mut(|input| input.consume_shortcut(&CLOSE))
            && let Some(current) = self.current
        {
            self.request_close(current);
        }
        if ctx.input_mut(|input| input.consume_shortcut(&QUIT)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// What shows while no image is open: the recent files, to open one again.
    fn welcome(&mut self, ui: &mut egui::Ui) {
        let recent = self.settings.recent_files();
        let heading = if recent.is_empty() { "Open an image, or drop one here" } else { "Recent files" };
        let mut opened = None;
        let width = ui.available_width().min(WELCOME_WIDTH);
        // Roughly halfway down, the list growing from there.
        ui.add_space(ui.available_height() / 3.0);
        ui.vertical_centered(|ui| {
            ui.set_max_width(width);
            ui.heading(heading);
            ui.add_space(8.0);
            // A column as wide as the list, its file names lined up on the left.
            let column = egui::Vec2::new(width, 0.0);
            ui.allocate_ui_with_layout(column, egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                for path in recent {
                    let name = path.file_name().map(|name| name.to_string_lossy()).unwrap_or_default();
                    let dir = path.parent().map(|dir| dir.display().to_string()).unwrap_or_default();
                    // A long path is cut short to the list's width.
                    let button = egui::Button::new(format!("{name}  —  {dir}")).frame(false).truncate();
                    if ui.add(button).on_hover_text(path.display().to_string()).clicked() {
                        opened = Some(path.clone());
                    }
                }
            });
        });
        if let Some(path) = opened {
            self.open_paths(ui.ctx(), vec![path]);
        }
    }

    /// Asks whether to save the changes of what is being closed, one image at a time,
    /// and closes it once the user has said.
    fn unsaved_prompt(&mut self, ctx: &egui::Context) {
        // An image being saved is first asked about flattening.
        if self.flatten_prompt.is_some() {
            return;
        }
        let index = match &self.closing {
            None => return,
            Some(Closing::Image(index)) => *index,
            Some(Closing::Quit { discarded }) => {
                let next = (0..self.images.len())
                    .find(|index| self.images[*index].document.is_unsaved() && !discarded.contains(index));
                let Some(index) = next else {
                    self.closing = None;
                    self.may_quit = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    return;
                };
                index
            }
        };
        // The image asked about is the one in view.
        if self.current != Some(index) {
            self.select(index);
        }

        let name = self.images[index].document.name();
        let response = dialog::show(ctx, "unsaved_changes", "Unsaved changes", dialog::DIM, |ui| {
            ui.label(format!("Save changes to {name}?"));
            let mut choice = None;
            dialog::buttons(ui, |ui| {
                let buttons =
                    [("Cancel", UnsavedChoice::Cancel), ("Don't Save", UnsavedChoice::Discard), ("Save", UnsavedChoice::Save)];
                for (label, button_choice) in buttons {
                    if ui.button(label).clicked() {
                        choice = Some(button_choice);
                    }
                }
            });
            choice
        });
        let choice = if response.should_close() { Some(UnsavedChoice::Cancel) } else { response.inner };

        let quitting = matches!(self.closing, Some(Closing::Quit { .. }));
        match choice {
            None => {}
            Some(UnsavedChoice::Cancel) => self.closing = None,
            Some(UnsavedChoice::Save) => match self.save(index, false) {
                Saving::Saved => self.close_saved(index),
                // Saving can be given up on as well, or fail.
                Saving::NotSaved => self.closing = None,
                // The flatten prompt carries on from here.
                Saving::AskingToFlatten => {}
            },
            Some(UnsavedChoice::Discard) if quitting => {
                if let Some(Closing::Quit { discarded }) = &mut self.closing {
                    discarded.push(index);
                }
            }
            Some(UnsavedChoice::Discard) => {
                self.closing = None;
                self.close(index);
            }
        }
    }

    /// Goes on closing once an image asked about has been saved. Quitting goes on to the
    /// next image with changes, closing none until then.
    fn close_saved(&mut self, index: usize) {
        if let Some(Closing::Image(_)) = self.closing {
            self.closing = None;
            self.close(index);
        }
    }

    /// Asks whether to flatten the image to save it, and saves it once the user agrees.
    /// The image is flattened once the file is written, to be as it is in the file.
    fn flatten_prompt(&mut self, ctx: &egui::Context) {
        let Some(prompt) = &self.flatten_prompt else {
            return;
        };
        let format = prompt.path.extension().map(|extension| extension.to_string_lossy().to_uppercase());
        let response = dialog::show(ctx, "flatten_prompt", "Flatten the image?", dialog::DIM, |ui| {
            ui.label(format!("{} files cannot keep layers.\nFlatten the image and save it?", format.unwrap_or_default()));
            dialog::buttons(ui, |ui| {
                let cancel = ui.button("Cancel").clicked();
                let flatten = ui.button("Flatten and Save").clicked();
                flatten.then_some(true).or(cancel.then_some(false))
            })
        });
        let flatten = if response.should_close() { Some(false) } else { response.inner };
        let Some(flatten) = flatten else {
            return;
        };
        let FlattenPrompt { image: index, path } = self.flatten_prompt.take().unwrap();
        if flatten && self.write(index, &path, true) {
            let image = &mut self.images[index];
            // A transform would go on with layers that are no longer there.
            image.transform = None;
            image.refresh_layers(ctx);
            self.close_saved(index);
        } else {
            self.closing = None;
        }
    }

    /// Shows the name of the current image in the window's title, marked when it has
    /// unsaved changes.
    fn update_title(&mut self, ctx: &egui::Context) {
        let title = match self.current {
            Some(current) => {
                let document = &self.images[current].document;
                let mark = if document.is_unsaved() { "*" } else { "" };
                format!("{mark}{} — {APP_NAME}", document.name())
            }
            None => APP_NAME.to_owned(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }
}

/// Records a change to the selection alone as a step, named after what made it, or
/// Deselect when nothing is left selected.
pub fn commit_selection(document: &mut Document, name: &str, icon: Icon) {
    let (name, icon) = if document.selection.is_empty() { ("Deselect", icons::DESELECT) } else { (name, icon) };
    document.commit(name, Some(icon.name()));
}

/// Shows why files could not be opened or saved, until the user closes the message.
fn show_errors(ctx: &egui::Context, id: &str, heading: &str, errors: &mut Vec<String>) {
    if errors.is_empty() {
        return;
    }
    let response = dialog::show(ctx, id, heading, dialog::DIM, |ui| {
        for error in errors.iter() {
            ui.label(error);
        }
        dialog::buttons(ui, |ui| ui.button("OK").clicked())
    });
    if response.inner || response.should_close() {
        errors.clear();
    }
}

/// The image shrunk to fit in a square of `size` points, sharp at the screen's scale.
fn make_thumbnail(ctx: &egui::Context, name: &str, image: &RgbaImage, size: f32) -> TextureHandle {
    let max_side = (size * ctx.pixels_per_point()).ceil() as u32;
    let small = thumbnail(image, max_side);
    let size = [small.width() as usize, small.height() as usize];
    let pixels = ColorImage::from_rgba_unmultiplied(size, small.as_raw());
    ctx.load_texture(format!("{name} thumbnail"), pixels, TextureOptions::LINEAR)
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.gpu.free_released();
        let ctx = ui.ctx().clone();
        let mut paths = std::mem::take(&mut self.pending_paths);
        ctx.input(|input| {
            let dropped = input.raw.dropped_files.iter().map(|file| file.path());
            paths.extend(dropped.filter(|path| !path.as_os_str().is_empty()).map(Path::to_path_buf));
        });
        if !paths.is_empty() {
            self.open_paths(&ctx, paths);
        }
        // Closing the window, from its title bar or with Quit, first asks about unsaved changes.
        let close_requested = ctx.input(|input| input.viewport().close_requested());
        if close_requested && !self.may_quit && self.images.iter().any(|image| image.document.is_unsaved()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if !matches!(self.closing, Some(Closing::Quit { .. })) {
                self.closing = Some(Closing::Quit { discarded: Vec::new() });
            }
        }
        // Other applications may have copied something while the window was away.
        if ctx.input(|input| input.events.contains(&Event::WindowFocused(true))) {
            self.clipboard.refresh();
        }
        self.handle_shortcuts(&ctx);

        let scroll_to_current = std::mem::take(&mut self.scroll_to_current);
        egui::Panel::top("top").show(ui, |ui| {
            // The strip goes in the menu bar, which takes the whole row.
            egui::MenuBar::new().ui(ui, |ui| {
                self.file_menu(ui);
                self.edit_menu(ui);
                self.layer_command_menu(ui, "Image", &LayerCommand::IMAGE_MENU);
                self.layer_command_menu(ui, "Layers", &LayerCommand::MENU);
                self.adjustments_menu(ui);
                self.filters_menu(ui);
                self.view_menu(ui);
                let action = image_strip::show(ui, &self.images, self.current, scroll_to_current, &self.checker);
                match action {
                    Some(image_strip::Action::Select(index)) => self.select(index),
                    Some(image_strip::Action::Close(index)) => self.request_close(index),
                    None => {}
                }
            });
        });

        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui));
        // The tools have nothing to work on until an image is open.
        if self.current.is_some() {
            egui::Panel::top("tool_options").show(ui, |ui| self.tool_options(ui));
        }
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));

        // There is nothing for the dock to work on without an image.
        if let Some(current) = self.current {
            let (mut tool_clicked, mut history_action, mut layers_action, mut navigated) = (None, None, None, None);
            let tool = self.tools.tool;
            let busy = self.is_dragging() || self.has_dialog();
            let image = &self.images[current];
            let (checker, icons) = (&self.checker, &mut self.icons);
            let (colors, right_click) = (&mut self.tools.colors, &mut self.tools.right_click);
            let right_click_before = *right_click;
            let moved = dock::show(ui, self.settings.dock(), |ui, section| match section {
                Section::Tools => tool_clicked = tools_panel::show(ui, icons, tool),
                Section::Colors => colors.show(ui, right_click),
                Section::History => {
                    let document = &image.document;
                    history_action = history_panel::show(ui, document, icons, !busy, scroll_to_current);
                }
                Section::Layers => {
                    let enabled = |command: LayerCommand| !busy && command.can_run(&image.document);
                    layers_action = layers_panel::show(ui, image, checker, icons, enabled);
                }
                Section::Navigator => navigated = navigator::show(ui, image, checker),
            });
            if self.tools.right_click != right_click_before {
                self.settings.set_right_click(self.tools.right_click);
            }
            if let Some(dock) = moved {
                self.settings.set_dock(dock);
            }
            if let Some(tool) = tool_clicked {
                self.set_tool(tool);
            }
            if let Some(index) = history_action {
                self.go_to_step(&ctx, index);
            }
            if let Some(action) = layers_action {
                self.layers_panel_action(&ctx, action);
            }
            if let Some(action) = navigated {
                let view = &mut self.images[current].view;
                match action {
                    navigator::Action::CenterOn(point) => view.center_on(point),
                    navigator::Action::StepZoom(steps) => {
                        for _ in 0..steps.unsigned_abs() {
                            if steps > 0 { view.zoom_in() } else { view.zoom_out() }
                        }
                    }
                    navigator::Action::ZoomTo(zoom) => view.zoom_to(zoom),
                }
            }
        }

        // The image goes right up to the panels around it.
        let frame = egui::Frame::central_panel(ui.style()).inner_margin(0.0);
        let grid = self.shown_grid();
        let checker_size = self.shown_checkerboard().size;
        self.tools.snap = grid.snapping();
        egui::CentralPanel::default().frame(frame).show(ui, |ui| match self.current {
            Some(current) => {
                let image = &mut self.images[current];
                let output = canvas::show(ui, image, &mut self.tools, &mut self.icons, &self.checker, checker_size, &grid);
                self.pointer_pixel = output.pointer_pixel;
                if let Some(tool) = output.tool_requested {
                    self.set_tool(tool);
                }
                if output.deselect_requested {
                    self.edit(&ctx, EditCommand::Deselect);
                }
            }
            None => {
                self.pointer_pixel = None;
                self.welcome(ui);
            }
        });

        // Closing an image or the app first gives up on the layer's properties or the size being edited.
        if self.closing.is_some() {
            self.cancel_layer_dialog();
            self.size_dialog = None;
            self.grid_dialog = None;
            self.checkerboard_dialog = None;
            self.cancel_adjustment_dialog();
        }
        self.layer_dialog(&ctx);
        self.adjustment_dialog(&ctx);
        self.size_dialog(&ctx);
        self.grid_dialog(&ctx);
        self.checkerboard_dialog(&ctx);
        self.unsaved_prompt(&ctx);
        self.flatten_prompt(&ctx);
        show_errors(&ctx, "open_errors", "Could not open", &mut self.open_errors);
        show_errors(&ctx, "save_errors", "Could not save", &mut self.save_errors);
        self.update_title(&ctx);
    }
}

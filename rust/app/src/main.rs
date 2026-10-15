mod canvas;
mod dock;
mod image_strip;
mod frame_select;
mod history_panel;
mod icons;
mod layer_commands;
mod layer_properties;
mod layers_panel;
mod settings;
mod tiled_texture;
mod tools;
mod transform;
mod view;

use std::path::{Path, PathBuf};

use dock::DockSide;
use eframe::egui::{self, ColorImage, Event, Key, KeyboardShortcut, Modifiers, TextureHandle, TextureOptions};
use power_paint_core::{
    Affine, Document, FloatingPixels, OPEN_EXTENSIONS, RgbaImage, SAVE_EXTENSIONS, SelectionMode, is_savable,
    thumbnail,
};
use icons::{ICON_SIZE, Icon, Icons};
use layer_commands::LayerCommand;
use layer_properties::Properties;
use settings::Settings;
use tiled_texture::TiledTexture;
use tools::{Tool, Tools};
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
const ZOOM_IN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Plus);
/// Plus is a shifted key on most keyboards, so the key it shares counts as well.
const ZOOM_IN_EQUALS: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Equals);
const ZOOM_OUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Minus);
const ZOOM_TO_WINDOW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::B);
const ACTUAL_SIZE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Num0);

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
    eframe::run_native(APP_NAME, options, Box::new(|cc| Ok(Box::new(App::new(&cc.egui_ctx)))))
}

/// A document with the textures that show it.
pub struct OpenImage {
    pub document: Document,
    pub texture: TiledTexture,
    /// The image shrunk for the image strip, as textures are not mipmapped.
    pub thumbnail: TextureHandle,
    /// Each layer shrunk for the layers list, in the order of the layers.
    pub layer_thumbnails: Vec<TextureHandle>,
    pub view: View,
    /// The selection being moved, resized or rotated, and its pixels with the selection tools.
    pub transform: Option<Transform>,
}

impl OpenImage {
    fn new(ctx: &egui::Context, document: Document) -> Self {
        let flat = document.flattened();
        let texture = TiledTexture::new(ctx, &document.name(), &flat);
        let view = View::new(texture.size());
        let thumbnail = make_thumbnail(ctx, &document.name(), &flat, image_strip::THUMBNAIL_SIZE);
        let mut image = Self { document, texture, thumbnail, layer_thumbnails: Vec::new(), view, transform: None };
        image.refresh_layer_thumbnails(ctx);
        image
    }

    /// Shows the image again after the pixels of the active layer changed.
    pub fn refresh(&mut self, ctx: &egui::Context) {
        self.refresh_image(ctx);
        let active = self.document.active_layer;
        self.layer_thumbnails[active] = self.make_layer_thumbnail(ctx, active);
    }

    /// Shows the image and its layers again after layers were added, removed, moved or
    /// changed.
    pub fn refresh_layers(&mut self, ctx: &egui::Context) {
        self.refresh_image(ctx);
        self.refresh_layer_thumbnails(ctx);
    }

    /// Shows the image again after how its layers are blended changed, not their pixels.
    pub fn refresh_image(&mut self, ctx: &egui::Context) {
        let flat = self.document.flattened();
        let name = self.document.name();
        self.texture = TiledTexture::new(ctx, &name, &flat);
        self.thumbnail = make_thumbnail(ctx, &name, &flat, image_strip::THUMBNAIL_SIZE);
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
    }
}

/// What the View menu and its shortcuts do.
#[derive(Clone, Copy)]
enum ViewCommand {
    ZoomIn,
    ZoomOut,
    ZoomToWindow,
    ActualSize,
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
}

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
    /// The pixels copied last, kept in the app until the system clipboard takes images.
    clipboard: Option<RgbaImage>,
    /// Set while the properties of a layer are being edited.
    layer_dialog: Option<layer_properties::Dialog>,
}

impl App {
    fn new(ctx: &egui::Context) -> Self {
        // Ctrl with plus, minus and 0 zoom the image, not the whole interface.
        ctx.options_mut(|options| options.zoom_with_keyboard = false);
        Self {
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
            settings: Settings::load(),
            checker: canvas::checker_texture(ctx),
            pointer_pixel: None,
            icons: Icons::default(),
            tools: Tools::default(),
            clipboard: None,
            layer_dialog: None,
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
        self.images.push(OpenImage::new(ctx, document));
        self.select(self.images.len() - 1);
    }

    fn find_image(&self, path: &Path) -> Option<usize> {
        let path = path.canonicalize().ok()?;
        self.images.iter().position(|image| {
            image.document.path.as_deref().and_then(|own| own.canonicalize().ok()).as_deref() == Some(path.as_path())
        })
    }

    fn select(&mut self, index: usize) {
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
        let Some(current) = self.current else {
            return false;
        };
        let view = &self.images[current].view;
        match command {
            ViewCommand::ZoomIn => view.can_zoom_in(),
            ViewCommand::ZoomOut => view.can_zoom_out(),
            ViewCommand::ZoomToWindow | ViewCommand::ActualSize => true,
        }
    }

    fn run(&mut self, command: ViewCommand) {
        let Some(view) = self.current_view() else {
            return;
        };
        match command {
            ViewCommand::ZoomIn => view.zoom_in(),
            ViewCommand::ZoomOut => view.zoom_out(),
            ViewCommand::ZoomToWindow => view.zoom_to_window(),
            ViewCommand::ActualSize => view.zoom_to(1.0),
        }
    }

    fn view_menu(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.menu_button("View", |ui| {
            let items = [
                ("Zoom In", ZOOM_IN, ViewCommand::ZoomIn),
                ("Zoom Out", ZOOM_OUT, ViewCommand::ZoomOut),
                ("Zoom to Window", ZOOM_TO_WINDOW, ViewCommand::ZoomToWindow),
                ("Actual Size", ACTUAL_SIZE, ViewCommand::ActualSize),
            ];
            for (label, shortcut, command) in items {
                let button = egui::Button::new(label).shortcut_text(ctx.format_shortcut(&shortcut));
                if ui.add_enabled(self.can_run(command), button).clicked() {
                    self.run(command);
                }
            }
            ui.separator();
            ui.menu_button("Dock", |ui| {
                for side in DockSide::ALL {
                    let selected = self.settings.dock_side() == side;
                    if ui.radio(selected, format!("On the {}", side.name())).clicked() {
                        self.settings.set_dock_side(side);
                    }
                }
            });
        });
    }

    fn can_edit(&self, command: EditCommand) -> bool {
        if command == EditCommand::PasteIntoNewImage {
            return self.clipboard.is_some();
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
            EditCommand::Paste | EditCommand::PasteIntoNewLayer => self.clipboard.is_some() && !self.is_dragging(),
            EditCommand::Cut | EditCommand::EraseSelection if self.is_dragging() => false,
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
            EditCommand::PasteIntoNewImage => self.add_image(ctx, Document::pasted(self.clipboard.clone().unwrap())),
            EditCommand::Cut | EditCommand::Copy | EditCommand::CopyMerged | EditCommand::EraseSelection => {
                self.copy_or_erase(ctx, command);
            }
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
        if copied.is_some() {
            self.clipboard = copied;
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
        // Move Selection would move the outline of the pasted pixels, without them.
        let tool = if self.tools.tool == Tool::MoveSelection { self.tools.last_select_tool } else { self.tools.tool };
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
        let floating = FloatingPixels::pasted(layer, self.clipboard.clone().unwrap(), at);
        *layer = floating.place(&Affine::IDENTITY);
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
            let items = [
                ("Undo", UNDO, EditCommand::Undo),
                ("Redo", REDO, EditCommand::Redo),
                ("Cut", CUT, EditCommand::Cut),
                ("Copy", COPY, EditCommand::Copy),
                ("Copy Merged", COPY_MERGED, EditCommand::CopyMerged),
                ("Paste", PASTE, EditCommand::Paste),
                ("Paste in to New Layer", PASTE_INTO_NEW_LAYER, EditCommand::PasteIntoNewLayer),
                ("Paste in to New Image", PASTE_INTO_NEW_IMAGE, EditCommand::PasteIntoNewImage),
                ("Erase Selection", ERASE_SELECTION, EditCommand::EraseSelection),
            ];
            for (label, shortcut, command) in items {
                if command == EditCommand::Cut {
                    ui.separator();
                }
                let button = egui::Button::new(label).shortcut_text(ctx.format_shortcut(&shortcut));
                if ui.add_enabled(self.can_edit(command), button).clicked() {
                    self.edit(&ctx, command);
                }
            }
        });
    }

    /// Whether the current image can be put back as it was after another of its steps.
    fn can_go_to_step(&self) -> bool {
        self.current.is_some() && !self.is_dragging() && self.layer_dialog.is_none()
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
        !self.is_dragging() && self.layer_dialog.is_none() && command.can_run(&self.images[current].document)
    }

    fn run_layer(&mut self, ctx: &egui::Context, command: LayerCommand) {
        if !self.can_run_layer(command) {
            return;
        }
        let current = self.current.unwrap();
        if command == LayerCommand::Properties {
            self.edit_layer_properties(current, self.images[current].document.active_layer);
            return;
        }
        let image = &mut self.images[current];
        // A transform would go on with the pixels of the layer it began on.
        image.transform = None;
        if command.run(&mut image.document) {
            image.document.commit(command.label().trim_end_matches('…'), command.icon().map(Icon::name));
            image.refresh_layers(ctx);
        }
    }

    /// Does what was done in the layers list.
    fn layers_panel_action(&mut self, ctx: &egui::Context, action: layers_panel::Action) {
        let Some(current) = self.current.filter(|_| !self.is_dragging() && self.layer_dialog.is_none()) else {
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
                image.refresh_image(ctx);
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
        image.refresh_image(ctx);
    }

    /// Puts the layer back as it was and closes the layer properties dialog, if it is open.
    fn cancel_layer_dialog(&mut self, ctx: &egui::Context) {
        if let Some(dialog) = self.layer_dialog.take() {
            let image = &mut self.images[dialog.image];
            dialog.before.apply(&mut image.document.layers[dialog.layer]);
            image.refresh_image(ctx);
        }
    }

    /// The settings of the tool, under the menu bar.
    fn tool_options(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for tool in Tool::ALL {
                let texture = self.icons.get(ui.ctx(), tool.icon());
                // The icons are light gray lines, made the colour of the text to suit either theme.
                let tint = ui.visuals().strong_text_color();
                let icon = egui::Image::new(texture).fit_to_exact_size(ICON_SIZE).tint(tint);
                let button = egui::Button::selectable(self.tools.tool == tool, icon);
                let key = ui.ctx().format_shortcut(&tool.key());
                let tooltip = format!("{} ({key})\n{}", tool.name(), tool.help());
                if ui.add(button).on_hover_text(tooltip).clicked() {
                    self.set_tool(tool);
                }
            }
            if self.tools.tool.select_shape().is_none() {
                return;
            }
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
        });
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
        }
    }

    /// Whether a tool is in the middle of a drag, which must not change tools or images under it.
    fn is_dragging(&self) -> bool {
        self.tools.frame_select.is_dragging() || self.images.iter().any(OpenImage::is_dragging)
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
        if self.closing.is_some() || self.flatten_prompt.is_some() || self.layer_dialog.is_some() {
            return;
        }
        let view_shortcuts = [
            (ZOOM_IN, ViewCommand::ZoomIn),
            (ZOOM_IN_EQUALS, ViewCommand::ZoomIn),
            (ZOOM_OUT, ViewCommand::ZoomOut),
            (ZOOM_TO_WINDOW, ViewCommand::ZoomToWindow),
            (ACTUAL_SIZE, ViewCommand::ActualSize),
        ];
        for (shortcut, command) in view_shortcuts {
            if ctx.input_mut(|input| input.consume_shortcut(&shortcut)) {
                self.run(command);
            }
        }
        // Not while typing, nor while a drag would carry on with another tool than it began with.
        let busy = ctx.memory(|memory| memory.focused().is_some()) || self.is_dragging();
        if !busy && ctx.input_mut(|input| input.consume_shortcut(&tools::SELECT_KEY)) {
            self.set_tool(self.tools.next_select_tool());
        }
        if !busy && ctx.input_mut(|input| input.consume_shortcut(&tools::MOVE_SELECTION_KEY)) {
            self.set_tool(Tool::MoveSelection);
        }
        let typing = ctx.memory(|memory| memory.focused().is_some());
        if !typing {
            // Undo would take Redo's other keys as well, so they go first.
            if ctx.input_mut(|input| input.consume_shortcut(&REDO_SHIFT) || input.consume_shortcut(&REDO)) {
                self.edit(ctx, EditCommand::Redo);
            }
            if ctx.input_mut(|input| input.consume_shortcut(&UNDO)) {
                self.edit(ctx, EditCommand::Undo);
            }
            let (cut, copy, shift) = ctx.input(|input| {
                let has = |wanted: &Event| input.events.iter().any(|event| event == wanted);
                (has(&Event::Cut), has(&Event::Copy), input.modifiers.shift)
            });
            if cut {
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
            for commands in LayerCommand::MENU.into_iter().chain(LayerCommand::IMAGE_MENU) {
                for &command in commands {
                    if let Some(shortcut) = command.shortcut()
                        && ctx.input_mut(|input| input.consume_shortcut(&shortcut))
                    {
                        self.run_layer(ctx, command);
                    }
                }
            }
            // After Delete Layer, which takes Delete with Ctrl and Shift.
            if ctx.input_mut(|input| input.consume_shortcut(&ERASE_SELECTION)) {
                self.edit(ctx, EditCommand::EraseSelection);
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
        let response = egui::Modal::new(egui::Id::new("unsaved_changes")).show(ctx, |ui| {
            ui.heading("Unsaved changes");
            ui.label(format!("Save changes to {name}?"));
            ui.add_space(8.0);
            let mut choice = None;
            ui.horizontal(|ui| {
                let buttons =
                    [("Save", UnsavedChoice::Save), ("Don't Save", UnsavedChoice::Discard), ("Cancel", UnsavedChoice::Cancel)];
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
        let response = egui::Modal::new(egui::Id::new("flatten_prompt")).show(ctx, |ui| {
            ui.heading("Flatten the image?");
            ui.label(format!("{} files cannot keep layers.\nFlatten the image and save it?", format.unwrap_or_default()));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let flatten = ui.button("Flatten and Save").clicked();
                let cancel = ui.button("Cancel").clicked();
                flatten.then_some(true).or(cancel.then_some(false))
            })
            .inner
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

/// Shows why files could not be opened or saved, until the user closes the message.
fn show_errors(ctx: &egui::Context, id: &str, heading: &str, errors: &mut Vec<String>) {
    if errors.is_empty() {
        return;
    }
    let response = egui::Modal::new(egui::Id::new(id)).show(ctx, |ui| {
        ui.heading(heading);
        for error in errors.iter() {
            ui.label(error);
        }
        ui.add_space(8.0);
        ui.button("OK").clicked()
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
        self.handle_shortcuts(&ctx);

        let scroll_to_current = std::mem::take(&mut self.scroll_to_current);
        egui::Panel::top("top").show(ui, |ui| {
            // The strip goes in the menu bar, which takes the whole row.
            egui::MenuBar::new().ui(ui, |ui| {
                self.file_menu(ui);
                self.edit_menu(ui);
                self.layer_command_menu(ui, "Image", &LayerCommand::IMAGE_MENU);
                self.layer_command_menu(ui, "Layers", &LayerCommand::MENU);
                self.view_menu(ui);
                let action = image_strip::show(ui, &self.images, self.current, scroll_to_current, &self.checker);
                match action {
                    Some(image_strip::Action::Select(index)) => self.select(index),
                    Some(image_strip::Action::Close(index)) => self.request_close(index),
                    None => {}
                }
            });
        });

        // The tools have nothing to work on until an image is open.
        if self.current.is_some() {
            egui::Panel::top("tool_options").show(ui, |ui| self.tool_options(ui));
        }
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));

        // The history and the layers are all the dock holds for now, which there are none of without an image.
        if let Some(current) = self.current {
            let (mut history_action, mut layers_action) = (None, None);
            let busy = self.is_dragging() || self.layer_dialog.is_some();
            let image = &self.images[current];
            let (checker, icons) = (&self.checker, &mut self.icons);
            let tooltip = |label: &str, shortcut| format!("{label} ({})", ctx.format_shortcut(&shortcut));
            let (undo, redo) = (tooltip("Undo", UNDO), tooltip("Redo", REDO));
            let moved = dock::show(ui, self.settings.dock_side(), |ui| {
                dock::section(ui, "History", |ui| {
                    let document = &image.document;
                    history_action = history_panel::show(ui, document, icons, !busy, scroll_to_current, &undo, &redo);
                });
                dock::section(ui, "Layers", |ui| {
                    let enabled = |command: LayerCommand| !busy && command.can_run(&image.document);
                    layers_action = layers_panel::show(ui, image, checker, icons, enabled);
                });
            });
            if let Some(side) = moved {
                self.settings.set_dock_side(side);
            }
            match history_action {
                Some(history_panel::Action::GoTo(index)) => self.go_to_step(&ctx, index),
                Some(history_panel::Action::Undo) => self.edit(&ctx, EditCommand::Undo),
                Some(history_panel::Action::Redo) => self.edit(&ctx, EditCommand::Redo),
                None => {}
            }
            if let Some(action) = layers_action {
                self.layers_panel_action(&ctx, action);
            }
        }

        // The image goes right up to the panels around it.
        let frame = egui::Frame::central_panel(ui.style()).inner_margin(0.0);
        egui::CentralPanel::default().frame(frame).show(ui, |ui| match self.current {
            Some(current) => {
                let image = &mut self.images[current];
                let output = canvas::show(ui, image, &mut self.tools, &mut self.icons, &self.checker);
                self.pointer_pixel = output.pointer_pixel;
                if let Some(tool) = output.tool_requested {
                    self.set_tool(tool);
                }
            }
            None => {
                self.pointer_pixel = None;
                self.welcome(ui);
            }
        });

        // Closing an image or the app first gives up on the layer's properties being edited.
        if self.closing.is_some() {
            self.cancel_layer_dialog(&ctx);
        }
        self.layer_dialog(&ctx);
        self.unsaved_prompt(&ctx);
        self.flatten_prompt(&ctx);
        show_errors(&ctx, "open_errors", "Could not open", &mut self.open_errors);
        show_errors(&ctx, "save_errors", "Could not save", &mut self.save_errors);
        self.update_title(&ctx);
    }
}

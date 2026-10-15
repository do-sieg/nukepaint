mod canvas;
mod image_strip;
mod frame_select;
mod icons;
mod settings;
mod tiled_texture;
mod tools;
mod transform;
mod view;

use std::path::{Path, PathBuf};

use eframe::egui::{self, ColorImage, Key, KeyboardShortcut, Modifiers, TextureHandle, TextureOptions};
use power_paint_core::{Document, OPEN_EXTENSIONS, SAVE_EXTENSIONS, SelectionMode, is_savable, thumbnail};
use icons::{ICON_SIZE, Icons};
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
    pub view: View,
    /// The selection being moved, resized or rotated, and its pixels with the selection tools.
    pub transform: Option<Transform>,
}

impl OpenImage {
    /// Shows the layers again after their pixels changed.
    pub fn refresh(&mut self, ctx: &egui::Context) {
        let flat = self.document.flattened();
        let name = self.document.name();
        self.texture = TiledTexture::new(ctx, &name, &flat);
        self.thumbnail = make_thumbnail(ctx, &name, &flat);
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

/// What is being closed, waiting on the user to say what becomes of unsaved changes.
enum Closing {
    Image(usize),
    /// The whole app, once every image has been saved or given up on, as these ones were.
    Quit { discarded: Vec<usize> },
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
            may_quit: false,
            title: APP_NAME.to_owned(),
            settings: Settings::load(),
            checker: canvas::checker_texture(ctx),
            pointer_pixel: None,
            icons: Icons::default(),
            tools: Tools::default(),
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
            let flat = document.flattened();
            let texture = TiledTexture::new(ctx, &document.name(), &flat);
            let view = View::new(texture.size());
            let thumbnail = make_thumbnail(ctx, &document.name(), &flat);
            if let Some(dir) = path.parent() {
                self.settings.set_last_dir(dir);
            }
            self.settings.add_recent_file(&path);
            self.images.push(OpenImage { document, texture, thumbnail, view, transform: None });
            self.select(self.images.len() - 1);
        }
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
    /// or its own file cannot be saved to. Returns whether it was saved.
    fn save(&mut self, index: usize, ask_path: bool) -> bool {
        let document = &self.images[index].document;
        let path = match &document.path {
            Some(path) if !ask_path && is_savable(path) => path.clone(),
            _ => match self.pick_save_path(document) {
                Some(path) => path,
                None => return false,
            },
        };
        if let Err(error) = self.images[index].document.save(&path) {
            self.save_errors.push(format!("{}: {error}", path.display()));
            return false;
        }
        if let Some(dir) = path.parent() {
            self.settings.set_last_dir(dir);
        }
        self.settings.add_recent_file(&path);
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
        if self.images[index].document.unsaved {
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
        });
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
        // Nothing else happens while the user is asked about unsaved changes.
        if self.closing.is_some() {
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
        let index = match &self.closing {
            None => return,
            Some(Closing::Image(index)) => *index,
            Some(Closing::Quit { discarded }) => {
                let next = (0..self.images.len())
                    .find(|index| self.images[*index].document.unsaved && !discarded.contains(index));
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
            // Saving can be given up on as well, or fail.
            Some(UnsavedChoice::Save) if !self.save(index, false) => self.closing = None,
            // Quitting goes on to the next image with changes, closing none until then.
            Some(UnsavedChoice::Save) if quitting => {}
            Some(UnsavedChoice::Discard) if quitting => {
                if let Some(Closing::Quit { discarded }) = &mut self.closing {
                    discarded.push(index);
                }
            }
            Some(UnsavedChoice::Save | UnsavedChoice::Discard) => {
                self.closing = None;
                self.close(index);
            }
        }
    }

    /// Shows the name of the current image in the window's title, marked when it has
    /// unsaved changes.
    fn update_title(&mut self, ctx: &egui::Context) {
        let title = match self.current {
            Some(current) => {
                let document = &self.images[current].document;
                let mark = if document.unsaved { "*" } else { "" };
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

/// The texture the image strip shows, sharp at the screen's scale.
fn make_thumbnail(ctx: &egui::Context, name: &str, image: &power_paint_core::RgbaImage) -> TextureHandle {
    let max_side = (image_strip::THUMBNAIL_SIZE * ctx.pixels_per_point()).ceil() as u32;
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
        if close_requested && !self.may_quit && self.images.iter().any(|image| image.document.unsaved) {
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

        self.unsaved_prompt(&ctx);
        show_errors(&ctx, "open_errors", "Could not open", &mut self.open_errors);
        show_errors(&ctx, "save_errors", "Could not save", &mut self.save_errors);
        self.update_title(&ctx);
    }
}

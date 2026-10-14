mod canvas;
mod image_strip;
mod frame_select;
mod settings;
mod tiled_texture;
mod view;

use std::path::{Path, PathBuf};

use eframe::egui::{self, ColorImage, Key, KeyboardShortcut, Modifiers, TextureHandle, TextureOptions};
use power_paint_core::{Document, OPEN_EXTENSIONS, SelectionMode, thumbnail};
use frame_select::{FrameSelect, Shape};
use settings::Settings;
use tiled_texture::TiledTexture;
use view::{View, ZOOM_LEVELS, format_zoom};

const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
const CLOSE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::W);
const QUIT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Q);
const ZOOM_IN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Plus);
/// Plus is a shifted key on most keyboards, so the key it shares counts as well.
const ZOOM_IN_EQUALS: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Equals);
const ZOOM_OUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Minus);
const ZOOM_TO_WINDOW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::B);
const ACTUAL_SIZE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Num0);
/// Goes from one selection tool to the next.
const SELECT_TOOL: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::S);

/// Widest the list of recent files gets, in points.
const WELCOME_WIDTH: f32 = 600.0;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Power Paint")
            .with_inner_size([1280.0, 800.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native("Power Paint", options, Box::new(|cc| Ok(Box::new(App::new(&cc.egui_ctx)))))
}

/// A document with the textures that show it.
pub struct OpenImage {
    pub document: Document,
    pub texture: TiledTexture,
    /// The image shrunk for the image strip, as textures are not mipmapped.
    pub thumbnail: TextureHandle,
    pub view: View,
}

/// What the View menu and its shortcuts do.
#[derive(Clone, Copy)]
enum ViewCommand {
    ZoomIn,
    ZoomOut,
    ZoomToWindow,
    ActualSize,
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
    settings: Settings,
    checker: TextureHandle,
    /// The pixel under the pointer, while it is over the image's view.
    pointer_pixel: Option<(i64, i64)>,
    frame_select: FrameSelect,
    /// What the selection tool selects: which tool it is.
    select_shape: Shape,
    /// What selecting does to the selection there already is, unless Ctrl is held.
    selection_mode: SelectionMode,
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
            settings: Settings::load(),
            checker: canvas::checker_texture(ctx),
            pointer_pixel: None,
            frame_select: FrameSelect::default(),
            select_shape: Shape::default(),
            selection_mode: SelectionMode::default(),
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
            self.images.push(OpenImage { document, texture, thumbnail, view });
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
            let close = egui::Button::new("Close").shortcut_text(ctx.format_shortcut(&CLOSE));
            if ui.add_enabled(self.current.is_some(), close).clicked() {
                self.close(self.current.unwrap());
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
            for shape in Shape::ALL {
                ui.selectable_value(&mut self.select_shape, shape, shape.tool_name())
                    .on_hover_text(format!("{} ({})", shape.tool_name(), ui.ctx().format_shortcut(&SELECT_TOOL)));
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
                .selected_text(name(self.selection_mode))
                .show_ui(ui, |ui| {
                    for mode in SelectionMode::ALL {
                        ui.selectable_value(&mut self.selection_mode, mode, name(mode));
                    }
                })
                .response
                .on_hover_text(
                    "What selecting does to what is already selected.\nWhatever this says, holding Ctrl adds to it.",
                );
        });
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
        // Not while a drag would carry on with another tool than it began with.
        let busy = ctx.memory(|memory| memory.focused().is_some()) || self.frame_select.is_dragging();
        if !busy && ctx.input_mut(|input| input.consume_shortcut(&SELECT_TOOL)) {
            self.select_shape = self.select_shape.next();
        }
        if ctx.input_mut(|input| input.consume_shortcut(&OPEN)) {
            self.show_open_dialog(ctx);
        }
        if ctx.input_mut(|input| input.consume_shortcut(&CLOSE))
            && let Some(current) = self.current
        {
            self.close(current);
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

    fn show_open_errors(&mut self, ctx: &egui::Context) {
        if self.open_errors.is_empty() {
            return;
        }
        let response = egui::Modal::new(egui::Id::new("open_errors")).show(ctx, |ui| {
            ui.heading("Could not open");
            for error in &self.open_errors {
                ui.label(error);
            }
            ui.add_space(8.0);
            ui.button("OK").clicked()
        });
        if response.inner || response.should_close() {
            self.open_errors.clear();
        }
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
                    Some(image_strip::Action::Close(index)) => self.close(index),
                    None => {}
                }
            });
        });

        egui::Panel::top("tool_options").show(ui, |ui| self.tool_options(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));

        // The image goes right up to the panels around it.
        let frame = egui::Frame::central_panel(ui.style()).inner_margin(0.0);
        egui::CentralPanel::default().frame(frame).show(ui, |ui| match self.current {
            Some(current) => {
                let image = &mut self.images[current];
                let (tool, shape, mode) = (&mut self.frame_select, self.select_shape, self.selection_mode);
                self.pointer_pixel = canvas::show(ui, image, tool, shape, mode, &self.checker);
            }
            None => {
                self.pointer_pixel = None;
                self.welcome(ui);
            }
        });

        self.show_open_errors(&ctx);
    }
}

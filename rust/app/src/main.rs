mod canvas;
mod image_strip;
mod settings;
mod tiled_texture;

use std::path::{Path, PathBuf};

use eframe::egui::{self, ColorImage, Key, KeyboardShortcut, Modifiers, TextureHandle, TextureOptions};
use power_paint_core::{Document, OPEN_EXTENSIONS, thumbnail};
use settings::Settings;
use tiled_texture::TiledTexture;

const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
const CLOSE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::W);
const QUIT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Q);

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
}

impl App {
    fn new(ctx: &egui::Context) -> Self {
        Self {
            images: Vec::new(),
            current: None,
            scroll_to_current: false,
            pending_paths: std::env::args_os().skip(1).map(PathBuf::from).collect(),
            open_errors: Vec::new(),
            settings: Settings::load(),
            checker: canvas::checker_texture(ctx),
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
            let options = TextureOptions {
                magnification: egui::TextureFilter::Nearest,
                minification: egui::TextureFilter::Linear,
                ..Default::default()
            };
            let texture = TiledTexture::new(ctx, &document.name(), &flat, options);
            let thumbnail = make_thumbnail(ctx, &document.name(), &flat);
            if let Some(dir) = path.parent() {
                self.settings.set_last_dir(dir);
            }
            self.settings.add_recent_file(&path);
            self.images.push(OpenImage { document, texture, thumbnail });
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

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
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
                let action = image_strip::show(ui, &self.images, self.current, scroll_to_current, &self.checker);
                match action {
                    Some(image_strip::Action::Select(index)) => self.select(index),
                    Some(image_strip::Action::Close(index)) => self.close(index),
                    None => {}
                }
            });
        });

        egui::CentralPanel::default().show(ui, |ui| match self.current {
            Some(current) => canvas::show(ui, &self.images[current].texture, &self.checker),
            None => self.welcome(ui),
        });

        self.show_open_errors(&ctx);
    }
}

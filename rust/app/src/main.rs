mod canvas;
mod image_strip;

use std::path::{Path, PathBuf};

use eframe::egui::{self, ColorImage, Key, KeyboardShortcut, Modifiers, TextureHandle, TextureOptions};
use power_paint_core::{Document, OPEN_EXTENSIONS};

const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
const CLOSE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::W);
const QUIT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Q);

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

/// A document with the texture that shows it.
pub struct OpenImage {
    pub document: Document,
    pub texture: TextureHandle,
}

struct App {
    images: Vec<OpenImage>,
    current: Option<usize>,
    /// Set when the current image changes, so the strip brings it into view.
    scroll_to_current: bool,
    /// Files given on the command line, opened on the first frame, once the
    /// largest texture size is known.
    pending_paths: Vec<PathBuf>,
    /// Files that could not be opened, with why, until the user closes the message.
    open_errors: Vec<String>,
    last_dir: Option<PathBuf>,
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
            last_dir: None,
            checker: canvas::checker_texture(ctx),
        }
    }

    fn show_open_dialog(&mut self, ctx: &egui::Context) {
        let mut dialog = rfd::FileDialog::new().add_filter("Images", OPEN_EXTENSIONS);
        if let Some(dir) = &self.last_dir {
            dialog = dialog.set_directory(dir);
        }
        if let Some(paths) = dialog.pick_files() {
            self.open_paths(ctx, paths);
        }
    }

    /// Opens each file as an image of its own, the last one becoming the current
    /// image. A file already open is made current instead.
    fn open_paths(&mut self, ctx: &egui::Context, paths: Vec<PathBuf>) {
        let max_side = ctx.input(|input| input.max_texture_side);
        for path in paths {
            if let Some(index) = self.find_image(&path) {
                self.select(index);
                continue;
            }
            let document = match Document::open(&path) {
                Ok(document) => document,
                Err(error) => {
                    self.open_errors.push(format!("{}: {error}", path.display()));
                    continue;
                }
            };
            let (width, height) = document.size();
            // Large images need tiled textures, which come with the canvas.
            if width.max(height) as usize > max_side {
                self.open_errors.push(format!(
                    "{}: {width} × {height} is larger than the {max_side} pixels that can be shown for now",
                    path.display()
                ));
                continue;
            }

            let flat = document.flattened();
            let size = [width as usize, height as usize];
            let options = TextureOptions {
                magnification: egui::TextureFilter::Nearest,
                minification: egui::TextureFilter::Linear,
                wrap_mode: egui::TextureWrapMode::ClampToEdge,
                // Smooth thumbnails.
                mipmap_mode: Some(egui::TextureFilter::Linear),
            };
            let texture = ctx.load_texture(document.name(), ColorImage::from_rgba_unmultiplied(size, &flat), options);
            self.last_dir = path.parent().map(Path::to_path_buf);
            self.images.push(OpenImage { document, texture });
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
            None => {
                ui.centered_and_justified(|ui| {
                    ui.label("Open an image with Ctrl+O, or drop files here");
                });
            }
        });

        self.show_open_errors(&ctx);
    }
}

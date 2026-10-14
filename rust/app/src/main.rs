use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Power Paint")
            .with_inner_size([1280.0, 800.0]),
        ..Default::default()
    };
    eframe::run_native("Power Paint", options, Box::new(|_cc| Ok(Box::new(App::default()))))
}

#[derive(Default)]
struct App {}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("Power Paint");
        });
    }
}

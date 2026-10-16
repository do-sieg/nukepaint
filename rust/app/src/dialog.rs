//! What the dialogs have in common: a title bar they are dragged by, which keeps the rest
//! of them free for their settings, with a button that closes them as Escape does, and
//! a row of buttons at the bottom right. Each dialog opens where it was last dragged
//! to, kept in the window.

use eframe::egui::{self, Align, Align2, Color32, CursorIcon, Layout, Rect, Sense, Stroke, Vec2};

/// Darkens what is behind a dialog that has nothing to show on it.
pub const DIM: Color32 = Color32::from_black_alpha(100);
/// Side of the close button, in points.
const CLOSE_SIZE: f32 = 20.0;
/// How far the cross of the close button is from its edges, in points.
const CROSS_INSET: f32 = 6.0;

/// What a dialog's contents returned, and whether it was closed: with its close button,
/// Escape or a click outside it.
pub struct Response<R> {
    pub inner: R,
    closed: bool,
}

impl<R> Response<R> {
    pub fn should_close(&self) -> bool {
        self.closed
    }
}

/// Shows a dialog with this title over the rest of the app, which it keeps from being
/// used, `backdrop` over that.
pub fn show<R>(ctx: &egui::Context, id: &str, title: &str, backdrop: Color32, content: impl FnOnce(&mut egui::Ui) -> R) -> Response<R> {
    let id = egui::Id::new(id);
    // How far from the middle of the window it was dragged.
    let offset_id = id.with("offset");
    let mut offset = ctx.data(|data| data.get_temp::<Vec2>(offset_id)).unwrap_or_default();
    let area = egui::Modal::default_area(id).anchor(Align2::CENTER_CENTER, offset);
    let response = egui::Modal::new(id).area(area).backdrop_color(backdrop).show(ctx, |ui| {
        // Room is kept on the right of the title for the close button.
        let title = ui.horizontal(|ui| {
            ui.add(egui::Label::new(egui::RichText::new(title).heading()).selectable(false));
            ui.add_space(CLOSE_SIZE);
        });
        let title = title.response.rect;
        ui.add_space(8.0);
        let inner = content(ui);
        // The bar the title is on, as wide as the dialog, up to its edges, known once
        // what is under it is shown.
        let margin = egui::Frame::popup(ui.style()).inner_margin;
        let right = ui.min_rect().right();
        let close = Rect::from_center_size(egui::pos2(right - CLOSE_SIZE / 2.0, title.center().y), Vec2::splat(CLOSE_SIZE));
        let bar = Rect::from_min_max(
            egui::pos2(ui.min_rect().left() - f32::from(margin.left), title.top() - f32::from(margin.top)),
            egui::pos2(close.left(), title.bottom()),
        );
        let bar = ui.interact(bar, ui.id().with("title_bar"), Sense::drag()).on_hover_cursor(CursorIcon::Grab);
        if bar.dragged() {
            ctx.set_cursor_icon(CursorIcon::Grabbing);
            offset += bar.drag_delta();
        }
        (inner, close_button(ui, close))
    });
    let rect = response.response.rect;
    let window = ctx.content_rect();
    offset.x += (window.left() - rect.left()).max(0.0) + (window.right() - rect.right()).min(0.0);
    offset.y += (window.top() - rect.top()).max(0.0) + (window.bottom() - rect.bottom()).min(0.0);
    ctx.data_mut(|data| data.insert_temp(offset_id, offset));
    let closed = response.should_close() || response.inner.1;
    Response { inner: response.inner.0, closed }
}

/// A cross in a square that lights up under the pointer, as a window's. Returns
/// whether it was clicked.
fn close_button(ui: &mut egui::Ui, rect: Rect) -> bool {
    let response = ui.interact(rect, ui.id().with("close"), Sense::click()).on_hover_text("Close");
    let visuals = ui.style().interact(&response);
    if response.hovered() {
        ui.painter().rect_filled(rect, visuals.corner_radius, visuals.bg_fill);
    }
    let cross = rect.shrink(CROSS_INSET);
    let stroke = Stroke::new(1.5, visuals.fg_stroke.color);
    ui.painter().line_segment([cross.left_top(), cross.right_bottom()], stroke);
    ui.painter().line_segment([cross.right_top(), cross.left_bottom()], stroke);
    response.clicked()
}

/// A row as wide as what the dialog shows above it, whose buttons are added from the
/// right: the last one first.
pub fn buttons<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.add_space(8.0);
    let row = Vec2::new(ui.min_rect().width(), ui.spacing().interact_size.y);
    ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), add).inner
}

/// The OK and Cancel buttons, at the bottom right: whether either was pressed, OK
/// with Enter as well.
pub fn ok_cancel(ui: &mut egui::Ui) -> Option<bool> {
    buttons(ui, |ui| {
        let cancel = ui.button("Cancel").clicked();
        let enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
        let ok = ui.button("OK").clicked() || enter;
        ok.then_some(true).or(cancel.then_some(false))
    })
}

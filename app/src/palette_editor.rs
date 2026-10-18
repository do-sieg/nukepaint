//! Edits a copy of an image's palette, which OK puts in its place as a step, and
//! Cancel, or Escape, gives up on.
//!
//! On the left, the entries: a click picks one, Shift and a click picks the entries
//! from the one picked before to it, as does a drag from the one it begins on. Under
//! them, the entry picked last can be edited and made the transparent one, the
//! primary colour added after it, the entries picked removed or made a ramp, the
//! palette resized, loaded from a file or saved to one. On the right, the presets,
//! each with its colours in a strip, which a click loads, narrowed down by a search of
//! their names and authors, their group and how many colours they have. Each change can be undone
//! in the dialog; editing a colour in its picker, or dragging or typing the size, is
//! one change however long it goes on, and sizes it goes through on the way lose no
//! colours.

use std::ops::RangeInclusive;

use eframe::egui::{self, Align2, Rect, Sense, Stroke, StrokeKind, Vec2};
use nukepaint_core::{MAX_PALETTE_SIZE, PRESET_GROUPS, PRESETS, Palette, Preset, Rgba};

use crate::{
    dialog,
    palette_panel::{Cells, entry_tip},
};

/// Smallest a cell gets before fewer go on a row, in points.
const MIN_CELL_SIZE: f32 = 20.0;
const ENTRIES_WIDTH: f32 = 380.0;
const ENTRIES_HEIGHT: f32 = 300.0;
/// How far the outlines of the entries picked go past them.
const OUTLINE_ROOM: f32 = 3.0;
const PRESETS_WIDTH: f32 = 260.0;
/// Room on either side of the line between the entries and the presets, in points.
const COLUMN_GAP: f32 = 6.0;
const PRESETS_HEIGHT: f32 = 420.0;
/// Room around a preset's name and strip, in points.
const PRESET_PADDING: f32 = 4.0;
const STRIP_HEIGHT: f32 = 12.0;

pub enum Outcome {
    /// To put the palette in place of the image's.
    Accepted(Palette),
    Canceled,
    /// To load a palette from a file the user picks, with [`Dialog::load`].
    Load,
    /// To save the palette, [`Dialog::palette`], to a file the user picks.
    Save,
}

/// The entries picked, from the one picked first to the one picked last, either way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Selection {
    anchor: usize,
    last: usize,
}

impl Selection {
    fn single(index: usize) -> Self {
        Self { anchor: index, last: index }
    }

    fn range(self) -> RangeInclusive<usize> {
        self.anchor.min(self.last)..=self.anchor.max(self.last)
    }

    /// The selection, kept to the entries of a palette of `len`.
    fn within(self, len: usize) -> Option<Self> {
        let last = len.checked_sub(1)?;
        Some(Self { anchor: self.anchor.min(last), last: self.last.min(last) })
    }
}

/// A change that goes on over frames, which is undone as one.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ongoing {
    /// In the colour picker.
    Color,
    /// Dragging or typing the size.
    Size,
}

pub struct Dialog {
    /// Which of the images the palette is of.
    pub image: usize,
    palette: Palette,
    selection: Option<Selection>,
    /// The palette before each change, the last one last, to undo.
    undo: Vec<Palette>,
    /// The palette before each change undone, the last one undone last, to redo.
    redo: Vec<Palette>,
    /// The change going on, which goes on recording in the last change.
    ongoing: Option<Ongoing>,
}

impl Dialog {
    pub fn new(image: usize, palette: Palette) -> Self {
        Self { image, palette, selection: None, undo: Vec::new(), redo: Vec::new(), ongoing: None }
    }

    pub fn palette(&self) -> &Palette {
        &self.palette
    }

    /// Puts `palette` in place of the one being edited, as a change.
    pub fn load(&mut self, palette: Palette) {
        let before = std::mem::replace(&mut self.palette, palette);
        self.selection = None;
        self.record(before, None);
    }

    /// Records the change from `before`, if the palette changed, as part of the last
    /// change if it carries on with `ongoing`.
    fn record(&mut self, before: Palette, ongoing: Option<Ongoing>) {
        if self.palette == before {
            return;
        }
        if ongoing.is_none() || ongoing != self.ongoing {
            self.undo.push(before);
            self.redo.clear();
        }
        self.ongoing = ongoing;
    }

    /// Lets the next change of `ongoing` be a change of its own, once the one going on is over.
    fn end(&mut self, ongoing: Ongoing, still: bool) {
        if !still && self.ongoing == Some(ongoing) {
            self.ongoing = None;
        }
    }

    fn undo(&mut self) {
        if let Some(before) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.palette, before));
            self.ongoing = None;
        }
    }

    fn redo(&mut self) {
        if let Some(after) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.palette, after));
            self.ongoing = None;
        }
    }

    /// Shows the dialog, adding `primary` as Add does. Returns what the user did, if anything.
    pub fn show(&mut self, ctx: &egui::Context, primary: Rgba<u8>) -> Option<Outcome> {
        let typing = ctx.memory(|memory| memory.focused().is_some());
        if !typing {
            // Undo would take Redo's other keys as well, so they go first.
            if ctx.input_mut(|input| input.consume_shortcut(&crate::REDO_SHIFT) || input.consume_shortcut(&crate::REDO)) {
                self.redo();
            }
            if ctx.input_mut(|input| input.consume_shortcut(&crate::UNDO)) {
                self.undo();
            }
        }
        let response = dialog::show(ctx, "palette_editor", "Edit Palette", dialog::DIM, |ui| {
            let mut outcome = None;
            let columns = ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(ENTRIES_WIDTH);
                    outcome = self.entries_and_controls(ui, primary);
                });
                // A line between the columns, drawn once their height is known: a
                // separator would take all the height left under it, the window's.
                ui.add_space(COLUMN_GAP);
                let line = ui.cursor().left();
                ui.add_space(COLUMN_GAP);
                ui.vertical(|ui| {
                    ui.set_width(PRESETS_WIDTH);
                    self.presets(ui);
                });
                line
            });
            let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
            ui.painter().vline(columns.inner, columns.response.rect.y_range(), stroke);
            let accepted = dialog::ok_cancel(ui).map(|ok| if ok { Outcome::Accepted(self.palette.clone()) } else { Outcome::Canceled });
            accepted.or(outcome)
        });
        if response.should_close() {
            return Some(Outcome::Canceled);
        }
        response.inner
    }

    fn entries_and_controls(&mut self, ui: &mut egui::Ui, primary: Rgba<u8>) -> Option<Outcome> {
        self.selection = self.selection.and_then(|selection| selection.within(self.palette.len()));
        egui::ScrollArea::vertical()
            .id_salt("palette_editor_entries")
            .max_height(ENTRIES_HEIGHT)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                // Room for the outlines of the entries along the edges, which the scrolling would cut off.
                egui::Frame::NONE.inner_margin(OUTLINE_ROOM).show(ui, |ui| {
                    if self.palette.is_empty() {
                        ui.weak("The palette is empty");
                    } else {
                        self.entries(ui);
                    }
                });
            });
        ui.add_space(8.0);
        self.entry_controls(ui);
        ui.add_space(4.0);
        self.palette_controls(ui, primary)
    }

    /// The entries as cells, the ones picked outlined.
    fn entries(&mut self, ui: &mut egui::Ui) {
        let (cells, response) = Cells::show(ui, &self.palette, MIN_CELL_SIZE, Sense::click_and_drag());
        if let Some(selection) = self.selection {
            let stroke = Stroke::new(2.0, ui.visuals().selection.stroke.color);
            for index in selection.range() {
                ui.painter().rect_stroke(cells.rect(index).expand(1.0), 0.0, stroke, StrokeKind::Outside);
            }
        }

        let extend = ui.input(|input| input.modifiers.shift);
        let mut pick = |index: usize| {
            self.selection = Some(match self.selection {
                Some(selection) if extend => Selection { last: index, ..selection },
                _ => Selection::single(index),
            });
        };
        if response.clicked()
            && let Some(index) = response.interact_pointer_pos().and_then(|at| cells.at(at))
        {
            pick(index);
        }
        if response.drag_started_by(egui::PointerButton::Primary)
            && let Some(press) = ui.input(|input| input.pointer.press_origin())
        {
            pick(cells.nearest(press));
        }
        if response.dragged_by(egui::PointerButton::Primary)
            && let (Some(selection), Some(at)) = (self.selection.as_mut(), response.interact_pointer_pos())
        {
            selection.last = cells.nearest(at);
        }
        if let Some(index) = response.hover_pos().and_then(|at| cells.at(at)) {
            let tip = format!("{}\nClick to pick it; Shift and click, or drag, to pick several", entry_tip(&self.palette, index));
            response.on_hover_text_at_pointer(tip);
        }
    }

    /// The entry picked last: its colour, and whether it is the transparent one.
    fn entry_controls(&mut self, ui: &mut egui::Ui) {
        let last = self.selection.map(|selection| selection.last);
        ui.horizontal(|ui| {
            ui.add_enabled_ui(last.is_some(), |ui| {
                // The picker's popup, which the button names after itself.
                let picking = egui::Popup::is_id_open(ui.ctx(), ui.auto_id_with("popup"));
                let mut code = last.map_or([0, 0, 0, 255], |index| self.palette.colors[index].0);
                let response = ui.color_edit_button_srgba_unmultiplied(&mut code);
                if response.on_hover_text("The color of the entry picked last").changed()
                    && let Some(index) = last
                {
                    let before = self.palette.clone();
                    self.palette.colors[index] = Rgba(code);
                    self.record(before, picking.then_some(Ongoing::Color));
                }
                self.end(Ongoing::Color, picking);

                let mut transparent = last.is_some() && self.palette.transparent == last;
                let checkbox = ui.checkbox(&mut transparent, "Transparent");
                if checkbox.on_hover_text("Treat the entry picked last as no color at all").changed() {
                    let before = self.palette.clone();
                    self.palette.transparent = last.filter(|_| transparent);
                    self.record(before, None);
                }
            });
            match last {
                Some(index) => ui.label(entry_tip(&self.palette, index)),
                None => ui.weak("Pick an entry to edit it"),
            };
        });
    }

    /// Adding, removing and ramping entries, the size, undoing, and loading and saving.
    fn palette_controls(&mut self, ui: &mut egui::Ui, primary: Rgba<u8>) -> Option<Outcome> {
        let last = self.selection.map(|selection| selection.last);
        let range = self.selection.map(Selection::range);
        ui.horizontal(|ui| {
            let add = ui.add_enabled(!self.palette.is_full(), egui::Button::new("Add"));
            if add.on_hover_text("Add the primary color after the entry picked last").clicked() {
                let before = self.palette.clone();
                let index = last.map_or(self.palette.len(), |last| last + 1);
                self.palette.insert(index, primary);
                self.selection = Some(Selection::single(index));
                self.record(before, None);
            }

            let remove = ui.add_enabled(range.is_some(), egui::Button::new("Remove"));
            if remove.on_hover_text("Remove the entries picked").clicked()
                && let Some(range) = range.clone()
            {
                let before = self.palette.clone();
                let first = *range.start();
                self.palette.remove(range);
                self.selection = Selection::single(first).within(self.palette.len());
                self.record(before, None);
            }

            let can_ramp = range.as_ref().is_some_and(|range| range.end() - range.start() >= 2);
            let ramp = ui.add_enabled(can_ramp, egui::Button::new("Ramp"));
            if ramp.on_hover_text("Fill the entries picked with the colors between the first and the last").clicked()
                && let Some(range) = range
            {
                let before = self.palette.clone();
                self.palette.ramp(range);
                self.record(before, None);
            }

            ui.separator();
            ui.label("Size:");
            let mut len = self.palette.len();
            let response = ui.add(egui::DragValue::new(&mut len).range(0..=MAX_PALETTE_SIZE));
            // Typed sizes take effect as they are typed, a digit at a time.
            let resizing = response.dragged() || response.has_focus();
            if response.on_hover_text("How many entries the palette has").changed() {
                let before = self.palette.clone();
                // While a drag or typing goes on, the size is cut from the palette as it
                // was before, so the colours a smaller size cut off come back with a larger one.
                let start = match self.ongoing {
                    Some(Ongoing::Size) => self.undo.last().unwrap_or(&before),
                    _ => &before,
                };
                let mut resized = start.clone();
                resized.resize(len);
                self.palette = resized;
                self.record(before, resizing.then_some(Ongoing::Size));
            }
            self.end(Ongoing::Size, resizing);
        });

        ui.horizontal(|ui| {
            let ctx = ui.ctx().clone();
            let undo = ui.add_enabled(!self.undo.is_empty(), egui::Button::new("Undo"));
            if undo.on_hover_text(format!("Undo the last change ({})", ctx.format_shortcut(&crate::UNDO))).clicked() {
                self.undo();
            }
            let redo = ui.add_enabled(!self.redo.is_empty(), egui::Button::new("Redo"));
            if redo.on_hover_text(format!("Redo the last change undone ({})", ctx.format_shortcut(&crate::REDO))).clicked() {
                self.redo();
            }
            ui.separator();
            let load = ui.button("Load…").on_hover_text("Load a .gpl, .pal, .act or .ase palette file").clicked();
            let save = ui.button("Save As…").on_hover_text("Save the palette to a .gpl, .pal, .act or .ase file");
            load.then_some(Outcome::Load).or(save.clicked().then_some(Outcome::Save))
        })
        .inner
    }

    /// The presets, by group, each its name and its colours in a strip. A click loads one.
    fn presets(&mut self, ui: &mut egui::Ui) {
        ui.strong("Presets");
        let filter_id = egui::Id::new("palette_editor_preset_filter");
        let mut filter: PresetFilter = ui.data(|data| data.get_temp(filter_id)).unwrap_or_default();
        filter.show(ui);
        ui.data_mut(|data| data.insert_temp(filter_id, filter.clone()));

        egui::ScrollArea::vertical().id_salt("palette_editor_presets").max_height(PRESETS_HEIGHT).show(ui, |ui| {
            let mut any = false;
            for group in PRESET_GROUPS {
                let mut matching = PRESETS.iter().filter(|preset| preset.group == group && filter.matches(preset)).peekable();
                if matching.peek().is_none() {
                    continue;
                }
                any = true;
                ui.add_space(4.0);
                ui.weak(group);
                for preset in matching {
                    if preset_row(ui, preset).clicked() {
                        self.load(preset.palette());
                    }
                }
            }
            if !any {
                ui.add_space(4.0);
                ui.weak("No preset matches");
            }
        });
    }
}

/// The choices of how many colours the presets shown have: a name, and the fewest and most.
const SIZES: [(&str, usize, usize); 6] = [
    ("Any size", 0, usize::MAX),
    ("Up to 4", 0, 4),
    ("5 to 8", 5, 8),
    ("9 to 16", 9, 16),
    ("17 to 32", 17, 32),
    ("33 or more", 33, usize::MAX),
];

/// Which presets are shown: those whose name or author has the search in it, of the
/// group chosen, if one is, and of the size chosen.
#[derive(Clone, Default)]
struct PresetFilter {
    search: String,
    group: Option<&'static str>,
    /// Which of [`SIZES`].
    size: usize,
}

impl PresetFilter {
    fn matches(&self, preset: &Preset) -> bool {
        let search = self.search.trim().to_lowercase();
        let found = [preset.name, preset.author].iter().any(|text| text.to_lowercase().contains(&search));
        let (_, fewest, most) = SIZES[self.size];
        found && self.group.is_none_or(|group| group == preset.group) && (fewest..=most).contains(&preset.color_count())
    }

    /// The search field, then the group and size choices.
    fn show(&mut self, ui: &mut egui::Ui) {
        let search = egui::TextEdit::singleline(&mut self.search).hint_text("Search names and authors");
        ui.add(search.desired_width(f32::INFINITY));
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("palette_editor_preset_group")
                .selected_text(self.group.unwrap_or("All groups"))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.group, None, "All groups");
                    for group in PRESET_GROUPS {
                        ui.selectable_value(&mut self.group, Some(group), group);
                    }
                });
            egui::ComboBox::from_id_salt("palette_editor_preset_size").selected_text(SIZES[self.size].0).show_ui(ui, |ui| {
                for (index, (name, ..)) in SIZES.iter().enumerate() {
                    ui.selectable_value(&mut self.size, index, *name);
                }
            });
            let filtered = !self.search.is_empty() || self.group.is_some() || self.size != 0;
            if ui.add_enabled(filtered, egui::Button::new("Clear")).on_hover_text("Show every preset").clicked() {
                *self = Self::default();
            }
        });
    }
}

/// A preset's name and how many colours it has over a strip of them, which lights up
/// under the pointer.
fn preset_row(ui: &mut egui::Ui, preset: &Preset) -> egui::Response {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let text_height = ui.fonts_mut(|fonts| fonts.row_height(&font));
    let size = Vec2::new(ui.available_width(), text_height + STRIP_HEIGHT + PRESET_PADDING * 3.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let visuals = ui.style().interact(&response);
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(rect, visuals.corner_radius, visuals.bg_fill);
    }
    let inner = rect.shrink(PRESET_PADDING);
    painter.text(inner.left_top(), Align2::LEFT_TOP, preset.name, font.clone(), ui.visuals().text_color());
    let len = preset.color_count();
    painter.text(inner.right_top(), Align2::RIGHT_TOP, len.to_string(), font, ui.visuals().weak_text_color());
    let strip = Rect::from_min_max(inner.left_bottom() - Vec2::Y * STRIP_HEIGHT, inner.right_bottom());
    let colors = preset.palette().colors;
    for (index, color) in colors.iter().enumerate() {
        let [left, right] = [index, index + 1].map(|edge| strip.left() + strip.width() * edge as f32 / len as f32);
        let cell = Rect::from_x_y_ranges(left..=right, strip.y_range());
        painter.rect_filled(cell, 0.0, egui::Color32::from_rgb(color[0], color[1], color[2]));
    }
    let by = if preset.author.is_empty() { String::new() } else { format!(" by {}", preset.author) };
    response.on_hover_text(format!("{}{by}: {len} colors\nClick to load it", preset.name))
}

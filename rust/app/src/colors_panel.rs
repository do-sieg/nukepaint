//! The Colors section of the dock: the primary colour, which the tools draw with, and
//! the secondary one. A click on either swatch picks which one the wheel edits; More
//! shows sliders and fields to set it to the exact value. A click on the palette sets the primary
//! colour, a right click the secondary one. The section also says what the right button
//! does with the tools that draw in a colour.

use std::f32::consts::TAU;

use eframe::egui::{
    self, Color32, Key, KeyboardShortcut, Mesh, Modifiers, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Vec2,
};
use power_paint_core::Rgba;

use crate::tools::RightClick;

pub const SWAP_KEY: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::X);
pub const RESET_KEY: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::D);

/// The standard palette, a row at a time: black, white and greys, then each hue at
/// full strength, darkened and lightened.
const PALETTE: [u32; 48] = [
    0x000000, 0x404040, 0xff0000, 0xff6a00, 0xffd800, 0xb6ff00, 0x4cff00, 0x00ff21, //
    0x00ff90, 0x00ffff, 0x0094ff, 0x0026ff, 0x4800ff, 0xb200ff, 0xff00dc, 0xff006e, //
    0xffffff, 0x808080, 0x7f0000, 0x7f3300, 0x7f6a00, 0x5b7f00, 0x267f00, 0x007f0e, //
    0x007f46, 0x007f7f, 0x004a7f, 0x00137f, 0x21007f, 0x57007f, 0x7f006e, 0x7f0037, //
    0xc0c0c0, 0xa0a0a0, 0xff7f7f, 0xffb27f, 0xffe97f, 0xdaff7f, 0xa5ff7f, 0x7fff8e, //
    0x7fffc5, 0x7fffff, 0x7fc9ff, 0x7f92ff, 0xa17fff, 0xd67fff, 0xff7fed, 0xff7fb6,
];
const PALETTE_COLUMNS: usize = 16;
const PALETTE_CELL_HEIGHT: f32 = 16.0;
const PALETTE_GAP: f32 = 1.0;

const SWATCHES_SIZE: f32 = 44.0;
/// How much of the swatches' side each square takes.
const SQUARE_RATIO: f32 = 0.66;

/// Widest the wheel gets, in points.
const MAX_WHEEL_SIZE: f32 = 240.0;
/// How much of the wheel's radius the hue ring takes.
const RING_RATIO: f32 = 0.16;
/// Room between the ring and the square inside it, in points.
const RING_GAP: f32 = 4.0;
/// How much of the room inside the ring the square takes.
const SQUARE_SCALE: f32 = 0.88;
/// How finely the ring and the square are cut into pieces of one colour each.
const RING_SEGMENTS: usize = 96;
const SQUARE_CELLS: usize = 24;
const MARKER_RADIUS: f32 = 5.0;

const SLIDER_HEIGHT: f32 = 16.0;
/// How finely a slider's gradient is cut into pieces.
const SLIDER_SEGMENTS: usize = 32;
/// Room the field beside each slider takes, in points.
const SLIDER_FIELD_WIDTH: f32 = 56.0;
const SLIDER_LABEL_WIDTH: f32 = 12.0;

const BLACK: Rgba<u8> = Rgba([0, 0, 0, 255]);
const WHITE: Rgba<u8> = Rgba([255, 255, 255, 255]);

/// A colour as hue, saturation and value, all from 0 to 1, of its sRGB channels.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Hsv {
    hue: f32,
    saturation: f32,
    value: f32,
}

impl Hsv {
    /// The colour, keeping the hue and saturation of `before` where the colour alone
    /// has none to give: at black, white and the greys between.
    fn of(color: Rgba<u8>, before: Hsv) -> Self {
        let [red, green, blue] = [color[0], color[1], color[2]].map(|channel| channel as f32 / 255.0);
        let max = red.max(green).max(blue);
        let range = max - red.min(green).min(blue);
        if max == 0.0 {
            return Self { value: 0.0, ..before };
        }
        if range == 0.0 {
            return Self { saturation: 0.0, value: max, ..before };
        }
        let sector = match max {
            _ if max == red => (green - blue) / range,
            _ if max == green => (blue - red) / range + 2.0,
            _ => (red - green) / range + 4.0,
        };
        Self { hue: (sector / 6.0).rem_euclid(1.0), saturation: range / max, value: max }
    }

    fn to_rgb(self) -> [u8; 3] {
        let Self { hue, saturation, value } = self;
        let channel = |offset: f32| {
            let k = (offset + hue * 6.0) % 6.0;
            let level = value - value * saturation * k.min(4.0 - k).clamp(0.0, 1.0);
            (level * 255.0).round() as u8
        };
        [channel(5.0), channel(3.0), channel(1.0)]
    }

    fn to_color32(self) -> Color32 {
        let [red, green, blue] = self.to_rgb();
        Color32::from_rgb(red, green, blue)
    }
}

/// The part of the wheel a drag began on, which it goes on setting.
#[derive(Clone, Copy, PartialEq, Eq)]
enum WheelPart {
    Hue,
    SaturationValue,
}

pub struct Colors {
    /// What the drawing tools draw with, in straight RGBA.
    pub primary: Rgba<u8>,
    pub secondary: Rgba<u8>,
    /// Which of the two the wheel and the fields edit.
    editing_secondary: bool,
    /// The colour being edited, kept to carry its hue and saturation through black,
    /// white and grey, which lose them.
    hsv: Hsv,
    wheel_drag: Option<WheelPart>,
    /// The hexadecimal code being typed, until it is one.
    hex: Option<String>,
}

impl Default for Colors {
    fn default() -> Self {
        let mut colors = Self {
            primary: BLACK,
            secondary: WHITE,
            editing_secondary: false,
            hsv: Hsv { hue: 0.0, saturation: 0.0, value: 0.0 },
            wheel_drag: None,
            hex: None,
        };
        colors.follow_edited();
        colors
    }
}

impl Colors {
    pub fn swap(&mut self) {
        std::mem::swap(&mut self.primary, &mut self.secondary);
    }

    /// Goes back to black as the primary colour and white as the secondary one.
    pub fn reset(&mut self) {
        self.primary = BLACK;
        self.secondary = WHITE;
    }

    /// Sets a colour picked from the image: the one the section is editing, or the
    /// other one with `other`.
    pub fn pick(&mut self, color: Rgba<u8>, other: bool) {
        if self.editing_secondary != other {
            self.secondary = color;
        } else {
            self.primary = color;
        }
    }

    fn edited(&self) -> Rgba<u8> {
        if self.editing_secondary { self.secondary } else { self.primary }
    }

    fn edited_mut(&mut self) -> &mut Rgba<u8> {
        if self.editing_secondary { &mut self.secondary } else { &mut self.primary }
    }

    /// Takes in the colour being edited after it changed some other way than on the
    /// wheel: from the palette, the fields, or swapping or resetting.
    fn follow_edited(&mut self) {
        let edited = self.edited();
        if self.hsv.to_rgb() != [edited[0], edited[1], edited[2]] {
            self.hsv = Hsv::of(edited, self.hsv);
        }
    }

    /// Sets the colour being edited from the wheel, keeping its opacity.
    fn set_from_hsv(&mut self, hsv: Hsv) {
        self.hsv = hsv;
        let [red, green, blue] = hsv.to_rgb();
        let edited = self.edited_mut();
        *edited = Rgba([red, green, blue, edited[3]]);
    }

    /// Shows the section, with `right_click`, what the right button does with the tools
    /// that draw in a colour.
    pub fn show(&mut self, ui: &mut egui::Ui, right_click: &mut RightClick) {
        self.follow_edited();
        ui.horizontal(|ui| {
            self.swatches(ui);
            ui.vertical(|ui| {
                let ctx = ui.ctx().clone();
                let shortcut = |shortcut| ctx.format_shortcut(&shortcut);
                let swap = format!("Swap the primary and secondary colors ({})", shortcut(SWAP_KEY));
                if ui.button("Swap").on_hover_text(swap).clicked() {
                    self.swap();
                }
                let reset = format!("Back to black and white ({})", shortcut(RESET_KEY));
                if ui.button("Reset").on_hover_text(reset).clicked() {
                    self.reset();
                }
            });
        });
        right_click_option(ui, right_click);
        self.follow_edited();
        self.wheel(ui);
        self.palette(ui);
        self.follow_edited();
        egui::CollapsingHeader::new("More").id_salt("colors_more").show(ui, |ui| self.fields(ui));
        self.follow_edited();
    }

    /// The primary colour as a square over a corner of the secondary one, the one being
    /// edited outlined.
    fn swatches(&mut self, ui: &mut egui::Ui) {
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(SWATCHES_SIZE), Sense::click());
        let side = (SWATCHES_SIZE * SQUARE_RATIO).floor();
        let primary = Rect::from_min_size(rect.min, Vec2::splat(side));
        let secondary = Rect::from_min_size(rect.max - Vec2::splat(side), Vec2::splat(side));
        let edited = if self.editing_secondary { secondary } else { primary };
        let painter = ui.painter();
        for (square, color) in [(secondary, self.secondary), (primary, self.primary)] {
            paint_swatch(painter, square, color);
        }
        let selection = ui.visuals().selection.stroke.color;
        painter.rect_stroke(edited.expand(1.0), 0.0, Stroke::new(2.0, selection), StrokeKind::Outside);
        // The primary square is on top, so it is the one a click on both reaches.
        if response.clicked()
            && let Some(at) = response.interact_pointer_pos()
        {
            if primary.contains(at) {
                self.editing_secondary = false;
            } else if secondary.contains(at) {
                self.editing_secondary = true;
            }
        }
        let target = if self.editing_secondary { "secondary" } else { "primary" };
        response.on_hover_text(format!("Editing the {target} color. Click a square to edit that one."));
    }

    /// A ring of hues around a square of the saturations, left to right, and values,
    /// bottom to top, of the hue chosen.
    fn wheel(&mut self, ui: &mut egui::Ui) {
        let size = ui.available_width().min(MAX_WHEEL_SIZE);
        // Sensing only drags, a drag begins as soon as the button is pressed.
        let (rect, response) =
            ui.vertical_centered(|ui| ui.allocate_exact_size(Vec2::splat(size), Sense::drag())).inner;
        let center = rect.center();
        let outer = size / 2.0;
        let inner = outer * (1.0 - RING_RATIO);
        let half_side = (inner - RING_GAP) / std::f32::consts::SQRT_2 * SQUARE_SCALE;
        let square = Rect::from_center_size(center, Vec2::splat(half_side * 2.0));

        if response.drag_started()
            && let Some(press) = ui.input(|input| input.pointer.press_origin())
        {
            let distance = (press - center).length();
            self.wheel_drag = if square.contains(press) {
                Some(WheelPart::SaturationValue)
            } else if distance >= inner - RING_GAP && distance <= outer {
                Some(WheelPart::Hue)
            } else {
                None
            };
        }
        if response.dragged()
            && let (Some(part), Some(at)) = (self.wheel_drag, response.interact_pointer_pos())
        {
            let hsv = match part {
                WheelPart::Hue => {
                    let offset = at - center;
                    // Red on the right, the hues going round counterclockwise, as y goes down.
                    Hsv { hue: (-offset.y).atan2(offset.x).rem_euclid(TAU) / TAU, ..self.hsv }
                }
                WheelPart::SaturationValue => Hsv {
                    saturation: ((at.x - square.left()) / square.width()).clamp(0.0, 1.0),
                    value: 1.0 - ((at.y - square.top()) / square.height()).clamp(0.0, 1.0),
                    ..self.hsv
                },
            };
            self.set_from_hsv(hsv);
        }
        if response.drag_stopped() {
            self.wheel_drag = None;
        }

        let painter = ui.painter();
        let mut mesh = Mesh::default();
        let point = |angle: f32, radius: f32| center + radius * Vec2::new(angle.cos(), -angle.sin());
        // The ring's edges fade out over a screen pixel, half on either side of them.
        let feather = 0.5 / ui.ctx().pixels_per_point();
        let bands = [
            (inner - feather, inner + feather, false, true),
            (inner + feather, outer - feather, true, true),
            (outer - feather, outer + feather, true, false),
        ];
        for segment in 0..RING_SEGMENTS {
            let [from, to] = [segment, segment + 1].map(|end| end as f32 / RING_SEGMENTS as f32);
            let [from_color, to_color] = [from, to].map(|hue| Hsv { hue, saturation: 1.0, value: 1.0 }.to_color32());
            for (near, far, near_shows, far_shows) in bands {
                let shown = |color: Color32, shows: bool| if shows { color } else { Color32::TRANSPARENT };
                let first = mesh.vertices.len() as u32;
                mesh.colored_vertex(point(from * TAU, near), shown(from_color, near_shows));
                mesh.colored_vertex(point(from * TAU, far), shown(from_color, far_shows));
                mesh.colored_vertex(point(to * TAU, far), shown(to_color, far_shows));
                mesh.colored_vertex(point(to * TAU, near), shown(to_color, near_shows));
                mesh.add_triangle(first, first + 1, first + 2);
                mesh.add_triangle(first, first + 2, first + 3);
            }
        }
        let cell = square.width() / SQUARE_CELLS as f32;
        let corner = |column: usize, row: usize| {
            let saturation = column as f32 / SQUARE_CELLS as f32;
            let value = 1.0 - row as f32 / SQUARE_CELLS as f32;
            let at = square.min + Vec2::new(column as f32, row as f32) * cell;
            (at, Hsv { saturation, value, ..self.hsv }.to_color32())
        };
        for row in 0..SQUARE_CELLS {
            for column in 0..SQUARE_CELLS {
                let first = mesh.vertices.len() as u32;
                for (column, row) in [(column, row), (column + 1, row), (column + 1, row + 1), (column, row + 1)] {
                    let (at, color) = corner(column, row);
                    mesh.colored_vertex(at, color);
                }
                mesh.add_triangle(first, first + 1, first + 2);
                mesh.add_triangle(first, first + 2, first + 3);
            }
        }
        painter.add(Shape::mesh(mesh));

        let hue_at = point(self.hsv.hue * TAU, (inner + outer) / 2.0);
        let value_at = Pos2::new(
            square.left() + self.hsv.saturation * square.width(),
            square.top() + (1.0 - self.hsv.value) * square.height(),
        );
        for at in [hue_at, value_at] {
            painter.circle_stroke(at, MARKER_RADIUS, Stroke::new(3.0, Color32::BLACK));
            painter.circle_stroke(at, MARKER_RADIUS, Stroke::new(1.5, Color32::WHITE));
        }
    }

    /// The palette's cells, which share the section's width between them.
    fn palette(&mut self, ui: &mut egui::Ui) {
        let rows = PALETTE.len().div_ceil(PALETTE_COLUMNS);
        let height = rows as f32 * (PALETTE_CELL_HEIGHT + PALETTE_GAP) - PALETTE_GAP;
        let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::click());
        let width = (rect.width() + PALETTE_GAP) / PALETTE_COLUMNS as f32 - PALETTE_GAP;
        let cell_rect = |index: usize| {
            let (column, row) = (index % PALETTE_COLUMNS, index / PALETTE_COLUMNS);
            let at = Vec2::new(column as f32 * (width + PALETTE_GAP), row as f32 * (PALETTE_CELL_HEIGHT + PALETTE_GAP));
            Rect::from_min_size(rect.min + at, Vec2::new(width, PALETTE_CELL_HEIGHT))
        };
        for (index, &code) in PALETTE.iter().enumerate() {
            let [_, red, green, blue] = code.to_be_bytes();
            ui.painter().rect_filled(cell_rect(index), 0.0, Color32::from_rgb(red, green, blue));
        }
        let clicked = (response.clicked(), response.secondary_clicked());
        if (clicked.0 || clicked.1)
            && let Some(at) = response.interact_pointer_pos()
            && let Some(index) = (0..PALETTE.len()).find(|&index| cell_rect(index).contains(at))
        {
            let [_, red, green, blue] = PALETTE[index].to_be_bytes();
            let color = Rgba([red, green, blue, 255]);
            if clicked.1 {
                self.secondary = color;
            } else {
                self.primary = color;
            }
        }
        response.on_hover_text("Click to set the primary color, right click the secondary");
    }

    /// The colour being edited as its hexadecimal code, then sliders of its channels,
    /// and of its hue, saturation and value.
    fn fields(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Hex:");
            let edited = self.edited();
            let mut text = self.hex.clone().unwrap_or_else(|| hex_code(edited));
            let response = ui.add(egui::TextEdit::singleline(&mut text).desired_width(80.0).char_limit(9));
            if response.changed() {
                if let Some(color) = parse_hex(&text) {
                    *self.edited_mut() = color;
                }
                self.hex = Some(text);
            }
            if !response.has_focus() {
                self.hex = None;
            }
        });
        ui.add_space(4.0);

        let color = self.edited();
        let opaque = |channel: usize, level: f32| {
            let mut rgb = [color[0], color[1], color[2]];
            rgb[channel] = (level * 255.0).round() as u8;
            Color32::from_rgb(rgb[0], rgb[1], rgb[2])
        };
        let see_through = |level: f32| {
            Color32::from_rgba_unmultiplied(color[0], color[1], color[2], (level * 255.0).round() as u8)
        };
        let gradients: [&dyn Fn(f32) -> Color32; 4] =
            [&|level| opaque(0, level), &|level| opaque(1, level), &|level| opaque(2, level), &see_through];
        let mut channels = color.0;
        for ((channel, label), gradient) in channels.iter_mut().zip(["R", "G", "B", "A"]).zip(gradients) {
            let mut level = *channel as f32 / 255.0;
            if slider_row(ui, label, &mut level, 255.0, "", label == "A", gradient) {
                *channel = (level * 255.0).round() as u8;
            }
        }
        *self.edited_mut() = Rgba(channels);
        self.follow_edited();
        ui.add_space(4.0);

        let mut hsv = self.hsv;
        let mut changed = false;
        let gradients: [&dyn Fn(f32) -> Color32; 3] = [
            &|hue| Hsv { hue, saturation: 1.0, value: 1.0 }.to_color32(),
            &|saturation| Hsv { saturation, ..hsv }.to_color32(),
            &|value| Hsv { value, ..hsv }.to_color32(),
        ];
        let mut parts = [hsv.hue, hsv.saturation, hsv.value];
        let rows = [("H", 360.0, "°"), ("S", 100.0, " %"), ("V", 100.0, " %")];
        for ((part, (label, scale, suffix)), gradient) in parts.iter_mut().zip(rows).zip(gradients) {
            changed |= slider_row(ui, label, part, scale, suffix, false, gradient);
        }
        if changed {
            [hsv.hue, hsv.saturation, hsv.value] = parts;
            self.set_from_hsv(hsv);
        }
    }
}

/// A slider of a part of a colour, from 0 to 1, over a gradient of the colours it goes
/// through, then a field of it as so many of `scale`. With `checkered`, the gradient
/// shows over light and dark squares, for a see-through colour to show as one.
/// Returns whether it changed.
fn slider_row(
    ui: &mut egui::Ui,
    label: &str,
    level: &mut f32,
    scale: f32,
    suffix: &str,
    checkered: bool,
    gradient: &dyn Fn(f32) -> Color32,
) -> bool {
    ui.horizontal(|ui| {
        let (label_rect, _) = ui.allocate_exact_size(Vec2::new(SLIDER_LABEL_WIDTH, SLIDER_HEIGHT), Sense::hover());
        ui.painter().text(
            label_rect.left_center(),
            egui::Align2::LEFT_CENTER,
            label,
            egui::TextStyle::Body.resolve(ui.style()),
            ui.visuals().text_color(),
        );
        let width = (ui.available_width() - SLIDER_FIELD_WIDTH - ui.spacing().item_spacing.x).max(SLIDER_HEIGHT);
        // Sensing only drags, a drag begins as soon as the button is pressed.
        let (rect, response) = ui.allocate_exact_size(Vec2::new(width, SLIDER_HEIGHT), Sense::drag());
        let mut changed = false;
        if response.dragged()
            && let Some(at) = response.interact_pointer_pos()
        {
            *level = ((at.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            changed = true;
        }

        let painter = ui.painter();
        if checkered {
            let side = rect.height() / 2.0;
            painter.rect_filled(rect, 0.0, Color32::WHITE);
            let columns = (rect.width() / side).ceil() as usize;
            for column in 0..columns {
                for row in 0..2 {
                    if (column + row) % 2 == 1 {
                        let min = rect.min + Vec2::new(column as f32 * side, row as f32 * side);
                        let square = Rect::from_min_max(min, (min + Vec2::splat(side)).min(rect.max));
                        painter.rect_filled(square, 0.0, Color32::from_gray(180));
                    }
                }
            }
        }
        let mut mesh = Mesh::default();
        for segment in 0..=SLIDER_SEGMENTS {
            let along = segment as f32 / SLIDER_SEGMENTS as f32;
            let x = rect.left() + along * rect.width();
            let color = gradient(along);
            mesh.colored_vertex(Pos2::new(x, rect.top()), color);
            mesh.colored_vertex(Pos2::new(x, rect.bottom()), color);
            if segment > 0 {
                let last = mesh.vertices.len() as u32 - 1;
                mesh.add_triangle(last - 3, last - 2, last);
                mesh.add_triangle(last - 3, last, last - 1);
            }
        }
        painter.add(Shape::mesh(mesh));
        painter.rect_stroke(rect, 0.0, Stroke::new(1.0, Color32::from_gray(128)), StrokeKind::Inside);
        let x = rect.left() + *level * rect.width();
        let marker = Rect::from_center_size(Pos2::new(x, rect.center().y), Vec2::new(4.0, rect.height() + 4.0));
        painter.rect_stroke(marker, 1.0, Stroke::new(3.0, Color32::BLACK), StrokeKind::Middle);
        painter.rect_stroke(marker, 1.0, Stroke::new(1.5, Color32::WHITE), StrokeKind::Middle);

        let mut shown = *level * scale;
        let field = egui::DragValue::new(&mut shown).range(0.0..=scale).speed(0.5).max_decimals(0).suffix(suffix);
        if ui.add_sized([SLIDER_FIELD_WIDTH, SLIDER_HEIGHT], field).changed() {
            *level = shown / scale;
            changed = true;
        }
        changed
    })
    .inner
}

/// What the right button does with the tools that draw in a colour.
fn right_click_option(ui: &mut egui::Ui, right_click: &mut RightClick) {
    ui.horizontal(|ui| {
        ui.label("Right click:");
        egui::ComboBox::from_id_salt("right_click")
            .selected_text(right_click.name())
            .show_ui(ui, |ui| {
                for option in RightClick::ALL {
                    ui.selectable_value(right_click, option, option.name());
                }
            })
            .response
            .on_hover_text(
                "What the right button does with the Paintbrush, the Pencil, the Spray, the Paint Bucket, the \
                 Gradient and the shapes: draw in the secondary color, or pick the color of the active layer as the \
                 one being edited here.",
            );
    });
}

/// A square of a colour, over light and dark quarters for a see-through one to show as one.
pub fn paint_swatch(painter: &egui::Painter, square: Rect, color: Rgba<u8>) {
    let quarter = square.size() / 2.0;
    let dark = Color32::from_gray(180);
    painter.rect_filled(square, 0.0, Color32::WHITE);
    painter.rect_filled(Rect::from_min_size(square.min, quarter), 0.0, dark);
    painter.rect_filled(Rect::from_min_size(square.min + quarter, quarter), 0.0, dark);
    let [red, green, blue, alpha] = color.0;
    painter.rect_filled(square, 0.0, Color32::from_rgba_unmultiplied(red, green, blue, alpha));
    painter.rect_stroke(square, 0.0, Stroke::new(1.0, Color32::from_gray(128)), StrokeKind::Inside);
}

/// The colour's code, RRGGBB, with AA after it for a colour that is not opaque.
pub fn hex_code(color: Rgba<u8>) -> String {
    let [red, green, blue, alpha] = color.0;
    match alpha {
        255 => format!("{red:02X}{green:02X}{blue:02X}"),
        _ => format!("{red:02X}{green:02X}{blue:02X}{alpha:02X}"),
    }
}

/// The colour of a code, RRGGBB or RRGGBBAA, after a # or not.
fn parse_hex(text: &str) -> Option<Rgba<u8>> {
    let digits = text.trim().trim_start_matches('#');
    if !matches!(digits.len(), 6 | 8) || !digits.is_ascii() {
        return None;
    }
    let byte = |index: usize| u8::from_str_radix(&digits[index * 2..index * 2 + 2], 16).ok();
    let alpha = if digits.len() == 8 { byte(3)? } else { 255 };
    Some(Rgba([byte(0)?, byte(1)?, byte(2)?, alpha]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsv_goes_both_ways() {
        let start = Hsv { hue: 0.0, saturation: 0.0, value: 0.0 };
        for code in PALETTE {
            let [_, red, green, blue] = code.to_be_bytes();
            assert_eq!(Hsv::of(Rgba([red, green, blue, 255]), start).to_rgb(), [red, green, blue], "{code:06x}");
        }
        let orange = Hsv::of(Rgba([255, 128, 0, 255]), start);
        assert!((orange.hue - 128.0 / 255.0 / 6.0).abs() < 1e-6);
        // Black and grey keep the hue they had.
        let black = Hsv::of(BLACK, orange);
        assert_eq!((black.hue, black.saturation, black.value), (orange.hue, orange.saturation, 0.0));
        let grey = Hsv::of(Rgba([128, 128, 128, 255]), orange);
        assert_eq!((grey.hue, grey.saturation), (orange.hue, 0.0));
    }

    #[test]
    fn hex_codes() {
        assert_eq!(parse_hex("#FF8000"), Some(Rgba([255, 128, 0, 255])));
        assert_eq!(parse_hex("ff800080"), Some(Rgba([255, 128, 0, 128])));
        assert_eq!(parse_hex("ff80"), None);
        assert_eq!(parse_hex("gg8000"), None);
        assert_eq!(hex_code(Rgba([255, 128, 0, 255])), "FF8000");
        assert_eq!(hex_code(Rgba([255, 128, 0, 128])), "FF800080");
    }
}

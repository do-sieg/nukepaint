//! The icons of the buttons, from the SVG files in `icons/`, built into the app. Each
//! is drawn at the screen's scale and the size it shows at the first time it shows,
//! and again when the scale changes.

use std::collections::HashMap;

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions, Vec2};
use power_paint_core::render_svg;

/// Size of an icon, in points, which the SVG files are drawn for.
pub const ICON_SIZE: Vec2 = Vec2::splat(16.0);

#[derive(Clone, Copy)]
pub struct Icon {
    name: &'static str,
    svg: &'static [u8],
}

macro_rules! icon {
    ($name:literal) => {
        Icon { name: $name, svg: include_bytes!(concat!("../../../icons/", $name, ".svg")) }
    };
}

pub const RECT_SELECT: Icon = icon!("rect_select");
pub const ELLIPSE_SELECT: Icon = icon!("ellipse_select");
pub const MOVE_SELECTION: Icon = icon!("move_selection");

#[derive(Default)]
pub struct Icons {
    /// Each icon at each size it was asked for, in points.
    textures: HashMap<(&'static str, u32), TextureHandle>,
    /// The screen's scale the textures were drawn at.
    pixels_per_point: f32,
}

impl Icons {
    pub fn get(&mut self, ctx: &egui::Context, icon: Icon) -> &TextureHandle {
        self.get_sized(ctx, icon, ICON_SIZE.x)
    }

    /// The icon to show `side` points wide, drawn that large rather than stretched.
    pub fn get_sized(&mut self, ctx: &egui::Context, icon: Icon, side: f32) -> &TextureHandle {
        let pixels_per_point = ctx.pixels_per_point();
        if pixels_per_point != self.pixels_per_point {
            self.textures.clear();
            self.pixels_per_point = pixels_per_point;
        }
        self.textures.entry((icon.name, side.to_bits())).or_insert_with(|| {
            // The icons are part of the app, so one that cannot be drawn is a bug.
            let scale = pixels_per_point * side / ICON_SIZE.x;
            let image = render_svg(icon.svg, scale).expect("icons are valid SVG");
            let size = [image.width() as usize, image.height() as usize];
            let pixels = ColorImage::from_rgba_unmultiplied(size, image.as_raw());
            ctx.load_texture(format!("{} icon", icon.name), pixels, TextureOptions::LINEAR)
        })
    }
}

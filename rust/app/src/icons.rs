//! The icons of the buttons, from the SVG files in `icons/`, built into the app. Each
//! is drawn at the screen's scale the first time it shows, and again when that changes.

use std::collections::HashMap;

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions, Vec2};
use power_paint_core::render_svg;

/// Size of an icon, in points.
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

#[derive(Default)]
pub struct Icons {
    textures: HashMap<&'static str, TextureHandle>,
    /// The screen's scale the textures were drawn at.
    pixels_per_point: f32,
}

impl Icons {
    pub fn get(&mut self, ctx: &egui::Context, icon: Icon) -> &TextureHandle {
        let pixels_per_point = ctx.pixels_per_point();
        if pixels_per_point != self.pixels_per_point {
            self.textures.clear();
            self.pixels_per_point = pixels_per_point;
        }
        self.textures.entry(icon.name).or_insert_with(|| {
            // The icons are part of the app, so one that cannot be drawn is a bug.
            let image = render_svg(icon.svg, pixels_per_point).expect("icons are valid SVG");
            let size = [image.width() as usize, image.height() as usize];
            let pixels = ColorImage::from_rgba_unmultiplied(size, image.as_raw());
            ctx.load_texture(format!("{} icon", icon.name), pixels, TextureOptions::LINEAR)
        })
    }
}

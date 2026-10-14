//! Pixels on the GPU, cut into tiles, each a texture of its own: a GPU takes no
//! texture wider or taller than it allows, which an image can easily be.

use eframe::egui::{self, Color32, ColorImage, Painter, Pos2, Rect, TextureHandle, TextureOptions, Vec2};
use power_paint_core::RgbaImage;

/// Widest and tallest a tile is. Every GPU takes textures this large.
const TILE_SIZE: u32 = 2048;

pub struct TiledTexture {
    size: Vec2,
    /// Each tile, with where it is in pixels of the whole.
    tiles: Vec<(Rect, TextureHandle)>,
}

impl TiledTexture {
    pub fn new(ctx: &egui::Context, name: &str, image: &RgbaImage, options: TextureOptions) -> Self {
        let (width, height) = image.dimensions();
        let mut tiles = Vec::new();
        for y in (0..height).step_by(TILE_SIZE as usize) {
            for x in (0..width).step_by(TILE_SIZE as usize) {
                let (tile_width, tile_height) = (TILE_SIZE.min(width - x), TILE_SIZE.min(height - y));
                let mut pixels = Vec::with_capacity((tile_width * tile_height * 4) as usize);
                for row in y..y + tile_height {
                    let start = ((row * width + x) * 4) as usize;
                    pixels.extend_from_slice(&image.as_raw()[start..start + (tile_width * 4) as usize]);
                }
                let tile = ColorImage::from_rgba_unmultiplied([tile_width as usize, tile_height as usize], &pixels);
                let rect = Rect::from_min_size(
                    Pos2::new(x as f32, y as f32),
                    Vec2::new(tile_width as f32, tile_height as f32),
                );
                tiles.push((rect, ctx.load_texture(format!("{name} {x},{y}"), tile, options)));
            }
        }
        Self { size: Vec2::new(width as f32, height as f32), tiles }
    }

    /// Size in pixels.
    pub fn size(&self) -> Vec2 {
        self.size
    }

    /// Draws the whole, stretched over `rect`.
    pub fn paint(&self, painter: &Painter, rect: Rect) {
        let scale = rect.size() / self.size;
        let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        for (tile, texture) in &self.tiles {
            let tile_rect = Rect::from_min_size(rect.min + tile.min.to_vec2() * scale, tile.size() * scale);
            painter.image(texture.id(), tile_rect, uv, Color32::WHITE);
        }
    }
}

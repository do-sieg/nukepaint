//! Pixels on the GPU, cut into tiles, each a texture of its own: a GPU takes no
//! texture wider or taller than it allows, which an image can easily be. Shrunk
//! copies, each half the size of the one before, show the image zoomed out, as
//! the textures are not mipmapped.

use eframe::egui::{self, Color32, ColorImage, Painter, Pos2, Rect, TextureHandle, TextureOptions, Vec2};
use power_paint_core::{RgbaImage, halve};

/// Widest and tallest a tile is. Every GPU takes textures this large.
const TILE_SIZE: u32 = 2048;
/// No shrunk copy is made once the image is this small, in pixels.
const SMALLEST_LEVEL: u32 = 64;

pub struct TiledTexture {
    size: Vec2,
    /// The image at its full size, then at half each time.
    levels: Vec<Level>,
}

struct Level {
    size: Vec2,
    /// Each tile, with where it is in pixels of the level.
    tiles: Vec<(Rect, TextureHandle)>,
}

impl TiledTexture {
    pub fn new(ctx: &egui::Context, name: &str, image: &RgbaImage) -> Self {
        let mut levels = vec![Level::new(ctx, &format!("{name} 0"), image)];
        let mut shrunk = None;
        loop {
            let previous: &RgbaImage = shrunk.as_ref().unwrap_or(image);
            if previous.width().max(previous.height()) / 2 < SMALLEST_LEVEL {
                break;
            }
            let half = halve(previous);
            levels.push(Level::new(ctx, &format!("{name} {}", levels.len()), &half));
            shrunk = Some(half);
        }
        Self { size: levels[0].size, levels }
    }

    /// Size in pixels.
    pub fn size(&self) -> Vec2 {
        self.size
    }

    /// Draws the whole, stretched over `rect`, from the copy closest to that size
    /// without being smaller.
    pub fn paint(&self, painter: &Painter, rect: Rect) {
        let screen_scale = rect.width() * painter.pixels_per_point() / self.size.x;
        let level = if screen_scale >= 1.0 { 0 } else { (1.0 / screen_scale).log2().floor() as usize };
        self.levels[level.min(self.levels.len() - 1)].paint(painter, rect);
    }
}

impl Level {
    fn new(ctx: &egui::Context, name: &str, image: &RgbaImage) -> Self {
        // Enlarged pixels stay sharp; a reduced image is smoothed.
        let options = TextureOptions {
            magnification: egui::TextureFilter::Nearest,
            minification: egui::TextureFilter::Linear,
            ..Default::default()
        };
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

    fn paint(&self, painter: &Painter, rect: Rect) {
        let scale = rect.size() / self.size;
        let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        for (tile, texture) in &self.tiles {
            let tile_rect = Rect::from_min_size(rect.min + tile.min.to_vec2() * scale, tile.size() * scale);
            painter.image(texture.id(), tile_rect, uv, Color32::WHITE);
        }
    }
}

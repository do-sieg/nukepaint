//! Document model and pixel operations. No UI code here.

mod affine;
mod blend;
mod document;
mod files;
mod floating;
mod layer;
mod scale;
mod selected_pixels;
mod selection;

pub use affine::Affine;
pub use blend::{blend_layer, composite};
pub use document::Document;
pub use files::{OPEN_EXTENSIONS, OpenError, SAVE_EXTENSIONS, is_savable, load_image, render_svg, save_image};
pub use floating::FloatingPixels;
pub use image::RgbaImage;
pub use layer::{BlendMode, Layer, MAX_OPACITY};
pub use scale::{halve, thumbnail};
pub use selected_pixels::{copy_selected, erase_selected};
pub use selection::{Bounds, PixelRect, Selection, SelectionMode};

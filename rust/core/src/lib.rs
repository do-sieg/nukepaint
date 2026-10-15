//! Document model and pixel operations. No UI code here.

mod affine;
mod blend;
mod document;
mod files;
mod floating;
mod flood_fill;
mod history;
mod layer;
mod mask;
mod scale;
mod selected_pixels;
mod selection;
mod spray;
mod stroke;

pub use affine::Affine;
pub use blend::{blend_layer, composite, composite_area};
pub use document::Document;
pub use files::{OPEN_EXTENSIONS, OpenError, SAVE_EXTENSIONS, is_savable, load_image, render_svg, save_image};
pub use floating::FloatingPixels;
pub use flood_fill::FloodFill;
pub use history::Step;
pub use image::{Rgba, RgbaImage};
pub use layer::{BlendMode, Layer, MAX_OPACITY};
pub use mask::SelectionMask;
pub use scale::{Interpolation, halve, resize, thumbnail};
pub use selected_pixels::{copy_selected, erase_selected};
pub use selection::{Bounds, Frame, PixelRect, Selection, SelectionMode};
pub use spray::{MAX_SPRAY_SIZE, Spray};
pub use stroke::{BrushStroke, Ink, MAX_TIP_SIZE, hard_tip};

//! Document model and pixel operations. No UI code here.

mod affine;
mod blend;
mod document;
mod files;
mod floating;
mod history;
mod layer;
mod mask;
mod pencil;
mod scale;
mod selected_pixels;
mod selection;

pub use affine::Affine;
pub use blend::{blend_layer, composite, composite_area};
pub use document::Document;
pub use files::{OPEN_EXTENSIONS, OpenError, SAVE_EXTENSIONS, is_savable, load_image, render_svg, save_image};
pub use floating::FloatingPixels;
pub use history::Step;
pub use image::{Rgba, RgbaImage};
pub use layer::{BlendMode, Layer, MAX_OPACITY};
pub use mask::SelectionMask;
pub use pencil::{MAX_PENCIL_SIZE, PencilStroke, pencil_tip};
pub use scale::{halve, thumbnail};
pub use selected_pixels::{copy_selected, erase_selected};
pub use selection::{Bounds, PixelRect, Selection, SelectionMode};

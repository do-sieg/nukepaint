//! Document model and pixel operations. No UI code here.

mod affine;
mod document;
mod files;
mod floating;
mod layer;
mod scale;
mod selection;

pub use affine::Affine;
pub use document::Document;
pub use files::{OPEN_EXTENSIONS, OpenError, load_image, render_svg};
pub use floating::FloatingPixels;
pub use image::RgbaImage;
pub use layer::Layer;
pub use scale::{halve, thumbnail};
pub use selection::{Bounds, PixelRect, Selection, SelectionMode};

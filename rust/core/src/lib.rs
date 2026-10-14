//! Document model and pixel operations. No UI code here.

mod document;
mod files;
mod layer;
mod scale;
mod selection;

pub use document::Document;
pub use files::{OPEN_EXTENSIONS, OpenError, load_image};
pub use image::RgbaImage;
pub use layer::Layer;
pub use scale::{halve, thumbnail};
pub use selection::{PixelRect, Selection, SelectionMode};

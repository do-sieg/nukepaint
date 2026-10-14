//! Document model and pixel operations. No UI code here.

mod document;
mod files;
mod layer;

pub use document::Document;
pub use files::{OPEN_EXTENSIONS, OpenError, load_image};
pub use image::RgbaImage;
pub use layer::Layer;

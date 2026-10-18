//! Document model and pixel operations. No UI code here.

mod adjustment;
mod affine;
mod blend;
mod document;
mod effect;
mod files;
mod floating;
mod flood_fill;
mod gradient;
mod history;
mod layer;
mod mask;
mod open_raster;
mod palette;
mod palette_file;
mod palette_presets;
mod retouch;
mod scale;
mod selected_pixels;
mod selection;
mod shape;
mod spray;
mod stroke;
mod turn;
mod wrap;

pub use adjustment::{Adjustment, Curves, InvertMode, adjust, sample_curve};
pub use affine::{Affine, Projective};
pub use blend::{blend_layer, composite, composite_area};
pub use document::Document;
pub(crate) use effect::GRAIN_MIDDLE;
pub use effect::{
    CellStyle, EdgeAlgorithm, Effect, FocusShape, FractalKind, LightSource, NewsprintScreen, Noise, NoiseColorSpace, NoiseKind, OffsetEdges, WaveShape, WindDirection, apply_effect,
    wavelet_decompose,
};
pub use files::{OPEN_EXTENSIONS, OpenError, SAVE_EXTENSIONS, is_savable, keeps_layers, load_image, render_svg, render_svg_to_width, save_image};
pub use floating::FloatingPixels;
pub use flood_fill::FloodFill;
pub use gradient::{Gradient, GradientFill, GradientKind, GradientMode};
pub use history::Step;
pub use image::{Rgba, RgbaImage};
pub use layer::{BlendMode, Layer, MAX_OPACITY};
pub use mask::SelectionMask;
pub use palette::{DEFAULT_PALETTE, MAX_PALETTE_SIZE, Palette, palette_of};
pub use palette_presets::{PRESET_GROUPS, PRESETS, Preset};
pub use palette_file::{PALETTE_EXTENSIONS, PaletteError, load_palette, save_palette};
pub use retouch::{Retouch, RetouchStroke};
pub use scale::{Interpolation, halve, resize, thumbnail};
pub use selected_pixels::{copy_selected, erase_selected};
pub use selection::{Bounds, Frame, Overlaps, PixelRect, Selection, SelectionMode};
pub use shape::{Shape, ShapeCanvas, ShapeKind, ShapeStyle};
pub use spray::{MAX_SPRAY_SIZE, Spray};
pub use stroke::{BrushStroke, Ink, MAX_TIP_SIZE, hard_tip};
pub use turn::{Turn, turn_pixels};

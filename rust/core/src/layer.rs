use image::RgbaImage;

/// Opacity of a layer that hides all it covers of the layers under it.
pub const MAX_OPACITY: u8 = 255;

/// One sheet of pixels in a document, in straight (not premultiplied) RGBA, and how
/// it is blended over the layers under it.
#[derive(Clone)]
pub struct Layer {
    pub name: String,
    pub pixels: RgbaImage,
    pub visible: bool,
    pub opacity: u8,
    pub blend_mode: BlendMode,
}

impl Layer {
    /// A layer that shows whole, blended normally.
    pub fn new(name: impl Into<String>, pixels: RgbaImage) -> Self {
        Self { name: name.into(), pixels, visible: true, opacity: MAX_OPACITY, blend_mode: BlendMode::Normal }
    }

    /// Whether any of it shows in the image.
    pub fn shows(&self) -> bool {
        self.visible && self.opacity > 0
    }
}

/// How a layer's colours are mixed with those of the layers under it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Additive,
    ColorBurn,
    ColorDodge,
    Reflect,
    Glow,
    Overlay,
    Difference,
    Negation,
    Lighten,
    Darken,
    Screen,
    Xor,
}

impl BlendMode {
    pub const ALL: [Self; 14] = [
        Self::Normal,
        Self::Multiply,
        Self::Additive,
        Self::ColorBurn,
        Self::ColorDodge,
        Self::Reflect,
        Self::Glow,
        Self::Overlay,
        Self::Difference,
        Self::Negation,
        Self::Lighten,
        Self::Darken,
        Self::Screen,
        Self::Xor,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Multiply => "Multiply",
            Self::Additive => "Additive",
            Self::ColorBurn => "Color Burn",
            Self::ColorDodge => "Color Dodge",
            Self::Reflect => "Reflect",
            Self::Glow => "Glow",
            Self::Overlay => "Overlay",
            Self::Difference => "Difference",
            Self::Negation => "Negation",
            Self::Lighten => "Lighten",
            Self::Darken => "Darken",
            Self::Screen => "Screen",
            Self::Xor => "Xor",
        }
    }
}

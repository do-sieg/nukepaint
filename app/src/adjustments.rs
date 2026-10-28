//! What the Colors menu and its shortcuts do: each command, its name, its keys and
//! the settings its dialog asks for, if any.

use std::collections::HashMap;

use eframe::egui::{Key, KeyboardShortcut, Modifiers};
use nukepaint_core::{Adjustment, AutoLevelMode, DesaturateMode, DitherMethod, InvertMode, Palette};

use crate::adjustment_dialog::{FilterKind, PRIMARY, SECONDARY, Setting, check, choice, color, pack_color, slider, unpack_color};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AdjustmentKind {
    AutoLevel,
    BlackAndWhite,
    BrightnessContrast,
    ColorBalance,
    ColorToAlpha,
    Colorize,
    Curves,
    Dither,
    Equalize,
    Exposure,
    GradientMap,
    HighlightsShadows,
    HueSaturation,
    Invert,
    InvertAlpha,
    InvertColors,
    Levels,
    PaletteMap,
    Posterize,
    ReplaceColor,
    Sepia,
    TemperatureTint,
    Threshold,
    WhiteBalance,
}

use AdjustmentKind::*;

const AUTO_LEVEL_MODES: [&str; 3] = ["Each channel", "Keep colors", "HSV"];
const AUTO_LEVEL: [Setting; 2] =
    [choice("Mode:", &AUTO_LEVEL_MODES, 0), slider("Clip:", 0.0, 10.0, 0.1, 0.0, " %")];
const BLACK_AND_WHITE_MODES: [&str; 5] = ["Luminance", "Luma", "Lightness", "Average", "Value"];
const BLACK_AND_WHITE: [Setting; 1] = [choice("Mode:", &BLACK_AND_WHITE_MODES, 1)];
const BRIGHTNESS_CONTRAST: [Setting; 2] = [
    slider("Brightness:", -100.0, 100.0, 1.0, 0.0, ""),
    slider("Contrast:", -100.0, 100.0, 1.0, 0.0, ""),
];
const RANGES: [&str; 3] = ["Shadows", "Midtones", "Highlights"];
/// Which range is shown, then the three sliders of each range, shown only while it is.
const COLOR_BALANCE: [Setting; 11] = [
    choice("Range:", &RANGES, 1),
    slider("Cyan – Red:", -100.0, 100.0, 1.0, 0.0, "").shown_when(0, 0.0),
    slider("Magenta – Green:", -100.0, 100.0, 1.0, 0.0, "").shown_when(0, 0.0),
    slider("Yellow – Blue:", -100.0, 100.0, 1.0, 0.0, "").shown_when(0, 0.0),
    slider("Cyan – Red:", -100.0, 100.0, 1.0, 0.0, "").shown_when(0, 1.0),
    slider("Magenta – Green:", -100.0, 100.0, 1.0, 0.0, "").shown_when(0, 1.0),
    slider("Yellow – Blue:", -100.0, 100.0, 1.0, 0.0, "").shown_when(0, 1.0),
    slider("Cyan – Red:", -100.0, 100.0, 1.0, 0.0, "").shown_when(0, 2.0),
    slider("Magenta – Green:", -100.0, 100.0, 1.0, 0.0, "").shown_when(0, 2.0),
    slider("Yellow – Blue:", -100.0, 100.0, 1.0, 0.0, "").shown_when(0, 2.0),
    check("Preserve luminosity", true),
];
const COLOR_TO_ALPHA: [Setting; 3] = [
    color("Color:", pack_color([255, 255, 255, 255])),
    slider("Transparency threshold:", 0.0, 100.0, 1.0, 0.0, " %"),
    slider("Opacity threshold:", 0.0, 100.0, 1.0, 100.0, " %"),
];
const COLORIZE: [Setting; 3] = [
    slider("Hue:", 0.0, 360.0, 1.0, 30.0, "°"),
    slider("Saturation:", 0.0, 100.0, 1.0, 50.0, " %"),
    slider("Lightness:", -100.0, 100.0, 1.0, 0.0, ""),
];
const DITHER_METHODS: [&str; 4] = ["Floyd–Steinberg", "Atkinson", "Ordered", "Random"];
const DITHER: [Setting; 2] =
    [slider("Levels:", 2.0, 256.0, 1.0, 4.0, "").logarithmic(), choice("Method:", &DITHER_METHODS, 0)];
const EQUALIZE_MODES: [&str; 2] = ["Each channel", "Lightness"];
const EQUALIZE: [Setting; 1] = [choice("Mode:", &EQUALIZE_MODES, 0)];
const EXPOSURE: [Setting; 1] = [slider("Exposure:", -5.0, 5.0, 0.01, 0.0, " EV")];
/// From the primary colour to the secondary one, which starts as black to white.
const GRADIENT_MAP: [Setting; 2] = [color("Dark:", PRIMARY), color("Light:", SECONDARY)];
const HIGHLIGHTS_SHADOWS: [Setting; 2] = [
    slider("Shadows:", -100.0, 100.0, 1.0, 0.0, ""),
    slider("Highlights:", -100.0, 100.0, 1.0, 0.0, ""),
];
const HUE_SATURATION_MODES: [&str; 2] = ["HSV", "Perceptual (OKLCh)"];
const HUE_SATURATION: [Setting; 4] = [
    choice("Mode:", &HUE_SATURATION_MODES, 0),
    slider("Hue:", -180.0, 180.0, 1.0, 0.0, "°"),
    slider("Saturation:", 0.0, 200.0, 1.0, 100.0, " %"),
    slider("Lightness:", -100.0, 100.0, 1.0, 0.0, ""),
];
const INVERT_MODES: [&str; 3] = ["Normal", "Linear", "Value"];
/// Value mode leaves red, green and blue to it.
const INVERT: [Setting; 5] = [
    choice("Mode:", &INVERT_MODES, 0),
    check("Red", true).enabled_unless(0, 2.0),
    check("Green", true).enabled_unless(0, 2.0),
    check("Blue", true).enabled_unless(0, 2.0),
    check("Alpha", false),
];
const LEVELS: [Setting; 8] = [
    slider("Input black:", 0.0, 255.0, 1.0, 0.0, ""),
    slider("Input white:", 0.0, 255.0, 1.0, 255.0, ""),
    slider("Gamma:", 0.1, 10.0, 0.01, 1.0, "").logarithmic(),
    slider("Output black:", 0.0, 255.0, 1.0, 0.0, ""),
    slider("Output white:", 0.0, 255.0, 1.0, 255.0, ""),
    check("Red", true),
    check("Green", true),
    check("Blue", true),
];
const PALETTE_MAP: [Setting; 2] = [check("Reverse", false), check("Smooth", false)];
const POSTERIZE: [Setting; 1] = [slider("Levels:", 2.0, 256.0, 1.0, 4.0, "").logarithmic()];
const REPLACE_COLOR: [Setting; 4] = [
    color("Color:", PRIMARY),
    color("Replace with:", SECONDARY),
    slider("Tolerance:", 0.0, 100.0, 1.0, 0.0, " %"),
    check("Keep shading", false),
];
const TEMPERATURE_TINT: [Setting; 2] = [
    slider("Temperature:", -100.0, 100.0, 1.0, 0.0, ""),
    slider("Tint:", -100.0, 100.0, 1.0, 0.0, ""),
];
const THRESHOLD: [Setting; 2] = [slider("Low:", 0.0, 255.0, 1.0, 128.0, ""), slider("High:", 0.0, 255.0, 1.0, 255.0, "")];
const WHITE_BALANCE: [Setting; 1] = [slider("Strength:", 0.0, 100.0, 1.0, 100.0, " %")];

impl AdjustmentKind {
    /// The Colors menu, in groups that separators keep apart: automatic, tone, colour,
    /// mapping, monochrome, fewer levels, inversions and transparency.
    pub const MENU: [&[Self]; 8] = [
        &[AutoLevel, Equalize, WhiteBalance],
        &[BrightnessContrast, Exposure, HighlightsShadows, Levels, Curves],
        &[HueSaturation, ColorBalance, TemperatureTint, Colorize],
        &[ReplaceColor, GradientMap, PaletteMap],
        &[BlackAndWhite, Sepia, Threshold],
        &[Posterize, Dither],
        &[InvertColors, InvertAlpha, Invert],
        &[ColorToAlpha],
    ];

    /// Its name, which its undo step is called as well.
    pub fn name(self) -> &'static str {
        match self {
            AutoLevel => "Auto-Level",
            BlackAndWhite => "Black and White",
            BrightnessContrast => "Brightness / Contrast",
            ColorBalance => "Color Balance",
            ColorToAlpha => "Color to Alpha",
            Colorize => "Colorize",
            Curves => "Curves",
            Dither => "Dither",
            Equalize => "Equalize",
            Exposure => "Exposure",
            GradientMap => "Gradient Map",
            HighlightsShadows => "Highlights / Shadows",
            HueSaturation => "Hue / Saturation",
            Invert => "Invert",
            InvertAlpha => "Invert Alpha",
            InvertColors => "Invert Colors",
            Levels => "Levels",
            PaletteMap => "Palette Map",
            Posterize => "Posterize",
            ReplaceColor => "Replace Color",
            Sepia => "Sepia",
            TemperatureTint => "Temperature and Tint",
            Threshold => "Threshold",
            WhiteBalance => "White Balance",
        }
    }

    /// Its name in the menu, which says whether a dialog asks for its settings first.
    pub fn label(self) -> String {
        if self.has_dialog() { format!("{}…", self.name()) } else { self.name().to_owned() }
    }

    pub fn shortcut(self) -> Option<KeyboardShortcut> {
        let key = match self {
            BlackAndWhite => Key::G,
            Curves => Key::M,
            HueSaturation => Key::U,
            InvertColors => Key::I,
            Levels => Key::L,
            Sepia => Key::E,
            _ => return None,
        };
        Some(KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), key))
    }

    /// The numbers its dialog asks for. Curves has a dialog of its own.
    pub fn settings(self) -> &'static [Setting] {
        match self {
            AutoLevel => &AUTO_LEVEL,
            BlackAndWhite => &BLACK_AND_WHITE,
            BrightnessContrast => &BRIGHTNESS_CONTRAST,
            ColorBalance => &COLOR_BALANCE,
            ColorToAlpha => &COLOR_TO_ALPHA,
            Colorize => &COLORIZE,
            Dither => &DITHER,
            Equalize => &EQUALIZE,
            Exposure => &EXPOSURE,
            GradientMap => &GRADIENT_MAP,
            HighlightsShadows => &HIGHLIGHTS_SHADOWS,
            HueSaturation => &HUE_SATURATION,
            Invert => &INVERT,
            Levels => &LEVELS,
            PaletteMap => &PALETTE_MAP,
            Posterize => &POSTERIZE,
            ReplaceColor => &REPLACE_COLOR,
            TemperatureTint => &TEMPERATURE_TINT,
            Threshold => &THRESHOLD,
            WhiteBalance => &WHITE_BALANCE,
            Curves | InvertAlpha | InvertColors | Sepia => &[],
        }
    }

    pub fn has_dialog(self) -> bool {
        self == Curves || !self.settings().is_empty()
    }

    /// The adjustment with these settings, in the order [`AdjustmentKind::settings`]
    /// lists them, or these curves, or the colours of this palette but its transparent one.
    pub fn adjustment(self, values: &[f64], curves: &nukepaint_core::Curves, palette: &Palette) -> Adjustment {
        let value = |index: usize| values[index] as f32;
        let checked = |index: usize| values[index] != 0.0;
        let rgb = |index: usize| {
            let [r, g, b, _] = unpack_color(values[index]);
            [r, g, b]
        };
        match self {
            AutoLevel => {
                let mode = [AutoLevelMode::Channels, AutoLevelMode::Together, AutoLevelMode::Hsv][values[0] as usize];
                Adjustment::AutoLevel { mode, clip: value(1) }
            }
            BlackAndWhite => {
                let modes = [
                    DesaturateMode::Luminance,
                    DesaturateMode::Luma,
                    DesaturateMode::Lightness,
                    DesaturateMode::Average,
                    DesaturateMode::Value,
                ];
                Adjustment::BlackAndWhite { mode: modes[values[0] as usize] }
            }
            BrightnessContrast => Adjustment::BrightnessContrast { brightness: value(0), contrast: value(1) },
            ColorBalance => {
                let range = |first: usize| [value(first), value(first + 1), value(first + 2)];
                Adjustment::ColorBalance { shadows: range(1), midtones: range(4), highlights: range(7), preserve_luminosity: checked(10) }
            }
            ColorToAlpha => Adjustment::ColorToAlpha { color: rgb(0), transparency: value(1), opacity: value(2) },
            Colorize => Adjustment::Colorize { hue: value(0), saturation: value(1), lightness: value(2) },
            Curves => Adjustment::Curves(curves.clone()),
            Dither => {
                let methods = [DitherMethod::FloydSteinberg, DitherMethod::Atkinson, DitherMethod::Ordered, DitherMethod::Random];
                Adjustment::Dither { levels: values[0].round() as u32, method: methods[values[1] as usize] }
            }
            Equalize => Adjustment::Equalize { lightness: checked(0) },
            Exposure => Adjustment::Exposure { exposure: value(0) },
            GradientMap => Adjustment::GradientMap { dark: rgb(0), light: rgb(1) },
            HighlightsShadows => Adjustment::HighlightsShadows { shadows: value(0), highlights: value(1) },
            HueSaturation => {
                Adjustment::HueSaturation { hue: value(1), saturation: value(2), lightness: value(3), perceptual: checked(0) }
            }
            Invert => {
                let mode = [InvertMode::Normal, InvertMode::Linear, InvertMode::Value][values[0] as usize];
                Adjustment::Invert { mode, channels: [checked(1), checked(2), checked(3)], alpha: checked(4) }
            }
            InvertAlpha => Adjustment::INVERT_ALPHA,
            InvertColors => Adjustment::INVERT_COLORS,
            Levels => Adjustment::Levels {
                channels: [checked(5), checked(6), checked(7)],
                input: [value(0), value(1)],
                gamma: value(2),
                output: [value(3), value(4)],
            },
            PaletteMap => {
                let colors = palette.colors.iter().enumerate().filter(|&(index, _)| palette.transparent != Some(index));
                let colors = colors.map(|(_, color)| [color[0], color[1], color[2]]).collect();
                Adjustment::PaletteMap { colors, reverse: checked(0), smooth: checked(1) }
            }
            Posterize => Adjustment::Posterize { levels: values[0].round() as u32 },
            ReplaceColor => {
                Adjustment::ReplaceColor { from: rgb(0), to: rgb(1), tolerance: value(2), keep_shading: checked(3) }
            }
            Sepia => Adjustment::Sepia,
            TemperatureTint => Adjustment::TemperatureTint { temperature: value(0), tint: value(1) },
            Threshold => Adjustment::Threshold { low: value(0), high: value(1) },
            WhiteBalance => Adjustment::WhiteBalance { strength: value(0) },
        }
    }
}

/// The settings each adjustment and each effect was last used with, which its dialog
/// opens with again.
#[derive(Default)]
pub struct LastSettings {
    values: HashMap<FilterKind, Vec<f64>>,
    pub curves: nukepaint_core::Curves,
}

impl LastSettings {
    pub fn values(&self, kind: FilterKind) -> Vec<f64> {
        let defaults = || kind.settings().iter().map(|setting| setting.default).collect();
        self.values.get(&kind).map_or_else(defaults, Vec::clone)
    }

    pub fn set_values(&mut self, kind: FilterKind, values: Vec<f64>) {
        self.values.insert(kind, values);
    }
}

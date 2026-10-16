//! What the Colors menu and its shortcuts do: each command, its name, its keys and
//! the settings its dialog asks for, if any.

use std::collections::HashMap;

use eframe::egui::{Key, KeyboardShortcut, Modifiers};
use power_paint_core::Adjustment;

use crate::adjustment_dialog::{FilterKind, Setting, slider};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AdjustmentKind {
    BlackAndWhite,
    BrightnessContrast,
    Curves,
    Exposure,
    HighlightsShadows,
    HueSaturation,
    Sepia,
}

use AdjustmentKind::*;

const BRIGHTNESS_CONTRAST: [Setting; 2] = [
    slider("Brightness:", -100.0, 100.0, 1.0, 0.0, ""),
    slider("Contrast:", -100.0, 100.0, 1.0, 0.0, ""),
];
const EXPOSURE: [Setting; 1] = [slider("Exposure:", -5.0, 5.0, 0.01, 0.0, " EV")];
const HIGHLIGHTS_SHADOWS: [Setting; 2] = [
    slider("Shadows:", -100.0, 100.0, 1.0, 0.0, ""),
    slider("Highlights:", -100.0, 100.0, 1.0, 0.0, ""),
];
const HUE_SATURATION: [Setting; 3] = [
    slider("Hue:", -180.0, 180.0, 1.0, 0.0, "°"),
    slider("Saturation:", 0.0, 200.0, 1.0, 100.0, " %"),
    slider("Lightness:", -100.0, 100.0, 1.0, 0.0, ""),
];

impl AdjustmentKind {
    /// The Colors menu.
    pub const MENU: [Self; 7] = [BlackAndWhite, BrightnessContrast, Curves, Exposure, HighlightsShadows, HueSaturation, Sepia];

    /// Its name, which its undo step is called as well.
    pub fn name(self) -> &'static str {
        match self {
            BlackAndWhite => "Black and White",
            BrightnessContrast => "Brightness / Contrast",
            Curves => "Curves",
            Exposure => "Exposure",
            HighlightsShadows => "Highlights / Shadows",
            HueSaturation => "Hue / Saturation",
            Sepia => "Sepia",
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
            Sepia => Key::E,
            _ => return None,
        };
        Some(KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), key))
    }

    /// The numbers its dialog asks for. Curves has a dialog of its own.
    pub fn settings(self) -> &'static [Setting] {
        match self {
            BrightnessContrast => &BRIGHTNESS_CONTRAST,
            Exposure => &EXPOSURE,
            HighlightsShadows => &HIGHLIGHTS_SHADOWS,
            HueSaturation => &HUE_SATURATION,
            BlackAndWhite | Curves | Sepia => &[],
        }
    }

    pub fn has_dialog(self) -> bool {
        self == Curves || !self.settings().is_empty()
    }

    /// The adjustment with these settings, in the order [`AdjustmentKind::settings`]
    /// lists them, or these curves.
    pub fn adjustment(self, values: &[f64], curves: &power_paint_core::Curves) -> Adjustment {
        let value = |index: usize| values[index] as f32;
        match self {
            BlackAndWhite => Adjustment::BlackAndWhite,
            BrightnessContrast => Adjustment::BrightnessContrast { brightness: value(0), contrast: value(1) },
            Curves => Adjustment::Curves(curves.clone()),
            Exposure => Adjustment::Exposure { exposure: value(0) },
            HighlightsShadows => Adjustment::HighlightsShadows { shadows: value(0), highlights: value(1) },
            HueSaturation => Adjustment::HueSaturation { hue: value(0), saturation: value(1), lightness: value(2) },
            Sepia => Adjustment::Sepia,
        }
    }
}

/// The settings each adjustment and each effect was last used with, which its dialog
/// opens with again.
#[derive(Default)]
pub struct LastSettings {
    values: HashMap<FilterKind, Vec<f64>>,
    pub curves: power_paint_core::Curves,
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

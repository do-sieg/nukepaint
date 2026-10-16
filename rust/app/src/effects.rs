//! What the Filters menu does: each effect, its name and the settings its dialog asks for.

use power_paint_core::Effect;

use crate::adjustment_dialog::{Setting, check, slider};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EffectKind {
    BokehBlur,
    FragmentBlur,
    GaussianBlur,
    MedianBlur,
    MotionBlur,
    RadialBlur,
    SketchBlur,
    SquareBlur,
    SurfaceBlur,
    ZoomBlur,
    Sharpen,
}

use EffectKind::*;

const BOKEH: [Setting; 2] = [slider("Radius:", 1.0, 100.0, 1.0, 10.0, " px"), slider("Highlights:", 0.0, 100.0, 1.0, 30.0, "")];
const FRAGMENT: [Setting; 3] = [
    slider("Fragments:", 2.0, 50.0, 1.0, 4.0, ""),
    slider("Distance:", 0.0, 100.0, 1.0, 8.0, " px"),
    slider("Rotation:", 0.0, 360.0, 1.0, 0.0, "°"),
];
const GAUSSIAN: [Setting; 1] = [slider("Radius:", 0.0, 200.0, 1.0, 2.0, " px")];
const MEDIAN: [Setting; 2] = [slider("Radius:", 1.0, 20.0, 1.0, 10.0, " px"), slider("Percentile:", 0.0, 100.0, 1.0, 50.0, " %")];
const MOTION: [Setting; 3] = [
    slider("Angle:", -180.0, 180.0, 1.0, 25.0, "°"),
    slider("Distance:", 1.0, 200.0, 1.0, 10.0, " px"),
    check("Centered", true),
];
const RADIAL: [Setting; 3] = [
    slider("Angle:", 0.0, 360.0, 1.0, 2.0, "°"),
    slider("Center X:", -100.0, 100.0, 1.0, 0.0, " %"),
    slider("Center Y:", -100.0, 100.0, 1.0, 0.0, " %"),
];
const SKETCH: [Setting; 1] = [slider("Radius:", 1.0, 100.0, 1.0, 10.0, " px")];
const SQUARE: [Setting; 2] = [slider("Radius:", 1.0, 100.0, 1.0, 10.0, " px"), slider("Highlights:", 0.0, 100.0, 1.0, 30.0, "")];
const SURFACE: [Setting; 2] = [slider("Radius:", 1.0, 100.0, 1.0, 6.0, " px"), slider("Threshold:", 1.0, 100.0, 1.0, 15.0, " %")];
const ZOOM: [Setting; 3] = [
    slider("Amount:", 0.0, 100.0, 1.0, 10.0, " %"),
    slider("Center X:", -100.0, 100.0, 1.0, 0.0, " %"),
    slider("Center Y:", -100.0, 100.0, 1.0, 0.0, " %"),
];
const SHARPEN: [Setting; 3] = [
    slider("Radius:", 1.0, 50.0, 1.0, 3.0, " px"),
    slider("Amount:", 0.0, 300.0, 1.0, 50.0, " %"),
    slider("Threshold:", 0.0, 100.0, 1.0, 0.0, " %"),
];

impl EffectKind {
    /// The menus of the Filters menu, each with its effects.
    pub const MENUS: [(&str, &[Self]); 2] = [
        ("Blurs", &[BokehBlur, FragmentBlur, GaussianBlur, MedianBlur, MotionBlur, RadialBlur, SketchBlur, SquareBlur, SurfaceBlur, ZoomBlur]),
        ("Photo", &[Sharpen]),
    ];

    /// Its name, which its undo step is called as well.
    pub fn name(self) -> &'static str {
        match self {
            BokehBlur => "Bokeh Blur",
            FragmentBlur => "Fragment Blur",
            GaussianBlur => "Gaussian Blur",
            MedianBlur => "Median Blur",
            MotionBlur => "Motion Blur",
            RadialBlur => "Radial Blur",
            SketchBlur => "Sketch Blur",
            SquareBlur => "Square Blur",
            SurfaceBlur => "Surface Blur",
            ZoomBlur => "Zoom Blur",
            Sharpen => "Sharpen",
        }
    }

    /// What its dialog asks for.
    pub fn settings(self) -> &'static [Setting] {
        match self {
            BokehBlur => &BOKEH,
            FragmentBlur => &FRAGMENT,
            GaussianBlur => &GAUSSIAN,
            MedianBlur => &MEDIAN,
            MotionBlur => &MOTION,
            RadialBlur => &RADIAL,
            SketchBlur => &SKETCH,
            SquareBlur => &SQUARE,
            SurfaceBlur => &SURFACE,
            ZoomBlur => &ZOOM,
            Sharpen => &SHARPEN,
        }
    }

    /// The effect with these settings, in the order [`EffectKind::settings`] lists them.
    pub fn effect(self, values: &[f64]) -> Effect {
        let value = |index: usize| values[index] as f32;
        match self {
            BokehBlur => Effect::BokehBlur { radius: value(0), highlights: value(1) },
            FragmentBlur => Effect::FragmentBlur { count: value(0) as u32, distance: value(1), rotation: value(2) },
            GaussianBlur => Effect::GaussianBlur { radius: value(0) },
            MedianBlur => Effect::MedianBlur { radius: value(0), percentile: value(1) },
            MotionBlur => Effect::MotionBlur { angle: value(0), distance: value(1), centered: values[2] != 0.0 },
            RadialBlur => Effect::RadialBlur { angle: value(0), center: [value(1), value(2)] },
            SketchBlur => Effect::SketchBlur { radius: value(0) },
            SquareBlur => Effect::SquareBlur { radius: value(0), highlights: value(1) },
            SurfaceBlur => Effect::SurfaceBlur { radius: value(0), threshold: value(1) },
            ZoomBlur => Effect::ZoomBlur { amount: value(0), center: [value(1), value(2)] },
            Sharpen => Effect::Sharpen { radius: value(0), amount: value(1), threshold: value(2) },
        }
    }
}

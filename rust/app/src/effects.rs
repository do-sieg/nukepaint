//! What the Filters menu does: each effect, its name and the settings its dialog asks for.

use std::sync::Arc;

use power_paint_core::{Effect, FocusShape, Interpolation, Layer, OffsetEdges, Turn};

use crate::adjustment_dialog::{Setting, check, choice, layer, slider};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EffectKind {
    ApplyCanvas,
    Cartoon,
    Clothify,
    Cubism,
    GlassTile,
    Glow,
    InkSketch,
    OilPainting,
    PencilSketch,
    Photocopy,
    Predator,
    Slic,
    VanGogh,
    Waterpixels,
    Weave,
    BokehBlur,
    FocusBlur,
    FragmentBlur,
    GaussianBlur,
    MeanCurvatureBlur,
    MedianBlur,
    MotionBlur,
    RadialBlur,
    SketchBlur,
    SquareBlur,
    SurfaceBlur,
    TileableBlur,
    VariableBlur,
    ZoomBlur,
    Antialias,
    Deinterlace,
    HighPass,
    RedEyeRemoval,
    ReduceNoise,
    WaveletDecompose,
    Sharpen,
    Offset,
    RotateZoom,
    Transform3d,
}

use EffectKind::*;

/// Where the Apply Canvas is lit from, as degrees anticlockwise from the right.
const CANVAS_LIGHTS: [f32; 4] = [135.0, 45.0, 225.0, 315.0];
/// The names of [`CANVAS_LIGHTS`].
const CANVAS_LIGHT_NAMES: [&str; 4] = ["Top-left", "Top-right", "Bottom-left", "Bottom-right"];
const CANVAS: [Setting; 2] = [choice("Light:", &CANVAS_LIGHT_NAMES, 0), slider("Depth:", 1.0, 50.0, 1.0, 4.0, "")];
const CARTOON: [Setting; 2] = [slider("Mask Radius:", 3.0, 50.0, 1.0, 7.0, " px"), slider("Percent Black:", 0.0, 100.0, 1.0, 20.0, " %")];
const CLOTHIFY: [Setting; 5] = [
    slider("Blur X:", 1.0, 100.0, 1.0, 9.0, " px"),
    slider("Blur Y:", 1.0, 100.0, 1.0, 9.0, " px"),
    slider("Azimuth:", 0.0, 360.0, 1.0, 135.0, "°"),
    slider("Elevation:", 0.0, 90.0, 1.0, 45.0, "°"),
    slider("Depth:", 1.0, 50.0, 1.0, 3.0, ""),
];
/// What the Cubism shows where there are no rectangles.
const CUBISM_BACKGROUNDS: [[u8; 4]; 3] = [[0, 0, 0, 0], [0, 0, 0, 255], [255, 255, 255, 255]];
/// The names of [`CUBISM_BACKGROUNDS`].
const CUBISM_BACKGROUND_NAMES: [&str; 3] = ["Transparent", "Black", "White"];
const CUBISM: [Setting; 3] = [
    slider("Tile Size:", 2.0, 100.0, 1.0, 10.0, " px"),
    slider("Tile Saturation:", 0.0, 10.0, 0.1, 2.5, ""),
    choice("Background:", &CUBISM_BACKGROUND_NAMES, 0),
];
const GLASS_TILE: [Setting; 2] = [slider("Tile Width:", 10.0, 500.0, 1.0, 25.0, " px"), slider("Tile Height:", 10.0, 500.0, 1.0, 25.0, " px")];
const GLOW: [Setting; 3] = [
    slider("Radius:", 1.0, 20.0, 1.0, 6.0, " px"),
    slider("Brightness:", -100.0, 100.0, 1.0, 10.0, ""),
    slider("Contrast:", -100.0, 100.0, 1.0, 10.0, ""),
];
const INK_SKETCH: [Setting; 2] = [slider("Ink Outline:", 0.0, 100.0, 1.0, 50.0, ""), slider("Coloring:", 0.0, 100.0, 1.0, 50.0, "")];
const OIL_PAINTING: [Setting; 2] = [slider("Brush Size:", 1.0, 8.0, 1.0, 3.0, " px"), slider("Coarseness:", 3.0, 255.0, 1.0, 50.0, "")];
const PENCIL_SKETCH: [Setting; 2] = [slider("Pencil Tip Size:", 1.0, 20.0, 1.0, 2.0, " px"), slider("Range:", -20.0, 20.0, 1.0, 0.0, "")];
const PHOTOCOPY: [Setting; 4] = [
    slider("Mask Radius:", 3.0, 50.0, 1.0, 8.0, " px"),
    slider("Sharpness:", 0.0, 100.0, 1.0, 50.0, " %"),
    slider("Percent Black:", 0.0, 100.0, 1.0, 20.0, " %"),
    slider("Percent White:", 0.0, 100.0, 1.0, 20.0, " %"),
];
const PREDATOR: [Setting; 3] = [
    slider("Edge Amount:", 1.0, 10.0, 0.1, 2.0, ""),
    check("Pixelize", true),
    slider("Pixel Width:", 1.0, 50.0, 1.0, 3.0, " px").enabled_by(1),
];
const SLIC: [Setting; 3] = [
    slider("Cluster Size:", 4.0, 256.0, 1.0, 32.0, " px"),
    slider("Compactness:", 0.0, 40.0, 1.0, 20.0, ""),
    slider("Iterations:", 1.0, 15.0, 1.0, 3.0, ""),
];
const VAN_GOGH: [Setting; 2] = [slider("Stroke Length:", 1.0, 64.0, 1.0, 10.0, " px"), slider("Strength:", 0.0, 100.0, 1.0, 50.0, " %")];
const WATERPIXELS_FILLS: [&str; 2] = ["Average", "Random"];
const WATERPIXELS: [Setting; 4] = [
    slider("Superpixel Size:", 8.0, 256.0, 1.0, 32.0, " px"),
    slider("Smoothness:", 0.0, 10.0, 0.5, 1.0, ""),
    slider("Regularization:", 0.0, 50.0, 1.0, 0.0, ""),
    choice("Fill:", &WATERPIXELS_FILLS, 0),
];
const WEAVE: [Setting; 5] = [
    slider("Ribbon Width:", 1.0, 200.0, 1.0, 30.0, " px"),
    slider("Ribbon Spacing:", 0.0, 100.0, 1.0, 10.0, " px"),
    slider("Shadow Darkness:", 0.0, 100.0, 1.0, 75.0, " %"),
    slider("Shadow Depth:", 0.0, 100.0, 1.0, 75.0, " %"),
    slider("Thread Intensity:", 0.0, 100.0, 1.0, 50.0, " %"),
];
const BOKEH: [Setting; 2] = [slider("Radius:", 1.0, 100.0, 1.0, 10.0, " px"), slider("Highlights:", 0.0, 100.0, 1.0, 30.0, "")];
/// The names of [`FocusShape::ALL`].
const FOCUS_SHAPES: [&str; 5] = ["Circle", "Square", "Diamond", "Horizontal", "Vertical"];
const FOCUS: [Setting; 6] = [
    slider("Radius:", 1.0, 100.0, 1.0, 10.0, " px"),
    choice("Shape:", &FOCUS_SHAPES, 0),
    slider("Center X:", -100.0, 100.0, 1.0, 0.0, " %"),
    slider("Center Y:", -100.0, 100.0, 1.0, 0.0, " %"),
    slider("Focus:", 0.0, 100.0, 1.0, 25.0, " %"),
    slider("Transition:", 0.0, 100.0, 1.0, 50.0, " %"),
];
const FRAGMENT: [Setting; 3] = [
    slider("Fragments:", 2.0, 50.0, 1.0, 4.0, ""),
    slider("Distance:", 0.0, 100.0, 1.0, 8.0, " px"),
    slider("Rotation:", 0.0, 360.0, 1.0, 0.0, "°"),
];
const GAUSSIAN: [Setting; 1] = [slider("Radius:", 0.0, 200.0, 1.0, 2.0, " px")];
const MEAN_CURVATURE: [Setting; 1] = [slider("Iterations:", 1.0, 100.0, 1.0, 20.0, "")];
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
const TILEABLE: [Setting; 1] = [slider("Radius:", 0.0, 200.0, 1.0, 2.0, " px")];
const VARIABLE: [Setting; 3] = [slider("Radius:", 1.0, 100.0, 1.0, 10.0, " px"), layer("Map:"), check("Invert", false)];
const ZOOM: [Setting; 3] = [
    slider("Amount:", 0.0, 100.0, 1.0, 10.0, " %"),
    slider("Center X:", -100.0, 100.0, 1.0, 0.0, " %"),
    slider("Center Y:", -100.0, 100.0, 1.0, 0.0, " %"),
];
/// The dialog shows these its own way: the moves are as long as the area they are in,
/// which starts them at half of it, and the edges are picked by name.
const OFFSET: [Setting; 3] = [
    slider("Horizontal:", -100_000.0, 100_000.0, 1.0, 0.0, " px"),
    slider("Vertical:", -100_000.0, 100_000.0, 1.0, 0.0, " px"),
    slider("Edges:", 0.0, 2.0, 1.0, 0.0, ""),
];
/// The names of [`Interpolation::ALL`].
const RESAMPLINGS: [&str; 4] = ["Nearest Neighbor", "Bilinear", "Bicubic", "Lanczos"];
const ROTATE_ZOOM: [Setting; 5] = [
    slider("Angle:", -180.0, 180.0, 1.0, 0.0, "°"),
    slider("Zoom:", 1.0, 1000.0, 1.0, 100.0, " %"),
    slider("Pan X:", -100.0, 100.0, 1.0, 0.0, " %"),
    slider("Pan Y:", -100.0, 100.0, 1.0, 0.0, " %"),
    choice("Resampling:", &RESAMPLINGS, 1),
];
const TRANSFORM_3D: [Setting; 8] = [
    slider("Rotate X:", -180.0, 180.0, 1.0, 0.0, "°"),
    slider("Rotate Y:", -180.0, 180.0, 1.0, 0.0, "°"),
    slider("Rotate Z:", -180.0, 180.0, 1.0, 0.0, "°"),
    slider("Field of View:", 1.0, 170.0, 1.0, 60.0, "°"),
    slider("Zoom:", 1.0, 1000.0, 1.0, 100.0, " %"),
    slider("Pan X:", -100.0, 100.0, 1.0, 0.0, " %"),
    slider("Pan Y:", -100.0, 100.0, 1.0, 0.0, " %"),
    choice("Resampling:", &RESAMPLINGS, 1),
];
const DEINTERLACE_FIELDS: [&str; 2] = ["Even", "Odd"];
const DEINTERLACE_ORIENTATIONS: [&str; 2] = ["Horizontal", "Vertical"];
const DEINTERLACE: [Setting; 3] = [
    choice("Keep:", &DEINTERLACE_FIELDS, 0),
    choice("Orientation:", &DEINTERLACE_ORIENTATIONS, 0),
    slider("Block Size:", 1.0, 64.0, 1.0, 1.0, " px"),
];
const HIGH_PASS: [Setting; 2] = [slider("Radius:", 1.0, 200.0, 1.0, 8.0, " px"), slider("Contrast:", 0.0, 500.0, 1.0, 100.0, " %")];
const RED_EYE: [Setting; 1] = [slider("Threshold:", 0.0, 100.0, 1.0, 50.0, " %")];
const REDUCE_NOISE: [Setting; 2] = [slider("Radius:", 1.0, 20.0, 1.0, 3.0, " px"), slider("Strength:", 0.0, 100.0, 1.0, 40.0, " %")];
const WAVELET: [Setting; 1] = [slider("Scales:", 1.0, 7.0, 1.0, 5.0, "")];
const SHARPEN: [Setting; 3] = [
    slider("Radius:", 1.0, 50.0, 1.0, 3.0, " px"),
    slider("Amount:", 0.0, 300.0, 1.0, 50.0, " %"),
    slider("Threshold:", 0.0, 100.0, 1.0, 0.0, " %"),
];

impl EffectKind {
    /// The menus of the Filters menu, each with its effects, then the niche ones it keeps
    /// in a More menu at its end.
    pub const MENUS: [(&str, &[Self], &[Self]); 5] = [
        (
            "Artistic",
            &[ApplyCanvas, Cartoon, GlassTile, Glow, InkSketch, OilPainting, PencilSketch, Photocopy, VanGogh],
            &[Clothify, Cubism, Predator, Slic, Waterpixels, Weave],
        ),
        (
            "Blurs",
            &[
                BokehBlur,
                FocusBlur,
                GaussianBlur,
                MedianBlur,
                MotionBlur,
                RadialBlur,
                SquareBlur,
                SurfaceBlur,
                TileableBlur,
                VariableBlur,
                ZoomBlur,
            ],
            &[FragmentBlur, MeanCurvatureBlur, SketchBlur],
        ),
        ("Enhance", &[Antialias, HighPass, RedEyeRemoval, ReduceNoise], &[Deinterlace, WaveletDecompose]),
        ("Map", &[Offset], &[]),
        ("Photo", &[Sharpen], &[]),
    ];

    /// Its name, which its undo step is called as well.
    pub fn name(self) -> &'static str {
        match self {
            ApplyCanvas => "Apply Canvas",
            Cartoon => "Cartoon",
            Clothify => "Clothify",
            Cubism => "Cubism",
            GlassTile => "Glass Tile",
            Glow => "Glow",
            InkSketch => "Ink Sketch",
            OilPainting => "Oil Painting",
            PencilSketch => "Pencil Sketch",
            Photocopy => "Photocopy",
            Predator => "Predator",
            Slic => "Simple Linear Iterative Clustering",
            VanGogh => "Van Gogh (LIC)",
            Waterpixels => "Waterpixels",
            Weave => "Weave",
            BokehBlur => "Bokeh Blur",
            FocusBlur => "Focus Blur",
            FragmentBlur => "Fragment Blur",
            GaussianBlur => "Gaussian Blur",
            MeanCurvatureBlur => "Mean Curvature Blur",
            MedianBlur => "Median Blur",
            MotionBlur => "Motion Blur",
            RadialBlur => "Radial Blur",
            SketchBlur => "Sketch Blur",
            SquareBlur => "Square Blur",
            SurfaceBlur => "Surface Blur",
            TileableBlur => "Tileable Blur",
            VariableBlur => "Variable Blur",
            ZoomBlur => "Zoom Blur",
            Antialias => "Antialias",
            Deinterlace => "Deinterlace",
            HighPass => "High Pass",
            RedEyeRemoval => "Red Eye Removal",
            ReduceNoise => "Reduce Noise",
            WaveletDecompose => "Wavelet-decompose",
            Sharpen => "Sharpen",
            Offset => "Offset",
            RotateZoom => "Rotate / Zoom",
            Transform3d => "3D Transform",
        }
    }

    /// What its dialog asks for.
    pub fn settings(self) -> &'static [Setting] {
        match self {
            ApplyCanvas => &CANVAS,
            Cartoon => &CARTOON,
            Clothify => &CLOTHIFY,
            Cubism => &CUBISM,
            GlassTile => &GLASS_TILE,
            Glow => &GLOW,
            InkSketch => &INK_SKETCH,
            OilPainting => &OIL_PAINTING,
            PencilSketch => &PENCIL_SKETCH,
            Photocopy => &PHOTOCOPY,
            Predator => &PREDATOR,
            Slic => &SLIC,
            VanGogh => &VAN_GOGH,
            Waterpixels => &WATERPIXELS,
            Weave => &WEAVE,
            BokehBlur => &BOKEH,
            FocusBlur => &FOCUS,
            FragmentBlur => &FRAGMENT,
            GaussianBlur => &GAUSSIAN,
            MeanCurvatureBlur => &MEAN_CURVATURE,
            MedianBlur => &MEDIAN,
            MotionBlur => &MOTION,
            RadialBlur => &RADIAL,
            SketchBlur => &SKETCH,
            SquareBlur => &SQUARE,
            SurfaceBlur => &SURFACE,
            TileableBlur => &TILEABLE,
            VariableBlur => &VARIABLE,
            ZoomBlur => &ZOOM,
            Antialias => &[],
            Deinterlace => &DEINTERLACE,
            HighPass => &HIGH_PASS,
            RedEyeRemoval => &RED_EYE,
            ReduceNoise => &REDUCE_NOISE,
            WaveletDecompose => &WAVELET,
            Sharpen => &SHARPEN,
            Offset => &OFFSET,
            RotateZoom => &ROTATE_ZOOM,
            Transform3d => &TRANSFORM_3D,
        }
    }

    /// Whether it splits the layer into new ones rather than changing its pixels, so
    /// has no [`EffectKind::effect`].
    pub fn makes_layers(self) -> bool {
        self == WaveletDecompose
    }

    /// The effect with these settings, in the order [`EffectKind::settings`] lists them,
    /// on an image with these layers.
    pub fn effect(self, values: &[f64], layers: &[Layer]) -> Effect {
        let value = |index: usize| values[index] as f32;
        let pick = |index: usize, count: usize| (values[index] as usize).min(count - 1);
        match self {
            ApplyCanvas => Effect::ApplyCanvas { azimuth: CANVAS_LIGHTS[pick(0, CANVAS_LIGHTS.len())], depth: value(1) },
            Cartoon => Effect::Cartoon { radius: value(0), black: value(1) },
            Clothify => Effect::Clothify { blur: [value(0), value(1)], azimuth: value(2), elevation: value(3), depth: value(4) },
            Cubism => Effect::Cubism {
                size: value(0),
                saturation: value(1),
                background: CUBISM_BACKGROUNDS[pick(2, CUBISM_BACKGROUNDS.len())],
            },
            GlassTile => Effect::GlassTile { width: value(0), height: value(1) },
            Glow => Effect::Glow { radius: value(0), brightness: value(1), contrast: value(2) },
            InkSketch => Effect::InkSketch { ink: value(0), coloring: value(1) },
            OilPainting => Effect::OilPainting { radius: value(0), levels: value(1) as u32 },
            PencilSketch => Effect::PencilSketch { tip: value(0), range: value(1) },
            Photocopy => Effect::Photocopy { radius: value(0), sharpness: value(1), black: value(2), white: value(3) },
            Predator => Effect::Predator { amount: value(0), pixel_size: if values[1] != 0.0 { value(2) as u32 } else { 1 } },
            Slic => Effect::Slic { size: value(0) as u32, compactness: value(1), iterations: value(2) as u32 },
            VanGogh => Effect::VanGogh { length: value(0), strength: value(1) },
            Waterpixels => Effect::Waterpixels {
                size: value(0) as u32,
                smoothness: value(1),
                regularization: value(2),
                random: pick(3, WATERPIXELS_FILLS.len()) == 1,
            },
            Weave => Effect::Weave { width: value(0), spacing: value(1), darkness: value(2), depth: value(3), threads: value(4) },
            BokehBlur => Effect::BokehBlur { radius: value(0), highlights: value(1) },
            FocusBlur => Effect::FocusBlur {
                radius: value(0),
                shape: FocusShape::ALL[(values[1] as usize).min(FocusShape::ALL.len() - 1)],
                center: [value(2), value(3)],
                size: value(4),
                transition: value(5),
            },
            FragmentBlur => Effect::FragmentBlur { count: value(0) as u32, distance: value(1), rotation: value(2) },
            GaussianBlur => Effect::GaussianBlur { radius: value(0) },
            MeanCurvatureBlur => Effect::MeanCurvatureBlur { iterations: value(0) as u32 },
            MedianBlur => Effect::MedianBlur { radius: value(0), percentile: value(1) },
            MotionBlur => Effect::MotionBlur { angle: value(0), distance: value(1), centered: values[2] != 0.0 },
            RadialBlur => Effect::RadialBlur { angle: value(0), center: [value(1), value(2)] },
            SketchBlur => Effect::SketchBlur { radius: value(0) },
            SquareBlur => Effect::SquareBlur { radius: value(0), highlights: value(1) },
            SurfaceBlur => Effect::SurfaceBlur { radius: value(0), threshold: value(1) },
            TileableBlur => Effect::TileableBlur { radius: value(0) },
            VariableBlur => Effect::VariableBlur {
                radius: value(0),
                map: Arc::new(layers[(values[1] as usize).min(layers.len() - 1)].pixels.clone()),
                invert: values[2] != 0.0,
            },
            ZoomBlur => Effect::ZoomBlur { amount: value(0), center: [value(1), value(2)] },
            Antialias => Effect::Antialias,
            Deinterlace => Effect::Deinterlace {
                keep_odd: pick(0, DEINTERLACE_FIELDS.len()) == 1,
                vertical: pick(1, DEINTERLACE_ORIENTATIONS.len()) == 1,
                size: value(2) as u32,
            },
            HighPass => Effect::HighPass { radius: value(0), contrast: value(1) },
            RedEyeRemoval => Effect::RedEyeRemoval { threshold: value(0) },
            ReduceNoise => Effect::ReduceNoise { radius: value(0), strength: value(1) },
            WaveletDecompose => unreachable!("makes layers"),
            Sharpen => Effect::Sharpen { radius: value(0), amount: value(1), threshold: value(2) },
            Offset => Effect::Offset {
                x: values[0] as i32,
                y: values[1] as i32,
                edges: OffsetEdges::ALL[(values[2] as usize).min(OffsetEdges::ALL.len() - 1)],
            },
            RotateZoom => Effect::Turn {
                turn: Turn { angles: [0.0, 0.0, values[0]], zoom: values[1], pan: [values[2], values[3]], ..Turn::NONE },
                interpolation: Interpolation::ALL[pick(4, Interpolation::ALL.len())],
            },
            Transform3d => Effect::Turn {
                turn: Turn {
                    angles: [values[0], values[1], values[2]],
                    field_of_view: values[3],
                    zoom: values[4],
                    pan: [values[5], values[6]],
                },
                interpolation: Interpolation::ALL[pick(7, Interpolation::ALL.len())],
            },
        }
    }
}

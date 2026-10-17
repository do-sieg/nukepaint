//! What the Filters menu does: each effect, its name and the settings its dialog asks for.

use std::sync::Arc;

use power_paint_core::{
    BlendMode, CellStyle, EdgeAlgorithm, Effect, FocusShape, FractalKind, Interpolation, Layer, LightSource, NewsprintScreen, Noise, NoiseColorSpace, NoiseKind, OffsetEdges, Turn,
    WaveShape, WindDirection,
};

use crate::adjustment_dialog::{PRIMARY, SECONDARY, Setting, check, choice, color, layer, pack_color, slider, unpack_color};

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
    Bulge,
    Crystallize,
    Kaleidoscope,
    LensDistortion,
    Newsprint,
    Pixelate,
    PolarCoordinates,
    Ripple,
    TileReflection,
    Twist,
    Waves,
    Wind,
    Antialias,
    Deinterlace,
    HighPass,
    RedEyeRemoval,
    ReduceNoise,
    WaveletDecompose,
    Sharpen,
    SoftenPortrait,
    Straighten,
    AddBevel,
    AddBorder,
    RoundCorners,
    DropShadow,
    LongShadow,
    Vignette,
    LensFlare,
    LightingEffects,
    Sparkle,
    CloudsAndNoise,
    FractalExplorer,
    BayerMatrix,
    Checkerboard,
    Grid,
    Maze,
    SpeedLines,
    EdgeDetect,
    Emboss,
    Outline,
    Relief,
    AddNoise,
    Spread,
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
const BULGE: [Setting; 4] = [
    slider("Amount:", -100.0, 100.0, 1.0, 45.0, " %"),
    slider("Radius:", 1.0, 200.0, 1.0, 100.0, " %"),
    slider("Center X:", -100.0, 100.0, 1.0, 0.0, " %"),
    slider("Center Y:", -100.0, 100.0, 1.0, 0.0, " %"),
];
const CRYSTALLIZE: [Setting; 1] = [slider("Cell Size:", 2.0, 250.0, 1.0, 16.0, " px")];
const KALEIDOSCOPE: [Setting; 4] = [
    slider("Segments:", 1.0, 32.0, 1.0, 6.0, ""),
    slider("Angle:", 0.0, 360.0, 1.0, 0.0, "°"),
    slider("Center X:", -100.0, 100.0, 1.0, 0.0, " %"),
    slider("Center Y:", -100.0, 100.0, 1.0, 0.0, " %"),
];
const LENS: [Setting; 5] = [
    slider("Main:", -100.0, 100.0, 1.0, 25.0, ""),
    slider("Edge:", -100.0, 100.0, 1.0, 0.0, ""),
    slider("Zoom:", -100.0, 100.0, 1.0, 0.0, ""),
    slider("Center X:", -100.0, 100.0, 1.0, 0.0, " %"),
    slider("Center Y:", -100.0, 100.0, 1.0, 0.0, " %"),
];
/// The names of [`NewsprintScreen::ALL`].
const NEWSPRINT_SCREENS: [&str; 3] = ["Black", "RGB", "CMYK"];
const NEWSPRINT: [Setting; 3] = [
    slider("Cell Size:", 2.0, 100.0, 1.0, 8.0, " px"),
    slider("Angle:", -90.0, 90.0, 1.0, 0.0, "°"),
    choice("Screen:", &NEWSPRINT_SCREENS, 0),
];
const PIXELATE: [Setting; 2] = [slider("Cell Width:", 1.0, 250.0, 1.0, 8.0, " px"), slider("Cell Height:", 1.0, 250.0, 1.0, 8.0, " px")];
const POLAR_MODES: [&str; 2] = ["Rectangular to Polar", "Polar to Rectangular"];
const POLAR: [Setting; 2] = [choice("Mode:", &POLAR_MODES, 0), slider("Rotation:", -180.0, 180.0, 1.0, 0.0, "°")];
const ORIENTATIONS: [&str; 2] = ["Horizontal", "Vertical"];
/// The names of [`WaveShape::ALL`].
const WAVE_SHAPES: [&str; 3] = ["Sine", "Triangle", "Sawtooth"];
const RIPPLE: [Setting; 5] = [
    slider("Amplitude:", 0.0, 200.0, 1.0, 5.0, " px"),
    slider("Wavelength:", 1.0, 500.0, 1.0, 30.0, " px"),
    slider("Phase:", 0.0, 360.0, 1.0, 0.0, "°"),
    choice("Orientation:", &ORIENTATIONS, 0),
    choice("Shape:", &WAVE_SHAPES, 0),
];
const TILE_REFLECTION: [Setting; 3] = [
    slider("Tile Size:", 2.0, 200.0, 1.0, 40.0, " px"),
    slider("Angle:", -180.0, 180.0, 1.0, 30.0, "°"),
    slider("Curvature:", -100.0, 100.0, 1.0, 8.0, ""),
];
const TWIST: [Setting; 5] = [
    slider("Angle:", -720.0, 720.0, 1.0, 90.0, "°"),
    slider("Pinch:", -100.0, 100.0, 1.0, 0.0, " %"),
    slider("Radius:", 1.0, 200.0, 1.0, 100.0, " %"),
    slider("Center X:", -100.0, 100.0, 1.0, 0.0, " %"),
    slider("Center Y:", -100.0, 100.0, 1.0, 0.0, " %"),
];
const WAVES: [Setting; 5] = [
    slider("Amplitude:", 0.0, 100.0, 1.0, 5.0, " px"),
    slider("Wavelength:", 1.0, 500.0, 1.0, 30.0, " px"),
    slider("Phase:", 0.0, 360.0, 1.0, 0.0, "°"),
    slider("Center X:", -100.0, 100.0, 1.0, 0.0, " %"),
    slider("Center Y:", -100.0, 100.0, 1.0, 0.0, " %"),
];
/// The names of [`WindDirection::ALL`].
const WIND_DIRECTIONS: [&str; 4] = ["Right", "Left", "Up", "Down"];
const WIND_STYLES: [&str; 2] = ["Wind", "Blast"];
const WIND: [Setting; 4] = [
    slider("Strength:", 1.0, 200.0, 1.0, 20.0, " px"),
    slider("Threshold:", 0.0, 100.0, 1.0, 10.0, " %"),
    choice("Direction:", &WIND_DIRECTIONS, 0),
    choice("Style:", &WIND_STYLES, 0),
];
const DEINTERLACE_FIELDS: [&str; 2] = ["Even", "Odd"];
const DEINTERLACE: [Setting; 3] = [
    choice("Keep:", &DEINTERLACE_FIELDS, 0),
    choice("Orientation:", &ORIENTATIONS, 0),
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
const SOFTEN_PORTRAIT: [Setting; 3] = [
    slider("Softness:", 0.0, 50.0, 1.0, 5.0, " px"),
    slider("Lighting:", -20.0, 20.0, 1.0, 0.0, ""),
    slider("Warmth:", 0.0, 20.0, 1.0, 10.0, ""),
];
const STRAIGHTEN: [Setting; 3] = [
    slider("Angle:", -45.0, 45.0, 0.1, 0.0, "°"),
    check("Zoom to Fill", true),
    choice("Resampling:", &RESAMPLINGS, 2),
];
const BEVEL: [Setting; 3] = [
    slider("Width:", 1.0, 200.0, 1.0, 10.0, " px"),
    slider("Depth:", 0.0, 100.0, 1.0, 50.0, " %"),
    slider("Azimuth:", 0.0, 360.0, 1.0, 135.0, "°"),
];
const BORDER: [Setting; 3] =
    [slider("Size:", 1.0, 500.0, 1.0, 16.0, " px"), color("Color:", PRIMARY), slider("Shading:", 0.0, 100.0, 1.0, 25.0, " %")];
const ROUND_CORNERS: [Setting; 1] = [slider("Radius:", 1.0, 1000.0, 1.0, 16.0, " px").logarithmic()];
const BLACK: f64 = pack_color([0, 0, 0, 255]);
const WHITE: f64 = pack_color([255, 255, 255, 255]);
const DROP_SHADOW: [Setting; 5] = [
    slider("Angle:", 0.0, 360.0, 1.0, 315.0, "°"),
    slider("Distance:", 0.0, 200.0, 1.0, 8.0, " px"),
    slider("Blur:", 0.0, 100.0, 1.0, 8.0, " px"),
    slider("Opacity:", 0.0, 100.0, 1.0, 60.0, " %"),
    color("Color:", BLACK),
];
const LONG_SHADOW: [Setting; 5] = [
    slider("Angle:", 0.0, 360.0, 1.0, 315.0, "°"),
    slider("Length:", 1.0, 2000.0, 1.0, 100.0, " px").logarithmic(),
    slider("Opacity:", 0.0, 100.0, 1.0, 40.0, " %"),
    check("Fade Out", true),
    color("Color:", BLACK),
];
const VIGNETTE: [Setting; 6] = [
    slider("Amount:", 0.0, 100.0, 1.0, 60.0, " %"),
    slider("Radius:", 1.0, 200.0, 1.0, 100.0, " %"),
    slider("Softness:", 0.0, 100.0, 1.0, 60.0, " %"),
    slider("Center X:", -100.0, 100.0, 1.0, 0.0, " %"),
    slider("Center Y:", -100.0, 100.0, 1.0, 0.0, " %"),
    color("Color:", BLACK),
];
const LENS_FLARE: [Setting; 6] = [
    slider("Center X:", -100.0, 100.0, 1.0, -50.0, " %"),
    slider("Center Y:", -100.0, 100.0, 1.0, -50.0, " %"),
    slider("Size:", 2.0, 1000.0, 1.0, 60.0, " px").logarithmic(),
    slider("Brightness:", 0.0, 200.0, 1.0, 100.0, " %"),
    slider("Rays:", 0.0, 64.0, 1.0, 0.0, ""),
    check("Ghosts", true),
];
/// The names of [`LightSource::ALL`].
const LIGHT_SOURCES: [&str; 2] = ["Spotlight", "Directional"];
const LIGHTING: [Setting; 11] = [
    choice("Light:", &LIGHT_SOURCES, 0),
    slider("Center X:", -100.0, 100.0, 1.0, -30.0, " %").enabled_when(0, 0.0),
    slider("Center Y:", -100.0, 100.0, 1.0, -30.0, " %").enabled_when(0, 0.0),
    slider("Height:", 1.0, 200.0, 1.0, 50.0, " %").enabled_when(0, 0.0),
    slider("Azimuth:", 0.0, 360.0, 1.0, 135.0, "°").enabled_when(0, 1.0),
    slider("Elevation:", 0.0, 90.0, 1.0, 45.0, "°").enabled_when(0, 1.0),
    slider("Intensity:", 0.0, 300.0, 1.0, 100.0, " %"),
    slider("Ambient:", 0.0, 100.0, 1.0, 20.0, " %"),
    slider("Shine:", 0.0, 100.0, 1.0, 20.0, " %"),
    slider("Bumps:", 0.0, 50.0, 1.0, 0.0, ""),
    color("Color:", WHITE),
];
const SPARKLE: [Setting; 6] = [
    slider("Threshold:", 0.0, 100.0, 1.0, 80.0, " %"),
    slider("Size:", 2.0, 200.0, 1.0, 15.0, " px"),
    slider("Points:", 2.0, 16.0, 1.0, 4.0, ""),
    slider("Angle:", 0.0, 180.0, 1.0, 0.0, "°"),
    slider("Amount:", 0.0, 100.0, 1.0, 50.0, " %"),
    SEED,
];
/// The names of [`NoiseKind::ALL`].
const NOISE_KINDS: [&str; 6] = ["Clouds", "Perlin", "Simplex", "Turbulence", "Cell Noise", "Plasma"];
/// The names of [`CellStyle::ALL`].
const CELL_STYLES: [&str; 3] = ["Distance", "Edges", "Flat"];
/// The names of [`BlendMode::ALL`].
const BLEND_MODES: [&str; 15] = [
    "Normal",
    "Multiply",
    "Additive",
    "Color Burn",
    "Color Dodge",
    "Reflect",
    "Glow",
    "Overlay",
    "Difference",
    "Negation",
    "Lighten",
    "Darken",
    "Screen",
    "Xor",
    "Grain Merge",
];
const SEED: Setting = slider("Seed:", 0.0, 9999.0, 1.0, 0.0, "");
const CLOUDS: [Setting; 11] = [
    choice("Type:", &NOISE_KINDS, 0),
    slider("Scale:", 2.0, 1000.0, 1.0, 250.0, " px").logarithmic(),
    slider("Detail:", 1.0, 10.0, 1.0, 6.0, ""),
    slider("Roughness:", 0.0, 100.0, 1.0, 50.0, " %"),
    SEED,
    color("Color 1:", PRIMARY),
    color("Color 2:", SECONDARY),
    choice("Blend Mode:", &BLEND_MODES, 0),
    check("Plasma Colors", true).enabled_when(0, 5.0),
    choice("Cells:", &CELL_STYLES, 0).enabled_when(0, 4.0),
    slider("Jitter:", 0.0, 100.0, 1.0, 100.0, " %").enabled_when(0, 4.0),
];
const FRACTAL_KINDS: [&str; 2] = ["Mandelbrot", "Julia"];
const FRACTAL_COLORINGS: [&str; 2] = ["Gradient", "Rainbow"];
const FRACTAL: [Setting; 11] = [
    choice("Type:", &FRACTAL_KINDS, 0),
    slider("Iterations:", 10.0, 5000.0, 1.0, 200.0, "").logarithmic(),
    slider("Zoom:", 10.0, 10_000_000.0, 1.0, 100.0, " %").logarithmic(),
    slider("Center X:", -2.0, 2.0, 0.0001, 0.0, ""),
    slider("Center Y:", -2.0, 2.0, 0.0001, 0.0, ""),
    slider("Julia Real:", -2.0, 2.0, 0.001, -0.8, "").enabled_when(0, 1.0),
    slider("Julia Imaginary:", -2.0, 2.0, 0.001, 0.156, "").enabled_when(0, 1.0),
    choice("Coloring:", &FRACTAL_COLORINGS, 0),
    color("Set Color:", PRIMARY),
    color("Outside Color:", SECONDARY).enabled_when(7, 0.0),
    check("Antialias", true),
];
/// The widths of the Bayer matrices, as powers of 2 from 1.
const BAYER_SIZES: [&str; 8] = ["2 × 2", "4 × 4", "8 × 8", "16 × 16", "32 × 32", "64 × 64", "128 × 128", "256 × 256"];
const BAYER: [Setting; 4] = [
    choice("Matrix:", &BAYER_SIZES, 2),
    slider("Pixel Size:", 1.0, 64.0, 1.0, 1.0, " px"),
    color("Color 1:", PRIMARY),
    color("Color 2:", SECONDARY),
];
const CHECKERBOARD: [Setting; 3] =
    [slider("Size:", 1.0, 500.0, 1.0, 16.0, " px"), color("Color 1:", PRIMARY), color("Color 2:", SECONDARY)];
const GRID: [Setting; 6] = [
    slider("Cell Width:", 2.0, 500.0, 1.0, 32.0, " px"),
    slider("Cell Height:", 2.0, 500.0, 1.0, 32.0, " px"),
    slider("Line Width:", 1.0, 50.0, 1.0, 1.0, " px"),
    slider("Offset X:", 0.0, 500.0, 1.0, 0.0, " px"),
    slider("Offset Y:", 0.0, 500.0, 1.0, 0.0, " px"),
    color("Color:", PRIMARY),
];
const MAZE_STYLES: [&str; 2] = ["Winding", "Branching"];
const MAZE: [Setting; 5] = [
    slider("Corridor Width:", 1.0, 100.0, 1.0, 8.0, " px"),
    choice("Style:", &MAZE_STYLES, 0),
    SEED,
    color("Wall Color:", PRIMARY),
    color("Corridor Color:", SECONDARY),
];
const SPEED_LINES: [Setting; 8] = [
    slider("Lines:", 3.0, 1000.0, 1.0, 120.0, "").logarithmic(),
    slider("Thickness:", 1.0, 100.0, 1.0, 40.0, " %"),
    slider("Clear Area:", 0.0, 100.0, 1.0, 35.0, " %"),
    slider("Randomness:", 0.0, 100.0, 1.0, 60.0, " %"),
    slider("Center X:", -100.0, 100.0, 1.0, 0.0, " %"),
    slider("Center Y:", -100.0, 100.0, 1.0, 0.0, " %"),
    SEED,
    color("Color:", PRIMARY),
];
/// The names of [`EdgeAlgorithm::ALL`].
const EDGE_ALGORITHMS: [&str; 3] = ["Sobel", "Laplace", "Difference of Gaussians"];
const EDGE_DETECT: [Setting; 8] = [
    choice("Algorithm:", &EDGE_ALGORITHMS, 0),
    check("Horizontal", true).enabled_when(0, 0.0),
    check("Vertical", true).enabled_when(0, 0.0),
    slider("Radius 1:", 0.0, 50.0, 1.0, 1.0, " px").enabled_when(0, 2.0),
    slider("Radius 2:", 0.0, 50.0, 1.0, 3.0, " px").enabled_when(0, 2.0),
    check("Normalize", true),
    slider("Amount:", 0.0, 1000.0, 1.0, 100.0, " %").enabled_when(5, 0.0),
    check("Invert", false),
];
/// The settings of the Emboss and the Relief.
const RELIEF: [Setting; 3] = [
    slider("Azimuth:", 0.0, 360.0, 1.0, 135.0, "°"),
    slider("Elevation:", 0.0, 90.0, 1.0, 45.0, "°"),
    slider("Depth:", 1.0, 50.0, 1.0, 3.0, ""),
];
const OUTLINE: [Setting; 2] = [slider("Thickness:", 1.0, 50.0, 1.0, 3.0, " px"), slider("Intensity:", 0.0, 100.0, 1.0, 50.0, " %")];
/// The names of [`NoiseColorSpace::ALL`].
const NOISE_COLOR_SPACES: [&str; 3] = ["RGB", "HSV", "CIE LCh"];
const ADD_NOISE: [Setting; 5] = [
    slider("Intensity:", 0.0, 100.0, 1.0, 64.0, " %"),
    slider("Color Saturation:", 0.0, 100.0, 1.0, 50.0, " %"),
    slider("Coverage:", 0.0, 100.0, 1.0, 100.0, " %"),
    choice("Color Space:", &NOISE_COLOR_SPACES, 0),
    SEED,
];
const SPREAD: [Setting; 3] = [
    slider("Amount:", 1.0, 200.0, 1.0, 5.0, " px").logarithmic(),
    slider("Downward Bias:", 0.0, 100.0, 1.0, 0.0, " %"),
    SEED,
];

impl EffectKind {
    /// The menus of the Filters menu, each with its effects, then the niche ones it keeps
    /// in a More menu at its end.
    pub const MENUS: [(&str, &[Self], &[Self]); 11] = [
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
        ("Decor", &[AddBevel, AddBorder, RoundCorners], &[]),
        (
            "Distort",
            &[Bulge, Crystallize, LensDistortion, Pixelate, PolarCoordinates, Ripple, Twist, Waves, Wind],
            &[Kaleidoscope, Newsprint, TileReflection],
        ),
        ("Enhance", &[Antialias, HighPass, RedEyeRemoval, ReduceNoise], &[Deinterlace, WaveletDecompose]),
        ("Light", &[DropShadow, LensFlare, LightingEffects, LongShadow, Sparkle, Vignette], &[]),
        ("Map", &[Offset], &[]),
        ("Noise", &[AddNoise, Spread], &[]),
        ("Photo", &[Sharpen, SoftenPortrait, Straighten], &[]),
        ("Render", &[CloudsAndNoise, FractalExplorer, Checkerboard, Grid, Maze, SpeedLines], &[BayerMatrix]),
        ("Stylize", &[EdgeDetect, Emboss, Outline, Relief], &[]),
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
            Bulge => "Bulge",
            Crystallize => "Crystallize",
            Kaleidoscope => "Kaleidoscope",
            LensDistortion => "Lens Distortion",
            Newsprint => "Newsprint",
            Pixelate => "Pixelate",
            PolarCoordinates => "Polar Coordinates",
            Ripple => "Ripple",
            TileReflection => "Tile Reflection",
            Twist => "Twist",
            Waves => "Waves",
            Wind => "Wind",
            Antialias => "Antialias",
            Deinterlace => "Deinterlace",
            HighPass => "High Pass",
            RedEyeRemoval => "Red Eye Removal",
            ReduceNoise => "Reduce Noise",
            WaveletDecompose => "Wavelet-decompose",
            Sharpen => "Sharpen",
            SoftenPortrait => "Soften Portrait",
            Straighten => "Straighten",
            AddBevel => "Add Bevel",
            AddBorder => "Add Border",
            RoundCorners => "Round Corners",
            DropShadow => "Drop Shadow",
            LongShadow => "Long Shadow",
            Vignette => "Vignette",
            LensFlare => "Lens Flare",
            LightingEffects => "Lighting Effects",
            Sparkle => "Sparkle",
            CloudsAndNoise => "Clouds & Noise",
            FractalExplorer => "Fractal Explorer",
            BayerMatrix => "Bayer Matrix",
            Checkerboard => "Checkerboard",
            Grid => "Grid",
            Maze => "Maze",
            SpeedLines => "Speed Lines",
            EdgeDetect => "Edge Detect",
            Emboss => "Emboss",
            Outline => "Outline",
            Relief => "Relief",
            AddNoise => "Add Noise",
            Spread => "Spread",
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
            Bulge => &BULGE,
            Crystallize => &CRYSTALLIZE,
            Kaleidoscope => &KALEIDOSCOPE,
            LensDistortion => &LENS,
            Newsprint => &NEWSPRINT,
            Pixelate => &PIXELATE,
            PolarCoordinates => &POLAR,
            Ripple => &RIPPLE,
            TileReflection => &TILE_REFLECTION,
            Twist => &TWIST,
            Waves => &WAVES,
            Wind => &WIND,
            Antialias => &[],
            Deinterlace => &DEINTERLACE,
            HighPass => &HIGH_PASS,
            RedEyeRemoval => &RED_EYE,
            ReduceNoise => &REDUCE_NOISE,
            WaveletDecompose => &WAVELET,
            Sharpen => &SHARPEN,
            SoftenPortrait => &SOFTEN_PORTRAIT,
            Straighten => &STRAIGHTEN,
            AddBevel => &BEVEL,
            AddBorder => &BORDER,
            RoundCorners => &ROUND_CORNERS,
            DropShadow => &DROP_SHADOW,
            LongShadow => &LONG_SHADOW,
            Vignette => &VIGNETTE,
            LensFlare => &LENS_FLARE,
            LightingEffects => &LIGHTING,
            Sparkle => &SPARKLE,
            CloudsAndNoise => &CLOUDS,
            FractalExplorer => &FRACTAL,
            BayerMatrix => &BAYER,
            Checkerboard => &CHECKERBOARD,
            Grid => &GRID,
            Maze => &MAZE,
            SpeedLines => &SPEED_LINES,
            EdgeDetect => &EDGE_DETECT,
            Emboss | Relief => &RELIEF,
            Outline => &OUTLINE,
            AddNoise => &ADD_NOISE,
            Spread => &SPREAD,
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
        let colors = |index: usize| [unpack_color(values[index]), unpack_color(values[index + 1])];
        let noise = |kind, first: usize| Noise {
            kind,
            scale: value(first),
            detail: value(first + 1) as u32,
            roughness: value(first + 2),
            seed: value(first + 3) as u32,
            cells: CellStyle::Distance,
            jitter: 100.0,
        };
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
            Bulge => Effect::Bulge { amount: value(0), radius: value(1), center: [value(2), value(3)] },
            Crystallize => Effect::Crystallize { size: value(0) },
            Kaleidoscope => Effect::Kaleidoscope { segments: value(0) as u32, angle: value(1), center: [value(2), value(3)] },
            LensDistortion => Effect::LensDistortion { main: value(0), edge: value(1), zoom: value(2), center: [value(3), value(4)] },
            Newsprint => Effect::Newsprint {
                size: value(0),
                angle: value(1),
                screen: NewsprintScreen::ALL[pick(2, NewsprintScreen::ALL.len())],
            },
            Pixelate => Effect::Pixelate { width: value(0) as u32, height: value(1) as u32 },
            PolarCoordinates => Effect::PolarCoordinates { to_polar: pick(0, POLAR_MODES.len()) == 0, rotation: value(1) },
            Ripple => Effect::Ripple {
                amplitude: value(0),
                wavelength: value(1),
                phase: value(2),
                vertical: pick(3, ORIENTATIONS.len()) == 1,
                shape: WaveShape::ALL[pick(4, WaveShape::ALL.len())],
            },
            TileReflection => Effect::TileReflection { size: value(0), angle: value(1), curvature: value(2) },
            Twist => Effect::Twist { angle: value(0), pinch: value(1), radius: value(2), center: [value(3), value(4)] },
            Waves => Effect::Waves { amplitude: value(0), wavelength: value(1), phase: value(2), center: [value(3), value(4)] },
            Wind => Effect::Wind {
                strength: value(0),
                threshold: value(1),
                direction: WindDirection::ALL[pick(2, WindDirection::ALL.len())],
                blast: pick(3, WIND_STYLES.len()) == 1,
            },
            Antialias => Effect::Antialias,
            Deinterlace => Effect::Deinterlace {
                keep_odd: pick(0, DEINTERLACE_FIELDS.len()) == 1,
                vertical: pick(1, ORIENTATIONS.len()) == 1,
                size: value(2) as u32,
            },
            HighPass => Effect::HighPass { radius: value(0), contrast: value(1) },
            RedEyeRemoval => Effect::RedEyeRemoval { threshold: value(0) },
            ReduceNoise => Effect::ReduceNoise { radius: value(0), strength: value(1) },
            WaveletDecompose => unreachable!("makes layers"),
            Sharpen => Effect::Sharpen { radius: value(0), amount: value(1), threshold: value(2) },
            SoftenPortrait => Effect::SoftenPortrait { softness: value(0), lighting: value(1), warmth: value(2) },
            Straighten => Effect::Straighten {
                angle: value(0),
                fill: values[1] != 0.0,
                interpolation: Interpolation::ALL[pick(2, Interpolation::ALL.len())],
            },
            AddBevel => Effect::AddBevel { width: value(0), depth: value(1), azimuth: value(2) },
            AddBorder => Effect::AddBorder { size: value(0), color: unpack_color(values[1]), shading: value(2) },
            RoundCorners => Effect::RoundCorners { radius: value(0) },
            DropShadow => Effect::DropShadow {
                angle: value(0),
                distance: value(1),
                blur: value(2),
                opacity: value(3),
                color: unpack_color(values[4]),
            },
            LongShadow => Effect::LongShadow {
                angle: value(0),
                length: value(1),
                opacity: value(2),
                fade: values[3] != 0.0,
                color: unpack_color(values[4]),
            },
            Vignette => Effect::Vignette {
                amount: value(0),
                radius: value(1),
                softness: value(2),
                center: [value(3), value(4)],
                color: unpack_color(values[5]),
            },
            LensFlare => Effect::LensFlare {
                center: [value(0), value(1)],
                size: value(2),
                brightness: value(3),
                rays: value(4) as u32,
                ghosts: values[5] != 0.0,
            },
            LightingEffects => Effect::LightingEffects {
                light: LightSource::ALL[pick(0, LightSource::ALL.len())],
                center: [value(1), value(2)],
                height: value(3),
                azimuth: value(4),
                elevation: value(5),
                intensity: value(6),
                ambient: value(7),
                shine: value(8),
                bumps: value(9),
                color: unpack_color(values[10]),
            },
            Sparkle => Effect::Sparkle {
                threshold: value(0),
                size: value(1),
                points: value(2) as u32,
                angle: value(3),
                amount: value(4),
                seed: value(5) as u32,
            },
            CloudsAndNoise => Effect::CloudsAndNoise {
                noise: Noise {
                    cells: CellStyle::ALL[pick(9, CellStyle::ALL.len())],
                    jitter: value(10),
                    ..noise(NoiseKind::ALL[pick(0, NoiseKind::ALL.len())], 1)
                },
                colors: colors(5),
                blend_mode: BlendMode::ALL[pick(7, BlendMode::ALL.len())],
                plasma_colors: values[8] != 0.0,
            },
            FractalExplorer => Effect::Fractal {
                kind: FractalKind::ALL[pick(0, FractalKind::ALL.len())],
                iterations: value(1) as u32,
                zoom: values[2],
                center: [values[3], values[4]],
                julia: [values[5], values[6]],
                rainbow: pick(7, FRACTAL_COLORINGS.len()) == 1,
                colors: colors(8),
                antialias: values[10] != 0.0,
            },
            BayerMatrix => {
                Effect::BayerMatrix { level: pick(0, BAYER_SIZES.len()) as u32 + 1, pixel_size: value(1) as u32, colors: colors(2) }
            }
            Checkerboard => Effect::Checkerboard { size: value(0) as u32, colors: colors(1) },
            Grid => Effect::Grid {
                width: value(0) as u32,
                height: value(1) as u32,
                line: value(2) as u32,
                offset: [values[3] as i32, values[4] as i32],
                color: unpack_color(values[5]),
            },
            Maze => Effect::Maze {
                size: value(0) as u32,
                seed: value(2) as u32,
                branching: pick(1, MAZE_STYLES.len()) == 1,
                colors: colors(3),
            },
            SpeedLines => Effect::SpeedLines {
                count: value(0) as u32,
                thickness: value(1),
                clear: value(2),
                randomness: value(3),
                center: [value(4), value(5)],
                seed: value(6) as u32,
                color: unpack_color(values[7]),
            },
            EdgeDetect => Effect::EdgeDetect {
                algorithm: EdgeAlgorithm::ALL[pick(0, EdgeAlgorithm::ALL.len())],
                horizontal: values[1] != 0.0,
                vertical: values[2] != 0.0,
                radii: [value(3), value(4)],
                normalize: values[5] != 0.0,
                amount: value(6),
                invert: values[7] != 0.0,
            },
            Emboss => Effect::Emboss { azimuth: value(0), elevation: value(1), depth: value(2) },
            Relief => Effect::Relief { azimuth: value(0), elevation: value(1), depth: value(2) },
            Outline => Effect::Outline { thickness: value(0), intensity: value(1) },
            AddNoise => Effect::AddNoise {
                intensity: value(0),
                saturation: value(1),
                coverage: value(2),
                color_space: NoiseColorSpace::ALL[pick(3, NoiseColorSpace::ALL.len())],
                seed: value(4) as u32,
            },
            Spread => Effect::Spread { amount: value(0), downward: value(1), seed: value(2) as u32 },
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

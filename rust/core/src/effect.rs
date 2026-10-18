//! The effects of the Filters menu, which, unlike the adjustments, look at
//! the pixels around each one. The colours are multiplied by how opaque they are while they are mixed, so
//! see-through pixels add nothing to those around them, and divided out again at the
//! end. Past its edges, the image goes on as its edge pixels. Points between pixels are
//! read between the four around them. Large images are cut into bands of rows, worked
//! out at the same time on as many threads as there are cores.

mod artistic;
mod decor;
mod distort;
mod enhance;
mod light;
mod noise;
mod photo;
mod render;
mod stylize;

pub(crate) use artistic::noise;

use std::{
    f32::consts::TAU,
    num::NonZero,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use image::{Rgba, RgbaImage};

pub use distort::{NewsprintScreen, WaveShape, WindDirection};
pub use enhance::wavelet_decompose;
pub use light::LightSource;
pub use noise::NoiseColorSpace;
pub use render::{CellStyle, FractalKind, Noise, NoiseKind};
pub use stylize::EdgeAlgorithm;
pub(crate) use enhance::GRAIN_MIDDLE;

use crate::{BlendMode, Interpolation, PixelRect, SelectionMask, Turn, turn_pixels};

/// Fewest pixels worth sharing between threads.
const PARALLEL_PIXELS: usize = 1 << 14;
/// The power the Bokeh Blur and the Square Blur raise colours to at full highlights.
const MAX_HIGHLIGHTS: f32 = 3.0;
/// Most points the blurs that follow a line or an arc take, however long it is.
const MAX_SAMPLES: usize = 256;
/// Least opacity a pixel keeps its colour with: those under it are stored as see-through.
const MIN_ALPHA: f32 = 0.5 / 255.0;
/// How much of red, green and blue makes up how light a colour looks.
const LUMA: [f32; 3] = [0.299, 0.587, 0.114];
/// How many box blurs, one after the other, the blurs whose radius changes from pixel
/// to pixel take along each side: three are much like a Gaussian Blur.
const BOX_PASSES: usize = 3;

/// An effect, with its settings: lengths in pixels, angles in degrees anticlockwise from
/// the right, and shares in percent. A centre is from -100, the left or top edge, to
/// 100, the right or bottom one, 0 being the middle of the image.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// Averages a disc of pixels, as an out-of-focus lens does. The highlights, from 0
    /// to 100, make bright colours stand out.
    BokehBlur { radius: f32, highlights: f32 },
    /// Blurs more the further from the centre, as a camera focused on it does: not at
    /// all within `size` of it, a share of half the image's longer side, measured as
    /// the shape says, then more and more over `transition` further, up to `radius`.
    FocusBlur { radius: f32, shape: FocusShape, center: [f32; 2], size: f32, transition: f32 },
    /// Averages copies of the image moved the same distance in directions evenly
    /// around a circle.
    FragmentBlur { count: u32, distance: f32, rotation: f32 },
    GaussianBlur { radius: f32 },
    /// Smooths the colours along the edges between them rather than across them, a
    /// little more with each step, which takes out noise and specks but keeps the
    /// edges sharp.
    MeanCurvatureBlur { iterations: u32 },
    /// Each channel's value that this share of the pixels within the radius are no
    /// lighter than.
    MedianBlur { radius: f32, percentile: f32 },
    /// Averages a line of pixels, as a camera that moves while the picture is taken
    /// does, as far behind as ahead if centred, or else only behind.
    MotionBlur { angle: f32, distance: f32, centered: bool },
    /// Averages an arc of pixels around the centre, as a picture that turns does.
    RadialBlur { angle: f32, center: [f32; 2] },
    /// Averages short lines of pixels whose way drifts slowly across the image, as
    /// coarse brush strokes would.
    SketchBlur { radius: f32 },
    SquareBlur { radius: f32, highlights: f32 },
    /// A Gaussian Blur that leaves out colours more different than the threshold from
    /// the pixel's, which keeps edges sharp.
    SurfaceBlur { radius: f32, threshold: f32 },
    /// A Gaussian Blur within the rectangle around the selected pixels, or the whole
    /// image, that goes on past each of its edges with the pixels of the other, as on a
    /// tile repeated side by side, so they still tile once blurred.
    TileableBlur { radius: f32 },
    /// Blurs each pixel by up to `radius`, as a Gaussian Blur does, as much as the map
    /// is light there and can be seen: not at all where it is black or see-through, or
    /// the other way round if inverted. The map is as large as the pixels.
    VariableBlur { radius: f32, map: Arc<RgbaImage>, invert: bool },
    /// Averages a line of pixels towards the centre, this much of the way, as a camera
    /// that zooms while the picture is taken does.
    ZoomBlur { amount: f32, center: [f32; 2] },
    /// Takes each colour further from the blurred one, by this much again, which brings
    /// out edges and details, as an unsharp mask does, unless it is closer to it than
    /// the threshold. How opaque each pixel is stays the same.
    Sharpen { radius: f32, amount: f32, threshold: f32 },
    /// Blurs the image by `softness` and lays it halfway over itself where lighter,
    /// which smooths skin but keeps edges, then lights it by `lighting`, from -20 to 20,
    /// and warms it by `warmth`, from 0 to 20. How opaque each pixel is stays the same.
    SoftenPortrait { softness: f32, lighting: f32, warmth: f32 },
    /// Turns the selected pixels, or all of them, by `angle` about the middle of the
    /// rectangle around them, redrawing them as `interpolation` says, and if `fill`,
    /// zooms them in just enough for that rectangle to stay full.
    Straighten { angle: f32, fill: bool, interpolation: Interpolation },
    /// Moves the pixels right and down by this many, or left and up if below zero,
    /// within the rectangle around the selected ones, or the whole image, filling where
    /// they move away from as `edges` says.
    Offset { x: i32, y: i32, edges: OffsetEdges },
    /// Turns, zooms and moves the selected pixels, or all of them, as the turn says,
    /// about the middle of the rectangle around them, redrawing them as `interpolation`
    /// says. Unlike the other effects, it may carry them out of the selection.
    Turn { turn: Turn, interpolation: Interpolation },
    /// Lights the edges of the selected pixels that can be seen, or of the whole image,
    /// as a rounded bevel `width` wide, lit from `azimuth`, lighter or darker by up to
    /// `depth` percent.
    AddBevel { width: f32, depth: f32, azimuth: f32 },
    /// A border `size` wide in the colour along the inside of the edges of the
    /// selection, or of the whole image: lighter at the top and darker at the bottom by
    /// `shading` percent, its sides half as much.
    AddBorder { size: f32, color: [u8; 4], shading: f32 },
    /// Makes the corners of the rectangle around the selected pixels, or of the whole
    /// image, see-through outside quarter circles of the radius.
    RoundCorners { radius: f32 },
    /// Lays a shadow of the pixels that can be seen under them, in the colour,
    /// `opacity` percent as dark, moved `distance` towards `angle` and blurred by `blur`.
    DropShadow { angle: f32, distance: f32, blur: f32, opacity: f32, color: [u8; 4] },
    /// Lays a shadow of the pixels that can be seen under them, drawn out `length`
    /// towards `angle`, as flat designs do, in the colour, `opacity` percent as dark,
    /// and if `fade`, fading out along its length.
    LongShadow { angle: f32, length: f32, opacity: f32, color: [u8; 4], fade: bool },
    /// Takes the colours towards `color`, by up to `amount` percent, the further from the
    /// centre: not within `radius` percent of the way to the corners, less `softness`
    /// percent of that, and all the way past it. How opaque each pixel is stays the same.
    Vignette { amount: f32, radius: f32, softness: f32, center: [f32; 2], color: [u8; 4] },
    /// Light shining into a camera from the centre: a glow `size` wide with a halo
    /// around it, `brightness` percent as bright, `rays` of light out of it if any, and
    /// if `ghosts`, rings of light along the line through the middle of the image, as
    /// lenses reflect it. Laid over the image as a screen does.
    LensFlare { center: [f32; 2], size: f32, brightness: f32, rays: u32, ghosts: bool },
    /// Lights the image with a light in the colour, from a lamp above the centre,
    /// `height` percent of the image's longer side up, or from `azimuth` and `elevation`,
    /// as `light` says: lighter the more it faces it, by `intensity` percent, shining as
    /// much as `shine` says, with `ambient` percent of its own colours where unlit. It
    /// stands out as light as it is, `bumps` times as much.
    LightingEffects {
        light: LightSource,
        center: [f32; 2],
        height: f32,
        azimuth: f32,
        elevation: f32,
        intensity: f32,
        ambient: f32,
        shine: f32,
        bumps: f32,
        color: [u8; 4],
    },
    /// Glints on the spots at least `threshold` percent light: stars of `points` rays,
    /// the first turned by `angle` from straight up, up to `size` long, at most one in
    /// each square that wide, and `amount` percent of those, picked as the seed says.
    Sparkle { threshold: f32, size: f32, points: u32, angle: f32, amount: f32, seed: u32 },
    /// Lays the image on woven canvas, lit from `azimuth`, which stands out by `depth`.
    ApplyCanvas { azimuth: f32, depth: f32 },
    /// Darkens what is darker than around it, `radius` away, as the outlines of a
    /// cartoon: the darkest `black` share of the pixels black.
    Cartoon { radius: f32, black: f32 },
    /// Lights the image as if it were woven cloth, of threads as long as `blur` across
    /// and down, lit from `azimuth` and `elevation`, in degrees, which stand out by `depth`.
    Clothify { blur: [f32; 2], azimuth: f32, elevation: f32, depth: f32 },
    /// Rectangles turned every way, about `size` apart, as large as `saturation` times
    /// that at most, each the colour of the image at its middle, on `background`.
    Cubism { size: f32, saturation: f32, background: [u8; 4] },
    /// Shows the image through tiles of glass this large, each a lens.
    GlassTile { width: f32, height: f32 },
    /// Lays the image, blurred, made brighter and more contrasted, from -100 to 100,
    /// over itself as a screen does.
    Glow { radius: f32, brightness: f32, contrast: f32 },
    /// Ink lines along the edges, stronger the more ink, over the colours faded towards
    /// white the less coloring there is.
    InkSketch { ink: f32, coloring: f32 },
    /// Each pixel the average of those within the radius whose lightness, cut into
    /// `levels` steps, is the most common there.
    OilPainting { radius: f32, levels: u32 },
    /// Grey pencil lines on white, as wide as the tip, darker or lighter as `range`,
    /// from -20 to 20, says.
    PencilSketch { tip: f32, range: f32 },
    /// Black where the image is darker than around it, `radius` away, the darkest
    /// `black` share of the pixels, and its tones elsewhere faded towards white by
    /// `white`. The sharper, the less the image is blurred before.
    Photocopy { radius: f32, sharpness: f32, black: f32, white: f32 },
    /// Bright edges on black, `amount` times as strong, in squares `pixel_size` wide.
    Predator { amount: f32, pixel_size: u32 },
    /// Clusters of pixels about `size` across that are close in colour, rounder the
    /// more compact, each its average colour, moved `iterations` times.
    Slic { size: u32, compactness: f32, iterations: u32 },
    /// Brush strokes `length` long either side of each pixel, along the edges, shaded
    /// by noise as much as `strength` says.
    VanGogh { length: f32, strength: f32 },
    /// Regions about `size` across grown out to where the image, blurred by
    /// `smoothness`, changes most, squarer the more regularization, each its average
    /// colour, or a random one.
    Waterpixels { size: u32, smoothness: f32, regularization: f32, random: bool },
    /// Shades the image as if woven of ribbons `width` wide with `spacing` between
    /// them, which darken by `darkness` over `depth` of the way down as they go under
    /// each other, with fibres as dark as `threads` says.
    Weave { width: f32, spacing: f32, darkness: f32, depth: f32, threads: f32 },
    /// Smooths the stairs along the edges of flat colours, as pixel art enlarged would
    /// have them rounded off.
    Antialias,
    /// Keeps every other band of `size` rows, or columns if vertical, the odd ones or
    /// the even ones from the first, and fills the others from those either side, which
    /// takes out the combing of a frame of interlaced video.
    Deinterlace { keep_odd: bool, vertical: bool, size: u32 },
    /// How far each colour is from the Gaussian blurred one, `contrast` percent of it,
    /// around middle grey: the details, without the broad tones.
    HighPass { radius: f32, contrast: f32 },
    /// Takes the red out of the pixels redder than their green and blue, more of them
    /// the higher the threshold, from 0 to 100. Meant for a selection around the eyes.
    RedEyeRemoval { threshold: f32 },
    /// Averages the pixels within the radius, less the more different their colour,
    /// which smooths noise but keeps edges: the stronger, the more different the
    /// colours it still mixes in.
    ReduceNoise { radius: f32, strength: f32 },
    /// Swells a disc around the centre out, as if seen through a lens, the more the
    /// nearer its middle, or shrinks it if `amount`, from -100 to 100, is below zero.
    /// The disc's radius is a share of half the image's shorter side.
    Bulge { amount: f32, radius: f32, center: [f32; 2] },
    /// Turns a disc around the centre by up to `angle`, the most in its middle and not
    /// at all at its edge, and pinches it towards its middle, or pushes it out if
    /// `pinch`, from -100 to 100, is below zero. The radius is as the Bulge's.
    Twist { angle: f32, pinch: f32, radius: f32, center: [f32; 2] },
    /// Breaks the image into irregular cells about `size` across, each the average
    /// colour of its pixels.
    Crystallize { size: f32 },
    /// Cuts the image into cells this large, from its top left corner, each the
    /// average colour of its pixels.
    Pixelate { width: u32, height: u32 },
    /// Wraps the image around its middle, its top there and its bottom along the
    /// ellipse as large as the image, its left and right edges meeting above the
    /// middle, or unwraps it again if not `to_polar`, turned by `rotation`. Nothing is
    /// left where the image does not reach.
    PolarCoordinates { to_polar: bool, rotation: f32 },
    /// Shows the image through square tiles `size` wide, turned by `angle`, each
    /// bending it as much as `curvature`, from -100 to 100, says: up to about that many
    /// pixels halfway out from its middle, and endlessly at its edges.
    TileReflection { size: f32, angle: f32, curvature: f32 },
    /// Bends the image as a camera lens does, by the square of how far each pixel is
    /// from the centre and by the fourth power, as `main` and `edge`, from -100 to 100,
    /// say: as a barrel above zero, as a pincushion below. Zoomed in, or out below zero.
    /// Nothing is left where the image does not reach.
    LensDistortion { main: f32, edge: f32, zoom: f32, center: [f32; 2] },
    /// The wedge turned by `angle` of `segments` pairs of them around the centre,
    /// mirrored around it, every other one the other way round.
    Kaleidoscope { segments: u32, angle: f32, center: [f32; 2] },
    /// Prints the image in dots, as `screen` says, on grids of cells `size` wide
    /// turned by `angle`.
    Newsprint { size: f32, angle: f32, screen: NewsprintScreen },
    /// Moves each row up and down, or each column sideways if vertical, by up to
    /// `amplitude`, in waves `wavelength` long along the image, `phase` degrees on.
    Ripple { amplitude: f32, wavelength: f32, phase: f32, vertical: bool, shape: WaveShape },
    /// Moves the pixels towards and away from the centre, by up to `amplitude`, in
    /// rings `wavelength` apart, `phase` degrees on, as waves on water.
    Waves { amplitude: f32, wavelength: f32, phase: f32, center: [f32; 2] },
    /// Blows streaks up to `strength` long from the edges the image gets darker past,
    /// by more than `threshold` percent, the way of `direction`, fading out, or as long
    /// as half that without fading if a blast.
    Wind { strength: f32, threshold: f32, direction: WindDirection, blast: bool },
    /// The edges, as bright as red, green and blue each change there, `amount` percent
    /// as much, or as much as makes the strongest full if `normalize`, on black, or dark
    /// on white if inverted. The algorithm measures the change across if `horizontal`
    /// and down if `vertical` for the Sobel, or between the image blurred by either of
    /// `radii` for the Difference of Gaussians.
    EdgeDetect { algorithm: EdgeAlgorithm, horizontal: bool, vertical: bool, radii: [f32; 2], amount: f32, normalize: bool, invert: bool },
    /// The image as a grey relief, standing out as light as it is, `depth` times as
    /// much, lit from `azimuth` and `elevation`, in degrees: flat ground middle grey.
    Emboss { azimuth: f32, elevation: f32, depth: f32 },
    /// The image's own colours lit as the Emboss lights its relief: flat ground as it was.
    Relief { azimuth: f32, elevation: f32, depth: f32 },
    /// Lines along the edges, `thickness` wide on either side, each pixel in its own
    /// colour as much as the colours change around it, the more so the more
    /// `intensity`, from 0 to 100, and white elsewhere.
    Outline { thickness: f32, intensity: f32 },
    /// Changes the colours of the `coverage` share of the pixels at random, as much as
    /// `intensity` says, in `color_space`: how light they are, and the more `saturation`,
    /// from 0 to 100, their colour too. The seed picks which pixels and how.
    AddNoise { intensity: f32, saturation: f32, coverage: f32, color_space: NoiseColorSpace, seed: u32 },
    /// Swaps each pixel for one picked at random up to `amount` away, as the Jumble tool
    /// does: from above, as far again in streaks down the image, rather than from every
    /// way, the more `downward`, from 0 to 100, which melts it. The seed picks which.
    Spread { amount: f32, downward: f32, seed: u32 },
    /// Draws the noise, from the first of `colors` where there is none to the second
    /// where it is full, blended over the image in `blend_mode`: the Difference one
    /// gives Difference Clouds. The Plasma draws red, green and blue each a noise of
    /// their own instead with `plasma_colors`.
    CloudsAndNoise { noise: Noise, colors: [[u8; 4]; 2], blend_mode: BlendMode, plasma_colors: bool },
    /// Draws the Mandelbrot set or a Julia set, that of `julia`, as many `iterations`
    /// deep, `zoom` percent large, around `center` away from the middle of the set: the
    /// set in the first of `colors`, and around it, going round the rainbow, or from the
    /// second colour far from it towards the first nearer it. With `antialias`, each
    /// pixel averages four points.
    Fractal {
        kind: FractalKind,
        iterations: u32,
        zoom: f64,
        center: [f64; 2],
        julia: [f64; 2],
        rainbow: bool,
        colors: [[u8; 4]; 2],
        antialias: bool,
    },
    /// Fills the image with a Bayer matrix as wide as `1 << level` squares, each
    /// `pixel_size` wide, as ordered dithering uses, the first square in the first of
    /// `colors` and the last in the second.
    BayerMatrix { level: u32, pixel_size: u32, colors: [[u8; 4]; 2] },
    /// Fills the image with squares `size` wide, of either colour in turn, the first
    /// at the top left.
    Checkerboard { size: u32, colors: [[u8; 4]; 2] },
    /// Draws lines `line` wide, `width` apart down the image and `height` apart across
    /// it, moved by `offset`.
    Grid { width: u32, height: u32, line: u32, offset: [i32; 2], color: [u8; 4] },
    /// Fills the image with a maze, its corridors `size` wide in the second of
    /// `colors` and its walls in the first, branching often, or winding.
    Maze { size: u32, seed: u32, branching: bool, colors: [[u8; 4]; 2] },
    /// Draws `count` lines radiating from the centre, narrowing to a point, as comics
    /// show speed or focus with: each `thickness` percent of the room between two at the
    /// furthest corner, starting `clear` percent of the way there. The more random, the
    /// more their places, widths and starts change.
    SpeedLines { count: u32, thickness: f32, clear: f32, randomness: f32, center: [f32; 2], seed: u32, color: [u8; 4] },
}

/// How the Focus Blur measures how far a pixel is from the centre, which gives the
/// shape of the area it leaves sharp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusShape {
    Circle,
    Square,
    Diamond,
    /// A band across the image, as a tilt-shift lens leaves sharp.
    Horizontal,
    /// A band down the image.
    Vertical,
}

impl FocusShape {
    pub const ALL: [Self; 5] = [Self::Circle, Self::Square, Self::Diamond, Self::Horizontal, Self::Vertical];

    /// How far a point is from the centre, `x` across and `y` down.
    fn distance(self, x: f32, y: f32) -> f32 {
        match self {
            Self::Circle => x.hypot(y),
            Self::Square => x.abs().max(y.abs()),
            Self::Diamond => x.abs() + y.abs(),
            Self::Horizontal => y.abs(),
            Self::Vertical => x.abs(),
        }
    }
}

/// What the Offset fills where the pixels move away from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OffsetEdges {
    /// With those moved off the other side, as on a tile repeated side by side.
    Wrap,
    Transparent,
    /// With the pixels along the edge they move away from, drawn out.
    Repeat,
}

impl OffsetEdges {
    pub const ALL: [Self; 3] = [Self::Wrap, Self::Transparent, Self::Repeat];

    pub fn name(self) -> &'static str {
        match self {
            Self::Wrap => "Wrap Around",
            Self::Transparent => "Transparent",
            Self::Repeat => "Repeat Edge Pixels",
        }
    }
}

/// The pixels as the effect makes them, but for those `mask` leaves out, if there is
/// one, which stay as they were. The mask must be as large as the pixels. Gives up
/// once `stop` is set, returning nothing.
pub fn apply_effect(pixels: &RgbaImage, effect: &Effect, mask: Option<&SelectionMask>, stop: &AtomicBool) -> Option<RgbaImage> {
    if pixels.width() == 0 || pixels.height() == 0 {
        return Some(pixels.clone());
    }
    let mut result = match *effect {
        Effect::MedianBlur { radius, percentile } => median(pixels, radius, percentile / 100.0, stop)?,
        Effect::Offset { x, y, edges } => offset(pixels, x, y, edges, mask),
        Effect::Turn { turn, interpolation } => return Some(turn_pixels(pixels, mask, &turn, interpolation)),
        Effect::Straighten { angle, .. } if angle.abs() < f32::EPSILON => return Some(pixels.clone()),
        Effect::Straighten { angle, fill, interpolation } => {
            let whole = PixelRect { left: 0, top: 0, right: pixels.width() as i32, bottom: pixels.height() as i32 };
            let area = mask.and_then(SelectionMask::bounds).unwrap_or(whole);
            let zoom = if fill { straightened_zoom(angle, area) } else { 100.0 };
            // The turn goes clockwise.
            let turn = Turn { angles: [0.0, 0.0, -angle as f64], zoom, ..Turn::NONE };
            turn_pixels(pixels, mask, &turn, interpolation)
        }
        Effect::AddBevel { width, depth, .. } if width < 1.0 || depth <= 0.0 => return Some(pixels.clone()),
        Effect::AddBorder { size, .. } if size < 1.0 => return Some(pixels.clone()),
        Effect::RoundCorners { radius } if radius < 0.5 => return Some(pixels.clone()),
        Effect::AddBevel { .. } | Effect::AddBorder { .. } | Effect::RoundCorners { .. } => decor::render(pixels, effect, mask, stop)?,
        Effect::TileableBlur { radius } if radius < 1.0 => return Some(pixels.clone()),
        Effect::TileableBlur { radius } => tileable(pixels, radius, mask, stop)?,
        Effect::CloudsAndNoise { .. }
        | Effect::Fractal { .. }
        | Effect::BayerMatrix { .. }
        | Effect::Checkerboard { .. }
        | Effect::Grid { .. }
        | Effect::Maze { .. }
        | Effect::SpeedLines { .. } => {
            let whole = PixelRect { left: 0, top: 0, right: pixels.width() as i32, bottom: pixels.height() as i32 };
            render::render(pixels, effect, mask.and_then(SelectionMask::bounds).unwrap_or(whole), stop)?
        }
        _ => match render(&Buffer::premultiplied(pixels), effect, stop)? {
            Some(buffer) => buffer.to_image(),
            None => return Some(pixels.clone()),
        },
    };
    if let Some(mask) = mask {
        for ((pixel, before), &selected) in result.pixels_mut().zip(pixels.pixels()).zip(mask.flags()) {
            if !selected {
                *pixel = *before;
            }
        }
    }
    Some(result)
}

/// The pixels moved by `x` and `y` within the rectangle around those `mask` holds, or
/// the whole image, the rest filled as `edges` says.
fn offset(pixels: &RgbaImage, x: i32, y: i32, edges: OffsetEdges, mask: Option<&SelectionMask>) -> RgbaImage {
    let whole = PixelRect { left: 0, top: 0, right: pixels.width() as i32, bottom: pixels.height() as i32 };
    let area = mask.and_then(SelectionMask::bounds).unwrap_or(whole);
    let mut result = pixels.clone();
    // Where a pixel of the area comes from, along one side: before the move, if it is in the area.
    let source = |at: i32, by: i32, start: i32, end: i32| {
        let from = at - by;
        match edges {
            OffsetEdges::Wrap => Some(start + (from - start).rem_euclid(end - start)),
            OffsetEdges::Transparent => (start..end).contains(&from).then_some(from),
            OffsetEdges::Repeat => Some(from.clamp(start, end - 1)),
        }
    };
    for row in area.top..area.bottom {
        for column in area.left..area.right {
            let from = source(column, x, area.left, area.right).zip(source(row, y, area.top, area.bottom));
            let pixel = from.map_or(Rgba([0, 0, 0, 0]), |(from_x, from_y)| *pixels.get_pixel(from_x as u32, from_y as u32));
            result.put_pixel(column as u32, row as u32, pixel);
        }
    }
    result
}

/// How much the Straighten zooms, as a percentage, for the area turned by `angle`
/// degrees to still cover all of it, and a pixel around it, so its corners are not
/// blended with nothing as they are redrawn.
fn straightened_zoom(angle: f32, area: PixelRect) -> f64 {
    let (width, height) = (area.width() as f64, area.height() as f64);
    let (sin, cos) = (angle as f64).to_radians().abs().sin_cos();
    // As wide and high as the area grown by a pixel each side is, turned the other way.
    let (wide, high) = ((width + 2.0) * cos + (height + 2.0) * sin, (width + 2.0) * sin + (height + 2.0) * cos);
    (wide / width).max(high / height) * 100.0
}

/// The pixels Gaussian blurred within the rectangle around those `mask` holds, or the
/// whole image, which goes on past each of its edges with the pixels of the other.
/// Gives up once `stop` is set.
fn tileable(pixels: &RgbaImage, radius: f32, mask: Option<&SelectionMask>, stop: &AtomicBool) -> Option<RgbaImage> {
    let whole = PixelRect { left: 0, top: 0, right: pixels.width() as i32, bottom: pixels.height() as i32 };
    let area = mask.and_then(SelectionMask::bounds).unwrap_or(whole);
    let (width, height) = (area.width() as usize, area.height() as usize);
    // The area, with as far around it as the blur reaches, from the other side.
    let margin = radius.max(0.0) as usize;
    let (tile_width, tile_height) = (width + 2 * margin, height + 2 * margin);
    let wrapped = |at: usize, size: usize| (at as isize - margin as isize).rem_euclid(size as isize) as u32;
    let tile = Buffer {
        width: tile_width,
        height: tile_height,
        pixels: (0..tile_height)
            .flat_map(|y| {
                (0..tile_width).map(move |x| {
                    premultiply(*pixels.get_pixel(area.left as u32 + wrapped(x, width), area.top as u32 + wrapped(y, height)))
                })
            })
            .collect(),
    };
    let blurred = blur(&tile, Weighing::Gaussian, radius, 1.0, stop)?.to_image();
    let mut result = pixels.clone();
    for y in 0..height as u32 {
        for x in 0..width as u32 {
            let pixel = *blurred.get_pixel(x + margin as u32, y + margin as u32);
            result.put_pixel(area.left as u32 + x, area.top as u32 + y, pixel);
        }
    }
    Some(result)
}

/// The colours, multiplied by how opaque they are, as the effect makes them, or
/// nothing for settings that change nothing. Gives up once `stop` is set.
fn render(source: &Buffer, effect: &Effect, stop: &AtomicBool) -> Option<Option<Buffer>> {
    let size = [source.width as f32, source.height as f32];
    let center = |[x, y]: [f32; 2]| [size[0] / 2.0 * (1.0 + x / 100.0), size[1] / 2.0 * (1.0 + y / 100.0)];
    let highlights = |highlights: f32| 1.0 + highlights / 100.0 * (MAX_HIGHLIGHTS - 1.0);
    let rendered = match *effect {
        Effect::BokehBlur { radius, highlights: h } => bokeh(source, radius, highlights(h), stop),
        Effect::FocusBlur { radius, .. } | Effect::VariableBlur { radius, .. } if radius <= 0.0 => return Some(None),
        Effect::FocusBlur { radius, shape, center: c, size: sharp, transition } => {
            let [cx, cy] = center(c);
            let unit = size[0].max(size[1]) / 2.0;
            let (sharp, transition) = (sharp / 100.0, transition / 100.0);
            let radii = (0..source.height)
                .flat_map(|y| {
                    (0..source.width).map(move |x| {
                        let distance = shape.distance((x as f32 + 0.5 - cx) / unit, (y as f32 + 0.5 - cy) / unit);
                        let past = distance - sharp;
                        let amount = if transition > 0.0 { smoothstep((past / transition).clamp(0.0, 1.0)) } else { (past > 0.0) as u8 as f32 };
                        radius * amount
                    })
                })
                .collect();
            variable_blur(source, radii, stop)
        }
        Effect::FragmentBlur { count, distance, rotation } => {
            let count = count.max(1);
            let rotation = rotation.to_radians();
            per_pixel(source, stop, |p| {
                let mut sum = [0.0; 4];
                for i in 0..count {
                    let turn = rotation + TAU * i as f32 / count as f32;
                    add(&mut sum, source.sample([p[0] + distance * turn.cos(), p[1] - distance * turn.sin()]), 1.0);
                }
                scale(sum, 1.0 / count as f32)
            })
        }
        Effect::GaussianBlur { radius } if radius < 1.0 => return Some(None),
        Effect::GaussianBlur { radius } => blur(source, Weighing::Gaussian, radius, 1.0, stop),
        Effect::MeanCurvatureBlur { iterations: 0 } => return Some(None),
        Effect::MeanCurvatureBlur { iterations } => mean_curvature(source, iterations, stop),
        Effect::MedianBlur { .. }
        | Effect::CloudsAndNoise { .. }
        | Effect::Fractal { .. }
        | Effect::BayerMatrix { .. }
        | Effect::Checkerboard { .. }
        | Effect::Grid { .. }
        | Effect::Maze { .. }
        | Effect::SpeedLines { .. }
        | Effect::Straighten { .. }
        | Effect::AddBevel { .. }
        | Effect::AddBorder { .. }
        | Effect::RoundCorners { .. } => unreachable!("worked out on the bytes"),
        Effect::MotionBlur { angle, distance, centered } => {
            let angle = angle.to_radians();
            let direction = [angle.cos(), -angle.sin()];
            let samples = (distance.ceil().max(0.0) as usize + 1).min(MAX_SAMPLES);
            per_pixel(source, stop, |p| {
                let mut sum = [0.0; 4];
                for i in 0..samples {
                    let along = if samples > 1 { i as f32 / (samples - 1) as f32 } else { 0.0 };
                    let offset = if centered { (along - 0.5) * distance } else { -along * distance };
                    add(&mut sum, source.sample([p[0] + direction[0] * offset, p[1] + direction[1] * offset]), 1.0);
                }
                scale(sum, 1.0 / samples as f32)
            })
        }
        Effect::RadialBlur { angle, .. } if angle.abs() < f32::EPSILON => return Some(None),
        Effect::RadialBlur { angle, center: c } => {
            let angle = angle.to_radians();
            let [cx, cy] = center(c);
            per_pixel(source, stop, |p| {
                let (dx, dy) = (p[0] - cx, p[1] - cy);
                let length = dx.hypot(dy);
                let samples = ((angle * length).ceil() as usize + 1).clamp(1, MAX_SAMPLES);
                let mut sum = [0.0; 4];
                for i in 0..samples {
                    let turn = if samples > 1 { (i as f32 / (samples - 1) as f32 - 0.5) * angle } else { 0.0 };
                    let (sin, cos) = turn.sin_cos();
                    add(&mut sum, source.sample([cx + cos * dx - sin * dy, cy + sin * dx + cos * dy]), 1.0);
                }
                scale(sum, 1.0 / samples as f32)
            })
        }
        Effect::SketchBlur { radius } => per_pixel(source, stop, |p| sketch(source, radius, p)),
        Effect::SquareBlur { radius, .. } | Effect::SurfaceBlur { radius, .. } if radius < 1.0 => return Some(None),
        Effect::SquareBlur { radius, highlights: h } => blur(source, Weighing::Square, radius, highlights(h), stop),
        Effect::SurfaceBlur { radius, threshold } => {
            blur(source, Weighing::Surface { threshold: (threshold / 100.0).max(f32::EPSILON) }, radius, 1.0, stop)
        }
        Effect::TileableBlur { .. } => unreachable!("worked out within the selection"),
        Effect::VariableBlur { radius, ref map, invert } => {
            let radii = (0..source.height as u32)
                .flat_map(|y| {
                    (0..source.width as u32).map(move |x| {
                        let light = map.get_pixel_checked(x, y).map_or(0.0, |&Rgba([r, g, b, a])| {
                            let luma = LUMA[0] * r as f32 + LUMA[1] * g as f32 + LUMA[2] * b as f32;
                            luma / 255.0 * a as f32 / 255.0
                        });
                        radius * if invert { 1.0 - light } else { light }
                    })
                })
                .collect();
            variable_blur(source, radii, stop)
        }
        Effect::ZoomBlur { amount, .. } if amount.abs() < f32::EPSILON => return Some(None),
        Effect::ZoomBlur { amount, center: c } => {
            let amount = amount / 100.0;
            let [cx, cy] = center(c);
            per_pixel(source, stop, |p| {
                let reach = [(cx - p[0]) * amount, (cy - p[1]) * amount];
                let samples = (reach[0].hypot(reach[1]).ceil() as usize + 1).clamp(1, MAX_SAMPLES);
                let mut sum = [0.0; 4];
                for i in 0..samples {
                    let along = if samples > 1 { i as f32 / (samples - 1) as f32 } else { 0.0 };
                    add(&mut sum, source.sample([p[0] + reach[0] * along, p[1] + reach[1] * along]), 1.0);
                }
                scale(sum, 1.0 / samples as f32)
            })
        }
        Effect::Sharpen { amount, .. } if amount.abs() < f32::EPSILON => return Some(None),
        Effect::Offset { .. } | Effect::Turn { .. } => return Some(None),
        Effect::ApplyCanvas { .. }
        | Effect::Cartoon { .. }
        | Effect::Clothify { .. }
        | Effect::Cubism { .. }
        | Effect::GlassTile { .. }
        | Effect::Glow { .. }
        | Effect::InkSketch { .. }
        | Effect::OilPainting { .. }
        | Effect::PencilSketch { .. }
        | Effect::Photocopy { .. }
        | Effect::Predator { .. }
        | Effect::Slic { .. }
        | Effect::VanGogh { .. }
        | Effect::Waterpixels { .. }
        | Effect::Weave { .. } => artistic::render(source, effect, stop),
        Effect::ReduceNoise { radius, strength } if radius < 1.0 || strength <= 0.0 => return Some(None),
        Effect::Antialias
        | Effect::Deinterlace { .. }
        | Effect::HighPass { .. }
        | Effect::RedEyeRemoval { .. }
        | Effect::ReduceNoise { .. } => enhance::render(source, effect, stop),
        Effect::Bulge { amount, .. } if amount.abs() < f32::EPSILON => return Some(None),
        Effect::Twist { angle, pinch, .. } if angle.abs() < f32::EPSILON && pinch.abs() < f32::EPSILON => return Some(None),
        Effect::Pixelate { width, height } if width <= 1 && height <= 1 => return Some(None),
        Effect::TileReflection { curvature, .. } if curvature.abs() < f32::EPSILON => return Some(None),
        Effect::LensDistortion { main, edge, zoom, .. } if main == 0.0 && edge == 0.0 && zoom == 0.0 => return Some(None),
        Effect::Ripple { amplitude, .. } | Effect::Waves { amplitude, .. } if amplitude.abs() < f32::EPSILON => return Some(None),
        Effect::Wind { strength, .. } if strength < 1.0 => return Some(None),
        Effect::Bulge { .. }
        | Effect::Twist { .. }
        | Effect::Crystallize { .. }
        | Effect::Pixelate { .. }
        | Effect::PolarCoordinates { .. }
        | Effect::TileReflection { .. }
        | Effect::LensDistortion { .. }
        | Effect::Kaleidoscope { .. }
        | Effect::Newsprint { .. }
        | Effect::Ripple { .. }
        | Effect::Waves { .. }
        | Effect::Wind { .. } => distort::render(source, effect, stop),
        Effect::EdgeDetect { .. } | Effect::Emboss { .. } | Effect::Relief { .. } | Effect::Outline { .. } => {
            stylize::render(source, effect, stop)
        }
        Effect::AddNoise { intensity, coverage, .. } if intensity <= 0.0 || coverage <= 0.0 => return Some(None),
        Effect::Spread { amount, .. } if amount < 0.5 => return Some(None),
        Effect::AddNoise { .. } | Effect::Spread { .. } => noise::render(source, effect, stop),
        Effect::SoftenPortrait { softness, lighting, warmth } if softness < 1.0 && lighting == 0.0 && warmth <= 0.0 => return Some(None),
        Effect::SoftenPortrait { .. } => photo::render(source, effect, stop),
        Effect::DropShadow { opacity, .. } | Effect::LongShadow { opacity, .. } if opacity <= 0.0 => return Some(None),
        Effect::LongShadow { length, .. } if length < 1.0 => return Some(None),
        Effect::Vignette { amount, color, .. } if amount <= 0.0 || color[3] == 0 => return Some(None),
        Effect::LensFlare { brightness, .. } if brightness <= 0.0 => return Some(None),
        Effect::Sparkle { amount, size, .. } if amount <= 0.0 || size < 1.0 => return Some(None),
        Effect::DropShadow { .. }
        | Effect::LongShadow { .. }
        | Effect::Vignette { .. }
        | Effect::LensFlare { .. }
        | Effect::LightingEffects { .. }
        | Effect::Sparkle { .. } => light::render(source, effect, stop),
        Effect::Sharpen { radius, amount, threshold } => {
            let blurred = if radius < 1.0 { None } else { Some(blur(source, Weighing::Gaussian, radius, 1.0, stop)?) };
            let blurred = blurred.as_ref().unwrap_or(source);
            let (amount, threshold) = (amount / 100.0, threshold / 100.0);
            let mut pixels = vec![[0.0; 4]; source.pixels.len()];
            let width = source.width;
            fill_rows(&mut pixels, width, stop, |y, row| {
                for (x, out) in row.iter_mut().enumerate() {
                    let own = source.pixels[y * width + x];
                    let blur = blurred.pixels[y * width + x];
                    let difference: [f32; 3] = std::array::from_fn(|channel| own[channel] - blur[channel]);
                    *out = if difference.iter().all(|d| d.abs() < threshold) {
                        own
                    } else {
                        let sharp = |channel: usize| (own[channel] + difference[channel] * amount).clamp(0.0, own[3]);
                        [sharp(0), sharp(1), sharp(2), own[3]]
                    };
                }
            })
            .then(|| source.with_pixels(pixels))
        }
    };
    rendered.map(Some)
}

/// How the pixels along a line are weighed in a blur along one axis.
#[derive(Clone, Copy)]
enum Weighing {
    Gaussian,
    Square,
    /// As the Gaussian, less the more any channel is different from the pixel's own,
    /// up to this difference, from 0 to 1, which leaves it out.
    Surface { threshold: f32 },
}

/// Averages a square of pixels a row then a column at a time, which is the same. The
/// colours are raised to the power of `highlights` before, and lowered back after.
fn blur(source: &Buffer, weighing: Weighing, radius: f32, highlights: f32, stop: &AtomicBool) -> Option<Buffer> {
    let across = blur_rows(source, weighing, radius, highlights, 1.0, stop)?;
    Some(blur_rows(&across.transposed(), weighing, radius, 1.0, highlights, stop)?.transposed())
}

/// Averages the pixels along each row, as far as `radius` on either side. The colours
/// are raised to the power of `raise` before, and to that of one over `lower` after.
fn blur_rows(source: &Buffer, weighing: Weighing, radius: f32, raise: f32, lower: f32, stop: &AtomicBool) -> Option<Buffer> {
    let width = source.width;
    let reach = radius.max(0.0) as usize;
    let sigma = (radius / 2.0).max(0.5);
    let weights: Vec<f32> = (0..=2 * reach)
        .map(|i| {
            let offset = i as f32 - reach as f32;
            match weighing {
                Weighing::Square => 1.0,
                _ => (-offset * offset / (2.0 * sigma * sigma)).exp(),
            }
        })
        .collect();
    let weights_total: f32 = weights.iter().sum();
    let mut pixels = vec![[0.0; 4]; source.pixels.len()];
    let done = fill_rows(&mut pixels, width, stop, |y, row| {
        let line = &source.pixels[y * width..(y + 1) * width];
        let padded: Vec<[f32; 4]> =
            (0..width + 2 * reach).map(|i| raised(line[i.saturating_sub(reach).min(width - 1)], raise)).collect();
        for (x, out) in row.iter_mut().enumerate() {
            let window = &padded[x..=x + 2 * reach];
            let mut sum = [0.0; 4];
            let mut total = weights_total;
            if let Weighing::Surface { threshold } = weighing {
                let middle = line[x];
                total = 0.0;
                for (&color, &weight) in window.iter().zip(&weights) {
                    let most = (0..4).map(|channel| (color[channel] - middle[channel]).abs()).fold(0.0, f32::max);
                    let weight = weight * (1.0 - most / threshold).clamp(0.0, 1.0);
                    add(&mut sum, color, weight);
                    total += weight;
                }
            } else {
                for (&color, &weight) in window.iter().zip(&weights) {
                    add(&mut sum, color, weight);
                }
            }
            *out = lowered(scale(sum, 1.0 / total), lower);
        }
    });
    done.then(|| source.with_pixels(pixels))
}

/// Blurs each pixel by its own radius, from `radii`, row after row, much as a Gaussian
/// Blur of it would: a few box blurs along each row, one after the other, then as many
/// along each column.
fn variable_blur(source: &Buffer, radii: Vec<f32>, stop: &AtomicBool) -> Option<Buffer> {
    // How far either side each box goes for the boxes to spread the colours as far as
    // the Gaussian Blur does, whose spread is half its radius: a box as wide as `w`
    // spreads them by the root of (w² - 1) / 12.
    let passes = BOX_PASSES as f32;
    let halves: Vec<f32> = radii.iter().map(|&radius| ((3.0 * radius * radius / passes + 1.0).sqrt() - 1.0) / 2.0).collect();
    let mut buffer = box_rows(source, &halves, stop)?;
    for _ in 1..BOX_PASSES {
        buffer = box_rows(&buffer, &halves, stop)?;
    }
    let halves = Buffer::transposed_items(&halves, source.width, source.height);
    let mut buffer = buffer.transposed();
    for _ in 0..BOX_PASSES {
        buffer = box_rows(&buffer, &halves, stop)?;
    }
    Some(buffer.transposed())
}

/// Averages the pixels along each row, as far either side of each as `halves` says
/// for it, a share of a pixel past the last whole one counting for as much of it,
/// worked out from the sums of the row up to each pixel.
fn box_rows(source: &Buffer, halves: &[f32], stop: &AtomicBool) -> Option<Buffer> {
    let width = source.width;
    let mut pixels = vec![[0.0; 4]; source.pixels.len()];
    let done = fill_rows(&mut pixels, width, stop, |y, row| {
        let line = &source.pixels[y * width..(y + 1) * width];
        let halves = &halves[y * width..(y + 1) * width];
        // The row goes on as its edge pixels as far as the widest box reaches, and a pixel more.
        let reach = halves.iter().copied().fold(0.0, f32::max) as usize + 1;
        let at = |i: usize| line[i.saturating_sub(reach).min(width - 1)];
        let mut sums = Vec::with_capacity(width + 2 * reach + 1);
        let mut total = [0.0f64; 4];
        sums.push(total);
        for i in 0..width + 2 * reach {
            for (total, channel) in total.iter_mut().zip(at(i)) {
                *total += channel as f64;
            }
            sums.push(total);
        }
        for (x, out) in row.iter_mut().enumerate() {
            let half = halves[x];
            if half <= 0.0 {
                *out = line[x];
                continue;
            }
            let whole = half as usize;
            let part = half - whole as f32;
            let (first, last) = (x + reach - whole, x + reach + whole);
            let mut sum: [f32; 4] = std::array::from_fn(|channel| (sums[last + 1][channel] - sums[first][channel]) as f32);
            if part > 0.0 {
                add(&mut sum, at(first - 1), part);
                add(&mut sum, at(last + 1), part);
            }
            *out = scale(sum, 1.0 / ((2 * whole + 1) as f32 + 2.0 * part));
        }
    });
    done.then(|| source.with_pixels(pixels))
}

/// Moves each colour along the curve of the edge it is on, `iterations` times, which
/// smooths it along the edge but not across: the change at each step is a quarter of
/// how steep the colour is times how much its edge bends, as GIMP's does.
fn mean_curvature(source: &Buffer, iterations: u32, stop: &AtomicBool) -> Option<Buffer> {
    let (width, height) = (source.width as isize, source.height as isize);
    let mut current = source.pixels.clone();
    for _ in 0..iterations {
        let mut next = vec![[0.0; 4]; current.len()];
        let previous = &current;
        let at = |x: isize, y: isize| previous[(y.clamp(0, height - 1) * width + x.clamp(0, width - 1)) as usize];
        let done = fill_rows(&mut next, source.width, stop, |y, row| {
            let y = y as isize;
            for (x, out) in row.iter_mut().enumerate() {
                let x = x as isize;
                let [top_left, top, top_right] = [-1, 0, 1].map(|dx| at(x + dx, y - 1));
                let [left, middle, right] = [-1, 0, 1].map(|dx| at(x + dx, y));
                let [bottom_left, bottom, bottom_right] = [-1, 0, 1].map(|dx| at(x + dx, y + 1));
                let mut color: [f32; 4] = std::array::from_fn(|c| {
                    let (dx, dy) = (right[c] - left[c], bottom[c] - top[c]);
                    let steepness = dx * dx + dy * dy;
                    if steepness < f32::EPSILON {
                        return middle[c];
                    }
                    let dxx = right[c] + left[c] - 2.0 * middle[c];
                    let dyy = bottom[c] + top[c] - 2.0 * middle[c];
                    let dxy = 0.25 * (bottom_right[c] - top_right[c] - bottom_left[c] + top_left[c]);
                    middle[c] + 0.25 * (dx * dx * dyy + dy * dy * dxx - 2.0 * dx * dy * dxy) / steepness
                });
                let alpha = color[3].clamp(0.0, 1.0);
                color[3] = alpha;
                for channel in &mut color[..3] {
                    *channel = channel.clamp(0.0, alpha);
                }
                *out = color;
            }
        });
        if !done {
            return None;
        }
        current = next;
    }
    Some(source.with_pixels(current))
}

/// Averages a disc of pixels, each row of it worked out from the sums of the row it
/// is on up to each pixel, raised to the power of `highlights` before and lowered
/// back after.
fn bokeh(source: &Buffer, radius: f32, highlights: f32, stop: &AtomicBool) -> Option<Buffer> {
    let source = source.with_pixels(source.pixels.iter().map(|&color| raised(color, highlights)).collect());
    let (width, height) = (source.width, source.height);
    let reach = radius.max(0.0) as usize;
    let half_widths = disc(radius);
    let count: usize = half_widths.iter().map(|half| half * 2 + 1).sum();
    let span = 2 * reach + 1;
    // Each row holds the sums up to each pixel, with as many pixels again past either edge as reach.
    let sums_width = width + 2 * reach + 1;
    let mut pixels = vec![[0.0; 4]; source.pixels.len()];
    fill_bands(&mut pixels, width, |first, band| {
        let mut sums = vec![[0.0f32; 4]; span * sums_width];
        let sum_row = |sums: &mut [[f32; 4]], y: isize| {
            let slot = y.rem_euclid(span as isize) as usize;
            let row = y.clamp(0, height as isize - 1) as usize;
            let line = &source.pixels[row * width..(row + 1) * width];
            let slot = &mut sums[slot * sums_width..(slot + 1) * sums_width];
            let mut total = [0.0; 4];
            slot[0] = total;
            for i in 0..width + 2 * reach {
                add(&mut total, line[i.saturating_sub(reach).min(width - 1)], 1.0);
                slot[i + 1] = total;
            }
        };
        for (i, row) in band.chunks_mut(width).enumerate() {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            let y = (first + i) as isize;
            let reach = reach as isize;
            if i == 0 {
                for row in y - reach..=y + reach {
                    sum_row(&mut sums, row);
                }
            } else {
                sum_row(&mut sums, y + reach);
            }
            for (x, out) in row.iter_mut().enumerate() {
                let mut sum = [0.0; 4];
                for (dy, &half) in (-reach..=reach).zip(&half_widths) {
                    let slot = (y + dy).rem_euclid(span as isize) as usize * sums_width;
                    let middle = x + reach as usize;
                    add(&mut sum, sums[slot + middle + half + 1], 1.0);
                    add(&mut sum, sums[slot + middle - half], -1.0);
                }
                *out = lowered(scale(sum, 1.0 / count as f32), highlights);
            }
        }
    });
    (!stop.load(Ordering::Relaxed)).then(|| source.with_pixels(pixels))
}

/// How far either side of the middle each row of a disc of pixels goes, from the top
/// row to the bottom one. Its edge is rounder for going half a pixel further than the
/// radius.
fn disc(radius: f32) -> Vec<usize> {
    let reach = radius.max(0.0) as isize;
    let limit = radius * radius + radius;
    (-reach..=reach)
        .map(|dy| {
            let left = limit - (dy * dy) as f32;
            let mut half = left.max(0.0).sqrt() as usize;
            while (half * half) as f32 > left {
                half -= 1;
            }
            half
        })
        .collect()
}

/// Each channel's value that this share, from 0 to 1, of the pixels within the
/// radius are no lighter than, kept as counts of each value the pixels along a row
/// have, which change as the disc moves along it. The colours are those of the pixels
/// that can be seen, so see-through ones do not darken those beside them.
fn median(image: &RgbaImage, radius: f32, percentile: f32, stop: &AtomicBool) -> Option<RgbaImage> {
    let (width, height) = (image.width() as usize, image.height() as usize);
    let reach = radius.max(0.0) as isize;
    let half_widths = disc(radius);
    let total: u32 = half_widths.iter().map(|&half| half as u32 * 2 + 1).sum();
    let wanted = |counted: u32| (percentile * (counted.max(1) - 1) as f32 + 0.5).floor() as u32 + 1;
    let pixel = |x: isize, y: isize| {
        image.get_pixel(x.clamp(0, width as isize - 1) as u32, y.clamp(0, height as isize - 1) as u32).0
    };
    let mut pixels = vec![[0u8; 4]; width * height];
    let done = fill_rows(&mut pixels, width, stop, |y, row| {
        let y = y as isize;
        let mut counts = Counts { values: Box::new([[0; 256]; 4]), seen: 0 };
        for (dy, &half) in (-reach..=reach).zip(&half_widths) {
            for dx in -(half as isize)..=half as isize {
                counts.tally(pixel(dx, y + dy), true);
            }
        }
        for (x, out) in row.iter_mut().enumerate() {
            let x = x as isize;
            if x > 0 {
                for (dy, &half) in (-reach..=reach).zip(&half_widths) {
                    counts.tally(pixel(x - 1 - half as isize, y + dy), false);
                    counts.tally(pixel(x + half as isize, y + dy), true);
                }
            }
            let alpha = counts.find(3, wanted(total));
            *out = if alpha == 0 {
                [0; 4]
            } else {
                let wanted = wanted(counts.seen);
                [counts.find(0, wanted), counts.find(1, wanted), counts.find(2, wanted), alpha]
            };
        }
    });
    done.then(|| RgbaImage::from_raw(width as u32, height as u32, pixels.into_flattened()).unwrap())
}

/// How many pixels of a disc have each value of red, green, blue and alpha, the
/// colours only of those that can be seen, and how many those are.
struct Counts {
    values: Box<[[u32; 256]; 4]>,
    seen: u32,
}

impl Counts {
    /// Counts a pixel in, or out.
    fn tally(&mut self, pixel: [u8; 4], add: bool) {
        let channels = if pixel[3] > 0 { 0..4 } else { 3..4 };
        for channel in channels {
            let count = &mut self.values[channel][pixel[channel] as usize];
            *count = if add { *count + 1 } else { *count - 1 };
        }
        if pixel[3] > 0 {
            self.seen = if add { self.seen + 1 } else { self.seen - 1 };
        }
    }

    /// The lowest value of the channel that this many pixels are at or under, or the
    /// highest if there are not that many.
    fn find(&self, channel: usize, wanted: u32) -> u8 {
        let mut under = 0;
        for (value, &count) in self.values[channel].iter().enumerate() {
            under += count;
            if under >= wanted {
                return value as u8;
            }
        }
        255
    }
}

/// Averages a short line of pixels, whose way drifts slowly across the image, as
/// coarse brush strokes would. The line is moved a little sideways, by as much for all
/// the pixels of a bristle, which runs along the stroke.
fn sketch(source: &Buffer, radius: f32, p: [f32; 2]) -> [f32; 4] {
    let cell = (radius * 2.0).max(4.0);
    let place = [p[0] / cell, p[1] / cell];
    let corner = place.map(f32::floor);
    let within = [0, 1].map(|axis| smoothstep(place[axis] - corner[axis]));
    let way_at = |dx: f32, dy: f32| stroke_way([corner[0] + dx, corner[1] + dy]);
    let mix = |a: [f32; 2], b: [f32; 2], t: f32| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
    let way = mix(
        mix(way_at(0.0, 0.0), way_at(1.0, 0.0), within[0]),
        mix(way_at(0.0, 1.0), way_at(1.0, 1.0), within[0]),
        within[1],
    );
    let turn = way[1].atan2(way[0]) / 2.0;
    let direction = [turn.cos(), turn.sin()];
    let across = [-direction[1], direction[0]];
    let bristle = ((p[0] * across[0] + p[1] * across[1]) / 1.5).floor();
    let shift = (hash([bristle, corner[0] + corner[1] * 101.0]) - 0.5) * radius * 0.5;
    let start = [p[0] + across[0] * shift, p[1] + across[1] * shift];
    let reach = radius.ceil() as i32;
    let mut sum = [0.0; 4];
    for i in -reach..=reach {
        let i = i as f32;
        add(&mut sum, source.sample([start[0] + direction[0] * i, start[1] + direction[1] * i]), 1.0);
    }
    scale(sum, 1.0 / (reach * 2 + 1) as f32)
}

fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn hash(cell: [f32; 2]) -> f32 {
    let value = (cell[0] * 12.9898 + cell[1] * 78.233).sin() * 43758.547;
    value - value.floor()
}

/// The way the strokes go around a corner of the Sketch Blur's grid, as twice its
/// angle: a line is the same whichever way along it is gone, and so is a line half a
/// turn round.
fn stroke_way(corner: [f32; 2]) -> [f32; 2] {
    let turn = hash(corner) * TAU;
    [turn.cos(), turn.sin()]
}

/// Works out each pixel from its middle, in pixels of the image.
fn per_pixel(source: &Buffer, stop: &AtomicBool, color: impl Fn([f32; 2]) -> [f32; 4] + Sync) -> Option<Buffer> {
    let mut pixels = vec![[0.0; 4]; source.pixels.len()];
    let done = fill_rows(&mut pixels, source.width, stop, |y, row| {
        for (x, out) in row.iter_mut().enumerate() {
            *out = color([x as f32 + 0.5, y as f32 + 0.5]);
        }
    });
    done.then(|| source.with_pixels(pixels))
}

/// Fills `items`, rows of `width` of them one after the other, each row from its
/// index, giving up between rows once `stop` is set. Returns whether it went through.
fn fill_rows<T: Send>(items: &mut [T], width: usize, stop: &AtomicBool, fill: impl Fn(usize, &mut [T]) + Sync) -> bool {
    fill_bands(items, width, |first, band| {
        for (index, row) in band.chunks_mut(width).enumerate() {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            fill(first + index, row);
        }
    });
    !stop.load(Ordering::Relaxed)
}

/// Fills `items`, rows of `width` of them one after the other, a band of rows on each
/// thread for many of them, each band from the index of its first row.
fn fill_bands<T: Send>(items: &mut [T], width: usize, fill: impl Fn(usize, &mut [T]) + Sync) {
    let rows = items.len() / width;
    let threads = thread::available_parallelism().map_or(1, NonZero::get);
    if threads == 1 || items.len() < PARALLEL_PIXELS {
        fill(0, items);
        return;
    }
    let band = rows.div_ceil(threads);
    let fill = &fill;
    thread::scope(|scope| {
        for (index, items) in items.chunks_mut(band * width).enumerate() {
            scope.spawn(move || fill(index * band, items));
        }
    });
}

/// Colours from 0 to 1, multiplied by how opaque they are, row after row.
struct Buffer {
    width: usize,
    height: usize,
    pixels: Vec<[f32; 4]>,
}

impl Buffer {
    fn premultiplied(image: &RgbaImage) -> Self {
        let pixels = image.pixels().map(|&pixel| premultiply(pixel)).collect();
        Self { width: image.width() as usize, height: image.height() as usize, pixels }
    }

    fn with_pixels(&self, pixels: Vec<[f32; 4]>) -> Self {
        Self { width: self.width, height: self.height, pixels }
    }

    /// The colour at a point of the image, read between the four pixels around it.
    fn sample(&self, [x, y]: [f32; 2]) -> [f32; 4] {
        let along = |at: f32, size: usize| {
            let at = at.clamp(0.5, size as f32 - 0.5) - 0.5;
            let below = (at.floor() as usize).min(size - 1);
            (below, (below + 1).min(size - 1), at - below as f32)
        };
        let (left, right, tx) = along(x, self.width);
        let (top, bottom, ty) = along(y, self.height);
        let at = |x: usize, y: usize| self.pixels[y * self.width + x];
        let mut color = [0.0; 4];
        add(&mut color, at(left, top), (1.0 - tx) * (1.0 - ty));
        add(&mut color, at(right, top), tx * (1.0 - ty));
        add(&mut color, at(left, bottom), (1.0 - tx) * ty);
        add(&mut color, at(right, bottom), tx * ty);
        color
    }

    /// Its columns as rows.
    fn transposed(&self) -> Self {
        Self { width: self.height, height: self.width, pixels: Self::transposed_items(&self.pixels, self.width, self.height) }
    }

    /// The columns of `items`, rows of `width` of them one after the other, as rows.
    fn transposed_items<T: Copy>(items: &[T], width: usize, height: usize) -> Vec<T> {
        let mut transposed = Vec::with_capacity(items.len());
        for x in 0..width {
            transposed.extend((0..height).map(|y| items[y * width + x]));
        }
        transposed
    }

    /// The pixels, their colours divided by how opaque they are again. Those that
    /// would be stored as see-through are left without a colour.
    fn to_image(&self) -> RgbaImage {
        let bytes = self
            .pixels
            .iter()
            .flat_map(|&[r, g, b, a]| {
                let color = if a >= MIN_ALPHA { [r / a, g / a, b / a, a] } else { [0.0; 4] };
                color.map(|channel| (channel * 255.0).round().clamp(0.0, 255.0) as u8)
            })
            .collect();
        RgbaImage::from_raw(self.width as u32, self.height as u32, bytes).unwrap()
    }
}

/// The colour from 0 to 1, multiplied by how opaque it is.
fn premultiply(pixel: Rgba<u8>) -> [f32; 4] {
    let [r, g, b, a] = pixel.0.map(|channel| channel as f32 / 255.0);
    [r * a, g * a, b * a, a]
}

/// The channel, from 0 to 1, taken this share of the way to 1, or to 0 below zero.
fn lightened(channel: f32, amount: f32) -> f32 {
    if amount >= 0.0 { channel + (1.0 - channel) * amount } else { channel * (1.0 + amount) }
}

fn add(sum: &mut [f32; 4], color: [f32; 4], weight: f32) {
    for (sum, channel) in sum.iter_mut().zip(color) {
        *sum += channel * weight;
    }
}

fn scale(color: [f32; 4], factor: f32) -> [f32; 4] {
    color.map(|channel| channel * factor)
}

/// The colour with red, green and blue raised to this power, which makes bright ones
/// stand out once averaged.
fn raised([r, g, b, a]: [f32; 4], power: f32) -> [f32; 4] {
    if power == 1.0 { [r, g, b, a] } else { [r.powf(power), g.powf(power), b.powf(power), a] }
}

/// The colour with red, green and blue lowered back from this power.
fn lowered([r, g, b, a]: [f32; 4], power: f32) -> [f32; 4] {
    if power == 1.0 {
        [r, g, b, a]
    } else {
        let lower = |channel: f32| channel.max(0.0).powf(1.0 / power);
        [lower(r), lower(g), lower(b), a]
    }
}

#[cfg(test)]
mod tests {
    use image::Rgba;

    use super::*;
    use crate::{BlendMode, Layer, PixelRect, Selection, composite};

    /// One of each effect, with settings that change something.
    fn all() -> Vec<Effect> {
        vec![
            Effect::BokehBlur { radius: 4.0, highlights: 50.0 },
            Effect::FocusBlur { radius: 6.0, shape: FocusShape::Circle, center: [10.0, 0.0], size: 10.0, transition: 40.0 },
            Effect::FragmentBlur { count: 4, distance: 3.0, rotation: 10.0 },
            Effect::GaussianBlur { radius: 3.0 },
            Effect::MeanCurvatureBlur { iterations: 5 },
            Effect::MedianBlur { radius: 3.0, percentile: 50.0 },
            Effect::MotionBlur { angle: 25.0, distance: 6.0, centered: true },
            Effect::RadialBlur { angle: 10.0, center: [0.0, 0.0] },
            Effect::SketchBlur { radius: 4.0 },
            Effect::SquareBlur { radius: 3.0, highlights: 30.0 },
            Effect::SurfaceBlur { radius: 3.0, threshold: 15.0 },
            Effect::TileableBlur { radius: 3.0 },
            Effect::VariableBlur { radius: 4.0, map: white_map(300, 300), invert: false },
            Effect::ZoomBlur { amount: 20.0, center: [10.0, -20.0] },
            Effect::Sharpen { radius: 3.0, amount: 100.0, threshold: 0.0 },
            Effect::Antialias,
            Effect::Deinterlace { keep_odd: false, vertical: false, size: 1 },
            Effect::Deinterlace { keep_odd: true, vertical: true, size: 3 },
            Effect::ReduceNoise { radius: 3.0, strength: 40.0 },
        ]
    }

    /// The effects of the Enhance menu that change even colours.
    fn enhancing() -> Vec<Effect> {
        vec![Effect::HighPass { radius: 4.0, contrast: 100.0 }, Effect::RedEyeRemoval { threshold: 50.0 }]
    }

    /// One of each effect of the Distort menu that leaves even colours as they are.
    fn distorting() -> Vec<Effect> {
        vec![
            Effect::Bulge { amount: 50.0, radius: 100.0, center: [10.0, 0.0] },
            Effect::Twist { angle: 90.0, pinch: 30.0, radius: 100.0, center: [0.0, 0.0] },
            Effect::Crystallize { size: 8.0 },
            Effect::Pixelate { width: 4, height: 3 },
            Effect::TileReflection { size: 20.0, angle: 30.0, curvature: 8.0 },
            Effect::Kaleidoscope { segments: 5, angle: 20.0, center: [0.0, 0.0] },
            Effect::Ripple { amplitude: 4.0, wavelength: 20.0, phase: 0.0, vertical: false, shape: WaveShape::Sine },
            Effect::Ripple { amplitude: 4.0, wavelength: 20.0, phase: 90.0, vertical: true, shape: WaveShape::Sawtooth },
            Effect::Waves { amplitude: 4.0, wavelength: 20.0, phase: 0.0, center: [0.0, 0.0] },
            Effect::Wind { strength: 20.0, threshold: 10.0, direction: WindDirection::Left, blast: false },
            Effect::Wind { strength: 20.0, threshold: 10.0, direction: WindDirection::Up, blast: true },
        ]
    }

    /// The effects of the Distort menu that change even colours, or leave nothing where
    /// the image does not reach.
    fn reshaping() -> Vec<Effect> {
        vec![
            Effect::PolarCoordinates { to_polar: true, rotation: 0.0 },
            Effect::PolarCoordinates { to_polar: false, rotation: 0.0 },
            Effect::LensDistortion { main: 25.0, edge: 10.0, zoom: -20.0, center: [0.0, 0.0] },
            Effect::Newsprint { size: 8.0, angle: 0.0, screen: NewsprintScreen::Cmyk },
        ]
    }

    /// One of each effect of the Artistic menu, most of which change even colours too.
    fn artistic() -> Vec<Effect> {
        vec![
            Effect::ApplyCanvas { azimuth: 135.0, depth: 4.0 },
            Effect::Cartoon { radius: 7.0, black: 20.0 },
            Effect::Clothify { blur: [9.0, 9.0], azimuth: 135.0, elevation: 45.0, depth: 3.0 },
            Effect::Cubism { size: 10.0, saturation: 2.5, background: [0, 0, 0, 0] },
            Effect::GlassTile { width: 25.0, height: 25.0 },
            Effect::Glow { radius: 6.0, brightness: 10.0, contrast: 10.0 },
            Effect::InkSketch { ink: 50.0, coloring: 50.0 },
            Effect::OilPainting { radius: 3.0, levels: 50 },
            Effect::PencilSketch { tip: 2.0, range: 0.0 },
            Effect::Photocopy { radius: 8.0, sharpness: 50.0, black: 20.0, white: 20.0 },
            Effect::Predator { amount: 2.0, pixel_size: 3 },
            Effect::Slic { size: 8, compactness: 20.0, iterations: 2 },
            Effect::VanGogh { length: 6.0, strength: 50.0 },
            Effect::Waterpixels { size: 8, smoothness: 1.0, regularization: 0.0, random: false },
            Effect::Weave { width: 12.0, spacing: 4.0, darkness: 75.0, depth: 75.0, threads: 50.0 },
        ]
    }

    /// One of each effect of the Render menu.
    fn rendering() -> Vec<Effect> {
        let colors = [[0, 0, 0, 255], [255, 255, 255, 255]];
        let mut effects: Vec<Effect> = NoiseKind::ALL
            .into_iter()
            .map(|kind| Effect::CloudsAndNoise { noise: noise(kind), colors, blend_mode: BlendMode::Difference, plasma_colors: true })
            .collect();
        effects.extend([
            Effect::Fractal {
                kind: FractalKind::Julia,
                iterations: 50,
                zoom: 100.0,
                center: [0.0, 0.0],
                julia: [-0.8, 0.156],
                rainbow: true,
                colors,
                antialias: true,
            },
            Effect::BayerMatrix { level: 3, pixel_size: 2, colors },
            Effect::Checkerboard { size: 8, colors },
            Effect::Grid { width: 10, height: 12, line: 2, offset: [3, 0], color: [255, 0, 0, 255] },
            Effect::Maze { size: 3, seed: 1, branching: false, colors },
            Effect::SpeedLines { count: 60, thickness: 40.0, clear: 30.0, randomness: 50.0, center: [0.0, 0.0], seed: 1, color: colors[0] },
        ]);
        effects
    }

    /// One of each effect of the Stylize menu, and each edge algorithm.
    fn stylizing() -> Vec<Effect> {
        let mut effects: Vec<Effect> = EdgeAlgorithm::ALL.into_iter().map(edges).collect();
        effects.extend([
            Effect::Emboss { azimuth: 135.0, elevation: 45.0, depth: 3.0 },
            Effect::Relief { azimuth: 135.0, elevation: 45.0, depth: 3.0 },
            Effect::Outline { thickness: 3.0, intensity: 50.0 },
        ]);
        effects
    }

    /// One of each effect of the Noise menu, and each colour space.
    fn noisy() -> Vec<Effect> {
        let mut effects: Vec<Effect> = NoiseColorSpace::ALL.into_iter().map(|color_space| add_noise(color_space, 100.0, 1)).collect();
        effects.extend([spread(0.0), spread(100.0)]);
        effects
    }

    /// One of each effect of the Decor menu, and the Soften Portrait.
    fn decorating() -> Vec<Effect> {
        vec![
            Effect::AddBevel { width: 4.0, depth: 50.0, azimuth: 135.0 },
            Effect::AddBorder { size: 3.0, color: [255, 0, 0, 255], shading: 25.0 },
            Effect::RoundCorners { radius: 6.0 },
            Effect::SoftenPortrait { softness: 3.0, lighting: 5.0, warmth: 10.0 },
        ]
    }

    /// One of each effect of the Light menu.
    fn lights() -> Vec<Effect> {
        let black = [0, 0, 0, 255];
        vec![
            Effect::DropShadow { angle: 315.0, distance: 4.0, blur: 3.0, opacity: 60.0, color: black },
            Effect::LongShadow { angle: 315.0, length: 20.0, opacity: 40.0, color: black, fade: true },
            Effect::Vignette { amount: 60.0, radius: 100.0, softness: 60.0, center: [0.0, 0.0], color: black },
            Effect::LensFlare { center: [-50.0, -50.0], size: 20.0, brightness: 100.0, rays: 6, ghosts: true },
            lighting(LightSource::Spotlight, 0.0),
            lighting(LightSource::Directional, 5.0),
            Effect::Sparkle { threshold: 0.0, size: 6.0, points: 4, angle: 0.0, amount: 100.0, seed: 1 },
        ]
    }

    /// A white Lighting Effects from the light source, with these bumps.
    fn lighting(light: LightSource, bumps: f32) -> Effect {
        Effect::LightingEffects {
            light,
            center: [0.0, 0.0],
            height: 50.0,
            azimuth: 135.0,
            elevation: 45.0,
            intensity: 100.0,
            ambient: 20.0,
            shine: 20.0,
            bumps,
            color: [255, 255, 255, 255],
        }
    }

    /// An Add Noise in the colour space, half saturated, over `coverage` percent of the pixels.
    fn add_noise(color_space: NoiseColorSpace, coverage: f32, seed: u32) -> Effect {
        Effect::AddNoise { intensity: 64.0, saturation: 50.0, coverage, color_space, seed }
    }

    /// A Spread 4 pixels far, `downward` percent of the way to melting.
    fn spread(downward: f32) -> Effect {
        Effect::Spread { amount: 4.0, downward, seed: 2 }
    }

    /// An Edge Detect with the algorithm, both ways and two radii, not normalized.
    fn edges(algorithm: EdgeAlgorithm) -> Effect {
        edge_detect(algorithm, [true, true], false, false)
    }

    /// An Edge Detect with the algorithm, across and down as `ways` says, and two radii.
    fn edge_detect(algorithm: EdgeAlgorithm, [horizontal, vertical]: [bool; 2], normalize: bool, invert: bool) -> Effect {
        Effect::EdgeDetect { algorithm, horizontal, vertical, radii: [1.0, 3.0], amount: 100.0, normalize, invert }
    }

    fn noise(kind: NoiseKind) -> Noise {
        Noise { kind, scale: 40.0, detail: 4, roughness: 50.0, seed: 3, cells: CellStyle::Edges, jitter: 100.0 }
    }

    fn white_map(width: u32, height: u32) -> Arc<RgbaImage> {
        Arc::new(RgbaImage::from_pixel(width, height, Rgba([255, 255, 255, 255])))
    }

    fn apply(pixels: &RgbaImage, effect: &Effect) -> RgbaImage {
        apply_effect(pixels, effect, None, &AtomicBool::new(false)).unwrap()
    }

    /// A white dot in the middle of black.
    fn dot() -> RgbaImage {
        let mut pixels = RgbaImage::from_pixel(21, 21, Rgba([0, 0, 0, 255]));
        pixels.put_pixel(10, 10, Rgba([255, 255, 255, 255]));
        pixels
    }

    #[test]
    fn an_even_colour_stays_as_it_is() {
        for pixels in [RgbaImage::from_pixel(40, 30, Rgba([200, 100, 50, 180])), RgbaImage::new(40, 30)] {
            let relief = Effect::Relief { azimuth: 135.0, elevation: 45.0, depth: 3.0 };
            for effect in &[all(), distorting(), vec![relief, spread(0.0), spread(100.0)]].concat() {
                assert_eq!(apply(&pixels, effect), pixels, "{effect:?}");
            }
        }
    }

    #[test]
    fn settings_that_change_nothing_leave_the_pixels_alone() {
        let pixels = dot();
        let unchanged = [
            Effect::GaussianBlur { radius: 0.0 },
            Effect::RadialBlur { angle: 0.0, center: [0.0, 0.0] },
            Effect::ZoomBlur { amount: 0.0, center: [0.0, 0.0] },
            Effect::Sharpen { radius: 3.0, amount: 0.0, threshold: 0.0 },
            Effect::FragmentBlur { count: 4, distance: 0.0, rotation: 0.0 },
            Effect::MotionBlur { angle: 0.0, distance: 0.0, centered: true },
            Effect::MeanCurvatureBlur { iterations: 0 },
            Effect::TileableBlur { radius: 0.0 },
            Effect::VariableBlur { radius: 5.0, map: Arc::new(RgbaImage::new(21, 21)), invert: false },
            Effect::VariableBlur { radius: 5.0, map: white_map(21, 21), invert: true },
            Effect::FocusBlur { radius: 5.0, shape: FocusShape::Square, center: [0.0, 0.0], size: 100.0, transition: 0.0 },
            Effect::AddNoise { intensity: 0.0, saturation: 50.0, coverage: 100.0, color_space: NoiseColorSpace::Rgb, seed: 0 },
            add_noise(NoiseColorSpace::Lch, 0.0, 0),
            Effect::Spread { amount: 0.0, downward: 50.0, seed: 0 },
            Effect::SoftenPortrait { softness: 0.0, lighting: 0.0, warmth: 0.0 },
            Effect::Straighten { angle: 0.0, fill: true, interpolation: Interpolation::Bilinear },
            Effect::AddBevel { width: 5.0, depth: 0.0, azimuth: 135.0 },
            Effect::DropShadow { angle: 0.0, distance: 5.0, blur: 2.0, opacity: 0.0, color: [0, 0, 0, 255] },
            Effect::Vignette { amount: 0.0, radius: 100.0, softness: 50.0, center: [0.0, 0.0], color: [0, 0, 0, 255] },
            Effect::LensFlare { center: [0.0, 0.0], size: 50.0, brightness: 0.0, rays: 4, ghosts: true },
            Effect::Sparkle { threshold: 50.0, size: 10.0, points: 4, angle: 0.0, amount: 0.0, seed: 0 },
        ];
        for effect in unchanged {
            assert_eq!(apply(&pixels, &effect), pixels, "{effect:?}");
        }
    }

    #[test]
    fn blurs_spread_a_dot_evenly() {
        for effect in [
            Effect::GaussianBlur { radius: 3.0 },
            Effect::SquareBlur { radius: 3.0, highlights: 0.0 },
            Effect::BokehBlur { radius: 3.0, highlights: 0.0 },
        ] {
            let blurred = apply(&dot(), &effect);
            let middle = blurred[(10, 10)][0];
            assert!(middle > 0 && middle < 255, "{effect:?}");
            assert!(blurred[(12, 10)][0] > 0, "{effect:?}");
            assert_eq!(blurred[(12, 10)], blurred[(8, 10)], "{effect:?}");
            assert_eq!(blurred[(12, 10)], blurred[(10, 12)], "{effect:?}");
            assert_eq!(blurred[(0, 0)], Rgba([0, 0, 0, 255]), "{effect:?}");
        }
        let square = apply(&dot(), &Effect::SquareBlur { radius: 1.0, highlights: 0.0 });
        assert_eq!(square[(11, 11)], Rgba([28, 28, 28, 255]), "a ninth of white");
    }

    #[test]
    fn a_line_blur_follows_its_way() {
        let blurred = apply(&dot(), &Effect::MotionBlur { angle: 0.0, distance: 4.0, centered: true });
        assert_eq!(blurred[(12, 10)][0], 51, "a fifth of white");
        assert_eq!(blurred[(10, 12)][0], 0);
        let behind = apply(&dot(), &Effect::MotionBlur { angle: 0.0, distance: 4.0, centered: false });
        // Each pixel averages those behind it, on its left, so the dot spreads to the right.
        assert!(behind[(12, 10)][0] > 0);
        assert_eq!(behind[(8, 10)][0], 0);
    }

    #[test]
    fn see_through_pixels_do_not_darken_their_neighbours() {
        let mut pixels = RgbaImage::new(9, 9);
        for x in 0..4 {
            for y in 0..9 {
                pixels.put_pixel(x, y, Rgba([255, 0, 0, 255]));
            }
        }
        for effect in [Effect::GaussianBlur { radius: 2.0 }, Effect::MedianBlur { radius: 2.0, percentile: 50.0 }] {
            let blurred = apply(&pixels, &effect);
            let edge = blurred[(4, 4)];
            assert!(edge[3] == 0 || edge.0[..3] == [255, 0, 0], "{effect:?}: {edge:?}");
            assert_eq!(blurred[(3, 4)].0[..3], [255, 0, 0], "{effect:?}");
        }
    }

    #[test]
    fn a_median_takes_out_specks() {
        assert_eq!(apply(&dot(), &Effect::MedianBlur { radius: 1.0, percentile: 50.0 })[(10, 10)], Rgba([0, 0, 0, 255]));
        let lightest = apply(&dot(), &Effect::MedianBlur { radius: 1.0, percentile: 100.0 });
        assert_eq!(lightest[(11, 11)], Rgba([255, 255, 255, 255]));
        assert_eq!(lightest[(12, 12)], Rgba([0, 0, 0, 255]));
    }

    #[test]
    fn sharpening_brings_out_the_dot() {
        let mut pixels = RgbaImage::from_pixel(21, 21, Rgba([100, 100, 100, 255]));
        pixels.put_pixel(10, 10, Rgba([150, 150, 150, 255]));
        let sharp = apply(&pixels, &Effect::Sharpen { radius: 2.0, amount: 100.0, threshold: 0.0 });
        assert!(sharp[(10, 10)][0] > 150);
        assert!(sharp[(11, 10)][0] < 100);
        assert_eq!(sharp[(0, 0)], Rgba([100, 100, 100, 255]));
        let kept = apply(&pixels, &Effect::Sharpen { radius: 2.0, amount: 100.0, threshold: 50.0 });
        assert_eq!(kept, pixels, "no difference reaches the threshold");
    }

    #[test]
    fn only_the_selection_changes() {
        let pixels = RgbaImage::from_fn(200, 200, |x, y| Rgba([(x % 2 * 255) as u8, (y % 2 * 255) as u8, 0, 255]));
        let selection = Selection::rect(PixelRect { left: 10, top: 150, right: 20, bottom: 160 });
        let mask = SelectionMask::of(&selection, 200, 200).unwrap();
        for effect in &[all(), artistic(), enhancing(), distorting(), reshaping(), rendering(), stylizing(), noisy(), decorating(), lights()].concat() {
            let changed = apply_effect(&pixels, effect, Some(&mask), &AtomicBool::new(false)).unwrap();
            assert_eq!(changed[(5, 155)], pixels[(5, 155)], "{effect:?}");
            assert_eq!(changed[(15, 100)], pixels[(15, 100)], "{effect:?}");
        }
    }

    #[test]
    fn gives_up_once_stopped() {
        let pixels = RgbaImage::from_pixel(300, 300, Rgba([1, 2, 3, 255]));
        for effect in &[all(), artistic(), enhancing(), distorting(), reshaping(), rendering(), stylizing(), noisy(), decorating(), lights()].concat() {
            assert!(apply_effect(&pixels, effect, None, &AtomicBool::new(true)).is_none(), "{effect:?}");
        }
    }

    #[test]
    fn offset_moves_the_pixels_within_the_selection() {
        // A row of four colours, with a pixel either side left out of the selection.
        let colors = [10, 20, 30, 40, 50, 60].map(|value| Rgba([value, 0, 0, 255]));
        let pixels = RgbaImage::from_fn(6, 1, |x, _| colors[x as usize]);
        let mask = SelectionMask::of(&Selection::rect(PixelRect { left: 1, top: 0, right: 5, bottom: 1 }), 6, 1);
        let reds = |effect: Effect| -> Vec<u8> {
            let result = apply_effect(&pixels, &effect, mask.as_ref(), &AtomicBool::new(false)).unwrap();
            result.pixels().map(|pixel| if pixel[3] == 0 { 0 } else { pixel[0] }).collect()
        };
        assert_eq!(reds(Effect::Offset { x: 1, y: 0, edges: OffsetEdges::Wrap }), [10, 50, 20, 30, 40, 60]);
        assert_eq!(reds(Effect::Offset { x: -1, y: 0, edges: OffsetEdges::Transparent }), [10, 30, 40, 50, 0, 60]);
        assert_eq!(reds(Effect::Offset { x: 2, y: 0, edges: OffsetEdges::Repeat }), [10, 20, 20, 20, 30, 60]);
        // A whole turn leaves it as it was.
        assert_eq!(reds(Effect::Offset { x: 4, y: 0, edges: OffsetEdges::Wrap }), [10, 20, 30, 40, 50, 60]);
    }

    #[test]
    fn a_variable_blur_with_an_even_map_is_much_as_a_gaussian_blur() {
        let gaussian = apply(&dot(), &Effect::GaussianBlur { radius: 6.0 });
        let variable = apply(&dot(), &Effect::VariableBlur { radius: 6.0, map: white_map(21, 21), invert: false });
        for (a, b) in gaussian.pixels().zip(variable.pixels()) {
            assert!(a[0].abs_diff(b[0]) <= 3, "{a:?} {b:?}");
        }
        assert_eq!(variable[(12, 10)], variable[(10, 12)]);
    }

    #[test]
    fn a_variable_blur_blurs_only_where_the_map_is_light() {
        let mut pixels = RgbaImage::from_pixel(41, 21, Rgba([0, 0, 0, 255]));
        pixels.put_pixel(5, 10, Rgba([255, 255, 255, 255]));
        pixels.put_pixel(35, 10, Rgba([255, 255, 255, 255]));
        // Light on the right half only.
        let map = Arc::new(RgbaImage::from_fn(41, 21, |x, _| if x > 20 { Rgba([255; 4]) } else { Rgba([0, 0, 0, 255]) }));
        let blurred = apply(&pixels, &Effect::VariableBlur { radius: 4.0, map: Arc::clone(&map), invert: false });
        assert_eq!(blurred[(5, 10)], pixels[(5, 10)]);
        assert!(blurred[(35, 10)][0] < 255 && blurred[(36, 10)][0] > 0);
        let inverted = apply(&pixels, &Effect::VariableBlur { radius: 4.0, map, invert: true });
        assert_eq!(inverted[(35, 10)], pixels[(35, 10)]);
        assert!(inverted[(5, 10)][0] < 255 && inverted[(6, 10)][0] > 0);
    }

    #[test]
    fn a_focus_blur_leaves_its_centre_sharp() {
        let pixels = RgbaImage::from_fn(60, 60, |x, y| Rgba([((x + y) % 2 * 255) as u8, 0, 0, 255]));
        for shape in FocusShape::ALL {
            let effect = Effect::FocusBlur { radius: 5.0, shape, center: [0.0, 0.0], size: 20.0, transition: 30.0 };
            let blurred = apply(&pixels, &effect);
            assert_eq!(blurred[(30, 30)], pixels[(30, 30)], "{shape:?}");
            let corner = blurred[(1, 1)][0];
            assert!(corner > 80 && corner < 180, "{shape:?}: {corner}");
        }
        // A band across leaves the whole of its middle row sharp.
        let band = apply(&pixels, &Effect::FocusBlur { radius: 5.0, shape: FocusShape::Horizontal, center: [0.0, 0.0], size: 20.0, transition: 30.0 });
        assert_eq!(band[(1, 30)], pixels[(1, 30)]);
    }

    #[test]
    fn a_mean_curvature_blur_rounds_off_specks_but_keeps_straight_edges() {
        let speck = RgbaImage::from_fn(21, 21, |x, y| {
            if (9..12).contains(&x) && (9..12).contains(&y) { Rgba([255, 255, 255, 255]) } else { Rgba([0, 0, 0, 255]) }
        });
        let smoothed = apply(&speck, &Effect::MeanCurvatureBlur { iterations: 10 });
        assert!(smoothed[(9, 9)][0] < 128, "the corners go: {:?}", smoothed[(9, 9)]);
        let light = |pixels: &RgbaImage| pixels.pixels().map(|pixel| pixel[0] as u32).sum::<u32>();
        assert!(light(&smoothed) < light(&speck));
        let edge = RgbaImage::from_fn(20, 20, |x, _| if x < 10 { Rgba([0, 0, 0, 255]) } else { Rgba([255, 255, 255, 255]) });
        assert_eq!(apply(&edge, &Effect::MeanCurvatureBlur { iterations: 10 }), edge);
    }

    #[test]
    fn a_tileable_blur_goes_on_past_each_edge_with_the_other() {
        let mut pixels = RgbaImage::from_pixel(21, 21, Rgba([0, 0, 0, 255]));
        for y in 0..21 {
            pixels.put_pixel(0, y, Rgba([255, 255, 255, 255]));
        }
        let blurred = apply(&pixels, &Effect::TileableBlur { radius: 3.0 });
        assert!(blurred[(20, 5)][0] > 0);
        assert_eq!(blurred[(20, 5)], blurred[(1, 5)]);
        // Within the selection, the far edge of the selection is what goes on past it.
        let selection = Selection::rect(PixelRect { left: 0, top: 0, right: 11, bottom: 21 });
        let mask = SelectionMask::of(&selection, 21, 21).unwrap();
        let within = apply_effect(&pixels, &Effect::TileableBlur { radius: 3.0 }, Some(&mask), &AtomicBool::new(false)).unwrap();
        assert_eq!(within[(10, 5)], blurred[(20, 5)]);
        assert_eq!(within[(20, 5)], pixels[(20, 5)]);
    }

    #[test]
    fn artistic_effects_leave_see_through_pixels_see_through() {
        let pixels = RgbaImage::new(40, 30);
        for effect in &artistic() {
            assert_eq!(apply(&pixels, effect), pixels, "{effect:?}");
        }
    }

    #[test]
    fn artistic_effects_that_only_move_colours_keep_an_even_one() {
        let pixels = RgbaImage::from_pixel(40, 30, Rgba([200, 100, 50, 180]));
        for effect in [
            Effect::Cartoon { radius: 7.0, black: 20.0 },
            Effect::GlassTile { width: 12.0, height: 10.0 },
            Effect::OilPainting { radius: 3.0, levels: 50 },
            Effect::Slic { size: 8, compactness: 20.0, iterations: 2 },
            Effect::Waterpixels { size: 8, smoothness: 1.0, regularization: 5.0, random: false },
            Effect::Cubism { size: 4.0, saturation: 3.0, background: [200, 100, 50, 180] },
        ] {
            assert_eq!(apply(&pixels, &effect), pixels, "{effect:?}");
        }
    }

    #[test]
    fn sketches_and_photocopies_draw_dark_lines_on_white() {
        // A dark line down a light grey image.
        let pixels = RgbaImage::from_fn(41, 21, |x, _| if x == 20 { Rgba([20, 20, 20, 255]) } else { Rgba([200, 200, 200, 255]) });
        for effect in [
            Effect::PencilSketch { tip: 2.0, range: 0.0 },
            Effect::Photocopy { radius: 8.0, sharpness: 50.0, black: 5.0, white: 20.0 },
            Effect::InkSketch { ink: 100.0, coloring: 0.0 },
        ] {
            let drawn = apply(&pixels, &effect);
            assert_eq!(drawn[(2, 10)], Rgba([255, 255, 255, 255]), "{effect:?}");
            assert!(drawn[(20, 10)][0] < 128 || drawn[(19, 10)][0] < 128, "{effect:?}: {:?}", drawn[(20, 10)]);
        }
        let photocopy = apply(&pixels, &Effect::Photocopy { radius: 8.0, sharpness: 50.0, black: 5.0, white: 20.0 });
        assert_eq!(photocopy[(20, 10)], Rgba([0, 0, 0, 255]));
    }

    #[test]
    fn a_cartoon_outlines_what_is_darker_than_around_it() {
        let pixels = RgbaImage::from_fn(41, 21, |x, _| if x < 20 { Rgba([60, 60, 200, 255]) } else { Rgba([240, 240, 100, 255]) });
        let cartoon = apply(&pixels, &Effect::Cartoon { radius: 7.0, black: 10.0 });
        // The darker side of the edge goes black, the lighter one and those far from it stay.
        assert_eq!(cartoon[(19, 10)], Rgba([0, 0, 0, 255]));
        assert_eq!(cartoon[(21, 10)], pixels[(21, 10)]);
        assert_eq!(cartoon[(2, 10)], pixels[(2, 10)]);
    }

    #[test]
    fn clusters_keep_to_the_edges_between_colours() {
        let pixels = RgbaImage::from_fn(32, 16, |x, _| if x < 16 { Rgba([200, 30, 30, 255]) } else { Rgba([30, 30, 200, 255]) });
        for effect in [
            Effect::Slic { size: 8, compactness: 20.0, iterations: 3 },
            Effect::Waterpixels { size: 8, smoothness: 1.0, regularization: 0.0, random: false },
        ] {
            assert_eq!(apply(&pixels, &effect), pixels, "{effect:?}");
        }
        let random = apply(&pixels, &Effect::Waterpixels { size: 8, smoothness: 1.0, regularization: 0.0, random: true });
        assert_ne!(random[(0, 0)], random[(31, 0)]);
    }

    #[test]
    fn a_cubism_without_rectangles_is_all_background() {
        let pixels = dot();
        let background = [10, 20, 30, 255];
        let cubism = apply(&pixels, &Effect::Cubism { size: 5.0, saturation: 0.0, background });
        assert!(cubism.pixels().all(|pixel| pixel.0 == background));
    }

    #[test]
    fn a_predator_sees_only_edges() {
        let predator = apply(&dot(), &Effect::Predator { amount: 2.0, pixel_size: 1 });
        assert_eq!(predator[(2, 2)], Rgba([0, 0, 0, 255]));
        // A white dot's edges have all three channels as strong.
        let edge = predator[(11, 10)];
        assert!(edge[0] > 0 && edge[0] == edge[1] && edge[1] == edge[2], "{edge:?}");
    }

    #[test]
    fn textures_keep_how_opaque_each_pixel_is() {
        let pixels = RgbaImage::from_fn(40, 40, |x, y| Rgba([180, 120, 60, (x * 6 + y) as u8]));
        for effect in [
            Effect::ApplyCanvas { azimuth: 45.0, depth: 10.0 },
            Effect::Clothify { blur: [9.0, 3.0], azimuth: 135.0, elevation: 30.0, depth: 5.0 },
            Effect::Weave { width: 10.0, spacing: 3.0, darkness: 75.0, depth: 75.0, threads: 50.0 },
        ] {
            let textured = apply(&pixels, &effect);
            assert!(textured.pixels().zip(pixels.pixels()).all(|(a, b)| a[3] == b[3]), "{effect:?}");
            assert_ne!(textured, pixels, "{effect:?}");
        }
    }

    #[test]
    fn reducing_noise_smooths_specks_but_keeps_edges() {
        let mut pixels = RgbaImage::from_fn(21, 21, |x, _| if x < 10 { Rgba([40, 40, 40, 255]) } else { Rgba([220, 220, 220, 255]) });
        pixels.put_pixel(4, 10, Rgba([60, 60, 60, 255]));
        let smoothed = apply(&pixels, &Effect::ReduceNoise { radius: 3.0, strength: 40.0 });
        assert!(smoothed[(4, 10)][0] < 60, "{:?}", smoothed[(4, 10)]);
        assert!(smoothed[(9, 10)][0] < 50 && smoothed[(10, 10)][0] > 210, "{:?} {:?}", smoothed[(9, 10)], smoothed[(10, 10)]);
    }

    #[test]
    fn red_eye_removal_takes_out_red_only() {
        let mut pixels = RgbaImage::from_pixel(4, 4, Rgba([200, 40, 40, 255]));
        pixels.put_pixel(0, 0, Rgba([40, 160, 200, 255]));
        let removed = apply(&pixels, &Effect::RedEyeRemoval { threshold: 50.0 });
        assert!(removed[(1, 1)][0] < 100, "{:?}", removed[(1, 1)]);
        assert_eq!(removed[(1, 1)].0[1..], [40, 40, 255]);
        assert_eq!(removed[(0, 0)], pixels[(0, 0)]);
    }

    #[test]
    fn antialias_rounds_off_stairs() {
        // A diagonal: white below it, black above.
        let pixels = RgbaImage::from_fn(10, 10, |x, y| if x <= y { Rgba([255, 255, 255, 255]) } else { Rgba([0, 0, 0, 255]) });
        let smoothed = apply(&pixels, &Effect::Antialias);
        let step = smoothed[(5, 5)][0];
        assert!(step > 0 && step < 255, "{step}");
        assert_eq!(smoothed[(1, 8)], pixels[(1, 8)]);
        assert_eq!(smoothed[(8, 1)], pixels[(8, 1)]);
        // Straight edges have no stairs.
        let edge = RgbaImage::from_fn(10, 10, |x, _| if x < 5 { Rgba([0, 0, 0, 255]) } else { Rgba([255, 255, 255, 255]) });
        assert_eq!(apply(&edge, &Effect::Antialias), edge);
    }

    #[test]
    fn deinterlacing_fills_the_other_rows_from_those_around() {
        let pixels = RgbaImage::from_fn(3, 6, |_, y| Rgba([(y * 40) as u8, 0, 0, 255]));
        let even = apply(&pixels, &Effect::Deinterlace { keep_odd: false, vertical: false, size: 1 });
        let reds: Vec<u8> = (0..6).map(|y| even[(1, y)][0]).collect();
        assert_eq!(reds, [0, 40, 80, 120, 160, 160]);
        let odd = apply(&pixels, &Effect::Deinterlace { keep_odd: true, vertical: false, size: 1 });
        let reds: Vec<u8> = (0..6).map(|y| odd[(1, y)][0]).collect();
        assert_eq!(reds, [40, 40, 80, 120, 160, 200]);
        // Across, the rows stay as they are.
        assert_eq!(apply(&pixels, &Effect::Deinterlace { keep_odd: false, vertical: true, size: 1 }), pixels);
    }

    #[test]
    fn a_high_pass_keeps_only_details() {
        let flat = apply(&RgbaImage::from_pixel(9, 9, Rgba([200, 30, 90, 180])), &Effect::HighPass { radius: 3.0, contrast: 100.0 });
        assert!(flat.pixels().all(|pixel| pixel.0 == [128, 128, 128, 180]));
        let detail = apply(&dot(), &Effect::HighPass { radius: 3.0, contrast: 100.0 });
        assert!(detail[(10, 10)][0] > 200);
        assert!(detail[(12, 10)][0] < 128);
    }

    #[test]
    fn wavelet_layers_blend_back_into_the_image() {
        // Detail layers hold differences of up to half the range either way, so a smooth image.
        let wave = |x: u32, y: u32| (128.0 + 100.0 * (x as f32 * 0.5).sin() * (y as f32 * 0.3).cos()) as u8;
        let pixels = RgbaImage::from_fn(40, 30, |x, y| Rgba([(x * 6) as u8, (y * 8) as u8, wave(x, y), 255]));
        let layers = wavelet_decompose(&pixels, 3, &AtomicBool::new(false)).unwrap();
        assert_eq!(layers.len(), 4);
        let layers: Vec<Layer> = layers
            .into_iter()
            .enumerate()
            .map(|(index, pixels)| Layer {
                blend_mode: if index > 0 { BlendMode::GrainMerge } else { BlendMode::Normal },
                ..Layer::new("", pixels)
            })
            .collect();
        let recomposed = composite(&layers);
        for (a, b) in recomposed.pixels().zip(pixels.pixels()) {
            for channel in 0..4 {
                assert!(a[channel].abs_diff(b[channel]) <= 3, "{a:?} {b:?}");
            }
        }
    }

    #[test]
    fn pixelate_averages_each_cell() {
        let pixels = RgbaImage::from_fn(4, 2, |x, _| Rgba([(x * 60) as u8, 0, 0, 255]));
        let cells = apply(&pixels, &Effect::Pixelate { width: 2, height: 2 });
        let reds: Vec<u8> = (0..4).map(|x| cells[(x, 1)][0]).collect();
        assert_eq!(reds, [30, 30, 150, 150]);
    }

    #[test]
    fn crystallize_makes_flat_cells() {
        let pixels = RgbaImage::from_fn(60, 60, |x, y| Rgba([(x * 4) as u8, (y * 4) as u8, 0, 255]));
        let cells = apply(&pixels, &Effect::Crystallize { size: 10.0 });
        let mut colors: Vec<[u8; 4]> = cells.pixels().map(|pixel| pixel.0).collect();
        colors.sort();
        colors.dedup();
        assert!(colors.len() <= 36 && colors.len() > 10, "{}", colors.len());
    }

    #[test]
    fn a_bulge_swells_the_middle_only() {
        let bulged = apply(&dot(), &Effect::Bulge { amount: 100.0, radius: 50.0, center: [0.0, 0.0] });
        assert!(bulged[(11, 10)][0] > 0, "the dot grows");
        assert_eq!(bulged[(0, 0)], Rgba([0, 0, 0, 255]));
        let shrunk = apply(&dot(), &Effect::Bulge { amount: -100.0, radius: 50.0, center: [0.0, 0.0] });
        assert!(shrunk[(10, 10)][0] > 0 && shrunk[(11, 10)][0] == 0);
    }

    #[test]
    fn a_twist_turns_within_its_disc() {
        let pixels = RgbaImage::from_fn(41, 41, |x, _| Rgba([(x * 6) as u8, 0, 0, 255]));
        let twisted = apply(&pixels, &Effect::Twist { angle: 180.0, pinch: 0.0, radius: 50.0, center: [0.0, 0.0] });
        assert_eq!(twisted[(20, 20)], pixels[(20, 20)]);
        assert_eq!(twisted[(1, 1)], pixels[(1, 1)]);
        assert_ne!(twisted[(20, 15)], pixels[(20, 15)]);
    }

    #[test]
    fn a_kaleidoscope_mirrors_around_its_centre() {
        let pixels = RgbaImage::from_fn(41, 41, |x, y| Rgba([(x * 6) as u8, (y * 6) as u8, 0, 255]));
        let mirrored = apply(&pixels, &Effect::Kaleidoscope { segments: 2, angle: 0.0, center: [0.0, 0.0] });
        // Four wedges a quarter turn wide, so it is the same either side of both middle lines.
        assert_eq!(mirrored[(30, 12)], mirrored[(30, 28)]);
        assert_eq!(mirrored[(30, 12)], mirrored[(10, 12)]);
    }

    #[test]
    fn polar_coordinates_unwrap_what_they_wrap() {
        let pixels = RgbaImage::from_fn(64, 64, |x, y| Rgba([(x * 4) as u8, (y * 4) as u8, 128, 255]));
        let wrapped = apply(&pixels, &Effect::PolarCoordinates { to_polar: true, rotation: 0.0 });
        assert_eq!(wrapped[(0, 0)][3], 0, "nothing past the ellipse");
        let back = apply(&wrapped, &Effect::PolarCoordinates { to_polar: false, rotation: 0.0 });
        for (x, y) in [(20, 20), (40, 30), (32, 50)] {
            let (a, b) = (back[(x, y)], pixels[(x, y)]);
            assert!(a[0].abs_diff(b[0]) <= 12 && a[1].abs_diff(b[1]) <= 12, "{a:?} {b:?}");
        }
    }

    #[test]
    fn zooming_a_lens_out_leaves_nothing_past_the_image() {
        let pixels = RgbaImage::from_pixel(30, 30, Rgba([200, 100, 50, 255]));
        let lens = apply(&pixels, &Effect::LensDistortion { main: 0.0, edge: 0.0, zoom: 100.0, center: [0.0, 0.0] });
        assert_eq!(lens[(15, 15)], pixels[(15, 15)]);
        assert_eq!(lens[(0, 0)], pixels[(0, 0)], "zoomed in");
        let lens = apply(&pixels, &Effect::LensDistortion { main: 0.0, edge: 0.0, zoom: -100.0, center: [0.0, 0.0] });
        assert_eq!(lens[(0, 0)][3], 0, "zoomed out");
    }

    #[test]
    fn a_ripple_moves_rows_by_up_to_its_amplitude() {
        let pixels = RgbaImage::from_fn(40, 40, |_, y| Rgba([(y * 6) as u8, 0, 0, 255]));
        let rippled = apply(&pixels, &Effect::Ripple { amplitude: 3.0, wavelength: 40.0, phase: 0.0, vertical: false, shape: WaveShape::Sine });
        // A quarter of the way along, the wave is at its top.
        assert_eq!(rippled[(10, 20)][0], pixels[(10, 23)][0]);
        assert_eq!(rippled[(30, 20)][0], pixels[(30, 17)][0]);
    }

    #[test]
    fn wind_blows_streaks_downwind_of_edges() {
        let pixels = RgbaImage::from_fn(40, 10, |x, _| if x < 10 { Rgba([255, 255, 255, 255]) } else { Rgba([0, 0, 0, 255]) });
        let blown = apply(&pixels, &Effect::Wind { strength: 20.0, threshold: 10.0, direction: WindDirection::Right, blast: false });
        assert!((0..10).any(|y| blown[(11, y)][0] > 0), "streaks to the right");
        assert_eq!(blown[(39, 5)], pixels[(39, 5)]);
        let left = apply(&pixels, &Effect::Wind { strength: 20.0, threshold: 10.0, direction: WindDirection::Left, blast: false });
        assert_eq!(left, pixels, "no edge gets darker to the left");
    }

    #[test]
    fn newsprint_prints_dots_as_large_as_the_ink() {
        for value in [0, 255] {
            let pixels = RgbaImage::from_pixel(32, 32, Rgba([value, value, value, 255]));
            let printed = apply(&pixels, &Effect::Newsprint { size: 8.0, angle: 0.0, screen: NewsprintScreen::Black });
            assert!(printed == pixels, "{value} stays as it is");
        }
        let grey = RgbaImage::from_pixel(64, 64, Rgba([128, 128, 128, 255]));
        let printed = apply(&grey, &Effect::Newsprint { size: 8.0, angle: 0.0, screen: NewsprintScreen::Black });
        let ink: f32 = printed.pixels().map(|pixel| 1.0 - pixel[0] as f32 / 255.0).sum::<f32>() / (64.0 * 64.0);
        assert!((ink - 0.5).abs() < 0.08, "{ink}");
    }

    #[test]
    fn noise_stays_between_its_colours_and_changes_with_its_seed() {
        let pixels = RgbaImage::from_pixel(64, 64, Rgba([10, 20, 30, 255]));
        let colors = [[40, 40, 40, 255], [200, 200, 200, 255]];
        for kind in NoiseKind::ALL {
            let effect = |seed| Effect::CloudsAndNoise {
                noise: Noise { seed, ..noise(kind) },
                colors,
                blend_mode: BlendMode::Normal,
                plasma_colors: false,
            };
            let drawn = apply(&pixels, &effect(3));
            assert!(drawn.pixels().all(|pixel| (40..=200).contains(&pixel[0]) && pixel[3] == 255), "{kind:?}");
            let shades: std::collections::HashSet<u8> = drawn.pixels().map(|pixel| pixel[0]).collect();
            assert!(shades.len() > 4, "{kind:?} has some features");
            assert_eq!(drawn, apply(&pixels, &effect(3)), "{kind:?}");
            assert_ne!(drawn, apply(&pixels, &effect(4)), "{kind:?}");
        }
    }

    #[test]
    fn difference_clouds_take_the_clouds_from_the_image() {
        let white = RgbaImage::from_pixel(32, 32, Rgba([255, 255, 255, 255]));
        let colors = [[0, 0, 0, 255], [255, 255, 255, 255]];
        let clouds = |blend_mode| Effect::CloudsAndNoise { noise: noise(NoiseKind::Clouds), colors, blend_mode, plasma_colors: false };
        let plain = apply(&white, &clouds(BlendMode::Normal));
        let difference = apply(&white, &clouds(BlendMode::Difference));
        for (plain, difference) in plain.pixels().zip(difference.pixels()) {
            assert_eq!(plain[0], 255 - difference[0]);
        }
    }

    #[test]
    fn a_checkerboard_alternates_its_squares() {
        let board = apply(&RgbaImage::new(8, 8), &Effect::Checkerboard { size: 2, colors: [[255, 0, 0, 255], [0, 0, 255, 255]] });
        assert_eq!(board[(0, 0)], Rgba([255, 0, 0, 255]));
        assert_eq!(board[(1, 1)], Rgba([255, 0, 0, 255]));
        assert_eq!(board[(2, 0)], Rgba([0, 0, 255, 255]));
        assert_eq!(board[(2, 2)], Rgba([255, 0, 0, 255]));
    }

    #[test]
    fn a_grid_leaves_its_cells_as_they_are() {
        let pixels = RgbaImage::from_pixel(20, 20, Rgba([0, 255, 0, 255]));
        let grid = apply(&pixels, &Effect::Grid { width: 10, height: 5, line: 1, offset: [2, 0], color: [255, 0, 0, 255] });
        assert_eq!(grid[(2, 3)], Rgba([255, 0, 0, 255]));
        assert_eq!(grid[(12, 3)], Rgba([255, 0, 0, 255]));
        assert_eq!(grid[(4, 5)], Rgba([255, 0, 0, 255]));
        assert_eq!(grid[(4, 3)], pixels[(4, 3)]);
    }

    #[test]
    fn a_bayer_matrix_spreads_its_shades_out() {
        let colors = [[0, 0, 0, 255], [255, 255, 255, 255]];
        let matrix = apply(&RgbaImage::new(4, 4), &Effect::BayerMatrix { level: 1, pixel_size: 1, colors });
        let shades: Vec<u8> = [(0, 0), (1, 0), (0, 1), (1, 1), (2, 2)].iter().map(|&at| matrix[at][0]).collect();
        assert_eq!(shades, [0, 170, 255, 85, 0]);
    }

    #[test]
    fn a_maze_joins_every_corridor() {
        let colors = [[0, 0, 0, 255], [255, 255, 255, 255]];
        for branching in [false, true] {
            let maze = apply(&RgbaImage::new(41, 31), &Effect::Maze { size: 1, seed: 5, branching, colors });
            let open = |x: u32, y: u32| maze[(x, y)][0] == 255;
            assert!(open(0, 1) && open(40, 29), "a way in and out");
            assert!(!open(0, 0) && !open(2, 2), "walls");
            // Everything open can be reached from the way in.
            let mut reached = vec![false; 41 * 31];
            let mut next = vec![(0u32, 1u32)];
            reached[41] = true;
            while let Some((x, y)) = next.pop() {
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (x, y) = (x as i32 + dx, y as i32 + dy);
                    if (0..41).contains(&x) && (0..31).contains(&y) && open(x as u32, y as u32) && !reached[(y * 41 + x) as usize] {
                        reached[(y * 41 + x) as usize] = true;
                        next.push((x as u32, y as u32));
                    }
                }
            }
            assert!(maze.enumerate_pixels().all(|(x, y, pixel)| pixel[0] == 0 || reached[(y * 41 + x) as usize]), "{branching}");
            // A tree: as many corridors between the cells as cells less one, and the two ways out.
            let count = maze.pixels().filter(|pixel| pixel[0] == 255).count();
            assert_eq!(count, 20 * 15 * 2 - 1 + 2, "{branching}");
        }
    }

    #[test]
    fn speed_lines_keep_their_middle_clear() {
        let pixels = RgbaImage::from_pixel(100, 100, Rgba([255, 255, 255, 255]));
        let effect =
            Effect::SpeedLines { count: 40, thickness: 50.0, clear: 40.0, randomness: 0.0, center: [0.0, 0.0], seed: 1, color: [0, 0, 0, 255] };
        let lines = apply(&pixels, &effect);
        assert!((40..60).all(|x| lines[(x, 50)] == pixels[(x, 50)]));
        assert!((0..100).any(|y| lines[(1, y)][0] < 128), "lines reach the edges");
        assert!((0..100).any(|y| lines[(1, y)][0] == 255), "with room between them");
    }

    #[test]
    fn speed_lines_radiate_from_the_middle_of_the_selection() {
        let pixels = RgbaImage::from_pixel(200, 100, Rgba([255, 255, 255, 255]));
        let selection = Selection::rect(PixelRect { left: 120, top: 10, right: 200, bottom: 90 });
        let mask = SelectionMask::of(&selection, 200, 100).unwrap();
        let effect =
            Effect::SpeedLines { count: 40, thickness: 50.0, clear: 40.0, randomness: 0.0, center: [0.0, 0.0], seed: 1, color: [0, 0, 0, 255] };
        let lines = apply_effect(&pixels, &effect, Some(&mask), &AtomicBool::new(false)).unwrap();
        assert!((150..170).all(|x| lines[(x, 50)] == pixels[(x, 50)]), "clear around the selection's middle");
        let inked = |points: Vec<(u32, u32)>| points.into_iter().any(|(x, y)| lines[(x, y)][0] < 128);
        assert!(inked((10..90).map(|y| (120, y)).collect()), "on the left side");
        assert!(inked((10..90).map(|y| (199, y)).collect()), "on the right side");
        assert!(inked((120..200).map(|x| (x, 10)).collect()), "on the top side");
        assert!(inked((120..200).map(|x| (x, 89)).collect()), "on the bottom side");
    }

    #[test]
    fn the_mandelbrot_set_is_drawn_in_the_first_colour() {
        let colors = [[10, 20, 30, 255], [250, 250, 250, 255]];
        let fractal = |rainbow| Effect::Fractal {
            kind: FractalKind::Mandelbrot,
            iterations: 100,
            zoom: 100.0,
            center: [0.0, 0.0],
            julia: [0.0, 0.0],
            rainbow,
            colors,
            antialias: false,
        };
        for rainbow in [false, true] {
            let drawn = apply(&RgbaImage::new(60, 60), &fractal(rainbow));
            // The middle of the view is at -0.75, inside the set; the corners are far out.
            assert_eq!(drawn[(30, 30)].0, colors[0]);
            assert_ne!(drawn[(0, 0)].0, colors[0]);
        }
    }

    /// A black square in the middle of white.
    fn square() -> RgbaImage {
        RgbaImage::from_fn(30, 30, |x, y| {
            if (10..20).contains(&x) && (10..20).contains(&y) { Rgba([0, 0, 0, 255]) } else { Rgba([255, 255, 255, 255]) }
        })
    }

    #[test]
    fn edges_show_only_where_the_colours_change() {
        for algorithm in EdgeAlgorithm::ALL {
            let found = apply(&square(), &edges(algorithm));
            assert_eq!(found[(3, 3)], Rgba([0, 0, 0, 255]), "{algorithm:?}");
            assert_eq!(found[(15, 15)], Rgba([0, 0, 0, 255]), "{algorithm:?}");
            assert!(found[(10, 15)][0] > 0 || found[(9, 15)][0] > 0, "{algorithm:?}");
            let inverted = apply(&square(), &edge_detect(algorithm, [true, true], true, true));
            assert_eq!(inverted[(3, 3)], Rgba([255, 255, 255, 255]), "{algorithm:?}");
            assert_eq!(inverted.pixels().map(|pixel| pixel[0]).min(), Some(0), "{algorithm:?} normalized");
        }
        let see_through = RgbaImage::from_pixel(10, 10, Rgba([50, 60, 70, 100]));
        assert_eq!(apply(&see_through, &edges(EdgeAlgorithm::Sobel)), RgbaImage::from_pixel(10, 10, Rgba([0, 0, 0, 100])));
    }

    #[test]
    fn a_sobel_measures_only_the_ways_it_is_asked_to() {
        let across = edge_detect(EdgeAlgorithm::Sobel, [true, false], false, false);
        let found = apply(&square(), &across);
        assert_eq!(found[(10, 15)][0], 255, "the left side");
        assert_eq!(found[(15, 10)][0], 0, "not the top");
        let neither = edge_detect(EdgeAlgorithm::Sobel, [false, false], false, false);
        assert!(apply(&square(), &neither).pixels().all(|pixel| pixel[0] == 0));
    }

    #[test]
    fn an_emboss_lights_the_sides_facing_the_light() {
        // Lit from the left: the square is a dip, so its right side faces the light.
        let embossed = apply(&square(), &Effect::Emboss { azimuth: 180.0, elevation: 45.0, depth: 3.0 });
        assert_eq!(embossed[(3, 3)], Rgba([128, 128, 128, 255]));
        assert_eq!(embossed[(15, 15)], Rgba([128, 128, 128, 255]));
        assert!(embossed[(19, 15)][0] > 128 && embossed[(10, 15)][0] < 128);
        let relief = apply(&square(), &Effect::Relief { azimuth: 180.0, elevation: 45.0, depth: 3.0 });
        assert_eq!(relief[(3, 3)], Rgba([255, 255, 255, 255]));
        assert!(relief[(10, 15)][0] < 255 || relief[(9, 15)][0] < 255);
    }

    #[test]
    fn an_outline_draws_lines_as_thick_as_asked_along_edges() {
        let outlined = apply(&square(), &Effect::Outline { thickness: 3.0, intensity: 50.0 });
        assert_eq!(outlined[(15, 15)], Rgba([255, 255, 255, 255]), "the middle of the square is flat");
        assert_eq!(outlined[(3, 3)], Rgba([255, 255, 255, 255]));
        assert_eq!(outlined[(12, 15)], Rgba([0, 0, 0, 255]), "within the thickness of the edge");
        assert_eq!(outlined[(13, 15)], Rgba([255, 255, 255, 255]), "past it");
    }

    /// A gradient from black on the left to white on the right, and from no blue at the
    /// top to full blue at the bottom, half opaque.
    fn ramp() -> RgbaImage {
        RgbaImage::from_fn(64, 64, |x, y| Rgba([(x * 4) as u8, (x * 4) as u8, (y * 4) as u8, 128]))
    }

    #[test]
    fn noise_changes_as_many_pixels_as_it_covers_and_keeps_how_opaque_they_are() {
        let pixels = ramp();
        for color_space in NoiseColorSpace::ALL {
            for coverage in [100.0, 30.0] {
                let noisy = apply(&pixels, &add_noise(color_space, coverage, 1));
                assert!(noisy.pixels().all(|pixel| pixel[3] == 128), "{color_space:?}");
                let changed = noisy.pixels().zip(pixels.pixels()).filter(|(after, before)| after != before).count() as f32;
                let share = changed / (64.0 * 64.0) * 100.0;
                assert!(share <= coverage + 3.0 && share > coverage * 0.8, "{color_space:?} changed {share}% for {coverage}%");
            }
            assert_eq!(apply(&pixels, &add_noise(color_space, 100.0, 1)), apply(&pixels, &add_noise(color_space, 100.0, 1)));
            assert_ne!(apply(&pixels, &add_noise(color_space, 100.0, 1)), apply(&pixels, &add_noise(color_space, 100.0, 2)));
        }
    }

    #[test]
    fn spreading_only_moves_colours_around_within_its_reach() {
        let pixels = ramp();
        for downward in [0.0, 50.0, 100.0] {
            let spread = apply(&pixels, &spread(downward));
            assert_ne!(spread, pixels);
            for (x, y, pixel) in spread.enumerate_pixels() {
                // Each column is a shade of grey, and each row a shade of blue.
                let (from_x, from_y) = (pixel[0] as i32 / 4, pixel[2] as i32 / 4);
                assert!((from_x - x as i32).abs() <= 4, "{downward}: from {from_x} to {x}");
                let above = (y as i32 - from_y).clamp(-4, 12);
                assert_eq!(y as i32 - from_y, above, "{downward}: from {from_y} to {y}");
                if downward == 100.0 {
                    assert!(from_y <= y as i32, "melting only pulls colours down");
                }
            }
        }
    }

    #[test]
    fn soften_portrait_fills_in_dark_specks_and_warms() {
        let mut pixels = RgbaImage::from_pixel(21, 21, Rgba([200, 160, 140, 255]));
        pixels.put_pixel(10, 10, Rgba([100, 80, 70, 255]));
        let soft = apply(&pixels, &Effect::SoftenPortrait { softness: 3.0, lighting: 0.0, warmth: 0.0 });
        assert!(soft[(10, 10)][0] > 130, "the speck is half filled in: {:?}", soft[(10, 10)]);
        assert_eq!(soft[(0, 0)], pixels[(0, 0)], "flat skin stays as it is");
        let warm = apply(&pixels, &Effect::SoftenPortrait { softness: 0.0, lighting: 0.0, warmth: 20.0 });
        assert!(warm[(0, 0)][0] > 200 && warm[(0, 0)][2] < 140, "{:?}", warm[(0, 0)]);
        let lit = apply(&pixels, &Effect::SoftenPortrait { softness: 0.0, lighting: 20.0, warmth: 0.0 });
        assert!(lit[(0, 0)].0[..3].iter().zip(&pixels[(0, 0)].0[..3]).all(|(after, before)| after > before));
    }

    #[test]
    fn straightening_turns_and_fills_the_area() {
        let pixels = RgbaImage::from_fn(80, 40, |x, y| Rgba([(x * 3) as u8, (y * 6) as u8, 0, 255]));
        let straightened = |fill| apply(&pixels, &Effect::Straighten { angle: 10.0, fill, interpolation: Interpolation::Bilinear });
        let filled = straightened(true);
        assert_ne!(filled, pixels);
        let empty: Vec<_> = filled.enumerate_pixels().filter(|p| p.2[3] < 255).map(|p| (p.0, p.1, p.2[3])).collect();
        assert!(empty.is_empty(), "no corner is left empty: {empty:?}");
        assert_eq!(straightened(false)[(0, 0)][3], 0, "the corners are left empty without zooming");
        // Turned anticlockwise, the top left comes down, taking its dark green there.
        let middle_left = filled[(2, 20)];
        assert!(middle_left[1] < pixels[(2, 20)][1], "{middle_left:?}");
    }

    #[test]
    fn a_bevel_lights_the_edges_facing_the_light() {
        let pixels = RgbaImage::from_pixel(40, 40, Rgba([128, 128, 128, 255]));
        let bevelled = apply(&pixels, &Effect::AddBevel { width: 6.0, depth: 50.0, azimuth: 135.0 });
        assert!(bevelled[(0, 20)][0] > 128 && bevelled[(20, 0)][0] > 128, "top and left are lit");
        assert!(bevelled[(39, 20)][0] < 128 && bevelled[(20, 39)][0] < 128, "bottom and right are in shade");
        assert!(bevelled[(1, 20)][0] < bevelled[(0, 20)][0], "rounded: steepest at the edge");
        assert_eq!(bevelled[(20, 20)], pixels[(20, 20)], "the middle stays flat");
        assert_eq!(bevelled[(6, 20)], pixels[(6, 20)], "past the width");
        // A see-through layer's shape is bevelled along its own edges.
        let mut shape = RgbaImage::new(40, 40);
        for (x, y, pixel) in shape.enumerate_pixels_mut() {
            if (10..30).contains(&x) && (10..30).contains(&y) {
                *pixel = Rgba([128, 128, 128, 255]);
            }
        }
        let bevelled = apply(&shape, &Effect::AddBevel { width: 4.0, depth: 50.0, azimuth: 135.0 });
        assert!(bevelled[(10, 20)][0] > 128 && bevelled[(29, 20)][0] < 128);
        assert_eq!(bevelled[(5, 5)], Rgba([0, 0, 0, 0]));
    }

    #[test]
    fn a_border_follows_the_edges_of_the_selection() {
        let pixels = RgbaImage::from_pixel(40, 40, Rgba([0, 0, 255, 255]));
        let red = [200, 0, 0, 255];
        let bordered = apply(&pixels, &Effect::AddBorder { size: 3.0, color: red, shading: 0.0 });
        assert_eq!(bordered[(0, 20)], Rgba(red));
        assert_eq!(bordered[(37, 37)], Rgba(red));
        assert_eq!(bordered[(3, 20)], pixels[(3, 20)], "past its size");
        let shaded = apply(&pixels, &Effect::AddBorder { size: 3.0, color: red, shading: 50.0 });
        let [top, left, right, bottom] = [(20, 0), (0, 20), (39, 20), (20, 39)].map(|at| shaded[at][0]);
        assert!(top > left && left > 200 && right < 200 && bottom < right, "{top} {left} {right} {bottom}");
        let mask = SelectionMask::of(&Selection::rect(PixelRect { left: 10, top: 10, right: 30, bottom: 30 }), 40, 40).unwrap();
        let framed = apply_effect(&pixels, &Effect::AddBorder { size: 2.0, color: red, shading: 0.0 }, Some(&mask), &AtomicBool::new(false)).unwrap();
        assert_eq!(framed[(10, 20)], Rgba(red));
        assert_eq!(framed[(11, 20)], Rgba(red));
        assert_eq!(framed[(12, 20)], pixels[(12, 20)]);
        assert_eq!(framed[(9, 20)], pixels[(9, 20)], "outside the selection");
    }

    #[test]
    fn rounding_cuts_the_corners_of_the_area() {
        let pixels = RgbaImage::from_pixel(40, 30, Rgba([10, 20, 30, 255]));
        let rounded = apply(&pixels, &Effect::RoundCorners { radius: 8.0 });
        for corner in [(0, 0), (39, 0), (0, 29), (39, 29), (1, 1)] {
            assert_eq!(rounded[corner], Rgba([0, 0, 0, 0]), "{corner:?}");
        }
        let edge = rounded[(0, 4)][3];
        assert!(edge > 0 && edge < 255, "the arc is smooth: {edge}");
        for kept in [(0, 15), (20, 0), (8, 8), (20, 15)] {
            assert_eq!(rounded[kept], pixels[kept], "{kept:?}");
        }
        let mask = SelectionMask::of(&Selection::rect(PixelRect { left: 10, top: 5, right: 30, bottom: 25 }), 40, 30).unwrap();
        let within = apply_effect(&pixels, &Effect::RoundCorners { radius: 4.0 }, Some(&mask), &AtomicBool::new(false)).unwrap();
        assert_eq!(within[(10, 5)][3], 0);
        assert_eq!(within[(0, 0)], pixels[(0, 0)]);
    }

    /// A grey square from 10 to 20 across and down on a see-through layer 40 wide.
    fn grey_square() -> RgbaImage {
        RgbaImage::from_fn(40, 40, |x, y| if (10..20).contains(&x) && (10..20).contains(&y) { Rgba([128, 128, 128, 255]) } else { Rgba([0; 4]) })
    }

    #[test]
    fn a_drop_shadow_falls_behind_away_from_the_light() {
        let pixels = grey_square();
        let shadowed = apply(&pixels, &Effect::DropShadow { angle: 315.0, distance: 5.0, blur: 0.0, opacity: 50.0, color: [0, 0, 0, 255] });
        assert_eq!(shadowed[(22, 22)], Rgba([0, 0, 0, 128]), "down and right");
        assert_eq!(shadowed[(8, 8)][3], 0, "not up and left");
        assert_eq!(shadowed[(15, 15)], pixels[(15, 15)], "under the square");
        let blurred = apply(&pixels, &Effect::DropShadow { angle: 315.0, distance: 5.0, blur: 4.0, opacity: 100.0, color: [0, 0, 0, 255] });
        let edge = blurred[(25, 20)][3];
        assert!(edge > 0 && edge < 200, "soft edge: {edge}");
    }

    #[test]
    fn a_long_shadow_reaches_far_and_fades() {
        let pixels = grey_square();
        let shadow = |fade| apply(&pixels, &Effect::LongShadow { angle: 315.0, length: 15.0, opacity: 100.0, color: [0, 0, 0, 255], fade });
        let (flat, fading) = (shadow(false), shadow(true));
        assert_eq!(flat[(28, 28)][3], 255, "as long as it is");
        assert_eq!(flat[(36, 36)][3], 0, "and no longer");
        assert_eq!(flat[(34, 20)][3], 0, "only down the way it goes");
        assert!(fading[(21, 21)][3] > fading[(28, 28)][3] && fading[(28, 28)][3] > 0);
        assert_eq!(flat[(15, 15)], pixels[(15, 15)]);
    }

    #[test]
    fn a_vignette_darkens_towards_the_corners() {
        let pixels = RgbaImage::from_pixel(41, 41, Rgba([200, 200, 200, 255]));
        let vignetted = apply(&pixels, &Effect::Vignette { amount: 100.0, radius: 100.0, softness: 50.0, center: [0.0, 0.0], color: [0, 0, 0, 255] });
        assert_eq!(vignetted[(20, 20)], pixels[(20, 20)], "the middle stays");
        assert_eq!(vignetted[(0, 0)][0], 1, "the corners all the way");
        assert!(vignetted[(0, 20)][0] < 200 && vignetted[(0, 20)][0] > vignetted[(0, 0)][0]);
        let moved = apply(&pixels, &Effect::Vignette { amount: 100.0, radius: 100.0, softness: 50.0, center: [-100.0, -100.0], color: [0, 0, 0, 255] });
        assert_eq!(moved[(0, 0)], pixels[(0, 0)], "around the centre wherever it is");
    }

    #[test]
    fn a_lens_flare_lights_around_its_centre_and_along_its_rays() {
        let pixels = RgbaImage::from_pixel(101, 101, Rgba([0, 0, 0, 255]));
        let flare = |rays| apply(&pixels, &Effect::LensFlare { center: [0.0, 0.0], size: 10.0, brightness: 100.0, rays, ghosts: false });
        let plain = flare(0);
        assert_eq!(plain[(50, 50)][0], 255);
        assert!(plain[(50, 50)][0] > plain[(70, 50)][0] && plain[(70, 50)][0] > plain[(95, 50)][0]);
        let rayed = flare(4);
        assert!(rayed[(50, 20)][0] > plain[(50, 20)][0] + 20, "a ray straight up");
        assert!(rayed[(72, 28)][0] <= plain[(72, 28)][0] + 2, "none between them");
        let ghosts = apply(&pixels, &Effect::LensFlare { center: [-80.0, -80.0], size: 10.0, brightness: 100.0, rays: 0, ghosts: true });
        let lone = apply(&pixels, &Effect::LensFlare { center: [-80.0, -80.0], size: 10.0, brightness: 100.0, rays: 0, ghosts: false });
        assert!(ghosts[(61, 61)][0] > lone[(61, 61)][0], "a ghost past the middle");
        let clear = apply(&RgbaImage::new(101, 101), &Effect::LensFlare { center: [0.0, 0.0], size: 10.0, brightness: 100.0, rays: 0, ghosts: false });
        assert_eq!(clear[(50, 50)], Rgba([255, 255, 255, 255]), "shows on a see-through layer");
        assert_eq!(clear[(0, 0)][3], 0);
    }

    #[test]
    fn lighting_effects_light_most_under_the_lamp_and_what_faces_the_sun() {
        let pixels = RgbaImage::from_pixel(60, 60, Rgba([100, 100, 100, 255]));
        let lit = apply(&pixels, &lighting(LightSource::Spotlight, 0.0));
        assert!(lit[(30, 30)][0] > 100, "{:?}", lit[(30, 30)]);
        assert!(lit[(30, 30)][0] > lit[(5, 5)][0]);
        assert!(lit[(0, 0)][0] >= 20, "ambient light everywhere");
        // A ridge down the middle, lit from the top left: its left side faces the light.
        let ridge = RgbaImage::from_fn(60, 60, |x, _| {
            let height = (255 - (x as i32 - 30).abs() * 8).max(0) as u8;
            Rgba([height, height, height, 255])
        });
        let flat = apply(&ridge, &lighting(LightSource::Directional, 0.0));
        let bumpy = apply(&ridge, &lighting(LightSource::Directional, 30.0));
        let gain = |at: (u32, u32)| bumpy[at][0] as i32 - flat[at][0] as i32;
        assert!(gain((25, 30)) > 0 && gain((35, 30)) < 0, "{} {}", gain((25, 30)), gain((35, 30)));
    }

    #[test]
    fn sparkles_glint_on_the_bright_spots() {
        let mut pixels = RgbaImage::from_pixel(41, 41, Rgba([20, 20, 20, 255]));
        pixels.put_pixel(20, 20, Rgba([255, 255, 255, 255]));
        let sparkle = |angle| apply(&pixels, &Effect::Sparkle { threshold: 50.0, size: 10.0, points: 4, angle, amount: 100.0, seed: 0 });
        let upright = sparkle(0.0);
        assert!(upright[(20, 14)][0] > 60 && upright[(26, 20)][0] > 60, "rays up and across");
        assert!(upright[(24, 16)][0] < 40, "none between: {:?}", upright[(24, 16)]);
        let turned = sparkle(45.0);
        assert!(turned[(24, 16)][0] > 60 && turned[(20, 14)][0] < 40);
        let dull = RgbaImage::from_pixel(41, 41, Rgba([100, 100, 100, 255]));
        assert_eq!(apply(&dull, &Effect::Sparkle { threshold: 50.0, size: 10.0, points: 4, angle: 0.0, amount: 100.0, seed: 0 }), dull);
    }
}

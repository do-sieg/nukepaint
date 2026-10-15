// Draws the grid over the image, as a line one screen pixel wide along the edges of
// its cells. The cells are worked out in whole image pixels, as integers: dividing
// in floating point now and then misses a line that falls exactly between two
// screen pixels.

struct Params {
    // Where the image's top left corner is, in pixels of the window.
    image_origin: vec2<f32>,
    // How many screen pixels wide an image pixel is shown.
    zoom: f32,
    // Where a cell starts, in image pixels, from 0 to the cell's size less one.
    origin: vec2<i32>,
    cell: vec2<i32>,
    // The lines' colour, its alpha multiplied in, as the window takes it.
    grid_color: vec4<f32>,
}

@group(0) @binding(0) var<uniform> params: Params;

// How far, in image pixels, a cell's edge can be off from a screen pixel's edge and
// still count as on it: far less than a screen pixel, at the largest zoom.
const EDGE_SLACK: f32 = 0.001;

// A triangle that covers the whole of the area drawn on.
@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
}

// Returns which cell an image pixel is in, counting from the one at the origin. The
// pixel is never left of or above the image, so what is divided is never negative.
fn get_cell(pixel: vec2<i32>) -> vec2<i32> {
    return (pixel - params.origin + params.cell) / params.cell - vec2<i32>(1);
}

fn color(position: vec4<f32>) -> vec4<f32> {
    // Which screen pixel of the image this is, from its top left one.
    let screen = floor(position.xy - params.image_origin);
    // The edges between image pixels this screen pixel covers, from the first up to,
    // but not including, the last. It is on a line when one of them starts a cell.
    let first = vec2<i32>(ceil(screen / params.zoom - EDGE_SLACK));
    let last = vec2<i32>(ceil((screen + 1.0) / params.zoom - EDGE_SLACK));
    let cell_start = get_cell(last - vec2<i32>(1)) * params.cell + params.origin;
    if any(cell_start >= first) {
        return params.grid_color;
    }
    return vec4<f32>(0.0);
}

// For a window that takes colours as they are, which egui prefers.
@fragment
fn fs_gamma(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return color(position);
}

fn to_linear(gamma: vec3<f32>) -> vec3<f32> {
    let low = gamma / 12.92;
    let high = pow((gamma + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, gamma <= vec3<f32>(0.04045));
}

// For a window that turns colours to sRGB itself.
@fragment
fn fs_linear(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let gamma = color(position);
    if gamma.a <= 0.0 {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(to_linear(gamma.rgb / gamma.a) * gamma.a, gamma.a);
}

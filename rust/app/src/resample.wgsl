// Draws pixels being moved, stretched and turned as they will be put down: each pixel
// of the image they cover takes the colour `FloatingPixels::place` would give it,
// from the pixels around the point it comes from, weighed as the resampling says.
// The pixels can be larger than a texture can be: each tile of them draws the points
// that come from within it, from a texture holding it and the pixels just around it.

struct Params {
    // Where the image's top left corner is, in pixels of the window, how many of those
    // an image pixel takes, and how opaque the layer is.
    view: vec4<f32>,
    // From a point of the image to where it comes from among the pixels moved, in
    // their own pixels from the middle of the first: x = dot(map_x.xy, point) + map_x.z.
    map_x: vec4<f32>,
    map_y: vec4<f32>,
    // The columns and rows of the pixels moved that this tile draws the points of, from
    // the first to past the last.
    core: vec4<i32>,
    // Where the tile's texture starts among the pixels moved, and their size.
    texture_origin: vec2<i32>,
    size: vec2<i32>,
    // 0 for the nearest pixel, 1 bilinear, 2 bicubic, 3 Lanczos; and 1 to draw each
    // image pixel from its middle alone, as it will be put down, rather than each
    // screen pixel from its own.
    mode: vec4<u32>,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var pixels: texture_2d<f32>;

const PI: f32 = 3.14159265358979;
// The widest reach of the kernels, in pixels on each side.
const MAX_RADIUS: i32 = 3;

// A triangle that covers the whole of the area drawn on.
@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
}

// Where the point drawn comes from, among the pixels moved.
fn source(position: vec4<f32>) -> vec2<f32> {
    var point = (position.xy - params.view.xy) / params.view.z;
    if params.mode.y == 1u {
        point = floor(point) + 0.5;
    }
    return vec2<f32>(dot(params.map_x.xy, point) + params.map_x.z, dot(params.map_y.xy, point) + params.map_y.z);
}

// The pixel moved at a place, its alpha multiplied in, or nothing off them.
fn fetch(at: vec2<i32>) -> vec4<f32> {
    if any(at < vec2<i32>(0)) || any(at >= params.size) {
        return vec4<f32>(0.0);
    }
    let color = textureLoad(pixels, at - params.texture_origin, 0);
    return vec4<f32>(color.rgb * color.a, color.a);
}

fn sinc(x: f32) -> f32 {
    if abs(x) < 1e-6 {
        return 1.0;
    }
    return sin(PI * x) / (PI * x);
}

fn kernel(t: f32) -> f32 {
    let x = abs(t);
    switch params.mode.x {
        case 1u: {
            return max(1.0 - x, 0.0);
        }
        case 2u: {
            // The bicubic kernel that goes through the pixels themselves.
            if x < 1.0 {
                return 1.5 * x * x * x - 2.5 * x * x + 1.0;
            }
            if x < 2.0 {
                return -0.5 * x * x * x + 2.5 * x * x - 4.0 * x + 2.0;
            }
            return 0.0;
        }
        default: {
            if x < 3.0 {
                return sinc(x) * sinc(x / 3.0);
            }
            return 0.0;
        }
    }
}

// Rounds halves away from zero, as the pixels are put down.
fn round_away(value: vec2<f32>) -> vec2<f32> {
    return sign(value) * floor(abs(value) + 0.5);
}

fn color(at: vec2<f32>) -> vec4<f32> {
    if params.mode.x == 0u {
        return fetch(vec2<i32>(round_away(at)));
    }
    let radius = i32(params.mode.x);
    let first = vec2<i32>(floor(at - f32(radius))) + 1;
    var columns: array<f32, 6>;
    var rows: array<f32, 6>;
    var total = vec2<f32>(0.0);
    for (var i = 0; i < 2 * radius; i++) {
        let tap = vec2<f32>(first + i);
        columns[i] = kernel(at.x - tap.x);
        rows[i] = kernel(at.y - tap.y);
        total += vec2<f32>(columns[i], rows[i]);
    }
    // The weights add up to one, which those of Lanczos only come close to.
    total = select(vec2<f32>(1.0), total, abs(total) > vec2<f32>(1e-6));
    var sum = vec4<f32>(0.0);
    for (var y = 0; y < 2 * radius; y++) {
        for (var x = 0; x < 2 * radius; x++) {
            let weight = columns[x] / total.x * rows[y] / total.y;
            if weight != 0.0 {
                sum += fetch(first + vec2<i32>(x, y)) * weight;
            }
        }
    }
    // The kernels that overshoot could leave more colour than there is alpha to carry it.
    let alpha = clamp(sum.a, 0.0, 1.0);
    return vec4<f32>(clamp(sum.rgb, vec3<f32>(0.0), vec3<f32>(alpha)), alpha);
}

// The colour of a point, as the window takes it, or nothing if another tile draws it.
fn shade(position: vec4<f32>) -> vec4<f32> {
    let at = source(position);
    let pixel = vec2<i32>(floor(at));
    if any(pixel < params.core.xy) || any(pixel >= params.core.zw) {
        return vec4<f32>(-1.0);
    }
    // Far off the pixels moved, nothing reaches.
    let reach = f32(MAX_RADIUS + 1);
    if any(at < vec2<f32>(-reach)) || any(at > vec2<f32>(params.size) + reach) {
        return vec4<f32>(-1.0);
    }
    return color(at) * params.view.w;
}

// For a window that takes colours as they are, which egui prefers.
@fragment
fn fs_gamma(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let color = shade(position);
    if color.a <= 0.0 {
        discard;
    }
    return color;
}

fn to_linear(gamma: vec3<f32>) -> vec3<f32> {
    let low = gamma / 12.92;
    let high = pow((gamma + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, gamma <= vec3<f32>(0.04045));
}

// For a window that turns colours to sRGB itself.
@fragment
fn fs_linear(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let gamma = shade(position);
    if gamma.a <= 0.0 {
        discard;
    }
    return vec4<f32>(to_linear(gamma.rgb / gamma.a) * gamma.a, gamma.a);
}

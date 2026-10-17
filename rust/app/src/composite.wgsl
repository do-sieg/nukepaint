// Blends the layers of an image one over the other, each pixel as
// `power_paint_core::composite` blends it on the CPU, rounded to bytes after each
// layer as it is. Then the image is made ready for egui to draw: its alpha
// multiplied in, and halved again and again to show it zoomed out.

struct Params {
    // How opaque the layer is, from 0 to 1.
    opacity: f32,
    // How it is blended, in the order `BlendMode` lists the modes.
    mode: u32,
    // 1 for the bottom layer, which has nothing under it.
    bottom: u32,
    _padding: u32,
}

@group(0) @binding(0) var<uniform> params: Params;
// What is under the layer, or the pixels to make ready for egui.
@group(0) @binding(1) var source: texture_2d<f32>;
@group(0) @binding(2) var layer: texture_2d<f32>;

const TINY: f32 = 0.0001;

// A triangle that covers the whole of the area drawn on.
@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
}

// A pixel's channels, from 0 to 255.
fn bytes(color: vec4<f32>) -> vec4<f32> {
    return round(color * 255.0);
}

// Rounds halves up, as Rust does, rather than to even.
fn to_byte(value: f32) -> f32 {
    return floor(clamp(value * 255.0, 0.0, 255.0) + 0.5);
}

// The colour two opaque channels give, from 0 to 1: one of the layer over one of
// what is under it, both from 0 to 255.
fn blend(under: f32, over: f32) -> f32 {
    let u = under / 255.0;
    let o = over / 255.0;
    switch params.mode {
        case 0u: {
            return o;
        }
        case 1u: {
            return u * o;
        }
        case 2u: {
            return min(u + o, 1.0);
        }
        case 3u: {
            if over == 0.0 {
                return 0.0;
            }
            return max(1.0 - (1.0 - u) / max(o, TINY), 0.0);
        }
        case 4u: {
            if over == 255.0 {
                return 1.0;
            }
            return min(u / max(1.0 - o, TINY), 1.0);
        }
        case 5u: {
            if over == 255.0 {
                return 1.0;
            }
            return min(u * u / max(1.0 - o, TINY), 1.0);
        }
        case 6u: {
            if under == 255.0 {
                return 1.0;
            }
            return min(o * o / max(1.0 - u, TINY), 1.0);
        }
        case 7u: {
            if u < 0.5 {
                return 2.0 * u * o;
            }
            return 1.0 - 2.0 * (1.0 - u) * (1.0 - o);
        }
        case 8u: {
            return abs(u - o);
        }
        case 9u: {
            return 1.0 - abs(1.0 - u - o);
        }
        case 10u: {
            return max(u, o);
        }
        case 11u: {
            return min(u, o);
        }
        case 12u: {
            return u + o - u * o;
        }
        case 13u: {
            return f32(u32(under) ^ u32(over)) / 255.0;
        }
        default: {
            return clamp(u + o - 128.0 / 255.0, 0.0, 1.0);
        }
    }
}

// The layer over what is under it. The two only mix where both are: elsewhere, each
// shows as it is.
@fragment
fn fs_blend(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let at = vec2<i32>(position.xy);
    var under = vec4<f32>(0.0);
    if params.bottom == 0u {
        under = bytes(textureLoad(source, at, 0));
    }
    let over = bytes(textureLoad(layer, at, 0));
    let over_alpha = over.a / 255.0 * params.opacity;
    if over_alpha <= 0.0 {
        return under / 255.0;
    }
    let under_alpha = under.a / 255.0;
    let both = over_alpha * under_alpha;
    let alpha = over_alpha + under_alpha - both;
    var blended = vec4<f32>(0.0, 0.0, 0.0, to_byte(alpha));
    for (var channel = 0; channel < 3; channel++) {
        let below = under[channel];
        let above = over[channel];
        var mixed = 0.0;
        if both > 0.0 {
            mixed = blend(below, above);
        }
        let color = (over_alpha - both) * (above / 255.0) + (under_alpha - both) * (below / 255.0) + both * mixed;
        blended[channel] = to_byte(color / alpha);
    }
    return blended / 255.0;
}

// The pixels with their alpha multiplied in, as egui takes them.
@fragment
fn fs_premultiply(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let color = textureLoad(source, vec2<i32>(position.xy), 0);
    return vec4<f32>(color.rgb * color.a, color.a);
}

// The pixels at half their size, each the average of the four it covers, their alpha
// already multiplied in. An odd last row or column is left out.
@fragment
fn fs_halve(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let last = vec2<i32>(textureDimensions(source)) - 1;
    let at = vec2<i32>(position.xy) * 2;
    let sum = textureLoad(source, min(at, last), 0)
        + textureLoad(source, min(at + vec2<i32>(1, 0), last), 0)
        + textureLoad(source, min(at + vec2<i32>(0, 1), last), 0)
        + textureLoad(source, min(at + vec2<i32>(1, 1), last), 0);
    return sum / 4.0;
}

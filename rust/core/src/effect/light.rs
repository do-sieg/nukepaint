//! The effects of the Light menu, which cast shadows of what can be seen on the layer,
//! or light the image: darker towards its edges, through a lens, from a lamp, or with
//! glints on its brightest spots.

use std::{
    f32::consts::{SQRT_2, TAU},
    sync::atomic::{AtomicBool, Ordering},
};

use super::{
    Buffer, Effect, MAX_SAMPLES,
    artistic::{clamped, each_pixel, gaussian, lightness, noise, opaque_as, straight},
    fill_rows, smoothstep,
};

/// Where the light of the Lighting Effects comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightSource {
    /// A lamp above a point of the image, which lights it less the further from there.
    Spotlight,
    /// The sun, far away, which lights the whole image from the same way.
    Directional,
}

impl LightSource {
    pub const ALL: [Self; 2] = [Self::Spotlight, Self::Directional];
}

/// The ghosts of the Lens Flare: how far along the line from the light through the
/// middle of the image each is, as a share of the way to the middle, how wide, as a
/// share of the flare's size, and its colour.
const GHOSTS: [(f32, f32, [f32; 3]); 5] = [
    (0.6, 0.25, [0.6, 1.0, 0.6]),
    (1.25, 0.6, [0.5, 0.7, 1.0]),
    (1.5, 0.15, [1.0, 0.8, 0.5]),
    (1.9, 0.9, [0.7, 0.6, 1.0]),
    (2.4, 0.4, [1.0, 0.6, 0.6]),
];
/// How bright each ghost of the Lens Flare is, at full brightness.
const GHOST_LIGHT: f32 = 0.12;
/// How narrow the highlights of the Lighting Effects are: the power the way a pixel
/// faces between the light and the eye is raised to.
const SHININESS: f32 = 32.0;

/// The colours, multiplied by how opaque they are, as the light effect makes them.
/// Gives up once `stop` is set.
pub(super) fn render(source: &Buffer, effect: &Effect, stop: &AtomicBool) -> Option<Buffer> {
    let size = [source.width as f32, source.height as f32];
    let center = |[x, y]: [f32; 2]| [size[0] / 2.0 * (1.0 + x / 100.0), size[1] / 2.0 * (1.0 + y / 100.0)];
    match *effect {
        Effect::DropShadow { angle, distance, blur, opacity, color } => {
            let way = [angle.to_radians().cos() * distance, -angle.to_radians().sin() * distance];
            let moved: Vec<f32> = (0..source.height)
                .flat_map(|y| (0..source.width).map(move |x| [x as f32 + 0.5 - way[0], y as f32 + 0.5 - way[1]]))
                .map(|at| alpha_at(source, at))
                .collect();
            let cover = gaussian(&moved, source.width, source.height, blur, stop)?;
            cast(source, &cover, color, opacity / 100.0, stop)
        }
        Effect::LongShadow { angle, length, opacity, color, fade } => {
            let way = [angle.to_radians().cos(), -angle.to_radians().sin()];
            let samples = (length.ceil() as usize).clamp(1, MAX_SAMPLES);
            let step = length / samples as f32;
            let mut cover = vec![0.0; source.pixels.len()];
            let done = fill_rows(&mut cover, source.width, stop, |y, row| {
                for (x, out) in row.iter_mut().enumerate() {
                    let at = [x as f32 + 0.5, y as f32 + 0.5];
                    for i in 1..=samples {
                        let along = step * i as f32;
                        let weight = if fade { 1.0 - (along - step) / length } else { 1.0 };
                        // The nearer a pixel casting it, the darker, so none further can be darker.
                        if *out >= weight {
                            break;
                        }
                        *out = out.max(alpha_at(source, [at[0] - way[0] * along, at[1] - way[1] * along]) * weight);
                    }
                }
            });
            if !done {
                return None;
            }
            cast(source, &cover, color, opacity / 100.0, stop)
        }
        Effect::Vignette { amount, radius, softness, center: c, color } => {
            let [cx, cy] = center(c);
            let [r, g, b, alpha] = color.map(|channel| channel as f32 / 255.0);
            let amount = amount / 100.0 * alpha;
            let outer = radius / 100.0;
            let inner = outer * (1.0 - softness / 100.0);
            each_pixel(source, stop, |x, y| {
                // From 0 at the centre to 1 at the corners, were the centre the middle.
                let away = ((x as f32 + 0.5 - cx) / (size[0] / 2.0)).hypot((y as f32 + 0.5 - cy) / (size[1] / 2.0)) / SQRT_2;
                let dark = if outer - inner > f32::EPSILON { smoothstep(((away - inner) / (outer - inner)).clamp(0.0, 1.0)) } else { (away >= outer) as u8 as f32 };
                let mix = dark * amount;
                let [own_r, own_g, own_b, own_alpha] = straight(source.pixels[y * source.width + x]);
                opaque_as([own_r + (r - own_r) * mix, own_g + (g - own_g) * mix, own_b + (b - own_b) * mix], own_alpha)
            })
        }
        Effect::LensFlare { center: c, size: flare, brightness, rays, ghosts } => {
            let light = center(c);
            let middle = [size[0] / 2.0, size[1] / 2.0];
            let brightness = brightness / 100.0;
            // How long each ray is, as a share of how far the rays reach.
            let ray_lengths: Vec<f32> = (0..rays).map(|ray| 0.6 + 0.8 * noise(ray as i64, rays as i64, 0)).collect();
            each_pixel(source, stop, |x, y| {
                let (dx, dy) = (x as f32 + 0.5 - light[0], y as f32 + 0.5 - light[1]);
                let away = dx.hypot(dy);
                let core = (-(away / (0.2 * flare)).powi(2)).exp();
                let glow = (-away / (0.6 * flare)).exp() * 0.6;
                let halo = (-((away - 2.0 * flare) / (0.12 * flare)).powi(2)).exp() * 0.2;
                let mut sum: [f32; 3] = std::array::from_fn(|channel| core + glow * [1.0, 0.8, 0.55][channel] + halo * [0.55, 0.75, 1.0][channel]);
                if rays > 0 {
                    // Straight up, then evenly around.
                    let turn = dx.atan2(-dy).rem_euclid(TAU);
                    let ray = (turn * rays as f32 / TAU).round() as usize % rays as usize;
                    let spike = (turn * rays as f32 / 2.0).cos().abs().powi(80);
                    let streak = spike * (-away / (3.0 * flare * ray_lengths[ray])).exp() * 0.8;
                    sum = sum.map(|channel| channel + streak);
                }
                if ghosts {
                    for (along, width, tint) in GHOSTS {
                        let ghost = [light[0] + (middle[0] - light[0]) * along, light[1] + (middle[1] - light[1]) * along];
                        let radius = width * flare;
                        let from = (x as f32 + 0.5 - ghost[0]).hypot(y as f32 + 0.5 - ghost[1]);
                        let disc = smoothstep(((radius - from) / (0.3 * radius)).clamp(0.0, 1.0)) * GHOST_LIGHT;
                        for (channel, tint) in sum.iter_mut().zip(tint) {
                            *channel += disc * tint;
                        }
                    }
                }
                screened(source.pixels[y * source.width + x], sum.map(|channel| channel * brightness))
            })
        }
        Effect::LightingEffects { light: kind, center: c, height, azimuth, elevation, intensity, ambient, shine, bumps, color } => {
            let lamp = center(c);
            let height = height / 100.0 * size[0].max(size[1]);
            let (azimuth, elevation) = (azimuth.to_radians(), elevation.clamp(0.0, 90.0).to_radians());
            let sun = [azimuth.cos() * elevation.cos(), -azimuth.sin() * elevation.cos(), elevation.sin()];
            let [r, g, b, alpha] = color.map(|channel| channel as f32 / 255.0);
            let tint = [r * alpha, g * alpha, b * alpha];
            let (intensity, ambient, shine) = (intensity / 100.0, ambient / 100.0, shine / 100.0);
            let heights = if bumps > 0.0 { lightness(source) } else { Vec::new() };
            each_pixel(source, stop, |x, y| {
                let normal = if bumps > 0.0 {
                    let at = |x: isize, y: isize| heights[clamped(x, y, source.width, source.height)];
                    let (x, y) = (x as isize, y as isize);
                    let dx = (at(x + 1, y) - at(x - 1, y)) * bumps / 2.0;
                    let dy = (at(x, y + 1) - at(x, y - 1)) * bumps / 2.0;
                    normalized([-dx, -dy, 1.0])
                } else {
                    [0.0, 0.0, 1.0]
                };
                let (light, falloff) = match kind {
                    LightSource::Directional => (sun, 1.0),
                    LightSource::Spotlight => {
                        let way = [lamp[0] - x as f32 - 0.5, lamp[1] - y as f32 - 0.5, height];
                        let squared = way[0] * way[0] + way[1] * way[1];
                        (normalized(way), height * height / (height * height + squared))
                    }
                };
                let diffuse = dot(normal, light).max(0.0) * falloff * intensity;
                // Halfway between the light and the eye, straight above.
                let between = normalized([light[0], light[1], light[2] + 1.0]);
                let highlight = dot(normal, between).max(0.0).powf(SHININESS) * shine * falloff * intensity;
                let [own_r, own_g, own_b, own_alpha] = straight(source.pixels[y * source.width + x]);
                let lit = |channel: f32, tint: f32| channel * (ambient + diffuse * tint) + highlight * tint;
                opaque_as([lit(own_r, tint[0]), lit(own_g, tint[1]), lit(own_b, tint[2])], own_alpha)
            })
        }
        Effect::Sparkle { threshold, size: glint, points, angle, amount, seed } => {
            sparkle(source, threshold / 100.0, glint, points, angle, amount / 100.0, seed, stop)
        }
        _ => unreachable!("not a light effect"),
    }
}

/// The image over a shadow `cover` makes, one for each pixel from 0 to 1, in `color`,
/// `opacity` of it, from 0 to 1.
fn cast(source: &Buffer, cover: &[f32], color: [u8; 4], opacity: f32, stop: &AtomicBool) -> Option<Buffer> {
    let [r, g, b, alpha] = color.map(|channel| channel as f32 / 255.0);
    each_pixel(source, stop, |x, y| {
        let index = y * source.width + x;
        let own = source.pixels[index];
        let shadow = cover[index].clamp(0.0, 1.0) * opacity * alpha * (1.0 - own[3]);
        [own[0] + r * shadow, own[1] + g * shadow, own[2] + b * shadow, own[3] + shadow]
    })
}

/// How opaque the image is at a point, read between the four pixels around it, past
/// its edges see-through.
fn alpha_at(source: &Buffer, [x, y]: [f32; 2]) -> f32 {
    let (x, y) = (x - 0.5, y - 0.5);
    let (left, top) = (x.floor(), y.floor());
    let (tx, ty) = (x - left, y - top);
    let at = |x: f32, y: f32| {
        let inside = x >= 0.0 && y >= 0.0 && x < source.width as f32 && y < source.height as f32;
        if inside { source.pixels[y as usize * source.width + x as usize][3] } else { 0.0 }
    };
    let above = at(left, top) * (1.0 - tx) + at(left + 1.0, top) * tx;
    let below = at(left, top + 1.0) * (1.0 - tx) + at(left + 1.0, top + 1.0) * tx;
    above * (1.0 - ty) + below * ty
}

/// The colour, multiplied by how opaque it is, with `light`, red, green and blue as
/// bright as they are, laid over it as a screen does: on see-through pixels, the light
/// shows as much as it is bright.
fn screened(own: [f32; 4], light: [f32; 3]) -> [f32; 4] {
    let light = light.map(|channel| channel.clamp(0.0, 1.0));
    let alpha = light.into_iter().fold(0.0, f32::max);
    if alpha <= 0.0 {
        return own;
    }
    let behind = straight(own);
    let [r, g, b] = std::array::from_fn(|channel| {
        let ahead = light[channel] / alpha;
        let screen = ahead + behind[channel] - ahead * behind[channel];
        light[channel] * (1.0 - own[3]) + own[channel] * (1.0 - alpha) + alpha * own[3] * screen
    });
    [r, g, b, alpha + own[3] - alpha * own[3]]
}

/// Glints on the brightest spots, those at least as light as `threshold`, from 0 to 1:
/// one in each square `size` wide at most, the brightest there, `amount` of those, from
/// 0 to 1, picked as the seed says. Each is a star of `points` rays, the first turned by
/// `angle` from straight up, as long as `size` on the brightest spots and half as long
/// on those just past the threshold, in the spot's colour lightened halfway to white.
#[allow(clippy::too_many_arguments)]
fn sparkle(source: &Buffer, threshold: f32, size: f32, points: u32, angle: f32, amount: f32, seed: u32, stop: &AtomicBool) -> Option<Buffer> {
    let (width, height) = (source.width, source.height);
    let cell = size.max(2.0) as usize;
    let light: Vec<f32> = lightness(source).iter().zip(&source.pixels).map(|(light, pixel)| light * pixel[3]).collect();
    // Where each glint is, how bright its spot is past the threshold, from 0 to 1, and its colour.
    let mut glints = Vec::new();
    for top in (0..height).step_by(cell) {
        for left in (0..width).step_by(cell) {
            // A little noise picks among spots as bright, rather than always the first.
            let jittered = |index: usize| light[index] + noise((index % width) as i64, (index / width) as i64, seed) * 1e-4;
            let brightest = (top..(top + cell).min(height))
                .flat_map(|y| (left..(left + cell).min(width)).map(move |x| y * width + x))
                .max_by(|&a, &b| jittered(a).total_cmp(&jittered(b)))
                .unwrap();
            let picked = noise((left / cell) as i64, (top / cell) as i64, seed.wrapping_add(1)) < amount;
            if picked && light[brightest] > 0.0 && light[brightest] >= threshold {
                let strength = if threshold < 1.0 { ((light[brightest] - threshold) / (1.0 - threshold)).clamp(0.0, 1.0) } else { 1.0 };
                let [r, g, b, _] = straight(source.pixels[brightest]);
                glints.push((brightest % width, brightest / width, strength, [r, g, b].map(|channel| (channel + 1.0) / 2.0)));
            }
        }
    }
    let points = points.max(1);
    let rays: Vec<[f32; 2]> = (0..points)
        .map(|ray| {
            let turn = angle.to_radians() + TAU * ray as f32 / points as f32;
            [turn.sin(), -turn.cos()]
        })
        .collect();
    let mut lit = vec![[0.0f32; 3]; source.pixels.len()];
    for (x, y, strength, color) in glints {
        if stop.load(Ordering::Relaxed) {
            return None;
        }
        let length = size * (0.5 + 0.5 * strength);
        let reach = length.ceil() as isize;
        let core = 0.12 * length + 0.5;
        for dy in -reach..=reach {
            for dx in -reach..=reach {
                let (px, py) = (x as isize + dx, y as isize + dy);
                if px < 0 || py < 0 || px >= width as isize || py >= height as isize {
                    continue;
                }
                let (fx, fy) = (dx as f32, dy as f32);
                let mut glow = (-(fx.hypot(fy) / core).powi(2)).exp();
                for &[ux, uy] in &rays {
                    let along = fx * ux + fy * uy;
                    if along < 0.0 || along >= length {
                        continue;
                    }
                    let left = 1.0 - along / length;
                    // Narrowing to a point at its end.
                    let across = (fx * uy - fy * ux).abs() / (0.06 * length * left).max(0.5);
                    glow = glow.max(left * (-across * across).exp());
                }
                let glow = glow * (0.5 + 0.5 * strength);
                for (out, channel) in lit[py as usize * width + px as usize].iter_mut().zip(color) {
                    *out += glow * channel;
                }
            }
        }
    }
    each_pixel(source, stop, |x, y| screened(source.pixels[y * width + x], lit[y * width + x]))
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The same way, one long.
fn normalized(way: [f32; 3]) -> [f32; 3] {
    let length = dot(way, way).sqrt();
    if length > f32::EPSILON { way.map(|axis| axis / length) } else { [0.0, 0.0, 1.0] }
}

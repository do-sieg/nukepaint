//! The effects of the Photo menu that touch up the colours of a photo: the Soften
//! Portrait smooths skin, lights it and warms it. How opaque each pixel is stays the same.

use std::sync::atomic::AtomicBool;

use super::{
    Buffer, Effect, Weighing,
    artistic::{each_pixel, opaque_as, straight},
    blur, lightened,
};

/// How much of the warmth the green takes on, beside the red, which makes it orange
/// rather than pink.
const WARM_GREEN: f32 = 0.4;

/// The colours, multiplied by how opaque they are, as the photo effect makes them.
/// Gives up once `stop` is set.
pub(super) fn render(source: &Buffer, effect: &Effect, stop: &AtomicBool) -> Option<Buffer> {
    match *effect {
        Effect::SoftenPortrait { softness, lighting, warmth } => soften_portrait(source, softness, lighting / 100.0, warmth / 100.0, stop),
        _ => unreachable!("not a photo effect"),
    }
}

/// The image blurred by `softness`, laid halfway over itself where it is lighter, which
/// fills in small dark details, as wrinkles and blemishes, but keeps the edges. Then
/// lit by `lighting`, from -1 to 1, and warmed by `warmth`, from 0 to 1: more red and
/// less blue.
fn soften_portrait(source: &Buffer, softness: f32, lighting: f32, warmth: f32, stop: &AtomicBool) -> Option<Buffer> {
    let blurred = if softness < 1.0 { None } else { Some(blur(source, Weighing::Gaussian, softness, 1.0, stop)?) };
    let blurred = blurred.as_ref().unwrap_or(source);
    let width = source.width;
    each_pixel(source, stop, |x, y| {
        let own = straight(source.pixels[y * width + x]);
        let soft = straight(blurred.pixels[y * width + x]);
        let [r, g, b] = std::array::from_fn(|channel| lightened((own[channel] + own[channel].max(soft[channel])) / 2.0, lighting));
        opaque_as([lightened(r, warmth), lightened(g, warmth * WARM_GREEN), lightened(b, -warmth)], own[3])
    })
}

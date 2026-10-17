//! Turns, zooms and moves of a layer's pixels, or of its selected ones, as Rotate / Zoom
//! and 3D Transform make them: about the middle of the rectangle around them, and about
//! the axes across and down it as well, seen in perspective, as a card turned in front
//! of the eye.

use image::RgbaImage;

use crate::{Affine, FloatingPixels, Interpolation, PixelRect, Projective, SelectionMask};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Turn {
    /// In degrees, about the axis across, which brings the top nearer the eye; about the
    /// axis down, which brings the right side nearer; and about the axis out of the
    /// screen, clockwise. In that order.
    pub angles: [f64; 3],
    /// How wide the eye sees, in degrees, across the longer side of the pixels: the
    /// wider, the nearer it is, and the stronger the perspective.
    pub field_of_view: f64,
    /// How large the pixels are made, as a percentage.
    pub zoom: f64,
    /// How far they are moved right and down, as a percentage of their width and height,
    /// rounded to whole pixels.
    pub pan: [f64; 2],
}

impl Turn {
    /// Nothing turned, zoomed or moved.
    pub const NONE: Self = Self { angles: [0.0; 3], field_of_view: 60.0, zoom: 100.0, pan: [0.0; 2] };

    /// The map that carries the pixels of `area` where the turn takes them.
    pub fn map(&self, area: PixelRect) -> Projective {
        let (width, height) = (area.width() as f64, area.height() as f64);
        let center = (area.left as f64 + width / 2.0, area.top as f64 + height / 2.0);
        let [x, y, z] = self.angles.map(f64::to_radians);
        let rows = multiply(about_z(z), multiply(about_y(y), about_x(x)));
        // From the eye to the middle of the pixels, for them to fill the field of view.
        let half_view = (self.field_of_view.clamp(1.0, 179.0) / 2.0).to_radians();
        let distance = width.max(height).max(1.0) / 2.0 / half_view.tan();
        let scale = self.zoom / 100.0;
        // A point of the plane, from its middle, goes to (rows · point); what is further
        // from the eye than the middle is drawn smaller, by distance / (distance + depth).
        let turned = Projective {
            xx: scale * rows[0][0],
            xy: scale * rows[0][1],
            yx: scale * rows[1][0],
            yy: scale * rows[1][1],
            dx: 0.0,
            dy: 0.0,
            wx: rows[2][0] / distance,
            wy: rows[2][1] / distance,
            ww: 1.0,
        };
        let pan = ((self.pan[0] / 100.0 * width).round(), (self.pan[1] / 100.0 * height).round());
        Projective::from(Affine::translation(-center.0, -center.1))
            .then(turned)
            .then(Affine::translation(center.0 + pan.0, center.1 + pan.1).into())
    }
}

/// The selected pixels, as `mask` holds them, or all of them, turned as `turn` says about
/// the middle of the rectangle around them, and redrawn as `interpolation` says. Where
/// they were is left transparent, and those carried behind the eye are not drawn.
pub fn turn_pixels(pixels: &RgbaImage, mask: Option<&SelectionMask>, turn: &Turn, interpolation: Interpolation) -> RgbaImage {
    let Some(floating) = FloatingPixels::lift_masked(pixels, mask) else {
        return pixels.clone();
    };
    floating.place(turn.map(floating.area()), interpolation)
}

type Rows = [[f64; 3]; 3];

fn about_x(angle: f64) -> Rows {
    let (sin, cos) = angle.sin_cos();
    [[1.0, 0.0, 0.0], [0.0, cos, -sin], [0.0, sin, cos]]
}

fn about_y(angle: f64) -> Rows {
    let (sin, cos) = angle.sin_cos();
    [[cos, 0.0, sin], [0.0, 1.0, 0.0], [-sin, 0.0, cos]]
}

/// Clockwise on screen, as y goes down.
fn about_z(angle: f64) -> Rows {
    let (sin, cos) = angle.sin_cos();
    [[cos, -sin, 0.0], [sin, cos, 0.0], [0.0, 0.0, 1.0]]
}

/// `b`, then `a`.
fn multiply(a: Rows, b: Rows) -> Rows {
    std::array::from_fn(|row| std::array::from_fn(|column| (0..3).map(|k| a[row][k] * b[k][column]).sum()))
}

#[cfg(test)]
mod tests {
    use image::Rgba;

    use super::*;
    use crate::Selection;

    const RED: Rgba<u8> = Rgba([255, 0, 0, 255]);

    fn close(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9
    }

    const AREA: PixelRect = PixelRect { left: 0, top: 0, right: 20, bottom: 10 };

    #[test]
    fn nothing_turned_leaves_the_pixels() {
        let map = Turn::NONE.map(AREA);
        assert!(FloatingPixels::copies_exactly(&map));
        let pixels = RgbaImage::from_fn(4, 4, |x, y| Rgba([x as u8, y as u8, 0, 255]));
        assert_eq!(turn_pixels(&pixels, None, &Turn::NONE, Interpolation::Bicubic), pixels);
    }

    #[test]
    fn turns_zooms_and_pans_about_the_middle() {
        let quarter = Turn { angles: [0.0, 0.0, 90.0], ..Turn::NONE };
        // The right end of the middle line goes below the middle.
        assert!(close(quarter.map(AREA).apply((20.0, 5.0)), (10.0, 15.0)));
        let double = Turn { zoom: 200.0, pan: [10.0, -10.0], ..Turn::NONE };
        assert!(close(double.map(AREA).apply((0.0, 0.0)), (-10.0 + 2.0, -5.0 - 1.0)));
        assert!(double.map(AREA).affine().is_some());
    }

    #[test]
    fn tilting_brings_one_side_nearer() {
        let tilted = Turn { angles: [30.0, 0.0, 0.0], ..Turn::NONE }.map(AREA);
        assert!(tilted.affine().is_none());
        // The top comes nearer, so wider than the bottom; the middle stays put.
        let width = |y: f64| tilted.apply((20.0, y)).0 - tilted.apply((0.0, y)).0;
        assert!(width(0.0) > 20.0 && width(10.0) < 20.0);
        assert!(close(tilted.apply((10.0, 5.0)), (10.0, 5.0)));
        let turned = Turn { angles: [0.0, 30.0, 0.0], ..Turn::NONE }.map(AREA);
        let height = |x: f64| turned.apply((x, 10.0)).1 - turned.apply((x, 0.0)).1;
        assert!(height(20.0) > 10.0 && height(0.0) < 10.0);
    }

    #[test]
    fn turns_the_selected_pixels_alone() {
        let pixels = RgbaImage::from_pixel(8, 8, RED);
        let selection = Selection::rect(PixelRect { left: 0, top: 0, right: 4, bottom: 2 });
        let mask = SelectionMask::of(&selection, 8, 8).unwrap();
        let quarter = Turn { angles: [0.0, 0.0, 90.0], ..Turn::NONE };
        let turned = turn_pixels(&pixels, Some(&mask), &quarter, Interpolation::Nearest);
        // Upright about the middle of the 4 × 2 rectangle, at 2, 1: from 1 to 3 across and -1 to 3 down.
        assert_eq!(turned.get_pixel(0, 0)[3], 0);
        assert_eq!(*turned.get_pixel(1, 2), RED);
        assert_eq!(turned.get_pixel(3, 1)[3], 0);
        assert_eq!(*turned.get_pixel(6, 6), RED);
    }
}

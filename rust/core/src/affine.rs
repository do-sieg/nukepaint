//! Moves, turns and stretches of the image's plane.

/// Maps a point (x, y) to (xx·x + xy·y + dx, yx·x + yy·y + dy).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    pub xx: f64,
    pub yx: f64,
    pub xy: f64,
    pub yy: f64,
    pub dx: f64,
    pub dy: f64,
}

impl Affine {
    pub const IDENTITY: Self = Self { xx: 1.0, yx: 0.0, xy: 0.0, yy: 1.0, dx: 0.0, dy: 0.0 };

    pub fn translation(x: f64, y: f64) -> Self {
        Self { dx: x, dy: y, ..Self::IDENTITY }
    }

    pub fn scale(x: f64, y: f64) -> Self {
        Self { xx: x, yy: y, ..Self::IDENTITY }
    }

    /// Turns by `angle`, in radians, clockwise on screen as y goes down.
    pub fn rotation(angle: f64) -> Self {
        let (sin, cos) = angle.sin_cos();
        Self { xx: cos, yx: sin, xy: -sin, yy: cos, dx: 0.0, dy: 0.0 }
    }

    /// This, then `next`.
    pub fn then(self, next: Self) -> Self {
        Self {
            xx: next.xx * self.xx + next.xy * self.yx,
            yx: next.yx * self.xx + next.yy * self.yx,
            xy: next.xx * self.xy + next.xy * self.yy,
            yy: next.yx * self.xy + next.yy * self.yy,
            dx: next.xx * self.dx + next.xy * self.dy + next.dx,
            dy: next.yx * self.dx + next.yy * self.dy + next.dy,
        }
    }

    pub fn apply(&self, (x, y): (f64, f64)) -> (f64, f64) {
        (self.xx * x + self.xy * y + self.dx, self.yx * x + self.yy * y + self.dy)
    }

    /// The map back, if this one does not flatten the plane.
    pub fn inverse(&self) -> Option<Self> {
        let determinant = self.xx * self.yy - self.xy * self.yx;
        if determinant.abs() < 1e-12 {
            return None;
        }
        let (xx, yx, xy, yy) =
            (self.yy / determinant, -self.yx / determinant, -self.xy / determinant, self.xx / determinant);
        Some(Self { xx, yx, xy, yy, dx: -(xx * self.dx + xy * self.dy), dy: -(yx * self.dx + yy * self.dy) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9
    }

    #[test]
    fn then_applies_in_order() {
        let map = Affine::translation(-1.0, 0.0).then(Affine::scale(2.0, 3.0)).then(Affine::translation(10.0, 20.0));
        assert!(close(map.apply((2.0, 1.0)), (12.0, 23.0)));
    }

    #[test]
    fn rotation_turns_clockwise_on_screen() {
        assert!(close(Affine::rotation(std::f64::consts::FRAC_PI_2).apply((1.0, 0.0)), (0.0, 1.0)));
    }

    #[test]
    fn inverse_goes_back() {
        let map = Affine::rotation(0.3).then(Affine::scale(2.0, -0.5)).then(Affine::translation(4.0, 5.0));
        let back = map.inverse().unwrap();
        assert!(close(back.apply(map.apply((7.0, -3.0))), (7.0, -3.0)));
        assert!(Affine::scale(0.0, 1.0).inverse().is_none());
    }
}

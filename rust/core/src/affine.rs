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

/// Maps a point (x, y) to ((xx·x + xy·y + dx) / w, (yx·x + yy·y + dy) / w), where
/// w = wx·x + wy·y + ww: what an [`Affine`] map does, and the plane tilted away as well,
/// as seen in perspective. Lines stay straight, but parallel ones can meet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Projective {
    pub xx: f64,
    pub yx: f64,
    pub xy: f64,
    pub yy: f64,
    pub dx: f64,
    pub dy: f64,
    pub wx: f64,
    pub wy: f64,
    pub ww: f64,
}

impl Projective {
    pub const IDENTITY: Self = Self { xx: 1.0, yx: 0.0, xy: 0.0, yy: 1.0, dx: 0.0, dy: 0.0, wx: 0.0, wy: 0.0, ww: 1.0 };

    /// The map that takes each of the four corners `from` onto the one of `to` in the same
    /// place, if both go around an area.
    pub fn between(from: [(f64, f64); 4], to: [(f64, f64); 4]) -> Option<Self> {
        Some(Self::from_square(from)?.inverse()?.then(Self::from_square(to)?))
    }

    /// The map that takes the corners of the square from (0, 0) to (1, 1), going round
    /// from the first, onto `corners`.
    fn from_square([(x0, y0), (x1, y1), (x2, y2), (x3, y3)]: [(f64, f64); 4]) -> Option<Self> {
        let (sum_x, sum_y) = (x0 - x1 + x2 - x3, y0 - y1 + y2 - y3);
        let (dx1, dx2, dy1, dy2) = (x1 - x2, x3 - x2, y1 - y2, y3 - y2);
        let determinant = dx1 * dy2 - dx2 * dy1;
        if determinant.abs() < 1e-12 {
            return None;
        }
        let wx = (sum_x * dy2 - dx2 * sum_y) / determinant;
        let wy = (dx1 * sum_y - sum_x * dy1) / determinant;
        Some(Self {
            xx: x1 - x0 + wx * x1,
            yx: y1 - y0 + wx * y1,
            xy: x3 - x0 + wy * x3,
            yy: y3 - y0 + wy * y3,
            dx: x0,
            dy: y0,
            wx,
            wy,
            ww: 1.0,
        })
    }

    /// This, then `next`.
    pub fn then(self, next: Self) -> Self {
        let (a, b) = (next.rows(), self.rows());
        Self::from_rows(std::array::from_fn(|row| {
            std::array::from_fn(|column| (0..3).map(|k| a[row][k] * b[k][column]).sum())
        }))
    }

    pub fn apply(&self, (x, y): (f64, f64)) -> (f64, f64) {
        let w = self.w((x, y));
        ((self.xx * x + self.xy * y + self.dx) / w, (self.yx * x + self.yy * y + self.dy) / w)
    }

    /// What a point's coordinates are divided by: it is behind the eye where this is
    /// of the other sign than at points in front, and at no distance where it is 0.
    pub fn w(&self, (x, y): (f64, f64)) -> f64 {
        self.wx * x + self.wy * y + self.ww
    }

    /// The map back, if this one does not flatten the plane.
    pub fn inverse(&self) -> Option<Self> {
        let [[a, b, c], [d, e, f], [g, h, i]] = self.rows();
        let determinant = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
        if determinant.abs() < 1e-12 {
            return None;
        }
        let rows = [
            [e * i - f * h, c * h - b * i, b * f - c * e],
            [f * g - d * i, a * i - c * g, c * d - a * f],
            [d * h - e * g, b * g - a * h, a * e - b * d],
        ];
        Some(Self::from_rows(rows.map(|row| row.map(|value| value / determinant))))
    }

    /// The same map as an affine one, if it tilts nothing away.
    pub fn affine(&self) -> Option<Affine> {
        if self.wx.abs() > 1e-12 || self.wy.abs() > 1e-12 || self.ww.abs() < 1e-12 {
            return None;
        }
        let w = self.ww;
        Some(Affine { xx: self.xx / w, yx: self.yx / w, xy: self.xy / w, yy: self.yy / w, dx: self.dx / w, dy: self.dy / w })
    }

    fn rows(&self) -> [[f64; 3]; 3] {
        [[self.xx, self.xy, self.dx], [self.yx, self.yy, self.dy], [self.wx, self.wy, self.ww]]
    }

    fn from_rows([[xx, xy, dx], [yx, yy, dy], [wx, wy, ww]]: [[f64; 3]; 3]) -> Self {
        Self { xx, yx, xy, yy, dx, dy, wx, wy, ww }
    }
}

impl From<Affine> for Projective {
    fn from(Affine { xx, yx, xy, yy, dx, dy }: Affine) -> Self {
        Self { xx, yx, xy, yy, dx, dy, wx: 0.0, wy: 0.0, ww: 1.0 }
    }
}

impl From<&Affine> for Projective {
    fn from(map: &Affine) -> Self {
        Self::from(*map)
    }
}

impl From<&Projective> for Projective {
    fn from(map: &Projective) -> Self {
        *map
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9
    }

    #[test]
    fn projective_between_corners() {
        let from = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        let to = [(2.0, 1.0), (8.0, 1.0), (12.0, 9.0), (-2.0, 9.0)];
        let map = Projective::between(from, to).unwrap();
        for (a, b) in from.into_iter().zip(to) {
            assert!(close(map.apply(a), b));
        }
        // Straight lines stay straight: the middle of the square is where its diagonals meet.
        let middle = map.apply((5.0, 5.0));
        let along = |a: (f64, f64), b: (f64, f64)| (b.0 - a.0) * (middle.1 - a.1) - (b.1 - a.1) * (middle.0 - a.0);
        assert!(along(to[0], to[2]).abs() < 1e-9 && along(to[1], to[3]).abs() < 1e-9);
        let back = map.inverse().unwrap();
        assert!(close(back.apply(map.apply((3.0, 7.0))), (3.0, 7.0)));
        assert!(map.affine().is_none());
        // Corners all in a line go around nothing.
        assert!(Projective::between(from, [(0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (3.0, 0.0)]).is_none());
    }

    #[test]
    fn projective_of_a_parallelogram_is_affine() {
        let from = [(0.0, 0.0), (4.0, 0.0), (4.0, 2.0), (0.0, 2.0)];
        let sheared = [(1.0, 0.0), (5.0, 0.0), (3.0, 2.0), (-1.0, 2.0)];
        let affine = Projective::between(from, sheared).unwrap().affine().unwrap();
        assert!(close(affine.apply((0.0, 1.0)), (0.0, 1.0)));
        assert!(close(affine.apply((4.0, 0.0)), (5.0, 0.0)));
        let turn = Affine::rotation(0.4).then(Affine::translation(3.0, -1.0));
        let then = Projective::from(turn).then(Projective::from(Affine::scale(2.0, 2.0)));
        assert!(close(then.apply((1.0, 2.0)), turn.then(Affine::scale(2.0, 2.0)).apply((1.0, 2.0))));
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

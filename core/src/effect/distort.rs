//! The effects of the Distort menu, which move the pixels around rather than change
//! their colours: they bend, fold, wave or break up the image. Each pixel is read from
//! where the distortion takes it, between the four pixels around that point.

use std::{
    f32::consts::{FRAC_PI_2, PI, TAU},
    sync::atomic::AtomicBool,
};

use super::{Buffer, Effect, LUMA, MIN_ALPHA, Weighing, add, artistic::noise, blur, fill_rows, per_pixel, scale};

/// How close to the edge of its tile, as a share of the tile, the Tile Reflection
/// bends a point at most: right at it, the bend is endless.
const TILE_EDGE: f32 = 0.499;
/// The seed of the noise that places the Crystallize's cells, and that of the one that
/// gives the Wind's streaks their lengths.
const CRYSTAL_SEED: u32 = 7;
const WIND_SEED: u32 = 11;

/// The dots a Newsprint prints the image with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewsprintScreen {
    /// Black ink on white, as a newspaper's photos are.
    Black,
    /// Red, green and blue light on black, as a screen's.
    Rgb,
    /// Cyan, magenta, yellow and black inks on white, as a colour magazine's.
    Cmyk,
}

impl NewsprintScreen {
    pub const ALL: [Self; 3] = [Self::Black, Self::Rgb, Self::Cmyk];
}

/// The shape of the Ripple's waves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaveShape {
    Sine,
    Triangle,
    Sawtooth,
}

impl WaveShape {
    pub const ALL: [Self; 3] = [Self::Sine, Self::Triangle, Self::Sawtooth];

    /// How far up the wave is, from -1 to 1, this many waves along.
    fn at(self, waves: f32) -> f32 {
        let within = waves.rem_euclid(1.0);
        match self {
            Self::Sine => (waves * TAU).sin(),
            Self::Triangle => 4.0 * ((within - 0.25).rem_euclid(1.0) - 0.5).abs() - 1.0,
            Self::Sawtooth => 2.0 * within - 1.0,
        }
    }
}

/// The way the Wind blows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindDirection {
    Right,
    Left,
    Up,
    Down,
}

impl WindDirection {
    pub const ALL: [Self; 4] = [Self::Right, Self::Left, Self::Up, Self::Down];
}

/// The colours, multiplied by how opaque they are, as the distorting effect makes them.
/// Gives up once `stop` is set.
pub(super) fn render(source: &Buffer, effect: &Effect, stop: &AtomicBool) -> Option<Buffer> {
    let size = [source.width as f32, source.height as f32];
    let center = |[x, y]: [f32; 2]| [size[0] / 2.0 * (1.0 + x / 100.0), size[1] / 2.0 * (1.0 + y / 100.0)];
    // Half the image's shorter side, as large as a disc of 100 percent is.
    let half = size[0].min(size[1]) / 2.0;
    match *effect {
        Effect::Bulge { amount, radius, center: c } => {
            let (amount, reach, [cx, cy]) = (amount / 100.0, (half * radius / 100.0).max(f32::EPSILON), center(c));
            per_pixel(source, stop, |[x, y]| {
                let (dx, dy) = (x - cx, y - cy);
                let left = 1.0 - dx.hypot(dy) / reach;
                if left <= 0.0 {
                    return source.sample([x, y]);
                }
                let scale = 1.0 - amount * left * left;
                source.sample([cx + dx * scale, cy + dy * scale])
            })
        }
        Effect::Twist { angle, pinch, radius, center: c } => {
            let (angle, pinch, reach, [cx, cy]) = (angle.to_radians(), pinch / 100.0, (half * radius / 100.0).max(f32::EPSILON), center(c));
            per_pixel(source, stop, |[x, y]| {
                let (dx, dy) = (x - cx, y - cy);
                let distance = dx.hypot(dy) / reach;
                if distance >= 1.0 {
                    return source.sample([x, y]);
                }
                // As GIMP's Whirl and Pinch does.
                let factor = if distance > 0.0 { (FRAC_PI_2 * distance).sin().powf(-pinch) } else { 1.0 };
                let (sin, cos) = (angle * (1.0 - distance) * (1.0 - distance)).sin_cos();
                let (dx, dy) = (dx * factor, dy * factor);
                source.sample([cx + cos * dx + sin * dy, cy - sin * dx + cos * dy])
            })
        }
        Effect::Crystallize { size } => crystallize(source, size.max(1.0), stop),
        Effect::Pixelate { width, height } => {
            let (width, height) = (width.max(1) as usize, height.max(1) as usize);
            let columns = source.width.div_ceil(width);
            let cells = (0..source.height).flat_map(|y| (0..source.width).map(move |x| y / height * columns + x / width)).collect();
            flat_cells(source, cells, columns * source.height.div_ceil(height), stop)
        }
        Effect::PolarCoordinates { to_polar, rotation } => {
            let (rotation, [cx, cy]) = (rotation.to_radians(), center([0.0, 0.0]));
            per_pixel(source, stop, |[x, y]| {
                if to_polar {
                    // The angle clockwise from the top, and how far out on the ellipse as large as the image.
                    let (u, v) = ((x - cx) / cx, (y - cy) / cy);
                    let out = u.hypot(v);
                    if out > 1.0 {
                        return [0.0; 4];
                    }
                    let turn = ((u.atan2(-v) - rotation) / TAU).rem_euclid(1.0);
                    source.sample([turn * size[0], out * size[1]])
                } else {
                    let (turn, out) = (x / size[0] * TAU + rotation, y / size[1]);
                    sample_inside(source, [cx + out * turn.sin() * cx, cy - out * turn.cos() * cy])
                }
            })
        }
        Effect::TileReflection { size: tile, angle, curvature } => {
            let (tile, [cx, cy]) = (tile.max(1.0), center([0.0, 0.0]));
            let (sin, cos) = angle.to_radians().sin_cos();
            // As Paint.NET's does: a point is pushed away from the middle of its tile, a
            // tile from the image's middle, by `curvature` pixels times the tangent of
            // how far, more and more steeply towards the tile's edges.
            let bend = |at: f32| {
                let within = ((at / tile + 0.5).rem_euclid(1.0) - 0.5).clamp(-TILE_EDGE, TILE_EDGE);
                at + curvature * (PI * within).tan()
            };
            per_pixel(source, stop, |[x, y]| {
                let (dx, dy) = (x - cx, y - cy);
                let (u, v) = (bend(cos * dx - sin * dy), bend(sin * dx + cos * dy));
                source.sample([cx + cos * u + sin * v, cy - sin * u + cos * v])
            })
        }
        Effect::LensDistortion { main, edge, zoom, center: c } => {
            // As GIMP's does: a point is moved out by the square of how far it is from
            // the centre, and by its fourth power, a share of half the image's diagonal.
            let [cx, cy] = center(c);
            let unit = (size[0] * size[0] + size[1] * size[1]) / 4.0;
            let (main, edge, rescale) = (main / 200.0, edge / 200.0, 2f32.powf(-zoom / 100.0));
            per_pixel(source, stop, |[x, y]| {
                let (dx, dy) = (x - cx, y - cy);
                let out = (dx * dx + dy * dy) / unit;
                let factor = rescale * (1.0 + out * main + out * out * edge);
                sample_inside(source, [cx + dx * factor, cy + dy * factor])
            })
        }
        Effect::Kaleidoscope { segments, angle, center: c } => {
            let (wedge, start, [cx, cy]) = (PI / segments.max(1) as f32, angle.to_radians(), center(c));
            per_pixel(source, stop, |[x, y]| {
                let (dx, dy) = (x - cx, y - cy);
                let turn = ((-dy).atan2(dx) - start).rem_euclid(TAU);
                let index = (turn / wedge).floor();
                let within = turn - index * wedge;
                // Every other wedge is mirrored, so they meet seamlessly.
                let within = if index as u32 % 2 == 1 { wedge - within } else { within };
                let (sin, cos) = (start + within).sin_cos();
                let out = dx.hypot(dy);
                source.sample([cx + out * cos, cy - out * sin])
            })
        }
        Effect::Newsprint { size: cell, angle, screen } => newsprint(source, cell.max(2.0), angle, screen, stop),
        Effect::Ripple { amplitude, wavelength, phase, vertical, shape } => {
            let (wavelength, phase) = (wavelength.max(1.0), phase / 360.0);
            per_pixel(source, stop, |[x, y]| {
                if vertical {
                    source.sample([x + amplitude * shape.at(y / wavelength + phase), y])
                } else {
                    source.sample([x, y + amplitude * shape.at(x / wavelength + phase)])
                }
            })
        }
        Effect::Waves { amplitude, wavelength, phase, center: c } => {
            let (wavelength, phase, [cx, cy]) = (wavelength.max(1.0), phase.to_radians(), center(c));
            per_pixel(source, stop, |[x, y]| {
                let (dx, dy) = (x - cx, y - cy);
                let out = dx.hypot(dy);
                if out < f32::EPSILON {
                    return source.sample([x, y]);
                }
                let factor = (out + amplitude * (TAU * out / wavelength - phase).sin()) / out;
                source.sample([cx + dx * factor, cy + dy * factor])
            })
        }
        Effect::Wind { strength, threshold, direction, blast } => wind(source, strength, threshold / 100.0, direction, blast, stop),
        _ => unreachable!("not a distorting effect"),
    }
}

/// The colour at a point of the image, or none past its edges.
fn sample_inside(source: &Buffer, point: [f32; 2]) -> [f32; 4] {
    let inside = (0.0..=source.width as f32).contains(&point[0]) && (0.0..=source.height as f32).contains(&point[1]);
    if inside { source.sample(point) } else { [0.0; 4] }
}

/// Each pixel the average colour of the pixels of its cell, from `cells`, one for each
/// pixel, out of `count`.
fn flat_cells(source: &Buffer, cells: Vec<usize>, count: usize, stop: &AtomicBool) -> Option<Buffer> {
    let mut sums = vec![[0.0f32; 4]; count];
    let mut counts = vec![0u32; count];
    for (&cell, &color) in cells.iter().zip(&source.pixels) {
        add(&mut sums[cell], color, 1.0);
        counts[cell] += 1;
    }
    let averages: Vec<[f32; 4]> = sums.into_iter().zip(counts).map(|(sum, count)| scale(sum, 1.0 / count.max(1) as f32)).collect();
    let mut pixels = vec![[0.0; 4]; source.pixels.len()];
    let width = source.width;
    let done = fill_rows(&mut pixels, width, stop, |y, row| {
        for (x, out) in row.iter_mut().enumerate() {
            *out = averages[cells[y * width + x]];
        }
    });
    done.then(|| source.with_pixels(pixels))
}

/// Irregular cells, each of the pixels nearer a point than any other, the points one
/// in each square of a grid `size` wide, somewhere random in it: each the average
/// colour of its pixels.
fn crystallize(source: &Buffer, size: f32, stop: &AtomicBool) -> Option<Buffer> {
    let columns = (source.width as f32 / size).ceil() as usize;
    let rows = (source.height as f32 / size).ceil() as usize;
    let point = |column: usize, row: usize| {
        let jitter = |seed: u32| noise(column as i64, row as i64, CRYSTAL_SEED + seed);
        [(column as f32 + jitter(0)) * size, (row as f32 + jitter(1)) * size]
    };
    let points: Vec<[f32; 2]> = (0..rows).flat_map(|row| (0..columns).map(move |column| (column, row))).map(|(c, r)| point(c, r)).collect();
    let mut cells = vec![0; source.pixels.len()];
    let done = fill_rows(&mut cells, source.width, stop, |y, row| {
        let (py, cell_row) = (y as f32 + 0.5, (y as f32 / size) as usize);
        for (x, out) in row.iter_mut().enumerate() {
            let (px, cell_column) = (x as f32 + 0.5, (x as f32 / size) as usize);
            let mut nearest = (f32::MAX, 0);
            // The nearest point is in the square of the grid the pixel is in or one next to it.
            for r in cell_row.saturating_sub(1)..(cell_row + 2).min(rows) {
                for c in cell_column.saturating_sub(1)..(cell_column + 2).min(columns) {
                    let index = r * columns + c;
                    let [x, y] = points[index];
                    let distance = (x - px).powi(2) + (y - py).powi(2);
                    if distance < nearest.0 {
                        nearest = (distance, index);
                    }
                }
            }
            *out = nearest.1;
        }
    });
    if !done {
        return None;
    }
    flat_cells(source, cells, points.len(), stop)
}

/// The image printed in dots on grids of cells `cell` wide, turned by `angle` and more
/// for each ink as printers do, each dot as large as there is of its ink, or light,
/// around it. Past half, the gaps between the dots shrink instead.
fn newsprint(source: &Buffer, cell: f32, angle: f32, screen: NewsprintScreen, stop: &AtomicBool) -> Option<Buffer> {
    let averaged = blur(source, Weighing::Square, cell / 2.0, 1.0, stop)?;
    let angles: &[f32] = match screen {
        NewsprintScreen::Black => &[45.0],
        NewsprintScreen::Rgb => &[15.0, 75.0, 0.0],
        NewsprintScreen::Cmyk => &[15.0, 75.0, 0.0, 45.0],
    };
    let turns: Vec<(f32, f32)> = angles.iter().map(|&ink| (ink + angle).to_radians().sin_cos()).collect();
    per_pixel(source, stop, |[x, y]| {
        // How much of each ink, from 0 to 1, a pixel at the middle of its cell, `x` and `y`, gets.
        let dot = |ink: usize, amount: &dyn Fn([f32; 4]) -> f32| {
            let (sin, cos) = turns[ink];
            let (u, v) = (cos * x - sin * y, sin * x + cos * y);
            let (cell_u, cell_v) = ((u / cell).floor() + 0.5, (v / cell).floor() + 0.5);
            let middle = [cos * cell_u * cell + sin * cell_v * cell, -sin * cell_u * cell + cos * cell_v * cell];
            let ink = amount(straight(averaged.sample(middle))).clamp(0.0, 1.0);
            let (within_u, within_v) = (u / cell - cell_u, v / cell - cell_v);
            if ink <= 0.0 || ink >= 1.0 {
                ink
            } else if ink <= 0.5 {
                (0.5 + cell * (ink / PI).sqrt() - cell * within_u.hypot(within_v)).clamp(0.0, 1.0)
            } else {
                let corner = (0.5 - within_u.abs()).hypot(0.5 - within_v.abs());
                (0.5 + cell * corner - cell * ((1.0 - ink) / PI).sqrt()).clamp(0.0, 1.0)
            }
        };
        let alpha = source.pixels[(y as usize) * source.width + x as usize][3];
        let rgb = match screen {
            NewsprintScreen::Black => [1.0 - dot(0, &|[r, g, b, _]: [f32; 4]| 1.0 - (LUMA[0] * r + LUMA[1] * g + LUMA[2] * b)); 3],
            NewsprintScreen::Rgb => [dot(0, &|color: [f32; 4]| color[0]), dot(1, &|color: [f32; 4]| color[1]), dot(2, &|color: [f32; 4]| color[2])],
            NewsprintScreen::Cmyk => {
                let black = |[r, g, b, _]: [f32; 4]| 1.0 - r.max(g).max(b);
                let ink = |channel: usize| move |color: [f32; 4]| {
                    let black = black(color);
                    if black >= 1.0 { 0.0 } else { (1.0 - color[channel] - black) / (1.0 - black) }
                };
                let black = 1.0 - dot(3, &black);
                [(1.0 - dot(0, &ink(0))) * black, (1.0 - dot(1, &ink(1))) * black, (1.0 - dot(2, &ink(2))) * black]
            }
        };
        [rgb[0] * alpha, rgb[1] * alpha, rgb[2] * alpha, alpha]
    })
}

/// Streaks blown from the edges where the image gets darker the way the wind blows,
/// by more than `threshold`, from 0 to 1, of their colour, up to `strength` long,
/// fading out, or all as strong but half as long if a blast.
fn wind(source: &Buffer, strength: f32, threshold: f32, direction: WindDirection, blast: bool, stop: &AtomicBool) -> Option<Buffer> {
    // Turned so it blows to the right.
    let across = matches!(direction, WindDirection::Up | WindDirection::Down);
    let backwards = matches!(direction, WindDirection::Left | WindDirection::Up);
    let mut turned = if across { source.transposed() } else { source.with_pixels(source.pixels.clone()) };
    if backwards {
        reverse_rows(&mut turned);
    }
    let longest = if blast { strength / 2.0 } else { strength };
    let width = turned.width;
    let light = |[r, g, b, _]: [f32; 4]| LUMA[0] * r + LUMA[1] * g + LUMA[2] * b;
    let mut pixels = vec![[0.0; 4]; turned.pixels.len()];
    let done = fill_rows(&mut pixels, width, stop, |y, row| {
        let line = &turned.pixels[y * width..(y + 1) * width];
        // The colour of the streak blowing over the pixel, how long it is and how far it has gone.
        let mut streak: Option<([f32; 4], f32, f32)> = None;
        for (x, out) in row.iter_mut().enumerate() {
            let own = line[x];
            *out = own;
            if let Some((color, length, gone)) = streak {
                let weight = if blast { 1.0 } else { 1.0 - gone / length };
                let mut mixed = scale(own, 1.0 - weight);
                add(&mut mixed, color, weight);
                *out = mixed;
                streak = (gone + 1.0 < length).then_some((color, length, gone + 1.0));
            }
            if x + 1 < width && light(own) - light(line[x + 1]) > threshold {
                let length = longest * (0.25 + 0.75 * noise(x as i64, y as i64, WIND_SEED));
                if length >= 1.0 {
                    streak = Some((*out, length, 0.0));
                }
            }
        }
    });
    if !done {
        return None;
    }
    let mut blown = turned.with_pixels(pixels);
    if backwards {
        reverse_rows(&mut blown);
    }
    Some(if across { blown.transposed() } else { blown })
}

fn reverse_rows(buffer: &mut Buffer) {
    for row in buffer.pixels.chunks_mut(buffer.width) {
        row.reverse();
    }
}

/// The colour divided by how opaque it is again, or none for one that is see-through.
fn straight([r, g, b, a]: [f32; 4]) -> [f32; 4] {
    if a >= MIN_ALPHA { [r / a, g / a, b / a, a] } else { [0.0; 4] }
}

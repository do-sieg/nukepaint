//! How an image is shown: how far it is zoomed in and where it is scrolled to.
//!
//! Tiled, the image, or a rectangle of it, is shown as a tile repeated side by side,
//! with no end to how far it scrolls: the copies it scrolls past are let go of on one
//! side as new ones come in on the other, so that the tile itself is always the copy
//! nearest the middle.

use eframe::egui::{Pos2, Rect, Vec2};
use power_paint_core::PixelRect;

/// The steps Zoom In and Zoom Out go through.
pub const ZOOM_LEVELS: [f32; 26] = [
    0.01, 0.02, 0.03, 0.04, 0.05, 0.06, 0.08, 0.12, 0.16, 0.25, 0.33, 0.5, 0.66, 1.0, 1.5, 2.0, 3.0, 4.0, 5.0, 6.0,
    7.0, 8.0, 12.0, 16.0, 24.0, 32.0,
];

pub struct View {
    /// Screen pixels per image pixel: 1.0 shows the image at its actual size.
    pub zoom: f32,
    /// How far the view is scrolled, in points.
    pub offset: Vec2,
    /// Where the view was on screen when last shown, and the screen's scale then.
    viewport: Rect,
    pixels_per_point: f32,
    /// Size of the image, in pixels.
    image_size: Vec2,
    /// Set until the view is first shown, to fit a large image in it.
    first_show: bool,
    /// The rectangle of the image shown tiled, in pixels, if it is.
    tile: Option<PixelRect>,
    /// Set while a tool drags across a tiled image, which must not jump by a copy
    /// under it: it then scrolls no further than a copy each way.
    held: bool,
}

impl View {
    pub fn new(image_size: Vec2) -> Self {
        Self {
            zoom: 1.0,
            offset: Vec2::ZERO,
            viewport: Rect::NOTHING,
            pixels_per_point: 1.0,
            image_size,
            first_show: true,
            tile: None,
            held: false,
        }
    }

    pub fn tiled(&self) -> bool {
        self.tile.is_some()
    }

    pub fn tile(&self) -> Option<PixelRect> {
        self.tile
    }

    /// All of the image, as a tile.
    pub fn whole(&self) -> PixelRect {
        PixelRect { left: 0, top: 0, right: self.image_size.x as i32, bottom: self.image_size.y as i32 }
    }

    /// Shows `tile`, a rectangle of the image, tiled, or the image as it is. What is in
    /// the middle of the view stays there, unless it is off the tile: then the tile is.
    pub fn set_tile(&mut self, tile: Option<PixelRect>) {
        let (x, y) = self.point_at(self.viewport.center());
        let mut middle = self.wrap_point(Pos2::new(x as f32, y as f32));
        self.tile = tile.filter(|tile| !tile.is_empty());
        if let Some(tile) = self.tile
            && !to_rect(tile).contains(middle)
        {
            middle = to_rect(tile).center();
        }
        self.center_on(middle);
    }

    /// Keeps a tiled image from jumping by a copy while `held`, as while a tool drags across it.
    pub fn hold(&mut self, held: bool) {
        self.held = held;
        if !held {
            self.pan(Vec2::ZERO);
        }
    }

    /// Size of what scrolls, in points: the image, or tiled, a copy more each way than the view.
    pub fn scroll_size(&self) -> Vec2 {
        match self.tile_size_on_screen() {
            Some(size) => self.viewport.size() + size * 2.0,
            None => self.image_size_on_screen(),
        }
    }

    /// Size of the tile on screen, in points, if the image is tiled.
    fn tile_size_on_screen(&self) -> Option<Vec2> {
        self.tile.map(|tile| to_rect(tile).size() * self.zoom / self.pixels_per_point)
    }

    /// Where the tile itself is on screen, if the image is tiled.
    pub fn tile_rect(&self) -> Option<Rect> {
        let tile = to_rect(self.tile?);
        Some(Rect::from_min_max(self.to_screen(tile.min.x, tile.min.y), self.to_screen(tile.max.x, tile.max.y)))
    }

    /// Scrolls to `offset`, in points, as far as the image goes, or tiled, to the same
    /// place on the copy nearest the middle.
    pub fn scroll_to(&mut self, offset: Vec2) {
        let max = (self.scroll_size() - self.viewport.size()).max(Vec2::ZERO);
        self.offset = if let Some(size) = self.tile_size_on_screen().filter(|_| !self.held) {
            // A copy away from the start, the tile is in the middle of the view.
            let wrap = |offset: f32, size: f32| size + (offset - size + size / 2.0).rem_euclid(size) - size / 2.0;
            Vec2::new(wrap(offset.x, size.x), wrap(offset.y, size.y))
        } else {
            offset.clamp(Vec2::ZERO, max)
        };
    }

    /// How far the image's corner is from where the view's would be with no scrolling.
    fn lead(&self) -> Vec2 {
        match (self.tile, self.tile_size_on_screen()) {
            (Some(tile), Some(size)) => {
                let corner = Vec2::new(tile.left as f32, tile.top as f32) * self.zoom / self.pixels_per_point;
                (self.viewport.size() + size) / 2.0 - corner
            }
            _ => ((self.viewport.size() - self.image_size_on_screen()) / 2.0).max(Vec2::ZERO),
        }
    }

    /// The point of the image that `point` shows, on or off it: tiled, the one of the
    /// tile that a copy shows there.
    fn wrap_point(&self, point: Pos2) -> Pos2 {
        let Some(tile) = self.tile.map(to_rect) else {
            return point;
        };
        let along = |at: f32, start: f32, size: f32| start + (at - start).rem_euclid(size);
        Pos2::new(along(point.x, tile.min.x, tile.width()), along(point.y, tile.min.y, tile.height()))
    }

    /// The pixel of the image that `pixel` shows, on or off it: tiled, the one of the
    /// tile that a copy shows there.
    pub fn wrap_pixel(&self, (x, y): (i64, i64)) -> (i64, i64) {
        let Some(tile) = self.tile else {
            return (x, y);
        };
        let along = |at: i64, start: i32, size: i32| start as i64 + (at - start as i64).rem_euclid(size as i64);
        (along(x, tile.left, tile.width()), along(y, tile.top, tile.height()))
    }

    /// Takes note of where the view is on screen, before it is drawn. An image too
    /// large for the view the first time is zoomed out to fit in it.
    pub fn place(&mut self, viewport: Rect, pixels_per_point: f32) {
        self.viewport = viewport;
        self.pixels_per_point = pixels_per_point;
        if std::mem::take(&mut self.first_show) {
            let fits = self.image_size.x <= viewport.width() * pixels_per_point
                && self.image_size.y <= viewport.height() * pixels_per_point;
            if !fits {
                self.zoom_to_window();
            }
        }
    }

    /// Takes in another size of the image, keeping the zoom, and scrolled no further
    /// than the image now goes.
    pub fn set_image_size(&mut self, image_size: Vec2) {
        let whole = self.tile == Some(self.whole());
        self.image_size = image_size;
        // A tile is cut down to what is left of it, or the image goes untiled if nothing is.
        self.tile = match self.tile {
            Some(_) if whole => Some(self.whole()),
            Some(tile) => Some(intersect(tile, self.whole())).filter(|tile| !tile.is_empty()),
            None => None,
        };
        self.pan(Vec2::ZERO);
    }

    /// Size of the image on screen, in points.
    pub fn image_size_on_screen(&self) -> Vec2 {
        self.image_size * self.zoom / self.pixels_per_point
    }

    /// Where the image is on screen: at the top left of what is scrolled to, or
    /// in the middle of the view along a side where it is smaller than the view.
    pub fn image_rect(&self) -> Rect {
        let size = self.image_size_on_screen();
        let corner = self.viewport.min - self.offset + self.lead();
        // On whole screen pixels, for enlarged pixels to keep their edges.
        let corner = (corner.to_vec2() * self.pixels_per_point).round() / self.pixels_per_point;
        Rect::from_min_size(corner.to_pos2(), size)
    }

    /// The image pixel at a point of the screen, which can be outside the image.
    pub fn pixel_at(&self, point: Pos2) -> (i64, i64) {
        let pixel = (point - self.image_rect().min) * self.pixels_per_point / self.zoom;
        (pixel.x.floor() as i64, pixel.y.floor() as i64)
    }

    /// Where a point of the screen is on the image, in pixels, to a fraction of one.
    pub fn point_at(&self, point: Pos2) -> (f64, f64) {
        let point = (point - self.image_rect().min) * self.pixels_per_point / self.zoom;
        (point.x as f64, point.y as f64)
    }

    /// The pixels of the image that are in view, whole or in part.
    pub fn visible_pixels(&self) -> PixelRect {
        let (left, top) = self.point_at(self.viewport.min);
        let (right, bottom) = self.point_at(self.viewport.max);
        let (width, height) = (self.image_size.x as f64, self.image_size.y as f64);
        PixelRect {
            left: left.floor().clamp(0.0, width) as i32,
            top: top.floor().clamp(0.0, height) as i32,
            right: right.ceil().clamp(0.0, width) as i32,
            bottom: bottom.ceil().clamp(0.0, height) as i32,
        }
    }

    /// The part of the image in view, in pixels, to a fraction of one: tiled, it may
    /// go past the image's edges, onto the copies.
    pub fn visible_area(&self) -> Rect {
        let (left, top) = self.point_at(self.viewport.min);
        let (right, bottom) = self.point_at(self.viewport.max);
        let area = Rect::from_min_max(Pos2::new(left as f32, top as f32), Pos2::new(right as f32, bottom as f32));
        if self.tiled() { area } else { area.intersect(Rect::from_min_size(Pos2::ZERO, self.image_size)) }
    }

    /// The part of the image that [`View::center_on`] would bring into view.
    pub fn area_centered_on(&self, point: Pos2) -> Rect {
        let size = self.viewport.size() * self.pixels_per_point / self.zoom;
        if self.tiled() {
            return Rect::from_center_size(point, size);
        }
        let max = (self.image_size - size).max(Vec2::ZERO);
        let min = (point.to_vec2() - size / 2.0).clamp(Vec2::ZERO, max);
        Rect::from_min_size(min.to_pos2(), size).intersect(Rect::from_min_size(Pos2::ZERO, self.image_size))
    }

    /// Scrolls to have a point of the image, in pixels, in the middle of the view, as
    /// far as scrolling allows.
    pub fn center_on(&mut self, point: Pos2) {
        self.pan(self.viewport.center() - self.to_screen(point.x, point.y));
    }

    /// How many pixels of the image a point of the screen takes.
    pub fn pixels_per_screen_point(&self) -> f64 {
        (self.pixels_per_point / self.zoom) as f64
    }

    /// The corner between pixels nearest to a point of the screen, within the image.
    pub fn corner_at(&self, point: Pos2) -> (i32, i32) {
        self.nearest_corner(self.point_at(point))
    }

    /// The corner between pixels nearest to a point of the image, within it.
    pub fn nearest_corner(&self, (x, y): (f64, f64)) -> (i32, i32) {
        let max = self.image_size;
        (x.round().clamp(0.0, max.x as f64) as i32, y.round().clamp(0.0, max.y as f64) as i32)
    }

    /// Where a point of the image, in pixels, is on screen.
    pub fn to_screen(&self, x: f32, y: f32) -> Pos2 {
        self.image_rect().min + Vec2::new(x, y) * self.zoom / self.pixels_per_point
    }

    pub fn can_zoom_in(&self) -> bool {
        self.zoom < ZOOM_LEVELS[ZOOM_LEVELS.len() - 1]
    }

    pub fn can_zoom_out(&self) -> bool {
        self.zoom > ZOOM_LEVELS[0]
    }

    pub fn zoom_in(&mut self) {
        self.step_zoom(1, self.viewport.center());
    }

    pub fn zoom_out(&mut self) {
        self.step_zoom(-1, self.viewport.center());
    }

    /// Zooms about the middle of the view.
    pub fn zoom_to(&mut self, zoom: f32) {
        self.set_zoom(zoom, self.viewport.center());
    }

    /// Shows the whole image, as large as the view allows.
    pub fn zoom_to_window(&mut self) {
        let fit = self.viewport.size() * self.pixels_per_point / self.image_size;
        self.set_zoom(fit.x.min(fit.y), self.viewport.center());
    }

    /// Shows an area of the image, in pixels, as large as the view allows, in the
    /// middle of the view as far as scrolling allows.
    pub fn zoom_to_rect(&mut self, area: Rect) {
        let fit = self.viewport.size() * self.pixels_per_point / area.size();
        self.set_zoom(fit.x.min(fit.y), self.viewport.center());
        self.center_on(area.center());
    }

    /// Moves to the next zoom level up or down. What is under `anchor`, a point of
    /// the screen, stays there.
    pub fn step_zoom(&mut self, direction: i32, anchor: Pos2) {
        // The zoom may sit between two levels, or a rounding error away from one.
        let next = |level: &&f32| (**level - self.zoom) * direction as f32 > 0.001 * self.zoom;
        let level = if direction > 0 {
            ZOOM_LEVELS.iter().find(next)
        } else {
            ZOOM_LEVELS.iter().rev().find(next)
        };
        if let Some(&level) = level {
            self.set_zoom(level, anchor);
        }
    }

    /// Zooms so that what is under `anchor`, a point of the screen, stays there, as
    /// far as scrolling allows.
    pub fn set_zoom(&mut self, zoom: f32, anchor: Pos2) {
        let zoom = zoom.clamp(ZOOM_LEVELS[0], ZOOM_LEVELS[ZOOM_LEVELS.len() - 1]);
        let anchored = (anchor - self.image_rect().min) / self.image_size_on_screen();
        self.zoom = zoom;
        let size = self.image_size_on_screen();
        self.scroll_to(self.viewport.min.to_vec2() + self.lead() + anchored * size - anchor.to_vec2());
    }

    /// Scrolls by this much, in points, as far as the image goes.
    pub fn pan(&mut self, delta: Vec2) {
        self.scroll_to(self.offset - delta);
    }
}

/// A rectangle of pixels, as a rectangle of points of the image.
fn to_rect(rect: PixelRect) -> Rect {
    Rect::from_min_max(Pos2::new(rect.left as f32, rect.top as f32), Pos2::new(rect.right as f32, rect.bottom as f32))
}

fn intersect(a: PixelRect, b: PixelRect) -> PixelRect {
    PixelRect { left: a.left.max(b.left), top: a.top.max(b.top), right: a.right.min(b.right), bottom: a.bottom.min(b.bottom) }
}

/// The zoom as a percentage.
pub fn format_zoom(zoom: f32) -> String {
    format!("{}%", (zoom * 100.0).round())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placed(image: Vec2, viewport: Vec2) -> View {
        let mut view = View::new(image);
        view.place(Rect::from_min_size(Pos2::new(10.0, 20.0), viewport), 1.0);
        view
    }

    #[test]
    fn small_image_opens_at_actual_size_centered() {
        let view = placed(Vec2::new(100.0, 50.0), Vec2::new(400.0, 300.0));
        assert_eq!(view.zoom, 1.0);
        assert_eq!(view.image_rect(), Rect::from_min_size(Pos2::new(160.0, 145.0), Vec2::new(100.0, 50.0)));
    }

    #[test]
    fn large_image_opens_fitted() {
        let view = placed(Vec2::new(12000.0, 9000.0), Vec2::new(1200.0, 600.0));
        assert!((view.zoom - 600.0 / 9000.0).abs() < 1e-6);
        assert!(view.image_rect().height() <= 600.0 + 0.01);
    }

    #[test]
    fn steps_through_levels() {
        let mut view = placed(Vec2::new(100.0, 100.0), Vec2::new(400.0, 400.0));
        view.zoom_in();
        assert_eq!(view.zoom, 1.5);
        view.zoom_out();
        view.zoom_out();
        assert_eq!(view.zoom, 0.66);
        // From between two levels, to the nearest one each way.
        view.zoom = 0.7;
        view.zoom_in();
        assert_eq!(view.zoom, 1.0);
        view.zoom = 0.7;
        view.zoom_out();
        assert_eq!(view.zoom, 0.66);
    }

    #[test]
    fn stops_at_the_ends() {
        let mut view = placed(Vec2::new(100.0, 100.0), Vec2::new(400.0, 400.0));
        view.zoom_to(32.0);
        assert!(!view.can_zoom_in());
        view.zoom_in();
        assert_eq!(view.zoom, 32.0);
        view.zoom_to(0.001);
        assert_eq!(view.zoom, 0.01);
        assert!(!view.can_zoom_out());
    }

    #[test]
    fn zoom_keeps_the_anchored_pixel_under_it() {
        let mut view = placed(Vec2::new(1000.0, 1000.0), Vec2::new(400.0, 400.0));
        view.zoom_to(1.0);
        let anchor = Pos2::new(110.0, 70.0);
        let before = view.pixel_at(anchor);
        view.step_zoom(1, anchor);
        view.step_zoom(1, anchor);
        assert_eq!(view.zoom, 2.0);
        assert_eq!(view.pixel_at(anchor), before);
    }

    #[test]
    fn zoom_to_rect_fits_and_centers_the_area() {
        let mut view = placed(Vec2::new(1000.0, 1000.0), Vec2::new(400.0, 200.0));
        view.zoom_to_rect(Rect::from_min_size(Pos2::new(600.0, 300.0), Vec2::new(100.0, 25.0)));
        assert_eq!(view.zoom, 4.0);
        assert_eq!(view.pixel_at(Pos2::new(210.0, 120.0)), (650, 312));
        // An area too thin to fit stops at the largest zoom.
        view.zoom_to_rect(Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::new(0.0, 5.0)));
        assert_eq!(view.zoom, 32.0);
    }

    #[test]
    fn corners_round_to_the_nearest_within_the_image() {
        let mut view = placed(Vec2::new(100.0, 50.0), Vec2::new(400.0, 300.0));
        view.zoom_to(4.0);
        let origin = view.image_rect().min;
        assert_eq!(view.corner_at(origin + Vec2::new(9.0, 6.0)), (2, 2));
        assert_eq!(view.corner_at(origin + Vec2::new(-50.0, 1000.0)), (0, 50));
        assert_eq!(view.to_screen(2.0, 2.0), origin + Vec2::new(8.0, 8.0));
    }

    #[test]
    fn center_on_shows_the_area_it_said() {
        let mut view = placed(Vec2::new(1000.0, 1000.0), Vec2::new(400.0, 200.0));
        view.zoom_to(1.0);
        let point = Pos2::new(700.0, 100.0);
        let area = view.area_centered_on(point);
        assert_eq!(area, Rect::from_min_size(Pos2::new(500.0, 0.0), Vec2::new(400.0, 200.0)));
        view.center_on(point);
        assert_eq!(view.visible_area(), area);
        // Near an edge, as far as scrolling allows.
        let area = view.area_centered_on(Pos2::new(990.0, 990.0));
        assert_eq!(area, Rect::from_min_size(Pos2::new(600.0, 800.0), Vec2::new(400.0, 200.0)));
        view.center_on(Pos2::new(990.0, 990.0));
        assert_eq!(view.visible_area(), area);
    }

    #[test]
    fn tiled_scrolls_on_past_the_edges() {
        let mut view = placed(Vec2::new(100.0, 100.0), Vec2::new(400.0, 300.0));
        view.set_tile(Some(view.whole()));
        // In the middle, as it was.
        assert_eq!(view.image_rect(), Rect::from_min_size(Pos2::new(160.0, 120.0), Vec2::new(100.0, 100.0)));
        let pixel = view.pixel_at(Pos2::new(200.0, 150.0));
        // Scrolled a copy and a half: the image is the copy nearest the middle again,
        // and the same pixel of it shows half a copy away.
        view.pan(Vec2::new(-150.0, 0.0));
        let shown = view.pixel_at(Pos2::new(200.0, 150.0));
        assert_eq!(view.wrap_pixel(shown), view.wrap_pixel((pixel.0 + 150, pixel.1)));
        assert!(view.image_rect().center().distance(view.viewport.center()) <= 50.0 * 2f32.sqrt() + 0.01);
        // Held, it scrolls no further than a copy, and stays put.
        view.hold(true);
        let before = view.image_rect();
        view.pan(Vec2::new(-30.0, 0.0));
        assert_eq!(view.image_rect(), before.translate(Vec2::new(-30.0, 0.0)));
        view.hold(false);
        assert_eq!(view.wrap_pixel((-1, 205)), (99, 5));
        // Back untiled, the image is whole and in the middle again.
        view.set_tile(None);
        assert_eq!(view.image_rect(), Rect::from_min_size(Pos2::new(160.0, 120.0), Vec2::new(100.0, 100.0)));
    }

    #[test]
    fn a_tile_of_the_image_goes_in_the_middle() {
        let mut view = placed(Vec2::new(100.0, 100.0), Vec2::new(400.0, 300.0));
        let tile = PixelRect { left: 60, top: 20, right: 80, bottom: 50 };
        view.set_tile(Some(tile));
        // The middle of the view was off the tile, so the tile's is there now.
        assert_eq!(view.tile_rect(), Some(Rect::from_min_size(Pos2::new(200.0, 155.0), Vec2::new(20.0, 30.0))));
        assert_eq!(view.wrap_pixel((59, 50)), (79, 20));
        view.pan(Vec2::new(-35.0, 0.0));
        assert_eq!(view.tile_rect().unwrap().size(), Vec2::new(20.0, 30.0));
        assert!((view.tile_rect().unwrap().center().x - view.viewport.center().x).abs() <= 10.0);
        // Cut down with the image, and gone once nothing is left of it.
        view.set_image_size(Vec2::new(70.0, 70.0));
        assert_eq!(view.tile(), Some(PixelRect { left: 60, top: 20, right: 70, bottom: 50 }));
        view.set_image_size(Vec2::new(50.0, 50.0));
        assert_eq!(view.tile(), None);
    }

    #[test]
    fn pan_stays_within_the_image() {
        let mut view = placed(Vec2::new(1000.0, 1000.0), Vec2::new(400.0, 400.0));
        view.zoom_to(1.0);
        // Actual Size zooms about the middle: the view is scrolled to (300, 300).
        view.pan(Vec2::new(-5000.0, 100.0));
        assert_eq!(view.offset, Vec2::new(600.0, 200.0));
        view.pan(Vec2::new(0.0, 1000.0));
        assert_eq!(view.offset, Vec2::new(600.0, 0.0));
    }
}

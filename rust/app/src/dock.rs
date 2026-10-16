//! The docks: panels along the edges of the window that hold the sections, each
//! under a header that folds it away when clicked. Down the left or right side they
//! go one above the other; along the top or bottom, one beside the other, each with
//! its own scroll. Only the edges with sections have a dock.
//!
//! A section is moved by dragging its header: into a dock, among its sections, or
//! anywhere else, to the edge of the drawing area it is let go nearest to. Each dock
//! can be made wider or narrower from its inner edge.

use eframe::egui::{
    self, Align2, Color32, CursorIcon, Id, LayerId, Order, PanelState, Pos2, Rect, Sense, Stroke, StrokeKind,
    TextStyle, Vec2, collapsing_header::CollapsingState,
};
use serde::{Deserialize, Serialize};

const DEFAULT_WIDTH: f32 = 240.0;
const MIN_WIDTH: f32 = 180.0;
const DEFAULT_HEIGHT: f32 = 240.0;
const MIN_HEIGHT: f32 = 64.0;
/// How wide each section is when they go one beside the other.
const SECTION_WIDTH: f32 = 240.0;
/// The dots at the end of a header, which say it can be dragged: across and down,
/// and their room.
const GRIP_DOTS: (usize, usize) = (2, 3);
const GRIP_DOT_SPACING: f32 = 4.0;
const GRIP_DOT_RADIUS: f32 = 1.0;
/// How thick the line is that marks where a dragged section would go.
const INSERT_LINE_WIDTH: f32 = 3.0;

/// Which edge of the window a dock is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockSide {
    Left,
    Right,
    Top,
    Bottom,
}

impl DockSide {
    /// Also the order the docks are laid out in: the side ones go the full height
    /// of the window, the top and bottom ones between them.
    pub const ALL: [Self; 4] = [Self::Left, Self::Right, Self::Top, Self::Bottom];

    pub fn name(self) -> &'static str {
        match self {
            Self::Left => "Left",
            Self::Right => "Right",
            Self::Top => "Top",
            Self::Bottom => "Bottom",
        }
    }

    /// Whether the dock goes down a side, rather than along an end.
    fn is_side(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }

    fn panel_id(self) -> &'static str {
        match self {
            Self::Left => "dock_left",
            Self::Right => "dock_right",
            Self::Top => "dock_top",
            Self::Bottom => "dock_bottom",
        }
    }

    /// How deep the dock is from its edge as it was last shown, if it was.
    fn shown_depth(self, ctx: &egui::Context) -> Option<f32> {
        let size = PanelState::load(ctx, Id::new(self.panel_id()))?.size();
        Some(if self.is_side() { size.x } else { size.y })
    }

    /// Where `pos` is along the dock: down a side, across an end.
    fn along(self, pos: Pos2) -> f32 {
        if self.is_side() { pos.y } else { pos.x }
    }
}

/// What the docks can hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Section {
    Tools,
    Colors,
    History,
    Layers,
    Navigator,
}

impl Section {
    pub const ALL: [Self; 5] = [Self::Tools, Self::Colors, Self::History, Self::Layers, Self::Navigator];

    pub fn title(self) -> &'static str {
        match self {
            Self::Tools => "Tools",
            Self::Colors => "Colors",
            Self::History => "History",
            Self::Layers => "Layers",
            Self::Navigator => "Navigator",
        }
    }
}

/// Which sections each dock holds, from its start: the top of a side, the left of an
/// end, and how deep each dock is from its edge.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DockLayout {
    left: Vec<Section>,
    right: Vec<Section>,
    top: Vec<Section>,
    bottom: Vec<Section>,
    sizes: DockSizes,
}

impl Default for DockLayout {
    fn default() -> Self {
        Self {
            left: Section::ALL.to_vec(),
            right: Vec::new(),
            top: Vec::new(),
            bottom: Vec::new(),
            sizes: DockSizes::default(),
        }
    }
}

/// How deep each dock is from its edge, kept even while it holds no sections.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct DockSizes {
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
}

impl Default for DockSizes {
    fn default() -> Self {
        Self { left: DEFAULT_WIDTH, right: DEFAULT_WIDTH, top: DEFAULT_HEIGHT, bottom: DEFAULT_HEIGHT }
    }
}

impl DockLayout {
    /// How deep the dock on `side` is from its edge.
    fn size(&self, side: DockSide) -> f32 {
        match side {
            DockSide::Left => self.sizes.left,
            DockSide::Right => self.sizes.right,
            DockSide::Top => self.sizes.top,
            DockSide::Bottom => self.sizes.bottom,
        }
    }

    fn set_size(&mut self, side: DockSide, size: f32) {
        let sizes = &mut self.sizes;
        let field = match side {
            DockSide::Left => &mut sizes.left,
            DockSide::Right => &mut sizes.right,
            DockSide::Top => &mut sizes.top,
            DockSide::Bottom => &mut sizes.bottom,
        };
        *field = size;
    }

    pub fn sections(&self, side: DockSide) -> &[Section] {
        match side {
            DockSide::Left => &self.left,
            DockSide::Right => &self.right,
            DockSide::Top => &self.top,
            DockSide::Bottom => &self.bottom,
        }
    }

    fn sections_mut(&mut self, side: DockSide) -> &mut Vec<Section> {
        match side {
            DockSide::Left => &mut self.left,
            DockSide::Right => &mut self.right,
            DockSide::Top => &mut self.top,
            DockSide::Bottom => &mut self.bottom,
        }
    }

    /// Has each section once: the ones in more than one place keep the first, and
    /// those missing, as from settings saved before they were made, go at the end of
    /// the left dock.
    pub fn normalize(&mut self) {
        let mut seen = Vec::new();
        for side in DockSide::ALL {
            self.sections_mut(side).retain(|section| {
                let new = !seen.contains(section);
                seen.push(*section);
                new
            });
        }
        let missing = Section::ALL.into_iter().filter(|section| !seen.contains(section));
        self.left.extend(missing);
    }

    /// Takes the section out of its dock and puts it at `index` among the sections of
    /// the dock on `side`, as they are without it.
    pub fn move_section(&mut self, section: Section, side: DockSide, index: usize) {
        for side in DockSide::ALL {
            self.sections_mut(side).retain(|&other| other != section);
        }
        let sections = self.sections_mut(side);
        sections.insert(index.min(sections.len()), section);
    }

    /// Puts all the sections in the dock on `side`, in the order of the docks they come from.
    pub fn move_all(&mut self, side: DockSide) {
        let all: Vec<Section> = DockSide::ALL.into_iter().flat_map(|side| self.sections(side).to_vec()).collect();
        for side in DockSide::ALL {
            self.sections_mut(side).clear();
        }
        *self.sections_mut(side) = all;
    }
}

/// Where the sections were shown this frame, and the one being dragged.
#[derive(Default)]
struct Placement {
    docks: Vec<(DockSide, Rect)>,
    sections: Vec<(DockSide, Section, Rect)>,
    /// The section whose header is being dragged, and whether it was just let go.
    dragged: Option<(Section, bool)>,
}

/// Shows the docks for `layout`, with what `add_section` puts in each of their
/// sections. Returns the layout as it is after a dock was resized or a section was
/// dragged somewhere else.
pub fn show(
    ui: &mut egui::Ui,
    layout: &DockLayout,
    mut add_section: impl FnMut(&mut egui::Ui, Section),
) -> Option<DockLayout> {
    let mut placement = Placement::default();
    let mut resized: Option<DockLayout> = None;
    for side in DockSide::ALL {
        let sections = layout.sections(side);
        if sections.is_empty() {
            continue;
        }
        let id = side.panel_id();
        let panel = match side {
            DockSide::Left => egui::Panel::left(id),
            DockSide::Right => egui::Panel::right(id),
            DockSide::Top => egui::Panel::top(id),
            DockSide::Bottom => egui::Panel::bottom(id),
        };
        let min_size = if side.is_side() { MIN_WIDTH } else { MIN_HEIGHT };
        let mut add_sections = |ui: &mut egui::Ui| {
            for &section in sections {
                show_section(ui, side, section, &mut placement, |ui| add_section(ui, section));
            }
        };
        let response = panel.resizable(true).default_size(layout.size(side)).min_size(min_size).show(ui, |ui| {
            if side.is_side() {
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, add_sections);
            } else {
                egui::ScrollArea::horizontal().auto_shrink(false).show(ui, |ui| {
                    ui.horizontal_top(|ui| add_sections(ui));
                });
            }
        });
        placement.docks.push((side, response.response.rect));

        // Kept once the edge of the dock is let go of, rather than all along its drag.
        let resizing = ui.input(|input| input.pointer.any_down());
        if let Some(depth) = side.shown_depth(ui.ctx())
            && !resizing
            && (depth - layout.size(side)).abs() >= 0.5
        {
            resized.get_or_insert_with(|| layout.clone()).set_size(side, depth);
        }
    }

    let moved = drag_section(ui, resized.as_ref().unwrap_or(layout), &placement);
    moved.or(resized)
}

/// Marks where the section being dragged would go. Returns the layout with it there once it is let go.
fn drag_section(ui: &egui::Ui, layout: &DockLayout, placement: &Placement) -> Option<DockLayout> {
    let (section, dropped) = placement.dragged?;
    let pointer = ui.ctx().pointer_latest_pos()?;
    // What is left between the docks.
    let drawing_area = ui.available_rect_before_wrap();
    let side = placement
        .docks
        .iter()
        .find(|(_, rect)| rect.contains(pointer))
        .map_or_else(|| nearest_edge(drawing_area, pointer), |&(side, _)| side);
    // The other sections of that dock, where they are.
    let others: Vec<Rect> = placement
        .sections
        .iter()
        .filter(|&&(other_side, other, _)| other_side == side && other != section)
        .map(|&(_, _, rect)| rect)
        .collect();
    let index = others.iter().filter(|rect| side.along(rect.center()) < side.along(pointer)).count();

    if dropped {
        let mut moved = layout.clone();
        moved.move_section(section, side, index);
        return (moved != *layout).then_some(moved);
    }
    let dock = placement.docks.iter().find(|&&(other, _)| other == side).map(|&(_, rect)| rect);
    mark_target(ui, side, dock, layout.size(side), drawing_area, &others, index);
    show_dragged_title(ui.ctx(), section, pointer);
    None
}

/// A section under its header, which folds it away when clicked and moves it when dragged.
fn show_section(
    ui: &mut egui::Ui,
    side: DockSide,
    section: Section,
    placement: &mut Placement,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    let add_section = |ui: &mut egui::Ui| {
        // The same wherever the section is, so it stays folded or not as it moves.
        let id = Id::new(("dock_section", section));
        let mut state = CollapsingState::load_with_default_open(ui.ctx(), id, true);
        let header = header(ui, section, state.openness(ui.ctx()));
        if header.clicked() {
            state.toggle(ui);
        }
        if header.drag_stopped() {
            placement.dragged = Some((section, true));
        } else if header.dragged() {
            placement.dragged = Some((section, false));
        }
        state.show_body_indented(&header, ui, add_contents);
    };
    let rect = if side.is_side() {
        ui.vertical(add_section).response.rect
    } else {
        // Beside the others: a column of its own, which scrolls on its own. It can be
        // as short as the dock: the dock keeps the height of what it holds once
        // resized, so a column taller than the dock would make it spring back.
        ui.vertical(|ui| {
            ui.set_width(SECTION_WIDTH);
            egui::ScrollArea::vertical()
                .id_salt(section)
                .min_scrolled_height(0.0)
                .auto_shrink([false, false])
                .show(ui, add_section);
        })
        .response
        .rect
    };
    placement.sections.push((side, section, rect));
}

/// The header of a section: the arrow that says whether it is open, its title, and
/// dots at the end that say it can be dragged.
fn header(ui: &mut egui::Ui, section: Section, openness: f32) -> egui::Response {
    let height = ui.spacing().interact_size.y;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::click_and_drag());
    let response = response.on_hover_text("Click to fold or unfold, drag to move");
    if response.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    } else if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::Grab);
    }
    let visuals = *ui.style().interact(&response);
    ui.painter().rect_filled(rect, visuals.corner_radius, visuals.weak_bg_fill);

    let indent = ui.spacing().indent;
    let icon_width = ui.spacing().icon_width;
    let icon_rect = Rect::from_center_size(
        Pos2::new(rect.left() + indent / 2.0, rect.center().y),
        Vec2::splat(icon_width * 0.5),
    );
    egui::collapsing_header::paint_default_icon(ui, openness, &response.clone().with_new_rect(icon_rect));
    let font = TextStyle::Button.resolve(ui.style());
    let title_pos = Pos2::new(rect.left() + indent, rect.center().y);
    ui.painter().text(title_pos, Align2::LEFT_CENTER, section.title(), font, visuals.text_color());

    let (across, down) = GRIP_DOTS;
    let span = Vec2::new((across - 1) as f32, (down - 1) as f32) * GRIP_DOT_SPACING;
    let first = Pos2::new(rect.right() - indent / 2.0, rect.center().y) - span / 2.0;
    for x in 0..across {
        for y in 0..down {
            let center = first + Vec2::new(x as f32, y as f32) * GRIP_DOT_SPACING;
            ui.painter().circle_filled(center, GRIP_DOT_RADIUS, visuals.fg_stroke.color);
        }
    }
    response
}

/// Marks where the dragged section would go: a line between the sections of the
/// dock on `side`, before the one at `index`, or, if there are none, the dock or the
/// room the dock would take along the edge of the drawing area.
fn mark_target(
    ui: &egui::Ui,
    side: DockSide,
    dock: Option<Rect>,
    depth: f32,
    drawing_area: Rect,
    others: &[Rect],
    index: usize,
) {
    let painter = ui.ctx().layer_painter(LayerId::new(Order::Foreground, Id::new("dock_drop_mark")));
    let selection = ui.visuals().selection;
    let (Some(dock), false) = (dock, others.is_empty()) else {
        let area = drawing_area;
        let mark = dock.unwrap_or_else(|| match side {
            DockSide::Left => Rect::from_min_size(area.min, Vec2::new(depth, area.height())),
            DockSide::Right => Rect::from_min_max(area.right_top() - Vec2::new(depth, 0.0), area.max),
            DockSide::Top => Rect::from_min_size(area.min, Vec2::new(area.width(), depth)),
            DockSide::Bottom => Rect::from_min_max(area.left_bottom() - Vec2::new(0.0, depth), area.max),
        });
        let fill = Color32::from_rgba_unmultiplied(selection.bg_fill.r(), selection.bg_fill.g(), selection.bg_fill.b(), 64);
        painter.rect(mark, 4.0, fill, Stroke::new(2.0, selection.bg_fill), StrokeKind::Inside);
        return;
    };
    // Between the end of the section before and the start of the one after.
    let span = |rect: &Rect| if side.is_side() { rect.y_range() } else { rect.x_range() };
    let at = match (index.checked_sub(1).map(|before| others[before]), others.get(index)) {
        (Some(before), Some(after)) => (span(&before).max + span(after).min) / 2.0,
        (Some(before), None) => span(&before).max,
        (None, Some(after)) => span(after).min,
        (None, None) => unreachable!("there are other sections"),
    };
    let stroke = Stroke::new(INSERT_LINE_WIDTH, selection.bg_fill);
    if side.is_side() {
        painter.hline(dock.x_range(), at, stroke);
    } else {
        painter.vline(at, dock.y_range(), stroke);
    }
}

/// The title of the dragged section, by the pointer.
fn show_dragged_title(ctx: &egui::Context, section: Section, pointer: Pos2) {
    egui::Area::new(Id::new("dock_dragged_title"))
        .order(Order::Tooltip)
        .fixed_pos(pointer + Vec2::splat(12.0))
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| ui.label(section.title()));
        });
}

/// The edge of `area` nearest to `pointer`, as a share of the area's size, so its
/// diagonals divide it between them.
fn nearest_edge(area: Rect, pointer: Pos2) -> DockSide {
    let x = (pointer.x - area.left()) / area.width().max(1.0);
    let y = (pointer.y - area.top()) / area.height().max(1.0);
    [(DockSide::Left, x), (DockSide::Right, 1.0 - x), (DockSide::Top, y), (DockSide::Bottom, 1.0 - y)]
        .into_iter()
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(DockSide::Left, |(side, _)| side)
}

#[cfg(test)]
mod tests {
    use super::*;
    use Section::*;

    #[test]
    fn move_section_within_and_between_docks() {
        let mut layout = DockLayout::default();
        layout.move_section(Layers, DockSide::Left, 0);
        assert_eq!(layout.sections(DockSide::Left), [Layers, Tools, Colors, History, Navigator]);
        layout.move_section(Colors, DockSide::Bottom, 5);
        assert_eq!(layout.sections(DockSide::Left), [Layers, Tools, History, Navigator]);
        assert_eq!(layout.sections(DockSide::Bottom), [Colors]);
        layout.move_section(Tools, DockSide::Bottom, 0);
        assert_eq!(layout.sections(DockSide::Bottom), [Tools, Colors]);
    }

    #[test]
    fn normalize_has_each_section_once() {
        let mut layout = DockLayout { left: vec![Colors], right: vec![Colors, Layers], top: vec![], bottom: vec![], ..DockLayout::default() };
        layout.normalize();
        assert_eq!(layout.sections(DockSide::Left), [Colors, Tools, History, Navigator]);
        assert_eq!(layout.sections(DockSide::Right), [Layers]);
    }

    #[test]
    fn layout_goes_through_toml() {
        let mut layout = DockLayout::default();
        layout.move_section(History, DockSide::Right, 0);
        layout.set_size(DockSide::Right, 300.0);
        let text = toml::to_string(&layout).unwrap();
        assert_eq!(toml::from_str::<DockLayout>(&text).unwrap(), layout);
    }
}

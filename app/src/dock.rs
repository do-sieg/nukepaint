//! The docks: panels along the edges of the window that hold the sections, each
//! with a header that folds it away when clicked. Down the left or right side they
//! go one above the other, each under its header; along the top or bottom, one
//! beside the other, each after its header, which runs down its side. Only the edges
//! with sections shown have a dock. A section hidden from the View menu keeps its
//! place for when it is shown again, and a folded one stays folded, across sessions.
//!
//! A section is made of parts, which go one under the other down a side. Along an
//! end they go down columns as tall as the dock, as many as they take, so the section
//! is as wide as its parts need, and scrolls on its own if one is taller than the dock.
//!
//! A section is moved by dragging its header: into a dock, among its sections, or
//! anywhere else, to the edge of the drawing area it is let go nearest to. Each dock
//! can be made wider or narrower from its inner edge.

use eframe::egui::{
    self, Align, Align2, Color32, CursorIcon, Id, LayerId, Layout, Order, PanelState, Pos2, Rect, Sense, Stroke,
    StrokeKind, TextStyle, UiBuilder, Vec2, collapsing_header::CollapsingState, epaint::TextShape,
};
use serde::{Deserialize, Deserializer, Serialize};

use crate::tools::Group;

const DEFAULT_WIDTH: f32 = 240.0;
const MIN_WIDTH: f32 = 180.0;
const DEFAULT_HEIGHT: f32 = 240.0;
const MIN_HEIGHT: f32 = 64.0;
/// Widest a part of a section gets in a column along an end.
const COLUMN_WIDTH: f32 = 220.0;
/// The dots at the end of a header, which say it can be dragged: across and down,
/// and their room.
const GRIP_DOTS: (usize, usize) = (2, 3);
const GRIP_DOT_SPACING: f32 = 4.0;
const GRIP_DOT_RADIUS: f32 = 1.0;
/// How thick the line is that marks where a dragged section would go.
pub const INSERT_LINE_WIDTH: f32 = 3.0;

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
    /// The tools of a group.
    Tools(Group),
    Palette,
    Colors,
    History,
    Layers,
    Navigator,
}

impl Section {
    pub const ALL: [Self; 11] = [
        Self::Tools(Group::Select),
        Self::Tools(Group::Navigate),
        Self::Tools(Group::Paint),
        Self::Tools(Group::Shapes),
        Self::Tools(Group::Retouch),
        Self::Tools(Group::Transform),
        Self::Palette,
        Self::Colors,
        Self::History,
        Self::Layers,
        Self::Navigator,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Tools(group) => group.name(),
            Self::Palette => "Palette",
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
    #[serde(deserialize_with = "saved_sections")]
    left: Vec<Section>,
    #[serde(deserialize_with = "saved_sections")]
    right: Vec<Section>,
    #[serde(deserialize_with = "saved_sections")]
    top: Vec<Section>,
    #[serde(deserialize_with = "saved_sections")]
    bottom: Vec<Section>,
    /// The sections turned off from the View menu, which keep their place in their dock.
    #[serde(deserialize_with = "saved_sections")]
    hidden: Vec<Section>,
    /// The sections folded away under their headers.
    #[serde(deserialize_with = "saved_sections")]
    folded: Vec<Section>,
    sizes: DockSizes,
}

/// A section as saved, or the name of one there is no longer.
#[derive(Deserialize)]
#[serde(untagged)]
enum SavedSection {
    Section(Section),
    Gone(String),
}

/// The sections saved for a dock. The one Tools section that held every tool is now
/// a section for each group, in its place, and the other sections no longer made
/// are left out.
fn saved_sections<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<Section>, D::Error> {
    let saved = Vec::<SavedSection>::deserialize(deserializer)?;
    Ok(saved
        .into_iter()
        .flat_map(|section| match section {
            SavedSection::Section(section) => vec![section],
            SavedSection::Gone(name) if name == "Tools" => Group::ALL.map(Section::Tools).to_vec(),
            SavedSection::Gone(_) => Vec::new(),
        })
        .collect())
}

impl Default for DockLayout {
    fn default() -> Self {
        Self {
            // The tools and the colours on the left, what is about the image on the right.
            left: Group::ALL.into_iter().map(Section::Tools).chain([Section::Palette, Section::Colors]).collect(),
            right: vec![Section::History, Section::Navigator, Section::Layers],
            top: Vec::new(),
            bottom: Vec::new(),
            hidden: vec![Section::Palette],
            folded: Vec::new(),
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
    /// those missing, as from settings saved before they were made, go where the
    /// default layout has them, before the section after them there, hidden if it
    /// hides them.
    pub fn normalize(&mut self) {
        let mut seen = Vec::new();
        for side in DockSide::ALL {
            self.sections_mut(side).retain(|section| {
                let new = !seen.contains(section);
                seen.push(*section);
                new
            });
        }
        let default = Self::default();
        for section in Section::ALL.into_iter().filter(|section| !seen.contains(section)) {
            let side = DockSide::ALL
                .into_iter()
                .find(|&side| default.sections(side).contains(&section))
                .unwrap_or(DockSide::Left);
            let defaults = default.sections(side);
            let after = defaults.iter().skip_while(|&&other| other != section).skip(1);
            let sections = self.sections_mut(side);
            let index = after
                .filter_map(|after| sections.iter().position(|other| other == after))
                .next()
                .unwrap_or(sections.len());
            sections.insert(index, section);
            if !default.is_shown(section) {
                self.set_shown(section, false);
            }
        }
    }

    pub fn is_shown(&self, section: Section) -> bool {
        !self.hidden.contains(&section)
    }

    pub fn set_shown(&mut self, section: Section, shown: bool) {
        self.hidden.retain(|&other| other != section);
        if !shown {
            self.hidden.push(section);
        }
    }

    pub fn is_folded(&self, section: Section) -> bool {
        self.folded.contains(&section)
    }

    pub fn set_folded(&mut self, section: Section, folded: bool) {
        self.folded.retain(|&other| other != section);
        if folded {
            self.folded.push(section);
        }
    }

    pub fn show_all(&mut self) {
        self.hidden.clear();
    }

    /// The sections shown in the dock on `side`.
    fn shown_sections(&self, side: DockSide) -> Vec<Section> {
        self.sections(side).iter().copied().filter(|&section| self.is_shown(section)).collect()
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
}

/// Where the sections were shown this frame, and the one being dragged.
#[derive(Default)]
struct Placement {
    docks: Vec<(DockSide, Rect)>,
    sections: Vec<(DockSide, Section, Rect)>,
    /// The section whose header is being dragged, and whether it was just let go.
    dragged: Option<(Section, bool)>,
    /// The section whose header was clicked, to be folded or unfolded.
    toggled: Option<Section>,
}

/// Where the parts of a section go: one under the other down a side, and along an
/// end, down columns as tall as the dock.
pub struct Flow<'a> {
    ui: &'a mut egui::Ui,
    columns: Option<Columns>,
    /// What the parts' ids are made from, the same whatever is around the section.
    id: Id,
    /// How many parts have been added.
    count: usize,
}

impl<'a> Flow<'a> {
    fn down(ui: &'a mut egui::Ui) -> Self {
        let id = ui.id();
        Self { ui, columns: None, id, count: 0 }
    }

    fn across(ui: &'a mut egui::Ui, section: Section, height: f32) -> Self {
        let columns = Columns::new(ui.cursor().min, height);
        Self { ui, columns: Some(columns), id: Id::new(("dock_section_parts", section)), count: 0 }
    }

    /// Whether the parts go in columns, along an end, rather than one under the other.
    pub fn is_along_end(&self) -> bool {
        self.columns.is_some()
    }

    /// Adds a part, with the tallest it should get: as tall as the dock along an end,
    /// as tall as it likes down a side.
    pub fn part<R>(&mut self, add: impl FnOnce(&mut egui::Ui, f32) -> R) -> R {
        let Some(columns) = &mut self.columns else {
            return add(self.ui, f32::INFINITY);
        };
        // Where it goes depends on how tall it was last time, kept under an id that
        // does not change with what was shown before it, such as a tooltip.
        let id = self.id.with(self.count);
        self.count += 1;
        let height: f32 = self.ui.data(|data| data.get_temp(id)).unwrap_or(0.0);
        let spacing = self.ui.spacing().item_spacing;
        let at = columns.place(height, spacing);
        let room = Rect::from_min_size(at, Vec2::new(COLUMN_WIDTH, columns.height));
        let builder = UiBuilder::new().id(id).max_rect(room).layout(Layout::top_down(Align::Min));
        let mut ui = self.ui.new_child(builder);
        let inner = add(&mut ui, columns.height);
        let used = ui.min_rect();
        columns.add(used, spacing);
        if (used.height() - height).abs() >= 0.5 {
            self.ui.data_mut(|data| data.insert_temp(id, used.height()));
            self.ui.ctx().request_discard("a part of a dock section changed height");
        }
        inner
    }

    /// Makes room for the parts added along an end. Returns how wide they are.
    fn finish(self) -> f32 {
        let Some(columns) = self.columns else {
            return self.ui.min_rect().width();
        };
        if columns.used.is_positive() {
            self.ui.advance_cursor_after_rect(columns.used);
        }
        columns.used.width().max(0.0)
    }
}

/// Where the parts of a section along an end have gone so far.
#[derive(Debug)]
struct Columns {
    /// Where the first column starts, and how tall each can be.
    top: f32,
    height: f32,
    /// The left of the column being filled, how wide it is so far, and where its next part goes.
    left: f32,
    width: f32,
    next: f32,
    /// What all the parts take.
    used: Rect,
}

impl Columns {
    fn new(top_left: Pos2, height: f32) -> Self {
        Self { top: top_left.y, height, left: top_left.x, width: 0.0, next: top_left.y, used: Rect::NOTHING }
    }

    /// Where a part `height` tall goes: under the last one, or at the top of a new
    /// column if it would go past the bottom of this one, unless it is the first.
    fn place(&mut self, height: f32, spacing: Vec2) -> Pos2 {
        if self.next > self.top && self.next + height > self.top + self.height {
            self.left += self.width + spacing.x;
            self.width = 0.0;
            self.next = self.top;
        }
        Pos2::new(self.left, self.next)
    }

    /// Notes what a part took.
    fn add(&mut self, rect: Rect, spacing: Vec2) {
        self.next = rect.bottom() + spacing.y;
        self.width = self.width.max(rect.right() - self.left);
        self.used = self.used.union(rect);
    }
}

/// Shows the docks for `layout`, with what `add_section` puts in each of their
/// sections. Returns the layout as it is after a dock was resized or a section was
/// dragged somewhere else.
pub fn show(
    ui: &mut egui::Ui,
    layout: &DockLayout,
    mut add_section: impl FnMut(&mut Flow, Section),
) -> Option<DockLayout> {
    let mut placement = Placement::default();
    let mut resized: Option<DockLayout> = None;
    for side in DockSide::ALL {
        let sections = layout.shown_sections(side);
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
        let mut add_sections = |ui: &mut egui::Ui, height: f32| {
            for &section in &sections {
                let folded = layout.is_folded(section);
                show_section(ui, side, section, folded, height, &mut placement, |flow| add_section(flow, section));
            }
        };
        let response = panel.resizable(true).default_size(layout.size(side)).min_size(min_size).show(ui, |ui| {
            if side.is_side() {
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| add_sections(ui, f32::INFINITY));
            } else {
                let height = ui.available_height();
                egui::ScrollArea::horizontal().auto_shrink(false).show(ui, |ui| {
                    ui.horizontal_top(|ui| add_sections(ui, height));
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

    let changed = resized.as_ref().unwrap_or(layout);
    let folded = placement.toggled.map(|section| {
        let mut folded = changed.clone();
        folded.set_folded(section, !changed.is_folded(section));
        folded
    });
    let changed = folded.as_ref().unwrap_or(changed);
    let moved = drag_section(ui, changed, &placement);
    moved.or(folded).or(resized)
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
    // The other sections shown in that dock, where they are.
    let (shown, others): (Vec<Section>, Vec<Rect>) = placement
        .sections
        .iter()
        .filter(|&&(other_side, other, _)| other_side == side && other != section)
        .map(|&(_, other, rect)| (other, rect))
        .unzip();
    let index = others.iter().filter(|rect| side.along(rect.center()) < side.along(pointer)).count();

    if dropped {
        // Before the shown section it goes before, among the hidden ones as well.
        let all: Vec<Section> = layout.sections(side).iter().copied().filter(|&other| other != section).collect();
        let index = shown.get(index).map_or(all.len(), |after| all.iter().position(|other| other == after).unwrap());
        let mut moved = layout.clone();
        moved.move_section(section, side, index);
        return (moved != *layout).then_some(moved);
    }
    let dock = placement.docks.iter().find(|&&(other, _)| other == side).map(|&(_, rect)| rect);
    mark_target(ui, side, dock, layout.size(side), drawing_area, &others, index);
    show_dragged_title(ui.ctx(), section.title(), pointer);
    None
}

/// A section with its header, which folds it away when clicked and moves it when
/// dragged: above it down a side, beside it along an end, as tall as the dock, `height`.
fn show_section(
    ui: &mut egui::Ui,
    side: DockSide,
    section: Section,
    folded: bool,
    height: f32,
    placement: &mut Placement,
    add_contents: impl FnOnce(&mut Flow),
) {
    let mut follow_header = |header: &egui::Response| {
        if header.clicked() {
            placement.toggled = Some(section);
        }
        if header.drag_stopped() {
            placement.dragged = Some((section, true));
        } else if header.dragged() {
            placement.dragged = Some((section, false));
        }
    };
    let rect = if side.is_side() {
        ui.vertical(|ui| {
            // The same wherever the section is, so it folds smoothly as it moves.
            let id = Id::new(("dock_section", section));
            let mut state = CollapsingState::load_with_default_open(ui.ctx(), id, !folded);
            state.set_open(!folded);
            let header = header(ui, section, state.openness(ui.ctx()));
            follow_header(&header);
            state.show_body_indented(&header, ui, |ui| add_contents(&mut Flow::down(ui)));
        })
        .response
        .rect
    } else {
        ui.horizontal_top(|ui| {
            let header = side_header(ui, section, !folded, height);
            follow_header(&header);
            if !folded {
                show_columns(ui, section, height, add_contents);
            }
        })
        .response
        .rect
    };
    placement.sections.push((side, section, rect));
}

/// The parts of a section along an end, in columns as tall as the dock, `height`, as
/// wide as they were last time. The dock gives no more room than it has to what it
/// holds, so it takes being told how wide they are to scroll past the others.
fn show_columns(ui: &mut egui::Ui, section: Section, height: f32, add_contents: impl FnOnce(&mut Flow)) {
    let width_id = Id::new(("dock_section_width", section));
    let width: f32 = ui.data(|data| data.get_temp(width_id)).unwrap_or(COLUMN_WIDTH);
    let used = ui
        .allocate_ui(Vec2::new(width, height), |ui| {
            ui.set_width(width);
            // It can be as short as the dock: the dock keeps the height of what it
            // holds once resized, so a section taller than it would make it spring back.
            egui::ScrollArea::vertical()
                .id_salt(section)
                .max_height(height)
                .min_scrolled_height(0.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    let mut flow = Flow::across(ui, section, height);
                    add_contents(&mut flow);
                    flow.finish()
                })
                .inner
        })
        .inner;
    if (used - width).abs() >= 0.5 {
        ui.data_mut(|data| data.insert_temp(width_id, used));
        ui.ctx().request_discard("a dock section changed width");
    }
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

/// The header of a section along an end, down its side, `height` tall: the arrow
/// that says whether it is open, its title going up, and dots at the bottom that say
/// it can be dragged.
fn side_header(ui: &mut egui::Ui, section: Section, open: bool, height: f32) -> egui::Response {
    let width = ui.spacing().interact_size.y;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click_and_drag());
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
        Pos2::new(rect.center().x, rect.top() + indent / 2.0),
        Vec2::splat(icon_width * 0.5),
    );
    let openness = if open { 1.0 } else { 0.0 };
    egui::collapsing_header::paint_default_icon(ui, openness, &response.clone().with_new_rect(icon_rect));

    let (across, down) = GRIP_DOTS;
    // Turned with the title: as many down as there were across.
    let span = Vec2::new((down - 1) as f32, (across - 1) as f32) * GRIP_DOT_SPACING;
    let first = Pos2::new(rect.center().x, rect.bottom() - indent / 2.0) - span / 2.0;
    for x in 0..down {
        for y in 0..across {
            let center = first + Vec2::new(x as f32, y as f32) * GRIP_DOT_SPACING;
            ui.painter().circle_filled(center, GRIP_DOT_RADIUS, visuals.fg_stroke.color);
        }
    }

    // Between the arrow and the dots, cut short if there is not room for all of it.
    let font = TextStyle::Button.resolve(ui.style());
    let room = (rect.height() - indent * 2.0).max(0.0);
    let mut job = egui::text::LayoutJob::simple_singleline(section.title().to_owned(), font, visuals.text_color());
    job.wrap = egui::text::TextWrapping::truncate_at_width(room);
    let galley = ui.painter().layout_job(job);
    let center = Pos2::new(rect.center().x, rect.top() + indent + galley.size().x / 2.0);
    let text = TextShape::new(center - galley.size() / 2.0, galley, visuals.text_color())
        .with_angle_and_anchor(-std::f32::consts::FRAC_PI_2, Align2::CENTER_CENTER);
    ui.painter().add(text);
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

/// The title of what is being dragged, by the pointer.
pub fn show_dragged_title(ctx: &egui::Context, title: &str, pointer: Pos2) {
    egui::Area::new(Id::new("dock_dragged_title"))
        .order(Order::Tooltip)
        .fixed_pos(pointer + Vec2::splat(12.0))
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| ui.label(title));
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

    const SELECT: Section = Tools(Group::Select);

    #[test]
    fn move_section_within_and_between_docks() {
        let mut layout = DockLayout { left: vec![SELECT, Colors, History, Layers, Navigator], ..DockLayout::default() };
        layout.move_section(Layers, DockSide::Left, 0);
        assert_eq!(layout.sections(DockSide::Left), [Layers, SELECT, Colors, History, Navigator]);
        layout.move_section(Colors, DockSide::Bottom, 5);
        assert_eq!(layout.sections(DockSide::Left), [Layers, SELECT, History, Navigator]);
        assert_eq!(layout.sections(DockSide::Bottom), [Colors]);
        layout.move_section(SELECT, DockSide::Bottom, 0);
        assert_eq!(layout.sections(DockSide::Bottom), [SELECT, Colors]);
    }

    #[test]
    fn normalize_has_each_section_once() {
        let mut layout = DockLayout { left: vec![Colors], right: vec![Colors, Layers], ..DockLayout::default() };
        layout.normalize();
        let tools = Group::ALL.map(Tools);
        assert_eq!(layout.sections(DockSide::Left)[..6], tools);
        assert_eq!(layout.sections(DockSide::Left)[6..], [Palette, Colors]);
        assert_eq!(layout.sections(DockSide::Right), [History, Navigator, Layers]);
    }

    #[test]
    fn normalize_puts_new_sections_where_the_default_has_them() {
        let mut left = Group::ALL.map(Tools).to_vec();
        left.push(Colors);
        let mut layout = DockLayout { left, right: vec![History, Navigator, Layers], ..DockLayout::default() };
        layout.hidden.clear();
        layout.normalize();
        assert_eq!(layout.sections(DockSide::Left), DockLayout::default().sections(DockSide::Left));
        assert!(!layout.is_shown(Palette));
        assert!(layout.is_shown(Colors));
    }

    #[test]
    fn layout_goes_through_toml() {
        let mut layout = DockLayout::default();
        layout.move_section(History, DockSide::Right, 0);
        layout.set_size(DockSide::Right, 300.0);
        let text = toml::to_string(&layout).unwrap();
        assert_eq!(toml::from_str::<DockLayout>(&text).unwrap(), layout);
    }

    #[test]
    fn saved_tools_section_becomes_the_groups() {
        let text = "left = [\"Colors\"]\nright = [\"Tools\", \"Gone\", \"Layers\"]\n";
        let layout: DockLayout = toml::from_str(text).unwrap();
        assert_eq!(layout.sections(DockSide::Left), [Colors]);
        let mut right = Group::ALL.map(Tools).to_vec();
        right.push(Layers);
        assert_eq!(layout.sections(DockSide::Right), right);
    }

    #[test]
    fn folded_sections_go_through_toml() {
        let mut layout = DockLayout::default();
        layout.set_folded(Colors, true);
        layout.set_folded(History, true);
        layout.set_folded(History, false);
        assert!(layout.is_folded(Colors) && !layout.is_folded(History));
        let text = toml::to_string(&layout).unwrap();
        assert_eq!(toml::from_str::<DockLayout>(&text).unwrap(), layout);
        // Saved before sections could be folded.
        assert!(!toml::from_str::<DockLayout>("left = [\"Colors\"]").unwrap().is_folded(Colors));
    }

    #[test]
    fn parts_go_down_columns_as_tall_as_the_dock() {
        let spacing = Vec2::splat(4.0);
        let mut columns = Columns::new(Pos2::ZERO, 100.0);
        let mut add = |width: f32, height: f32| {
            let at = columns.place(height, spacing);
            columns.add(Rect::from_min_size(at, Vec2::new(width, height)), spacing);
            at
        };
        assert_eq!(add(50.0, 40.0), Pos2::new(0.0, 0.0));
        assert_eq!(add(80.0, 40.0), Pos2::new(0.0, 44.0));
        // Past the bottom: in a column after the widest of the first.
        assert_eq!(add(30.0, 40.0), Pos2::new(84.0, 0.0));
        // Taller than the dock: at the top of a column of its own, past the bottom.
        assert_eq!(add(30.0, 150.0), Pos2::new(118.0, 0.0));
        assert_eq!(columns.used, Rect::from_min_max(Pos2::ZERO, Pos2::new(148.0, 150.0)));
    }

    #[test]
    fn hidden_sections_keep_their_place() {
        let mut layout = DockLayout { left: vec![Colors, History, Layers], ..DockLayout::default() };
        layout.set_shown(History, false);
        assert_eq!(layout.shown_sections(DockSide::Left), [Colors, Layers]);
        assert_eq!(layout.sections(DockSide::Left), [Colors, History, Layers]);
        let text = toml::to_string(&layout).unwrap();
        assert_eq!(toml::from_str::<DockLayout>(&text).unwrap(), layout);
        layout.set_shown(History, true);
        assert_eq!(layout.shown_sections(DockSide::Left), [Colors, History, Layers]);
    }
}

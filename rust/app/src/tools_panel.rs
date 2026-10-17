//! The tools of a group, as a button each, which wrap onto as many rows as the
//! dock's width needs. A button dragged moves its tool among those of its group.
//! The current tool's stands out.

use eframe::egui::{self, CursorIcon, Id, LayerId, Order, Rect, Sense, Stroke};
use serde::{Deserialize, Deserializer, Serialize, de::IgnoredAny};

use crate::dock::{self, INSERT_LINE_WIDTH};
use crate::icons::{ICON_SIZE, Icons};
use crate::tools::{Group, Tool};

/// The order of the tools within their group.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolOrder {
    /// All the tools, each group's in the order they go in it.
    #[serde(deserialize_with = "saved_tools")]
    tools: Vec<Tool>,
}

/// A tool as saved, or the name of one there is no longer.
#[derive(Deserialize)]
#[serde(untagged)]
enum SavedTool {
    Tool(Tool),
    Gone(IgnoredAny),
}

/// The tools saved, without those no longer made.
fn saved_tools<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<Tool>, D::Error> {
    let saved = Vec::<SavedTool>::deserialize(deserializer)?;
    Ok(saved
        .into_iter()
        .filter_map(|tool| match tool {
            SavedTool::Tool(tool) => Some(tool),
            SavedTool::Gone(_) => None,
        })
        .collect())
}

impl Default for ToolOrder {
    fn default() -> Self {
        Self { tools: Tool::ALL.to_vec() }
    }
}

impl ToolOrder {
    /// Has each tool once: those in more than one place keep the first, and those
    /// missing, as from settings saved before they were made, go at the end.
    pub fn normalize(&mut self) {
        let mut seen = Vec::new();
        self.tools.retain(|tool| {
            let new = !seen.contains(tool);
            seen.push(*tool);
            new
        });
        self.tools.extend(Tool::ALL.into_iter().filter(|tool| !seen.contains(tool)));
    }

    fn tools_in(&self, group: Group) -> impl Iterator<Item = Tool> + '_ {
        self.tools.iter().copied().filter(move |tool| tool.group() == group)
    }

    /// Puts the tool at `index` among the others of its group, as they are without it.
    fn move_tool(&mut self, tool: Tool, index: usize) {
        self.tools.retain(|&other| other != tool);
        let places: Vec<usize> = (0..self.tools.len()).filter(|&at| self.tools[at].group() == tool.group()).collect();
        let at = match places.get(index) {
            Some(&at) => at,
            None => places.last().map_or(self.tools.len(), |&last| last + 1),
        };
        self.tools.insert(at, tool);
    }
}

/// What was done in the panel.
pub enum Action {
    Select(Tool),
    /// A tool was dragged somewhere else in its group.
    Reorder(ToolOrder),
}

/// Shows the buttons of the tools in `group`, in `order`, with `current` as the tool in use.
pub fn show(ui: &mut egui::Ui, icons: &mut Icons, current: Tool, order: &ToolOrder, group: Group) -> Option<Action> {
    let mut clicked = None;
    // Where the buttons were shown, and the tool being dragged and whether it was just let go.
    let mut buttons: Vec<(Tool, Rect)> = Vec::new();
    let mut dragged = None;
    ui.horizontal_wrapped(|ui| {
        for tool in order.tools_in(group) {
            let button = tool_button(ui, icons, tool, current == tool);
            if button.clicked() {
                clicked = Some(tool);
            }
            if button.drag_stopped() {
                dragged = Some((tool, true));
            } else if button.dragged() {
                ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
                dragged = Some((tool, false));
            }
            buttons.push((tool, button.rect));
        }
    });
    let reordered = dragged.and_then(|(tool, dropped)| drag_tool(ui, order, &buttons, tool, dropped));
    reordered.map(Action::Reorder).or(clicked.map(Action::Select))
}

fn tool_button(ui: &mut egui::Ui, icons: &mut Icons, tool: Tool, selected: bool) -> egui::Response {
    let texture = icons.get(ui.ctx(), tool.icon());
    // The icons are light gray lines, made the colour of the text to suit either theme.
    let tint = ui.visuals().strong_text_color();
    let icon = egui::Image::new(texture).fit_to_exact_size(ICON_SIZE).tint(tint);
    let key = ui.ctx().format_shortcut(&tool.key());
    let tooltip = format!("{} ({key})\n{}\nDrag to move it among its group.", tool.name(), tool.help());
    let button = egui::Button::selectable(selected, icon).sense(Sense::click_and_drag());
    ui.add(button).on_hover_text(tooltip)
}

/// Marks where the tool being dragged would go among the other `buttons`. Returns
/// the order with it there once it is let go.
fn drag_tool(ui: &egui::Ui, order: &ToolOrder, buttons: &[(Tool, Rect)], tool: Tool, dropped: bool) -> Option<ToolOrder> {
    let pointer = ui.ctx().pointer_latest_pos()?;
    let others: Vec<Rect> = buttons.iter().filter(|&&(other, _)| other != tool).map(|&(_, rect)| rect).collect();
    // In reading order: on a row above the pointer's, or on its row to its left.
    let before = |rect: &&Rect| rect.bottom() < pointer.y || (rect.y_range().contains(pointer.y) && rect.center().x < pointer.x);
    let index = others.iter().filter(before).count();
    if dropped {
        let mut moved = order.clone();
        moved.move_tool(tool, index);
        return (moved != *order).then_some(moved);
    }
    let gap = ui.spacing().item_spacing.x / 2.0;
    let (x, row) = match (index.checked_sub(1).map(|before| others[before]), others.get(index)) {
        (Some(before), Some(after)) if before.y_range() == after.y_range() => {
            ((before.right() + after.left()) / 2.0, after.y_range())
        }
        (_, Some(after)) => (after.left() - gap, after.y_range()),
        (Some(before), None) => (before.right() + gap, before.y_range()),
        (None, None) => return None,
    };
    let painter = ui.ctx().layer_painter(LayerId::new(Order::Foreground, Id::new("tools_drop_mark")));
    painter.vline(x, row, Stroke::new(INSERT_LINE_WIDTH, ui.visuals().selection.bg_fill));
    dock::show_dragged_title(ui.ctx(), tool.name(), pointer);
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_tool_within_its_group() {
        let mut order = ToolOrder::default();
        order.move_tool(Tool::Jumble, 0);
        assert_eq!(order.tools_in(Group::Retouch).collect::<Vec<_>>(), [Tool::Jumble, Tool::Blur, Tool::Sharpen]);
        order.move_tool(Tool::Jumble, 2);
        assert_eq!(order.tools_in(Group::Retouch).collect::<Vec<_>>(), [Tool::Blur, Tool::Sharpen, Tool::Jumble]);
        order.move_tool(Tool::Blur, 1);
        assert_eq!(order.tools_in(Group::Retouch).collect::<Vec<_>>(), [Tool::Sharpen, Tool::Blur, Tool::Jumble]);
        assert_eq!(order.tools.len(), Tool::ALL.len());
    }

    #[test]
    fn normalize_has_each_tool_once() {
        let mut order = ToolOrder { tools: vec![Tool::Pan, Tool::Pan] };
        order.normalize();
        assert_eq!(order.tools.len(), Tool::ALL.len());
        assert_eq!(order.tools_in(Group::Navigate).collect::<Vec<_>>(), [Tool::Pan, Tool::Zoom]);
    }

    #[test]
    fn order_goes_through_toml() {
        let mut order = ToolOrder::default();
        order.move_tool(Tool::Curve, 0);
        let text = toml::to_string(&order).unwrap();
        assert_eq!(toml::from_str::<ToolOrder>(&text).unwrap(), order);
    }

    #[test]
    fn saved_tools_no_longer_made_are_left_out() {
        let order: ToolOrder = toml::from_str(r#"tools = ["Pan", "Gone", "Zoom"]"#).unwrap();
        assert_eq!(order.tools, [Tool::Pan, Tool::Zoom]);
    }
}

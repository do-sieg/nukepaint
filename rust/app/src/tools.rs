//! The tools that act on the image, and what they are set to.

use eframe::egui::{Key, KeyboardShortcut, Modifiers, PointerButton};
use power_paint_core::SelectionMode;

use crate::{
    frame_select::{FrameSelect, Shape},
    icons::{self, Icon},
};

/// Goes from one selection tool to the next.
pub const SELECT_KEY: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::S);
pub const MOVE_SELECTION_KEY: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::M);
pub const ZOOM_KEY: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::Z);
pub const PAN_KEY: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::H);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    RectangleSelect,
    EllipseSelect,
    MoveSelection,
    Zoom,
    Pan,
}

impl Tool {
    pub const ALL: [Self; 5] = [Self::RectangleSelect, Self::EllipseSelect, Self::MoveSelection, Self::Zoom, Self::Pan];
    const SELECT_TOOLS: [Self; 2] = [Self::RectangleSelect, Self::EllipseSelect];

    pub fn name(self) -> &'static str {
        match self {
            Self::RectangleSelect => "Rectangle Select",
            Self::EllipseSelect => "Ellipse Select",
            Self::MoveSelection => "Move Selection",
            Self::Zoom => "Zoom",
            Self::Pan => "Pan",
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            Self::RectangleSelect => icons::RECT_SELECT,
            Self::EllipseSelect => icons::ELLIPSE_SELECT,
            Self::MoveSelection => icons::MOVE_SELECTION,
            Self::Zoom => icons::ZOOM,
            Self::Pan => icons::PAN,
        }
    }

    pub fn key(self) -> KeyboardShortcut {
        match self {
            Self::RectangleSelect | Self::EllipseSelect => SELECT_KEY,
            Self::MoveSelection => MOVE_SELECTION_KEY,
            Self::Zoom => ZOOM_KEY,
            Self::Pan => PAN_KEY,
        }
    }

    /// How to use the tool.
    pub fn help(self) -> &'static str {
        match self {
            Self::RectangleSelect => {
                "Drag to select, a square with Shift. Then drag the selection's box to move, resize or rotate its \
                 pixels, or hold Ctrl to add to the selection."
            }
            Self::EllipseSelect => {
                "Drag to select, a circle with Shift. Then drag the selection's box to move, resize or rotate its \
                 pixels, or hold Ctrl to add to the selection."
            }
            Self::MoveSelection => {
                "Drag the selection's box to move, resize or rotate the selection, without its pixels. Shift keeps \
                 its proportions, or turns it in steps of 15°."
            }
            Self::Zoom => "Click to zoom in, right click to zoom out, or drag to frame what to zoom in on.",
            Self::Pan => {
                "Drag to scroll the image. The middle button, or the left one while Space is held, does it with \
                 any tool."
            }
        }
    }

    /// What the tool selects, for the selection tools.
    pub fn select_shape(self) -> Option<Shape> {
        match self {
            Self::RectangleSelect => Some(Shape::Rectangle),
            Self::EllipseSelect => Some(Shape::Ellipse),
            Self::MoveSelection | Self::Zoom | Self::Pan => None,
        }
    }
}

#[derive(Default)]
pub struct Tools {
    pub tool: Tool,
    /// The selection tool used last, which Move Selection goes back to.
    pub last_select_tool: Tool,
    /// What selecting does to the selection there already is, unless Ctrl is held.
    pub selection_mode: SelectionMode,
    pub frame_select: FrameSelect,
    pub zoom_drag: Option<ZoomDrag>,
}

/// A press of the Zoom tool, until it is let go.
pub struct ZoomDrag {
    pub button: PointerButton,
    /// Where it was pressed and where it is now, on the image, in pixels.
    pub from: (f64, f64),
    pub to: (f64, f64),
    /// Whether the left button has gone far enough to frame an area, rather than click.
    pub framing: bool,
}

impl Tools {
    pub fn set_tool(&mut self, tool: Tool) {
        self.tool = tool;
        if tool.select_shape().is_some() {
            self.last_select_tool = tool;
        }
    }

    /// The next selection tool, or the one used last from another tool.
    pub fn next_select_tool(&self) -> Tool {
        if self.tool.select_shape().is_none() {
            return self.last_select_tool;
        }
        let index = Tool::SELECT_TOOLS.iter().position(|&tool| tool == self.tool).unwrap_or(0);
        Tool::SELECT_TOOLS[(index + 1) % Tool::SELECT_TOOLS.len()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_key_goes_round_and_back() {
        let mut tools = Tools::default();
        assert_eq!(tools.next_select_tool(), Tool::EllipseSelect);
        tools.set_tool(Tool::EllipseSelect);
        assert_eq!(tools.next_select_tool(), Tool::RectangleSelect);
        tools.set_tool(Tool::MoveSelection);
        assert_eq!(tools.last_select_tool, Tool::EllipseSelect);
        assert_eq!(tools.next_select_tool(), Tool::EllipseSelect);
    }
}

//! The tools that act on the image, and what they are set to.

use eframe::egui::{Key, KeyboardShortcut, Modifiers, PointerButton};
use power_paint_core::{Ink, Rgba, SelectionMode};

use crate::{
    frame_select::{FrameSelect, Shape},
    icons::{self, Icon},
};

/// Goes from one selection tool to the next.
pub const SELECT_KEY: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::S);
pub const MOVE_SELECTION_KEY: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::M);
pub const ZOOM_KEY: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::Z);
pub const PAN_KEY: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::H);
pub const PAINTBRUSH_KEY: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::B);
pub const PENCIL_KEY: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::P);
pub const ERASER_KEY: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::E);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    RectangleSelect,
    EllipseSelect,
    MoveSelection,
    Zoom,
    Pan,
    Paintbrush,
    Pencil,
    Eraser,
}

impl Tool {
    pub const ALL: [Self; 8] = [
        Self::RectangleSelect,
        Self::EllipseSelect,
        Self::MoveSelection,
        Self::Zoom,
        Self::Pan,
        Self::Paintbrush,
        Self::Pencil,
        Self::Eraser,
    ];
    const SELECT_TOOLS: [Self; 2] = [Self::RectangleSelect, Self::EllipseSelect];

    pub fn name(self) -> &'static str {
        match self {
            Self::RectangleSelect => "Rectangle Select",
            Self::EllipseSelect => "Ellipse Select",
            Self::MoveSelection => "Move Selection",
            Self::Zoom => "Zoom",
            Self::Pan => "Pan",
            Self::Paintbrush => "Paintbrush",
            Self::Pencil => "Pencil",
            Self::Eraser => "Eraser",
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            Self::RectangleSelect => icons::RECT_SELECT,
            Self::EllipseSelect => icons::ELLIPSE_SELECT,
            Self::MoveSelection => icons::MOVE_SELECTION,
            Self::Zoom => icons::ZOOM,
            Self::Pan => icons::PAN,
            Self::Paintbrush => icons::PAINTBRUSH,
            Self::Pencil => icons::PENCIL,
            Self::Eraser => icons::ERASER,
        }
    }

    pub fn key(self) -> KeyboardShortcut {
        match self {
            Self::RectangleSelect | Self::EllipseSelect => SELECT_KEY,
            Self::MoveSelection => MOVE_SELECTION_KEY,
            Self::Zoom => ZOOM_KEY,
            Self::Pan => PAN_KEY,
            Self::Paintbrush => PAINTBRUSH_KEY,
            Self::Pencil => PENCIL_KEY,
            Self::Eraser => ERASER_KEY,
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
            Self::Paintbrush => {
                "Drag to paint lines in the colour, blended over the pixels under it, with smooth edges if \
                 Antialiasing is on. With a selection, only the selected pixels change. Drag a handle of its box \
                 to resize it, the arrows by its corner to move it, or around it to rotate it."
            }
            Self::Pencil => {
                "Drag to draw hard-edged lines in the colour, which replaces the pixels under it. With a \
                 selection, only the selected pixels change. Drag a handle of its box to resize it, the \
                 arrows by its corner to move it, or around it to rotate it."
            }
            Self::Eraser => {
                "Drag to rub out the pixels of the active layer, with smooth edges if Antialiasing is on. With a \
                 selection, only the selected pixels change. Drag a handle of its box to resize it, the arrows by \
                 its corner to move it, or around it to rotate it."
            }
        }
    }

    /// What the tool selects, for the selection tools.
    pub fn select_shape(self) -> Option<Shape> {
        match self {
            Self::RectangleSelect => Some(Shape::Rectangle),
            Self::EllipseSelect => Some(Shape::Ellipse),
            Self::MoveSelection | Self::Zoom | Self::Pan | Self::Paintbrush | Self::Pencil | Self::Eraser => None,
        }
    }

    /// Whether the tool draws lines on the active layer.
    pub fn draws(self) -> bool {
        matches!(self, Self::Paintbrush | Self::Pencil | Self::Eraser)
    }

    /// Whether the tool can draw with smooth edges.
    pub fn can_smooth(self) -> bool {
        matches!(self, Self::Paintbrush | Self::Eraser)
    }
}

pub struct Tools {
    pub tool: Tool,
    /// The selection tool used last, which Move Selection goes back to.
    pub last_select_tool: Tool,
    /// What selecting does to the selection there already is, unless Ctrl is held.
    pub selection_mode: SelectionMode,
    pub frame_select: FrameSelect,
    pub zoom_drag: Option<ZoomDrag>,
    /// What the drawing tools draw with, in straight RGBA.
    pub color: Rgba<u8>,
    /// How wide the Paintbrush, the Pencil and the Eraser draw, in pixels.
    pub brush_size: u32,
    pub pencil_size: u32,
    pub eraser_size: u32,
    /// Whether the tools that can draw with smooth edges do.
    pub antialiasing: bool,
}

impl Default for Tools {
    fn default() -> Self {
        Self {
            tool: Tool::default(),
            last_select_tool: Tool::default(),
            selection_mode: SelectionMode::default(),
            frame_select: FrameSelect::default(),
            zoom_drag: None,
            color: Rgba([0, 0, 0, 255]),
            brush_size: 2,
            pencil_size: 1,
            eraser_size: 8,
            antialiasing: true,
        }
    }
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

    /// How wide the tool draws, in pixels: the Pencil's width for the other tools.
    pub fn tip_size(&self) -> u32 {
        match self.tool {
            Tool::Paintbrush => self.brush_size,
            Tool::Eraser => self.eraser_size,
            _ => self.pencil_size,
        }
    }

    /// How wide the tool draws, in pixels, for the drawing tools.
    pub fn tip_size_mut(&mut self) -> Option<&mut u32> {
        match self.tool {
            Tool::Paintbrush => Some(&mut self.brush_size),
            Tool::Pencil => Some(&mut self.pencil_size),
            Tool::Eraser => Some(&mut self.eraser_size),
            _ => None,
        }
    }

    /// What the tool leaves on the pixels it goes over, for the drawing tools.
    pub fn ink(&self) -> Option<Ink> {
        match self.tool {
            Tool::Paintbrush => Some(Ink::Paint(self.color)),
            Tool::Pencil => Some(Ink::Replace(self.color)),
            Tool::Eraser => Some(Ink::Erase),
            _ => None,
        }
    }

    /// Whether the tool draws with smooth edges.
    pub fn smooth(&self) -> bool {
        self.antialiasing && self.tool.can_smooth()
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

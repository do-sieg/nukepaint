//! The tools that act on the image, and what they are set to.

use eframe::egui::{Key, KeyboardShortcut, Modifiers, PointerButton};
use power_paint_core::{Ink, Rgba, SelectionMode};
use serde::{Deserialize, Serialize};

use crate::{
    colors_panel::Colors,
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
pub const SPRAY_KEY: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::A);
pub const COLOR_PICKER_KEY: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::K);

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
    ColorPicker,
    Spray,
}

impl Tool {
    pub const ALL: [Self; 10] = [
        Self::RectangleSelect,
        Self::EllipseSelect,
        Self::MoveSelection,
        Self::Zoom,
        Self::Pan,
        Self::Paintbrush,
        Self::Pencil,
        Self::Eraser,
        Self::ColorPicker,
        Self::Spray,
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
            Self::ColorPicker => "Color Picker",
            Self::Spray => "Spray",
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
            Self::ColorPicker => icons::COLOR_PICKER,
            Self::Spray => icons::SPRAY,
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
            Self::ColorPicker => COLOR_PICKER_KEY,
            Self::Spray => SPRAY_KEY,
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
                "Drag to paint lines in the primary colour, blended over the pixels under it, with smooth edges if \
                 Antialiasing is on. Right drag to paint in the secondary colour, or to pick the colour of the \
                 active layer, as Right click says. With a selection, only the selected pixels change. Drag a handle of its box \
                 to resize it, the arrows by its corner to move it, or around it to rotate it."
            }
            Self::Pencil => {
                "Drag to draw hard-edged lines in the primary colour, which replaces the pixels under it. Right \
                 drag to draw in the secondary colour, or to pick the colour of the active layer, as Right click \
                 says. With a selection, only the selected pixels change. Drag a handle of its box to resize it, the \
                 arrows by its corner to move it, or around it to rotate it."
            }
            Self::Eraser => {
                "Drag to rub out the pixels of the active layer, with smooth edges if Antialiasing is on. With a \
                 selection, only the selected pixels change. Drag a handle of its box to resize it, the arrows by \
                 its corner to move it, or around it to rotate it."
            }
            Self::ColorPicker => {
                "Click or drag to pick the colour of the active layer as the colour the Colors section is \
                 editing, or with the right button as the other one."
            }
            Self::Spray => {
                "Hold to scatter dots of the primary colour around the pointer, which build up the longer it stays. \
                 Right drag to spray the secondary colour, or to pick the colour of the active layer, as Right \
                 click says. With a selection, only the selected pixels change. Drag a handle of its box to resize it, the arrows \
                 by its corner to move it, or around it to rotate it."
            }
        }
    }

    /// What the tool selects, for the selection tools.
    pub fn select_shape(self) -> Option<Shape> {
        match self {
            Self::RectangleSelect => Some(Shape::Rectangle),
            Self::EllipseSelect => Some(Shape::Ellipse),
            Self::MoveSelection
            | Self::Zoom
            | Self::Pan
            | Self::Paintbrush
            | Self::Pencil
            | Self::Eraser
            | Self::ColorPicker
            | Self::Spray => None,
        }
    }

    /// Whether the tool draws on the active layer.
    pub fn draws(self) -> bool {
        matches!(self, Self::Paintbrush | Self::Pencil | Self::Eraser | Self::Spray)
    }

    /// Whether the tool draws in a colour, which the right button can change.
    pub fn draws_in_color(self) -> bool {
        matches!(self, Self::Paintbrush | Self::Pencil | Self::Spray)
    }

    /// Whether the tool can draw with smooth edges.
    pub fn can_smooth(self) -> bool {
        matches!(self, Self::Paintbrush | Self::Eraser | Self::Spray)
    }
}

/// What the right button does with the tools that draw in a colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RightClick {
    /// Draws in the secondary colour.
    #[default]
    SecondaryColor,
    /// Picks the colour of the active layer as the colour the Colors section is editing,
    /// as the Color Picker does.
    PickColor,
}

impl RightClick {
    pub const ALL: [Self; 2] = [Self::SecondaryColor, Self::PickColor];

    pub fn name(self) -> &'static str {
        match self {
            Self::SecondaryColor => "Secondary Color",
            Self::PickColor => "Pick Color",
        }
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
    /// The primary colour, which the drawing tools draw with, and the secondary one.
    pub colors: Colors,
    /// How wide the Paintbrush, the Pencil and the Eraser draw, and the Spray sprays,
    /// in pixels.
    pub brush_size: u32,
    pub pencil_size: u32,
    pub eraser_size: u32,
    pub spray_size: u32,
    /// How fast the Spray leaves dots, as a percentage of the fastest it can.
    pub spray_speed: u32,
    /// Whether the tools that can draw with smooth edges do.
    pub antialiasing: bool,
    pub right_click: RightClick,
}

impl Default for Tools {
    fn default() -> Self {
        Self {
            tool: Tool::default(),
            last_select_tool: Tool::default(),
            selection_mode: SelectionMode::default(),
            frame_select: FrameSelect::default(),
            zoom_drag: None,
            colors: Colors::default(),
            brush_size: 2,
            pencil_size: 1,
            eraser_size: 8,
            spray_size: 32,
            spray_speed: 25,
            antialiasing: true,
            right_click: RightClick::default(),
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
            Tool::Spray => self.spray_size,
            _ => self.pencil_size,
        }
    }

    /// How wide the tool draws, in pixels, for the drawing tools.
    pub fn tip_size_mut(&mut self) -> Option<&mut u32> {
        match self.tool {
            Tool::Paintbrush => Some(&mut self.brush_size),
            Tool::Pencil => Some(&mut self.pencil_size),
            Tool::Eraser => Some(&mut self.eraser_size),
            Tool::Spray => Some(&mut self.spray_size),
            _ => None,
        }
    }

    /// What the tool leaves on the pixels it goes over in `color`, for the tools that draw lines.
    pub fn ink(&self, color: Rgba<u8>) -> Option<Ink> {
        match self.tool {
            Tool::Paintbrush => Some(Ink::Paint(color)),
            Tool::Pencil => Some(Ink::Replace(color)),
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

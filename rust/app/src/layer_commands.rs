//! What the Layers menu, the Image menu, their shortcuts, the buttons under the layers
//! list and the menu of a layer in it do.

use eframe::egui::{Key, KeyboardShortcut, Modifiers};
use power_paint_core::Document;

use crate::icons::{self, Icon};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerCommand {
    Add,
    Delete,
    Duplicate,
    MergeDown,
    CropToSelection,
    Resize,
    CanvasSize,
    Flatten,
    FlipImageHorizontal,
    FlipImageVertical,
    RotateImageClockwise,
    RotateImageCounterClockwise,
    RotateImage180,
    FlipHorizontal,
    FlipVertical,
    RotateClockwise,
    RotateCounterClockwise,
    Rotate180,
    RotateZoom,
    Transform3d,
    GoToTop,
    GoToAbove,
    GoToBelow,
    GoToBottom,
    MoveToTop,
    MoveUp,
    MoveDown,
    MoveToBottom,
    ShowOnly,
    Properties,
}

use LayerCommand::*;

impl LayerCommand {
    /// The Layers menu, in groups that separators keep apart.
    pub const MENU: [&[Self]; 5] = [
        &[Add, Delete, Duplicate, MergeDown],
        &[FlipHorizontal, FlipVertical, RotateClockwise, RotateCounterClockwise, Rotate180, RotateZoom, Transform3d],
        &[GoToTop, GoToAbove, GoToBelow, GoToBottom],
        &[MoveToTop, MoveUp, MoveDown, MoveToBottom],
        &[ShowOnly, Properties],
    ];
    /// The Image menu.
    pub const IMAGE_MENU: [&[Self]; 3] = [
        &[CropToSelection, Resize, CanvasSize],
        &[FlipImageHorizontal, FlipImageVertical, RotateImageClockwise, RotateImageCounterClockwise, RotateImage180],
        &[Flatten],
    ];
    /// The menu of a layer in the layers list, which acts on it: the Layers menu without
    /// the commands that go to another layer, and Flatten.
    pub const CONTEXT_MENU: [&[Self]; 4] = [
        &[Add, Delete, Duplicate, MergeDown, Flatten],
        &[FlipHorizontal, FlipVertical, RotateClockwise, RotateCounterClockwise, Rotate180, RotateZoom, Transform3d],
        &[MoveToTop, MoveUp, MoveDown, MoveToBottom],
        &[ShowOnly, Properties],
    ];
    /// The buttons under the layers list, which show their icon.
    pub const BUTTONS: [Self; 7] = [Add, Delete, Duplicate, MergeDown, MoveUp, MoveDown, Properties];

    pub fn label(self) -> &'static str {
        match self {
            Add => "Add New Layer",
            Delete => "Delete Layer",
            Duplicate => "Duplicate Layer",
            MergeDown => "Merge Layer Down",
            CropToSelection => "Crop to Selection",
            Resize => "Resize…",
            CanvasSize => "Canvas Size…",
            Flatten => "Flatten",
            FlipImageHorizontal | FlipHorizontal => "Flip Horizontal",
            FlipImageVertical | FlipVertical => "Flip Vertical",
            RotateImageClockwise | RotateClockwise => "Rotate 90° Clockwise",
            RotateImageCounterClockwise | RotateCounterClockwise => "Rotate 90° Counter-Clockwise",
            RotateImage180 | Rotate180 => "Rotate 180°",
            RotateZoom => "Rotate / Zoom…",
            Transform3d => "3D Transform…",
            GoToTop => "Go to Top Layer",
            GoToAbove => "Go to Layer Above",
            GoToBelow => "Go to Layer Below",
            GoToBottom => "Go to Bottom Layer",
            MoveToTop => "Move Layer to Top",
            MoveUp => "Move Layer Up",
            MoveDown => "Move Layer Down",
            MoveToBottom => "Move Layer to Bottom",
            ShowOnly => "Show Only Active Layer",
            Properties => "Layer Properties…",
        }
    }

    /// The label in the menu of a layer in the layers list, which names the layer right
    /// clicked rather than the active one, though it is the same.
    pub fn context_label(self) -> &'static str {
        match self {
            ShowOnly => "Show Only This Layer",
            _ => self.label(),
        }
    }

    /// The name of the step it makes in the history, which tells the image's flips
    /// from the layer's, and those from the selected pixels'.
    pub fn step_name(self, document: &Document) -> &'static str {
        let selected = !document.selection.is_empty();
        match self {
            FlipHorizontal if selected => "Flip Selection Horizontal",
            FlipVertical if selected => "Flip Selection Vertical",
            RotateClockwise if selected => "Rotate Selection 90° Clockwise",
            RotateCounterClockwise if selected => "Rotate Selection 90° Counter-Clockwise",
            Rotate180 if selected => "Rotate Selection 180°",
            FlipImageHorizontal => "Flip Image Horizontal",
            FlipImageVertical => "Flip Image Vertical",
            RotateImageClockwise => "Rotate Image 90° Clockwise",
            RotateImageCounterClockwise => "Rotate Image 90° Counter-Clockwise",
            RotateImage180 => "Rotate Image 180°",
            FlipHorizontal => "Flip Layer Horizontal",
            FlipVertical => "Flip Layer Vertical",
            RotateClockwise => "Rotate Layer 90° Clockwise",
            RotateCounterClockwise => "Rotate Layer 90° Counter-Clockwise",
            Rotate180 => "Rotate Layer 180°",
            _ => self.label().trim_end_matches('…'),
        }
    }

    pub fn shortcut(self) -> Option<KeyboardShortcut> {
        let ctrl_shift = Modifiers::COMMAND.plus(Modifiers::SHIFT);
        let ctrl_alt = Modifiers::COMMAND.plus(Modifiers::ALT);
        let (modifiers, key) = match self {
            Add => (ctrl_shift, Key::N),
            Delete => (ctrl_shift, Key::Delete),
            Duplicate => (ctrl_shift, Key::D),
            MergeDown => (Modifiers::COMMAND, Key::M),
            // Which comes as a Cut event rather than as keys: this is for the menu.
            CropToSelection => (ctrl_shift, Key::X),
            Resize => (Modifiers::COMMAND, Key::R),
            CanvasSize => (ctrl_shift, Key::R),
            Flatten => (ctrl_shift, Key::F),
            RotateImageClockwise => (Modifiers::COMMAND, Key::H),
            RotateImageCounterClockwise => (Modifiers::COMMAND, Key::G),
            GoToTop => (ctrl_alt, Key::PageUp),
            GoToAbove => (Modifiers::ALT, Key::PageUp),
            GoToBelow => (Modifiers::ALT, Key::PageDown),
            GoToBottom => (ctrl_alt, Key::PageDown),
            Properties => (Modifiers::NONE, Key::F4),
            _ => return None,
        };
        Some(KeyboardShortcut::new(modifiers, key))
    }

    pub fn icon(self) -> Option<Icon> {
        Some(match self {
            Add => icons::LAYER_ADD,
            Delete => icons::LAYER_DELETE,
            Duplicate => icons::LAYER_DUPLICATE,
            MergeDown => icons::LAYER_MERGE_DOWN,
            CropToSelection => icons::CROP,
            MoveUp => icons::LAYER_UP,
            MoveDown => icons::LAYER_DOWN,
            Properties => icons::LAYER_PROPERTIES,
            ShowOnly => icons::LAYERS,
            _ => return None,
        })
    }

    /// Whether the command has anything to do on the document.
    pub fn can_run(self, document: &Document) -> bool {
        let active = document.active_layer;
        let has_above = active + 1 < document.layers.len();
        let has_below = active > 0;
        match self {
            Delete => document.layers.len() > 1,
            CropToSelection => !document.selection.is_empty(),
            Flatten => !document.is_flat(),
            ShowOnly => document.layers.iter().enumerate().any(|(index, layer)| layer.visible != (index == active)),
            GoToTop | GoToAbove | MoveToTop | MoveUp => has_above,
            MergeDown | GoToBottom | GoToBelow | MoveToBottom | MoveDown => has_below,
            Add | Duplicate | Resize | CanvasSize | RotateZoom | Transform3d | Properties => true,
            FlipImageHorizontal | FlipImageVertical | RotateImageClockwise | RotateImageCounterClockwise | RotateImage180 => true,
            FlipHorizontal | FlipVertical | RotateClockwise | RotateCounterClockwise | Rotate180 => true,
        }
    }

    /// Runs the command on the document, but for those that ask the user first:
    /// Resize, Canvas Size, Rotate / Zoom, 3D Transform and Properties. The layer's flips
    /// and rotations turn only the selected pixels while there is a selection, and its
    /// quarter turns lose what they carry off the layer. Returns whether the layers changed, rather
    /// than only which one is active.
    pub fn run(self, document: &mut Document) -> bool {
        let active = document.active_layer;
        let top = document.layers.len() - 1;
        if let FlipHorizontal | FlipVertical | Rotate180 = self
            && !document.selection.is_empty()
        {
            let selection = document.selection.clone();
            return document.flip_selected(&selection, self != FlipVertical, self != FlipHorizontal);
        }
        if let RotateClockwise | RotateCounterClockwise = self
            && !document.selection.is_empty()
        {
            let selection = document.selection.clone();
            return document.rotate_selected(&selection, self == RotateClockwise);
        }
        match self {
            Add => document.add_layer(),
            Delete => return document.delete_layer(),
            Duplicate => document.duplicate_layer(),
            MergeDown => return document.merge_layer_down(),
            CropToSelection => return document.crop_to_selection(),
            Flatten => document.flatten(),
            FlipImageHorizontal => document.flip_image(true, false),
            FlipImageVertical => document.flip_image(false, true),
            RotateImageClockwise => document.rotate_image(true),
            RotateImageCounterClockwise => document.rotate_image(false),
            RotateImage180 => document.flip_image(true, true),
            FlipHorizontal => document.flip_layer(true, false),
            FlipVertical => document.flip_layer(false, true),
            RotateClockwise => document.rotate_layer(true),
            RotateCounterClockwise => document.rotate_layer(false),
            Rotate180 => document.flip_layer(true, true),
            GoToTop | GoToAbove | GoToBelow | GoToBottom => {
                let index = match self {
                    GoToTop => top,
                    GoToAbove => active + 1,
                    GoToBelow => active.saturating_sub(1),
                    _ => 0,
                };
                document.set_active_layer(index);
                return false;
            }
            MoveToTop => return document.move_layer(active, top),
            MoveUp => return document.move_layer(active, active + 1),
            MoveDown => return document.move_layer(active, active.saturating_sub(1)),
            MoveToBottom => return document.move_layer(active, 0),
            ShowOnly => {
                for (index, layer) in document.layers.iter_mut().enumerate() {
                    layer.visible = index == active;
                }
            }
            Resize | CanvasSize | RotateZoom | Transform3d | Properties => return false,
        }
        true
    }
}

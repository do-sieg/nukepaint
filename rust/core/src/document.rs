use std::path::{Path, PathBuf};

use image::{RgbaImage, imageops};

use crate::{
    BlendMode, Layer, MAX_OPACITY, OpenError, Selection, blend_layer, composite, copy_selected, erase_selected,
    load_image, save_image,
};

/// An image being edited: its layers, bottom first, and the file it came from.
pub struct Document {
    /// The file the document is saved to, if it has one yet.
    pub path: Option<PathBuf>,
    pub layers: Vec<Layer>,
    pub active_layer: usize,
    pub selection: Selection,
    /// Whether the pixels changed since the document was opened or last saved.
    pub unsaved: bool,
}

impl Document {
    /// A document never saved, of a single layer.
    pub fn new(pixels: RgbaImage) -> Self {
        Self {
            path: None,
            layers: vec![Layer::new("Background", pixels)],
            active_layer: 0,
            selection: Selection::default(),
            unsaved: false,
        }
    }

    /// Reads a document from an image file, as a single layer.
    pub fn open(path: &Path) -> Result<Self, OpenError> {
        Ok(Self { path: Some(path.to_path_buf()), ..Self::new(load_image(path)?) })
    }

    /// Writes the image to a file, in the format its extension names, and makes that
    /// file the document's own.
    pub fn save(&mut self, path: &Path) -> Result<(), image::ImageError> {
        save_image(path, &self.flattened())?;
        self.path = Some(path.to_path_buf());
        self.unsaved = false;
        Ok(())
    }

    /// The file name, or "Untitled" for a document never saved.
    pub fn name(&self) -> String {
        self.path
            .as_deref()
            .and_then(Path::file_name)
            .map_or_else(|| "Untitled".to_owned(), |name| name.to_string_lossy().into_owned())
    }

    pub fn size(&self) -> (u32, u32) {
        self.layers[0].pixels.dimensions()
    }

    /// The selected pixels of the active layer, the others transparent, if any of the
    /// selection is on it.
    pub fn copy(&self) -> Option<RgbaImage> {
        copy_selected(&self.layers[self.active_layer].pixels, &self.selection).map(|(_, pixels)| pixels)
    }

    /// The selected pixels of all the layers blended together, if any of the selection
    /// is on the image.
    pub fn copy_merged(&self) -> Option<RgbaImage> {
        copy_selected(&self.flattened(), &self.selection).map(|(_, pixels)| pixels)
    }

    /// Makes the selected pixels of the active layer transparent. Returns whether any were.
    pub fn erase_selection(&mut self) -> bool {
        let erased = erase_selected(&mut self.layers[self.active_layer].pixels, &self.selection);
        self.unsaved |= erased;
        erased
    }

    /// The layers blended into one image.
    pub fn flattened(&self) -> RgbaImage {
        composite(&self.layers)
    }

    /// Blends all the layers into a single one, named after the bottom one.
    pub fn flatten(&mut self) {
        if self.is_flat() {
            return;
        }
        let name = std::mem::take(&mut self.layers[0].name);
        self.layers = vec![Layer::new(name, self.flattened())];
        self.active_layer = 0;
        self.unsaved = true;
    }

    /// Whether the image is no more than one plain layer, which a file without layers holds whole.
    pub fn is_flat(&self) -> bool {
        let only = &self.layers[0];
        self.layers.len() == 1 && only.visible && only.opacity == MAX_OPACITY && only.blend_mode == BlendMode::Normal
    }

    pub fn active(&self) -> &Layer {
        &self.layers[self.active_layer]
    }

    pub fn active_mut(&mut self) -> &mut Layer {
        &mut self.layers[self.active_layer]
    }

    /// Makes a layer the one the tools and edits act on. Returns whether it changed.
    pub fn set_active_layer(&mut self, index: usize) -> bool {
        if index == self.active_layer || index >= self.layers.len() {
            return false;
        }
        self.active_layer = index;
        true
    }

    /// Adds an empty layer over the active one, which it becomes.
    pub fn add_layer(&mut self) {
        let (width, height) = self.size();
        let layer = Layer::new(self.new_layer_name(), RgbaImage::new(width, height));
        self.insert_layer(layer);
    }

    /// Puts a layer over the active one, which it becomes.
    pub fn insert_layer(&mut self, layer: Layer) {
        self.active_layer += 1;
        self.layers.insert(self.active_layer, layer);
        self.unsaved = true;
    }

    /// Removes the active layer, unless it is the only one. The one under it becomes
    /// active, or the one now at the bottom. Returns whether it was removed.
    pub fn delete_layer(&mut self) -> bool {
        if self.layers.len() < 2 {
            return false;
        }
        self.layers.remove(self.active_layer);
        self.active_layer = self.active_layer.saturating_sub(1);
        self.unsaved = true;
        true
    }

    /// Adds a copy of the active layer over it, which becomes active.
    pub fn duplicate_layer(&mut self) {
        let mut copy = self.active().clone();
        copy.name.push_str(" copy");
        self.insert_layer(copy);
    }

    /// Blends the active layer into the one under it, which keeps its own properties
    /// and becomes active. Returns whether there was a layer under it.
    pub fn merge_layer_down(&mut self) -> bool {
        if self.active_layer == 0 {
            return false;
        }
        let upper = self.layers.remove(self.active_layer);
        self.active_layer -= 1;
        blend_layer(&mut self.active_mut().pixels, &upper);
        self.unsaved = true;
        true
    }

    /// Moves a layer to another place among the layers, as near as there is to the one
    /// given. The active layer stays the same one. Returns whether it moved.
    pub fn move_layer(&mut self, from: usize, to: usize) -> bool {
        let to = to.min(self.layers.len() - 1);
        if from == to || from >= self.layers.len() {
            return false;
        }
        let active = self.active_layer;
        let layer = self.layers.remove(from);
        self.layers.insert(to, layer);
        self.active_layer = match active {
            _ if active == from => to,
            _ if from < active && active <= to => active - 1,
            _ if to <= active && active < from => active + 1,
            _ => active,
        };
        self.unsaved = true;
        true
    }

    /// Mirrors the active layer, along either axis or both, which turns it upside down.
    pub fn flip_layer(&mut self, horizontally: bool, vertically: bool) {
        let pixels = &mut self.active_mut().pixels;
        if horizontally {
            imageops::flip_horizontal_in_place(pixels);
        }
        if vertically {
            imageops::flip_vertical_in_place(pixels);
        }
        self.unsaved = true;
    }

    /// "Layer" and the number of layers there would then be, or the next number no
    /// other layer is named with.
    fn new_layer_name(&self) -> String {
        (self.layers.len() + 1..)
            .map(|number| format!("Layer {number}"))
            .find(|name| self.layers.iter().all(|layer| &layer.name != name))
            .expect("some number is free")
    }
}

#[cfg(test)]
mod tests {
    use image::Rgba;

    use super::*;

    fn names(document: &Document) -> Vec<&str> {
        document.layers.iter().map(|layer| layer.name.as_str()).collect()
    }

    fn document() -> Document {
        Document::new(RgbaImage::from_pixel(2, 2, Rgba([255, 0, 0, 255])))
    }

    #[test]
    fn added_layers_go_over_the_active_one() {
        let mut document = document();
        document.add_layer();
        document.add_layer();
        assert_eq!(names(&document), ["Background", "Layer 2", "Layer 3"]);
        assert_eq!(document.active_layer, 2);
        document.set_active_layer(0);
        document.duplicate_layer();
        assert_eq!(names(&document), ["Background", "Background copy", "Layer 2", "Layer 3"]);
        assert_eq!(document.active_layer, 1);
        // Taken names are skipped.
        document.add_layer();
        assert_eq!(document.active().name, "Layer 5");
        assert!(document.unsaved);
    }

    #[test]
    fn deleting_keeps_one_layer() {
        let mut document = document();
        document.add_layer();
        assert!(document.delete_layer());
        assert_eq!((names(&document), document.active_layer), (vec!["Background"], 0));
        assert!(!document.delete_layer());
    }

    #[test]
    fn moving_keeps_the_active_layer() {
        let mut document = document();
        document.add_layer();
        document.add_layer();
        document.set_active_layer(1);
        assert!(document.move_layer(0, 2));
        assert_eq!(names(&document), ["Layer 2", "Layer 3", "Background"]);
        assert_eq!(document.active().name, "Layer 2");
        assert!(document.move_layer(0, 9));
        assert_eq!(names(&document), ["Layer 3", "Background", "Layer 2"]);
        assert_eq!(document.active().name, "Layer 2");
        assert!(!document.move_layer(2, 2));
    }

    #[test]
    fn flattening_leaves_one_plain_layer() {
        let mut document = document();
        assert!(document.is_flat());
        document.layers[0].opacity = 100;
        assert!(!document.is_flat());
        document.add_layer();
        document.active_mut().pixels.put_pixel(0, 0, Rgba([0, 0, 255, 255]));
        let flat = document.flattened();
        document.unsaved = false;
        document.flatten();
        assert!(document.is_flat() && document.unsaved);
        assert_eq!((document.active().name.as_str(), document.active_layer), ("Background", 0));
        assert_eq!(document.active().pixels, flat);
    }

    #[test]
    fn merging_down_blends_into_the_lower_layer() {
        let mut document = document();
        document.add_layer();
        document.active_mut().pixels.put_pixel(0, 0, Rgba([0, 0, 255, 255]));
        document.layers[0].opacity = 100;
        assert!(document.merge_layer_down());
        assert_eq!((document.layers.len(), document.active_layer), (1, 0));
        let merged = &document.layers[0];
        assert_eq!((merged.name.as_str(), merged.opacity), ("Background", 100));
        assert_eq!(merged.pixels[(0, 0)], Rgba([0, 0, 255, 255]));
        assert_eq!(merged.pixels[(1, 0)], Rgba([255, 0, 0, 255]));
        assert!(!document.merge_layer_down());
    }
}

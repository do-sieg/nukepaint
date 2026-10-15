use std::path::{Path, PathBuf};

use image::{RgbaImage, imageops};

use crate::{
    BlendMode, Layer, MAX_OPACITY, OpenError, PixelRect, Selection, SelectionMask, SelectionMode, Step, blend_layer,
    composite, composite_area, copy_selected, erase_selected, history::History, load_image, save_image,
};

/// An image being edited: its layers, bottom first, the file it came from, and the
/// steps it went through since, to undo and redo.
///
/// The layers, the active one and the selection are changed directly, then recorded
/// as a step with [`Document::commit`]. Until then the history knows nothing of the
/// change, and neither does [`Document::is_unsaved`].
pub struct Document {
    /// The file the document is saved to, if it has one yet.
    pub path: Option<PathBuf>,
    pub layers: Vec<Layer>,
    pub active_layer: usize,
    pub selection: Selection,
    history: History,
}

impl Document {
    /// A document never saved, of a single layer, and with nothing to save either.
    pub fn new(pixels: RgbaImage) -> Self {
        Self::with_first_step(pixels, "New Image", None, true)
    }

    /// A document of pasted pixels, which has changes to save as it is in no file.
    pub fn pasted(pixels: RgbaImage) -> Self {
        Self::with_first_step(pixels, "Paste in to New Image", Some("paste"), false)
    }

    /// Reads a document from an image file, as a single layer.
    pub fn open(path: &Path) -> Result<Self, OpenError> {
        let document = Self::with_first_step(load_image(path)?, "Open", Some("open"), true);
        Ok(Self { path: Some(path.to_path_buf()), ..document })
    }

    fn with_first_step(pixels: RgbaImage, name: &str, icon: Option<&'static str>, saved: bool) -> Self {
        let layers = vec![Layer::new("Background", pixels)];
        let selection = Selection::default();
        let history = History::new(Step { name: name.to_owned(), icon }, &layers, 0, &selection, saved);
        Self { path: None, layers, active_layer: 0, selection, history }
    }

    /// Writes the image to a file, in the format its extension names, and makes that
    /// file the document's own. With `flatten`, the layers are then blended into a
    /// single one as well, as one more step, to be as they are in the file.
    pub fn save(&mut self, path: &Path, flatten: bool) -> Result<(), image::ImageError> {
        save_image(path, &self.flattened())?;
        self.path = Some(path.to_path_buf());
        if flatten && !self.is_flat() {
            self.flatten();
            self.commit("Flatten", None);
        }
        self.history.mark_saved();
        Ok(())
    }

    /// Whether the document changed since it was opened or last saved: undoing back to
    /// then makes it as it was.
    pub fn is_unsaved(&self) -> bool {
        !self.history.is_saved()
    }

    /// Records the layers, the active one and the selection as they now are, as a step
    /// after the current one, in place of any undone.
    pub fn commit(&mut self, name: impl Into<String>, icon: Option<&'static str>) {
        let step = Step { name: name.into(), icon };
        self.history.record(step, &self.layers, self.active_layer, &self.selection);
    }

    /// Each step, the first being how the document was opened or made.
    pub fn steps(&self) -> impl ExactSizeIterator<Item = &Step> {
        self.history.steps()
    }

    /// Which of the steps the document is as of.
    pub fn current_step(&self) -> usize {
        self.history.current()
    }

    pub fn can_undo(&self) -> bool {
        self.history.current() > 0
    }

    pub fn can_redo(&self) -> bool {
        self.history.current() + 1 < self.history.len()
    }

    /// Returns whether there was a step to undo.
    pub fn undo(&mut self) -> bool {
        self.can_undo() && self.go_to_step(self.history.current() - 1)
    }

    /// Returns whether there was a step to redo.
    pub fn redo(&mut self) -> bool {
        self.go_to_step(self.history.current() + 1)
    }

    /// Puts the document back as it was after one of its steps, undoing or redoing as
    /// many as that takes. Returns whether it changed.
    pub fn go_to_step(&mut self, index: usize) -> bool {
        let Some((layers, active_layer, selection)) = self.history.go_to(index) else {
            return false;
        };
        self.layers = layers;
        self.active_layer = active_layer;
        self.selection = selection;
        true
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
        erase_selected(&mut self.layers[self.active_layer].pixels, &self.selection)
    }

    /// Selects the whole image. Returns whether the selection changed.
    pub fn select_all(&mut self) -> bool {
        self.set_selection(self.whole())
    }

    /// Selects nothing. Returns whether anything was selected.
    pub fn deselect(&mut self) -> bool {
        self.set_selection(Selection::default())
    }

    /// Selects what of the image was not selected, and nothing else. With nothing
    /// selected, that is the whole image. Returns whether the selection changed.
    pub fn invert_selection(&mut self) -> bool {
        self.set_selection(self.whole().combine(&self.selection, SelectionMode::Subtract))
    }

    fn whole(&self) -> Selection {
        let (width, height) = self.size();
        Selection::rect(PixelRect { left: 0, top: 0, right: width as i32, bottom: height as i32 })
    }

    fn set_selection(&mut self, selection: Selection) -> bool {
        if selection == self.selection {
            return false;
        }
        self.selection = selection;
        true
    }

    /// The layers blended into one image.
    pub fn flattened(&self) -> RgbaImage {
        composite(&self.layers)
    }

    /// The layers blended together over one area of the image, which must be on it.
    pub fn flattened_area(&self, area: PixelRect) -> RgbaImage {
        composite_area(&self.layers, area)
    }

    /// The pixels drawing, filling and adjusting may change: the selected ones, or all
    /// of them, as nothing says, while nothing is selected.
    pub fn selection_mask(&self) -> Option<SelectionMask> {
        let (width, height) = self.size();
        SelectionMask::of(&self.selection, width, height)
    }

    /// Blends all the layers into a single one, named after the bottom one.
    pub fn flatten(&mut self) {
        if self.is_flat() {
            return;
        }
        let name = std::mem::take(&mut self.layers[0].name);
        self.layers = vec![Layer::new(name, self.flattened())];
        self.active_layer = 0;
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
    }

    /// Removes the active layer, unless it is the only one. The one under it becomes
    /// active, or the one now at the bottom. Returns whether it was removed.
    pub fn delete_layer(&mut self) -> bool {
        if self.layers.len() < 2 {
            return false;
        }
        self.layers.remove(self.active_layer);
        self.active_layer = self.active_layer.saturating_sub(1);
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
        document.flatten();
        assert!(document.is_flat());
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

    #[test]
    fn select_all_and_deselect() {
        let mut document = Document::new(RgbaImage::new(4, 3));
        assert!(!document.deselect());
        assert!(document.select_all());
        assert_eq!(document.selection, Selection::rect(PixelRect { left: 0, top: 0, right: 4, bottom: 3 }));
        assert!(!document.select_all());
        assert!(document.deselect());
        assert!(document.selection.is_empty());
    }

    #[test]
    fn invert_selects_the_rest_of_the_image() {
        let mut document = Document::new(RgbaImage::new(4, 3));
        assert!(document.invert_selection());
        assert_eq!(document.selection, Selection::rect(PixelRect { left: 0, top: 0, right: 4, bottom: 3 }));
        assert!(document.invert_selection());
        assert!(document.selection.is_empty());
        // What is off the image is left out.
        document.selection = Selection::rect(PixelRect { left: -2, top: 1, right: 2, bottom: 2 });
        assert!(document.invert_selection());
        let selected = |document: &Document| {
            (0..3).flat_map(|y| (0..4).map(move |x| (x, y))).filter(|&(x, y)| document.selection.contains(x, y)).count()
        };
        assert_eq!(selected(&document), 12 - 2);
        assert!(!document.selection.contains(1, 1) && document.selection.contains(2, 1));
        assert!(!document.selection.contains(-1, 1));
        assert!(document.invert_selection());
        assert_eq!(selected(&document), 2);
    }

    #[test]
    fn undo_and_redo_go_between_steps() {
        let mut document = document();
        assert!(!document.can_undo() && !document.can_redo() && !document.is_unsaved());
        document.add_layer();
        document.commit("Add New Layer", Some("layer_add"));
        document.active_mut().pixels.put_pixel(0, 0, Rgba([0, 0, 255, 255]));
        document.selection = Selection::rect(crate::PixelRect { left: 0, top: 0, right: 1, bottom: 1 });
        document.commit("Paint", None);
        assert!(document.is_unsaved());
        let names = |document: &Document| document.steps().map(|step| step.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&document), ["New Image", "Add New Layer", "Paint"]);

        assert!(document.undo());
        assert_eq!((document.layers.len(), document.current_step()), (2, 1));
        assert_eq!(document.active().pixels[(0, 0)], Rgba([0, 0, 0, 0]));
        assert!(document.selection.is_empty());
        assert!(document.go_to_step(0));
        assert_eq!((document.layers.len(), document.active_layer), (1, 0));
        // Back where it was opened, there is nothing to save.
        assert!(!document.is_unsaved() && !document.undo());
        assert!(document.go_to_step(2));
        assert_eq!(document.active().pixels[(0, 0)], Rgba([0, 0, 255, 255]));
        assert!(!document.selection.is_empty());
        assert!(!document.redo());
    }

    #[test]
    fn a_new_step_drops_those_undone() {
        let mut document = document();
        document.add_layer();
        document.commit("Add New Layer", None);
        document.add_layer();
        document.commit("Add New Layer", None);
        document.undo();
        document.flip_layer(true, false);
        document.commit("Flip Horizontal", None);
        assert_eq!(document.steps().len(), 3);
        assert_eq!(document.current_step(), 2);
        assert!(!document.can_redo());
    }

    #[test]
    fn saving_undone_steps_away_leaves_changes() {
        let mut document = document();
        document.add_layer();
        document.commit("Add New Layer", None);
        document.history.mark_saved();
        document.undo();
        assert!(document.is_unsaved());
        // The step the file holds is gone for good.
        document.flip_layer(true, false);
        document.commit("Flip Horizontal", None);
        document.undo();
        assert!(document.is_unsaved());
        assert!(Document::pasted(RgbaImage::new(1, 1)).is_unsaved());
    }
}

use std::path::{Path, PathBuf};

use image::RgbaImage;

use crate::{Layer, OpenError, Selection, copy_selected, erase_selected, load_image, save_image};

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
    // Only a single layer can be opened for now, so it is the image as it is.
    pub fn flattened(&self) -> RgbaImage {
        self.layers[0].pixels.clone()
    }
}

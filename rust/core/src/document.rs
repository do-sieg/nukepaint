use std::path::{Path, PathBuf};

use image::RgbaImage;

use crate::{Layer, OpenError, Selection, load_image};

/// An image being edited: its layers, bottom first, and the file it came from.
pub struct Document {
    /// The file the document is saved to, if it has one yet.
    pub path: Option<PathBuf>,
    pub layers: Vec<Layer>,
    pub active_layer: usize,
    pub selection: Selection,
}

impl Document {
    /// Reads a document from an image file, as a single layer.
    pub fn open(path: &Path) -> Result<Self, OpenError> {
        let pixels = load_image(path)?;
        Ok(Self {
            path: Some(path.to_path_buf()),
            layers: vec![Layer::new("Background", pixels)],
            active_layer: 0,
            selection: Selection::default(),
        })
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

    /// The layers blended into one image.
    // Only a single layer can be opened for now, so it is the image as it is.
    pub fn flattened(&self) -> RgbaImage {
        self.layers[0].pixels.clone()
    }
}

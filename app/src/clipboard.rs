//! Copied pixels, shared with other applications through the system clipboard.

use std::borrow::Cow;

use arboard::ImageData;
use nukepaint_core::RgbaImage;

pub struct Clipboard {
    /// Kept for as long as the app runs: on Linux, what was copied goes away with it.
    system: Option<arboard::Clipboard>,
    /// What the system clipboard held when last looked at, or what was copied last
    /// when there is no system clipboard to be had.
    image: Option<RgbaImage>,
}

impl Clipboard {
    pub fn new() -> Self {
        let mut clipboard = Self { system: arboard::Clipboard::new().ok(), image: None };
        clipboard.refresh();
        clipboard
    }

    pub fn image(&self) -> Option<&RgbaImage> {
        self.image.as_ref()
    }

    /// Takes whatever image another application may have copied since. Something
    /// other than an image, like text, leaves nothing to paste.
    pub fn refresh(&mut self) {
        let Some(system) = &mut self.system else {
            return;
        };
        match system.get_image() {
            Ok(data) => {
                let (width, height) = (data.width as u32, data.height as u32);
                self.image = RgbaImage::from_raw(width, height, data.bytes.into_owned());
            }
            Err(arboard::Error::ContentNotAvailable) => self.image = None,
            // Keeps what the app copied when the system clipboard cannot be read.
            Err(_) => {}
        }
    }

    pub fn set(&mut self, image: RgbaImage) {
        if let Some(system) = &mut self.system {
            let data = ImageData {
                width: image.width() as usize,
                height: image.height() as usize,
                bytes: Cow::Borrowed(image.as_raw()),
            };
            // The app can still paste it itself.
            let _ = system.set_image(data);
        }
        self.image = Some(image);
    }
}

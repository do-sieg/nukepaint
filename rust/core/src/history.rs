//! The undo history of a document: how it was after each change made to it, its
//! layers, selection and palette, to go back or forward to. The pixels of a layer a change left alone are shared with the
//! step before rather than copied again.

use std::sync::Arc;

use image::RgbaImage;

use crate::{BlendMode, Layer, Palette, Selection};

/// A change made to a document, as the history lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub name: String,
    /// The name of its icon's file in `icons/`, without the extension, if it has one.
    pub icon: Option<&'static str>,
}

/// A layer as a step left it.
struct LayerState {
    name: String,
    visible: bool,
    opacity: u8,
    blend_mode: BlendMode,
    pixels: Arc<RgbaImage>,
}

impl LayerState {
    /// The layer as it now is, sharing the pixels of a layer of the step before that
    /// holds the same ones: the one in the same place first, as it most often is.
    fn of(layer: &Layer, index: usize, before: &[LayerState]) -> Self {
        let others = before.iter().enumerate().filter(|&(other, _)| other != index).map(|(_, state)| state);
        let same = before.get(index).into_iter().chain(others).find(|state| *state.pixels == layer.pixels);
        Self {
            name: layer.name.clone(),
            visible: layer.visible,
            opacity: layer.opacity,
            blend_mode: layer.blend_mode,
            pixels: same.map_or_else(|| Arc::new(layer.pixels.clone()), |state| Arc::clone(&state.pixels)),
        }
    }

    /// Whether it is the same layer as `other`, which a file would hold alike: the same
    /// pixels, shared, and the same name and blending.
    fn same_as(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.pixels, &other.pixels)
            && self.name == other.name
            && self.visible == other.visible
            && self.opacity == other.opacity
            && self.blend_mode == other.blend_mode
    }

    fn to_layer(&self) -> Layer {
        Layer {
            name: self.name.clone(),
            pixels: RgbaImage::clone(&self.pixels),
            visible: self.visible,
            opacity: self.opacity,
            blend_mode: self.blend_mode,
        }
    }
}

/// A document as a step left it.
struct State {
    step: Step,
    layers: Vec<LayerState>,
    /// Which of the layers' contents the step left: steps that leave the layers as the
    /// one before did, changing only the selection or the active layer, share it, as
    /// they leave the same file.
    content: usize,
    active_layer: usize,
    selection: Selection,
    /// Which no file holds, so changing it alone leaves the file saved.
    palette: Palette,
}

/// A document as a step left it, to put back.
pub(crate) struct Restored {
    pub layers: Vec<Layer>,
    pub active_layer: usize,
    pub selection: Selection,
    pub palette: Palette,
}

pub(crate) struct History {
    /// The first is how the document was opened or made.
    states: Vec<State>,
    /// The one the document is as of; those after it were undone.
    current: usize,
    /// The content the document's file holds, if any does.
    saved: Option<usize>,
    /// The content the next step that changes the layers leaves.
    next_content: usize,
}

impl History {
    /// A history that starts with the document as it is, held by its file with `saved`.
    pub(crate) fn new(
        step: Step,
        layers: &[Layer],
        active_layer: usize,
        selection: &Selection,
        palette: &Palette,
        saved: bool,
    ) -> Self {
        let layers = layers.iter().enumerate().map(|(index, layer)| LayerState::of(layer, index, &[])).collect();
        let first = State { step, layers, content: 0, active_layer, selection: selection.clone(), palette: palette.clone() };
        Self { states: vec![first], current: 0, saved: saved.then_some(0), next_content: 1 }
    }

    /// The content `layers` hold: that of `before`, the step before them, if they are
    /// the same, or else a new one.
    fn content_of(&mut self, layers: &[LayerState], before: Option<usize>) -> usize {
        let before = before.map(|index| &self.states[index]);
        let same = |before: &State| {
            before.layers.len() == layers.len() && before.layers.iter().zip(layers).all(|(a, b)| a.same_as(b))
        };
        match before {
            Some(before) if same(before) => before.content,
            _ => {
                self.next_content += 1;
                self.next_content - 1
            }
        }
    }

    /// Puts the document as it now is in place of the current step, which keeps its name:
    /// for a change that carries on the one it records.
    pub(crate) fn amend(&mut self, layers: &[Layer], active_layer: usize, selection: &Selection, palette: &Palette) {
        let previous = self.current.checked_sub(1);
        let before = previous.map_or(&[][..], |previous| &self.states[previous].layers[..]);
        let layers: Vec<_> = layers.iter().enumerate().map(|(index, layer)| LayerState::of(layer, index, before)).collect();
        let content = self.content_of(&layers, previous);
        let state = &mut self.states[self.current];
        state.layers = layers;
        state.content = content;
        state.active_layer = active_layer;
        state.selection = selection.clone();
        state.palette = palette.clone();
    }

    /// Gives the current step another name and icon.
    pub(crate) fn rename(&mut self, step: Step) {
        self.states[self.current].step = step;
    }

    /// Adds the document as it now is as the step after the current one, in place of
    /// any that were undone.
    pub(crate) fn record(&mut self, step: Step, layers: &[Layer], active_layer: usize, selection: &Selection, palette: &Palette) {
        let before = &self.states[self.current].layers;
        let layers: Vec<_> = layers.iter().enumerate().map(|(index, layer)| LayerState::of(layer, index, before)).collect();
        let content = self.content_of(&layers, Some(self.current));
        self.states.truncate(self.current + 1);
        let (selection, palette) = (selection.clone(), palette.clone());
        self.states.push(State { step, layers, content, active_layer, selection, palette });
        self.current += 1;
    }

    /// Makes another step the current one. Returns the document as of it, or nothing
    /// if there is no such step or it is already current.
    pub(crate) fn go_to(&mut self, index: usize) -> Option<Restored> {
        if index == self.current || index >= self.states.len() {
            return None;
        }
        self.current = index;
        let state = &self.states[index];
        Some(Restored {
            layers: state.layers.iter().map(LayerState::to_layer).collect(),
            active_layer: state.active_layer,
            selection: state.selection.clone(),
            palette: state.palette.clone(),
        })
    }

    pub(crate) fn steps(&self) -> impl ExactSizeIterator<Item = &Step> {
        self.states.iter().map(|state| &state.step)
    }

    pub(crate) fn current(&self) -> usize {
        self.current
    }

    /// The content of the current step's layers.
    pub(crate) fn current_content(&self) -> usize {
        self.states[self.current].content
    }

    pub(crate) fn len(&self) -> usize {
        self.states.len()
    }

    /// The file now holds the document as of the current step.
    pub(crate) fn mark_saved(&mut self) {
        self.saved = Some(self.states[self.current].content);
    }

    /// Whether the file holds the layers as the current step left them, whatever is
    /// selected.
    pub(crate) fn is_saved(&self) -> bool {
        self.saved == Some(self.states[self.current].content)
    }
}

#[cfg(test)]
mod tests {
    use image::Rgba;

    use super::*;

    fn step(name: &str) -> Step {
        Step { name: name.to_owned(), icon: None }
    }

    #[test]
    fn unchanged_pixels_are_shared() {
        let mut layers = vec![Layer::new("Background", RgbaImage::new(4, 4))];
        let selection = Selection::default();
        let mut history = History::new(step("Open"), &layers, 0, &selection, &Palette::default(), true);
        layers.push(Layer::new("Layer 2", RgbaImage::from_pixel(4, 4, Rgba([0, 0, 255, 255]))));
        layers.swap(0, 1);
        history.record(step("Add and Move"), &layers, 0, &selection, &Palette::default());
        layers[0].pixels.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        history.record(step("Paint"), &layers, 0, &selection, &Palette::default());
        let [opened, added, painted] = &history.states[..] else { panic!("three steps") };
        // The background moved up, and kept its pixels.
        assert!(Arc::ptr_eq(&opened.layers[0].pixels, &added.layers[1].pixels));
        assert!(Arc::ptr_eq(&added.layers[1].pixels, &painted.layers[1].pixels));
        assert!(!Arc::ptr_eq(&added.layers[0].pixels, &painted.layers[0].pixels));
        let restored = history.go_to(1).unwrap().layers;
        assert_eq!(restored[0].pixels, RgbaImage::from_pixel(4, 4, Rgba([0, 0, 255, 255])));
        assert!(history.go_to(1).is_none());
    }

    #[test]
    fn amending_replaces_the_current_step() {
        let mut layers = vec![Layer::new("Background", RgbaImage::new(4, 4))];
        let selection = Selection::default();
        let mut history = History::new(step("Open"), &layers, 0, &selection, &Palette::default(), true);
        layers[0].pixels.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        history.record(step("Move"), &layers, 0, &selection, &Palette::default());
        history.mark_saved();
        layers[0].pixels.put_pixel(0, 0, Rgba([0, 255, 0, 255]));
        history.amend(&layers, 0, &selection, &Palette::default());
        assert_eq!(history.len(), 2);
        assert_eq!(history.states[1].step, step("Move"));
        assert_eq!(*history.states[1].layers[0].pixels.get_pixel(0, 0), Rgba([0, 255, 0, 255]));
        // What was saved is no longer what the step holds.
        assert!(!history.is_saved());
    }

    #[test]
    fn renaming_names_the_current_step() {
        let layers = vec![Layer::new("Background", RgbaImage::new(4, 4))];
        let selection = Selection::default();
        let mut history = History::new(step("Open"), &layers, 0, &selection, &Palette::default(), true);
        history.record(step("Magic Wand"), &layers, 0, &selection, &Palette::default());
        history.rename(step("Deselect"));
        assert_eq!(history.len(), 2);
        assert_eq!(history.states[0].step, step("Open"));
        assert_eq!(history.states[1].step, step("Deselect"));
    }

    #[test]
    fn selecting_leaves_the_file_saved() {
        let mut layers = vec![Layer::new("Background", RgbaImage::new(4, 4))];
        let mut history = History::new(step("Open"), &layers, 0, &Selection::default(), &Palette::default(), true);
        let square = Selection::rect(crate::PixelRect { left: 0, top: 0, right: 2, bottom: 2 });
        history.record(step("Rectangle Select"), &layers, 0, &square, &Palette::default());
        assert!(history.is_saved());
        layers[0].pixels.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        history.record(step("Paint"), &layers, 0, &square, &Palette::default());
        assert!(!history.is_saved());
        history.record(step("Deselect"), &layers, 0, &Selection::default(), &Palette::default());
        assert!(!history.is_saved());
        history.mark_saved();
        history.go_to(2);
        assert!(history.is_saved(), "the same pixels as were saved");
        history.go_to(1);
        assert!(!history.is_saved());
        // How a layer is blended is saved with it.
        history.go_to(3);
        layers[0].opacity = 10;
        history.record(step("Layer Properties"), &layers, 0, &Selection::default(), &Palette::default());
        assert!(!history.is_saved());
    }
}

//! The undo history of a document: how it was after each change made to it, to go
//! back or forward to. The pixels of a layer a change left alone are shared with the
//! step before rather than copied again.

use std::sync::Arc;

use image::RgbaImage;

use crate::{BlendMode, Layer, Selection};

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
    active_layer: usize,
    selection: Selection,
}

pub(crate) struct History {
    /// The first is how the document was opened or made.
    states: Vec<State>,
    /// The one the document is as of; those after it were undone.
    current: usize,
    /// The one the document's file holds, if any does.
    saved: Option<usize>,
}

impl History {
    /// A history that starts with the document as it is, held by its file with `saved`.
    pub(crate) fn new(step: Step, layers: &[Layer], active_layer: usize, selection: &Selection, saved: bool) -> Self {
        let layers = layers.iter().enumerate().map(|(index, layer)| LayerState::of(layer, index, &[])).collect();
        let first = State { step, layers, active_layer, selection: selection.clone() };
        Self { states: vec![first], current: 0, saved: saved.then_some(0) }
    }

    /// Puts the document as it now is in place of the current step, which keeps its name:
    /// for a change that carries on the one it records.
    pub(crate) fn amend(&mut self, layers: &[Layer], active_layer: usize, selection: &Selection) {
        let before = self.current.checked_sub(1).map_or(&[][..], |previous| &self.states[previous].layers[..]);
        let layers = layers.iter().enumerate().map(|(index, layer)| LayerState::of(layer, index, before)).collect();
        let state = &mut self.states[self.current];
        state.layers = layers;
        state.active_layer = active_layer;
        state.selection = selection.clone();
        if self.saved == Some(self.current) {
            self.saved = None;
        }
    }

    /// Adds the document as it now is as the step after the current one, in place of
    /// any that were undone.
    pub(crate) fn record(&mut self, step: Step, layers: &[Layer], active_layer: usize, selection: &Selection) {
        let before = &self.states[self.current].layers;
        let layers = layers.iter().enumerate().map(|(index, layer)| LayerState::of(layer, index, before)).collect();
        self.states.truncate(self.current + 1);
        if self.saved.is_some_and(|saved| saved > self.current) {
            self.saved = None;
        }
        self.states.push(State { step, layers, active_layer, selection: selection.clone() });
        self.current += 1;
    }

    /// Makes another step the current one. Returns the document's layers, active layer
    /// and selection as of it, or nothing if there is no such step or it is already current.
    pub(crate) fn go_to(&mut self, index: usize) -> Option<(Vec<Layer>, usize, Selection)> {
        if index == self.current || index >= self.states.len() {
            return None;
        }
        self.current = index;
        let state = &self.states[index];
        let layers = state.layers.iter().map(LayerState::to_layer).collect();
        Some((layers, state.active_layer, state.selection.clone()))
    }

    pub(crate) fn steps(&self) -> impl ExactSizeIterator<Item = &Step> {
        self.states.iter().map(|state| &state.step)
    }

    pub(crate) fn current(&self) -> usize {
        self.current
    }

    pub(crate) fn len(&self) -> usize {
        self.states.len()
    }

    /// The file now holds the document as of the current step.
    pub(crate) fn mark_saved(&mut self) {
        self.saved = Some(self.current);
    }

    pub(crate) fn is_saved(&self) -> bool {
        self.saved == Some(self.current)
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
        let mut history = History::new(step("Open"), &layers, 0, &selection, true);
        layers.push(Layer::new("Layer 2", RgbaImage::from_pixel(4, 4, Rgba([0, 0, 255, 255]))));
        layers.swap(0, 1);
        history.record(step("Add and Move"), &layers, 0, &selection);
        layers[0].pixels.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        history.record(step("Paint"), &layers, 0, &selection);
        let [opened, added, painted] = &history.states[..] else { panic!("three steps") };
        // The background moved up, and kept its pixels.
        assert!(Arc::ptr_eq(&opened.layers[0].pixels, &added.layers[1].pixels));
        assert!(Arc::ptr_eq(&added.layers[1].pixels, &painted.layers[1].pixels));
        assert!(!Arc::ptr_eq(&added.layers[0].pixels, &painted.layers[0].pixels));
        let (restored, ..) = history.go_to(1).unwrap();
        assert_eq!(restored[0].pixels, RgbaImage::from_pixel(4, 4, Rgba([0, 0, 255, 255])));
        assert!(history.go_to(1).is_none());
    }

    #[test]
    fn amending_replaces_the_current_step() {
        let mut layers = vec![Layer::new("Background", RgbaImage::new(4, 4))];
        let selection = Selection::default();
        let mut history = History::new(step("Open"), &layers, 0, &selection, true);
        layers[0].pixels.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        history.record(step("Move"), &layers, 0, &selection);
        history.mark_saved();
        layers[0].pixels.put_pixel(0, 0, Rgba([0, 255, 0, 255]));
        history.amend(&layers, 0, &selection);
        assert_eq!(history.len(), 2);
        assert_eq!(history.states[1].step, step("Move"));
        assert_eq!(*history.states[1].layers[0].pixels.get_pixel(0, 0), Rgba([0, 255, 0, 255]));
        // What was saved is no longer what the step holds.
        assert!(!history.is_saved());
    }
}

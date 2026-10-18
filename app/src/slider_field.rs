//! A number in a field to type it in or drag it, on a slider beside it unless there is
//! no room for one, as the dialogs and the tool options ask for numbers. The mouse
//! wheel over either steps it.

use std::ops::RangeInclusive;

use eframe::egui::{self, Event, MouseWheelUnit, Response, Ui, Vec2, Widget, emath::Numeric};

/// Width of the field, in points, unless set: wide enough for any of the dialogs'
/// numbers, so a dialog keeps its width as they change.
const FIELD_WIDTH: f32 = 72.0;
/// Share of the number a notch of the wheel steps a logarithmic one by, so that it goes
/// as fast through the big numbers as the slider does.
const LOGARITHMIC_STEP: f64 = 0.1;

pub struct SliderField<'a, Num: Numeric> {
    value: &'a mut Num,
    range: RangeInclusive<Num>,
    step: f64,
    suffix: &'a str,
    logarithmic: bool,
    /// The slider's width, if not the usual one, or none at all without a slider.
    slider: Option<Option<f32>>,
    field_width: f32,
}

impl<'a, Num: Numeric> SliderField<'a, Num> {
    /// The number from the start of `range` to its end, going up and down by 1.
    pub fn new(value: &'a mut Num, range: RangeInclusive<Num>) -> Self {
        Self { value, range, step: 1.0, suffix: "", logarithmic: false, slider: Some(None), field_width: FIELD_WIDTH }
    }

    /// Going up and down by `step`, with as many decimals as it has.
    pub fn step(self, step: f64) -> Self {
        Self { step, ..self }
    }

    /// Shown after the number in the field.
    pub fn suffix(self, suffix: &'a str) -> Self {
        Self { suffix, ..self }
    }

    /// The slider and the wheel going further as the numbers grow, for the small ones
    /// to have more room and the big ones to be reached quickly.
    pub fn logarithmic(self) -> Self {
        Self { logarithmic: true, ..self }
    }

    /// The slider's width, in points, in place of the usual one.
    pub fn slider_width(self, width: f32) -> Self {
        Self { slider: Some(Some(width)), ..self }
    }

    /// Only the field, with no slider.
    pub fn field_only(self) -> Self {
        Self { slider: None, ..self }
    }

    /// The field's width, in points.
    pub fn field_width(self, width: f32) -> Self {
        Self { field_width: width, ..self }
    }
}

impl<Num: Numeric> Widget for SliderField<'_, Num> {
    fn ui(self, ui: &mut Ui) -> Response {
        ui.horizontal(|ui| {
            let slider = self.slider.map(|width| {
                if let Some(width) = width {
                    ui.spacing_mut().slider_width = width;
                }
                let slider = egui::Slider::new(&mut *self.value, self.range.clone())
                    .step_by(self.step)
                    .logarithmic(self.logarithmic)
                    .show_value(false);
                ui.add(slider)
            });
            let decimals = (-self.step.log10()).ceil().max(0.0) as usize;
            let field = egui::DragValue::new(&mut *self.value)
                .range(self.range.clone())
                .speed(self.step)
                .fixed_decimals(decimals)
                .suffix(self.suffix);
            let height = ui.spacing().interact_size.y;
            let field = ui.add_sized([self.field_width, height], field);
            let mut response = match slider {
                Some(slider) => slider.union(field),
                None => field,
            };
            if response.hovered() && ui.is_enabled() {
                let notches = wheel_notches(ui, response.id);
                if notches != 0 {
                    let value = self.value.to_f64();
                    let step = if self.logarithmic { (value * LOGARITHMIC_STEP).round().max(self.step) } else { self.step };
                    let (min, max) = (self.range.start().to_f64(), self.range.end().to_f64());
                    let stepped = (value + notches as f64 * step).clamp(min, max);
                    // On the grid of steps, as the slider keeps it.
                    let stepped = min + ((stepped - min) / self.step).round() * self.step;
                    *self.value = Num::from_f64(stepped.min(max));
                    if self.value.to_f64() != value {
                        response.mark_changed();
                    }
                }
            }
            response
        })
        .inner
    }
}

/// Whole notches the wheel turned this frame, up being more, carrying over parts of one,
/// as a touchpad scrolls by, to the next frames. The scroll is taken, for what is
/// around not to scroll as well.
fn wheel_notches(ui: &Ui, id: egui::Id) -> i64 {
    let line = ui.ctx().options(|options| options.input_options.line_scroll_speed);
    let delta: f32 = ui.input(|input| {
        let wheel = input.events.iter().filter_map(|event| match event {
            Event::MouseWheel { unit, delta, .. } => Some(match unit {
                MouseWheelUnit::Point => delta.y / line,
                MouseWheelUnit::Line => delta.y,
                MouseWheelUnit::Page => delta.y.signum(),
            }),
            _ => None,
        });
        wheel.sum()
    });
    if delta == 0.0 {
        return 0;
    }
    ui.input_mut(|input| input.smooth_scroll_delta = Vec2::ZERO);
    let id = id.with("wheel");
    let total = ui.data_mut(|data| data.get_temp::<f32>(id)).unwrap_or(0.0) + delta;
    let notches = total.trunc();
    ui.data_mut(|data| data.insert_temp(id, total - notches));
    notches as i64
}

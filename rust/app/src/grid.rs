//! The grid: cells of a set size from a set origin, drawn as lines one screen pixel
//! wide over the image. While snapping to it, the selection tools put their corners on the corners of its
//! cells, moving the selection's box puts its top left corner on one, and resizing an
//! upright box puts its edges on its lines.
//!
//! The lines are drawn by `grid.wgsl` on the GPU, which works out each screen pixel
//! from whole image pixels, for every line to show however the image is zoomed.

use std::num::NonZeroU64;

use eframe::{
    egui::{Color32, Painter, Rect},
    egui_wgpu::{self, wgpu},
};
use power_paint_core::PixelRect;
use serde::{Deserialize, Serialize};

use crate::view::View;

/// Smallest a cell can be on screen, in screen pixels, for its lines to show: any
/// smaller and they would cover the image.
const MIN_CELL: f32 = 4.0;
const DEFAULT_COLOR: [u8; 4] = [0, 77, 255, 128];
/// Size of what `grid.wgsl` is given, in bytes.
const PARAMS_SIZE: u64 = 48;

/// Where the grid's cells are, in image pixels: where one starts, and how large each is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cells {
    pub origin: [i32; 2],
    pub size: [i32; 2],
}

impl Cells {
    /// Cells of the rectangle's size, one of which is the rectangle.
    pub fn around(rect: PixelRect) -> Self {
        let size = [(rect.right - rect.left).max(1), (rect.bottom - rect.top).max(1)];
        Self { origin: [rect.left.rem_euclid(size[0]), rect.top.rem_euclid(size[1])], size }
    }

    /// The corner of a cell nearest to a point of the image.
    pub fn snap(self, (x, y): (f64, f64)) -> (f64, f64) {
        let along = |value: f64, axis: usize| {
            let (origin, size) = (self.origin[axis] as f64, self.size[axis] as f64);
            origin + ((value - origin) / size).round() * size
        };
        (along(x, 0), along(y, 1))
    }

    /// The origin moved to within the first cell from the image's top left corner.
    fn first_origin(self) -> [i32; 2] {
        [self.origin[0].rem_euclid(self.size[0]), self.origin[1].rem_euclid(self.size[1])]
    }

    fn smallest_side(self) -> f32 {
        self.size[0].min(self.size[1]) as f32
    }
}

/// How the grid is set, as the settings keep it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Grid {
    pub visible: bool,
    pub snap: bool,
    /// Where a cell starts, from the image's top left corner, in pixels.
    pub origin: [i32; 2],
    /// How large each cell is, in pixels.
    pub cell: [i32; 2],
    /// The lines' colour, its alpha not multiplied in.
    pub color: [u8; 4],
}

impl Default for Grid {
    fn default() -> Self {
        Self {
            visible: false,
            snap: false,
            origin: [0, 0],
            cell: [16, 16],
            color: DEFAULT_COLOR,
        }
    }
}

impl Grid {
    pub fn cells(&self) -> Cells {
        Cells { origin: self.origin, size: self.cell.map(|side| side.max(1)) }
    }

    /// The cells to snap to, while snapping.
    pub fn snapping(&self) -> Option<Cells> {
        self.snap.then(|| self.cells())
    }

    pub fn color32(&self) -> Color32 {
        let [r, g, b, a] = self.color;
        Color32::from_rgba_unmultiplied(r, g, b, a)
    }
}

/// What the GPU draws the grid with, kept with egui's renderer.
struct Resources {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    buffer: wgpu::Buffer,
}

/// Gets the GPU ready to draw the grid.
pub fn init(render_state: &egui_wgpu::RenderState) {
    let device = &render_state.device;
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("grid"),
        source: wgpu::ShaderSource::Wgsl(include_str!("grid.wgsl").into()),
    });
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("grid"),
        size: PARAMS_SIZE,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("grid"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: NonZeroU64::new(PARAMS_SIZE),
            },
            count: None,
        }],
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("grid"),
        layout: &layout,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("grid"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let format = render_state.target_format;
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("grid"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some(if format.is_srgb() { "fs_linear" } else { "fs_gamma" }),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                // As egui blends, its colours having their alpha multiplied in.
                blend: Some(wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::One,
                        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::OneMinusDstAlpha,
                        dst_factor: wgpu::BlendFactor::One,
                        operation: wgpu::BlendOperation::Add,
                    },
                }),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        multiview_mask: None,
        cache: None,
    });
    render_state.renderer.write().callback_resources.insert(Resources { pipeline, bind_group, buffer });
}

/// Draws the grid's lines over the image, if it shows, unless its cells are too small
/// on screen to tell apart.
pub fn paint_lines(painter: &Painter, view: &View, grid: &Grid) {
    let cells = grid.cells();
    if !grid.visible || cells.smallest_side() * view.zoom < MIN_CELL {
        return;
    }
    // Only over the part of the image in view.
    let image = view.image_rect();
    let rect = image.intersect(painter.clip_rect());
    if rect.is_positive() {
        let params = params(image, painter.pixels_per_point(), view.zoom, cells, grid.color32());
        painter.add(egui_wgpu::Callback::new_paint_callback(rect, GridCallback { params }));
    }
}

/// What `grid.wgsl` is given, laid out as it reads it.
fn params(image: Rect, pixels_per_point: f32, zoom: f32, cells: Cells, color: Color32) -> Vec<u8> {
    // The image's corner is on a whole screen pixel; rounding keeps it from being a hair off.
    let corner = (image.min.to_vec2() * pixels_per_point).round();
    let mut bytes = Vec::with_capacity(PARAMS_SIZE as usize);
    for value in [corner.x, corner.y, zoom] {
        bytes.extend(value.to_ne_bytes());
    }
    // Room the shader leaves for `origin` to start on a multiple of 8 bytes.
    bytes.extend([0; 4]);
    for value in cells.first_origin().into_iter().chain(cells.size) {
        bytes.extend(value.to_ne_bytes());
    }
    for value in color.to_normalized_gamma_f32() {
        bytes.extend(value.to_ne_bytes());
    }
    bytes
}

struct GridCallback {
    params: Vec<u8>,
}

impl egui_wgpu::CallbackTrait for GridCallback {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        callback_resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(resources) = callback_resources.get::<Resources>() {
            queue.write_buffer(&resources.buffer, 0, &self.params);
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: eframe::egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        callback_resources: &egui_wgpu::CallbackResources,
    ) {
        let Some(resources) = callback_resources.get::<Resources>() else {
            return;
        };
        render_pass.set_pipeline(&resources.pipeline);
        render_pass.set_bind_group(0, &resources.bind_group, &[]);
        render_pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snap_to_the_nearest_corner() {
        let cells = Cells { origin: [3, -2], size: [10, 4] };
        assert_eq!(cells.snap((7.0, -1.0)), (3.0, -2.0));
        assert_eq!(cells.snap((9.0, 0.0)), (13.0, 2.0));
    }

    #[test]
    fn cells_around_a_rectangle_line_up_with_it() {
        let cells = Cells::around(PixelRect { left: 37, top: 5, right: 47, bottom: 21 });
        assert_eq!(cells, Cells { origin: [7, 5], size: [10, 16] });
        assert_eq!(cells.snap((36.0, 4.0)), (37.0, 5.0));
    }

    #[test]
    fn the_shader_is_valid() {
        let module = naga::front::wgsl::parse_str(include_str!("grid.wgsl")).unwrap();
        let flags = naga::valid::ValidationFlags::all();
        naga::valid::Validator::new(flags, naga::valid::Capabilities::empty()).validate(&module).unwrap();
        let params = module.global_variables.iter().find(|(_, variable)| variable.name.as_deref() == Some("params"));
        let (_, params) = params.unwrap();
        let size = module.types[params.ty].inner.size(module.to_ctx());
        assert_eq!(u64::from(size), PARAMS_SIZE);
    }

    #[test]
    fn params_are_laid_out_as_the_shader_reads_them() {
        let cells = Cells { origin: [0, 0], size: [16, 16] };
        let rect = Rect::from_min_size(Default::default(), eframe::egui::Vec2::splat(1.0));
        assert_eq!(params(rect, 1.0, 1.0, cells, Color32::RED).len() as u64, PARAMS_SIZE);
    }
}

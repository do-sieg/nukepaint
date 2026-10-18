//! Shows pixels being moved, stretched, turned, sheared or put in perspective exactly
//! as they will be put down, however they are resampled: `resample.wgsl` works out each
//! pixel on the GPU as `FloatingPixels::place` does, which would be too slow for the
//! CPU at every frame.

use std::{
    num::NonZeroU64,
    sync::{Arc, OnceLock},
};

use eframe::{
    egui::{Painter, Rect, Vec2},
    egui_wgpu::{self, wgpu, wgpu::util::DeviceExt},
};
use nukepaint_core::{Affine, FloatingPixels, Interpolation, PixelRect, Projective, RgbaImage};

use crate::{grid::uniform_bind_group, view::View};

/// Widest and tallest the part of the pixels each texture holds is, and how many pixels
/// around it it holds as well, for the kernels to reach.
const TILE_SIZE: u32 = 2048;
const TILE_MARGIN: u32 = 3;
/// Size of what `resample.wgsl` is given, in bytes.
const PARAMS_SIZE: u64 = 112;
/// Stands for no end, for the tiles on the edges of the pixels.
const UNBOUNDED: i32 = 1 << 30;

/// What the GPU draws with, kept with egui's renderer.
struct Resources {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
}

/// Gets the GPU ready to draw moved pixels.
pub fn init(render_state: &egui_wgpu::RenderState) {
    let device = &render_state.device;
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("resample"),
        source: wgpu::ShaderSource::Wgsl(include_str!("resample.wgsl").into()),
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("resample"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(PARAMS_SIZE),
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("resample"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let format = render_state.target_format;
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("resample"),
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
    render_state.renderer.write().callback_resources.insert(Resources { pipeline, layout });
}

/// The pixels being moved, sent to the GPU the first time they are drawn.
pub struct ResamplePreview {
    pixels: Arc<RgbaImage>,
    /// Where on the layer they were taken from.
    area: PixelRect,
    tiles: Arc<OnceLock<Vec<Tile>>>,
}

/// A part of the pixels, on the GPU.
struct Tile {
    /// The columns and rows it draws the points of, from the first to past the last,
    /// and where its texture starts, in pixels of the whole.
    core: [i32; 4],
    texture_origin: [i32; 2],
    view: wgpu::TextureView,
}

impl ResamplePreview {
    pub fn new(pixels: &FloatingPixels) -> Self {
        Self { pixels: Arc::new(pixels.pixels().clone()), area: pixels.area(), tiles: Arc::default() }
    }

    /// Draws the pixels where `map` carries them on the image, as `interpolation`
    /// redraws them, as opaque as `opacity` says, from 0 to 1, moved by `shift` on
    /// screen, as onto a copy of a tiled image. Only what is within the painter's clip
    /// is drawn: the caller keeps it to the image.
    pub fn paint(
        &self,
        painter: &Painter,
        view: &View,
        map: &Projective,
        interpolation: Interpolation,
        opacity: f32,
        shift: Vec2,
    ) {
        let PixelRect { left, top, right, bottom } = self.area;
        let corners = [(left, top), (right, top), (right, bottom), (left, bottom)].map(|(x, y)| (x as f64, y as f64));
        // Nothing is put down if part of the pixels is tilted behind the eye.
        let sides = corners.map(|corner| map.w(corner));
        let in_front = sides.iter().all(|&w| w > 1e-9) || sides.iter().all(|&w| w < -1e-9);
        let Some(back) = map.inverse().filter(|_| in_front) else {
            return;
        };
        let radius = match interpolation {
            _ if FloatingPixels::copies_exactly(map) => 0,
            Interpolation::Nearest => 0,
            Interpolation::Bilinear => 1,
            Interpolation::Bicubic => 2,
            Interpolation::Lanczos => 3,
        };
        // Only over where the pixels land, a little wider for the blended edges, in view.
        let corners = corners.map(|corner| map.apply(corner)).map(|(x, y)| view.to_screen(x as f32, y as f32) + shift);
        let margin = (radius + 1) as f32 * view.zoom / painter.pixels_per_point();
        let rect = Rect::from_points(&corners).expand(margin).intersect(painter.clip_rect());
        if !rect.is_positive() {
            return;
        }
        let corner = ((view.image_rect().min.to_vec2() + shift) * painter.pixels_per_point()).round();
        // In pixels of the area, from the middle of its first one.
        let back = back.then(Affine::translation(-left as f64 - 0.5, -top as f64 - 0.5).into());
        let params = Params {
            view: [corner.x, corner.y, view.zoom, opacity],
            map_x: [back.xx as f32, back.xy as f32, back.dx as f32, 0.0],
            map_y: [back.yx as f32, back.yy as f32, back.dy as f32, 0.0],
            map_w: [back.wx as f32, back.wy as f32, back.ww as f32, 0.0],
            size: [self.pixels.width() as i32, self.pixels.height() as i32],
            // Enlarged, each pixel of the image shows as it will be put down.
            mode: [radius, (view.zoom >= 1.0) as u32],
        };
        let callback = PreviewCallback {
            pixels: Arc::clone(&self.pixels),
            tiles: Arc::clone(&self.tiles),
            params,
            bind_groups: OnceLock::new(),
        };
        painter.add(egui_wgpu::Callback::new_paint_callback(rect, callback));
    }
}

/// What all the tiles are given alike.
#[derive(Clone, Copy)]
struct Params {
    view: [f32; 4],
    map_x: [f32; 4],
    map_y: [f32; 4],
    map_w: [f32; 4],
    size: [i32; 2],
    mode: [u32; 2],
}

impl Params {
    /// What `resample.wgsl` is given for a tile, laid out as it reads it.
    fn bytes(&self, core: [i32; 4], texture_origin: [i32; 2]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(PARAMS_SIZE as usize);
        for value in self.view.into_iter().chain(self.map_x).chain(self.map_y).chain(self.map_w) {
            bytes.extend(value.to_ne_bytes());
        }
        for value in core.into_iter().chain(texture_origin).chain(self.size) {
            bytes.extend(value.to_ne_bytes());
        }
        for value in self.mode.into_iter().chain([0, 0]) {
            bytes.extend(value.to_ne_bytes());
        }
        bytes
    }
}

/// The pixels drawn once: on the image, or on a copy of a tiled one, each drawing
/// with its own buffers.
struct PreviewCallback {
    pixels: Arc<RgbaImage>,
    tiles: Arc<OnceLock<Vec<Tile>>>,
    params: Params,
    /// Each tile's, for this drawing.
    bind_groups: OnceLock<Vec<wgpu::BindGroup>>,
}

/// The pixels cut into tiles, each in a texture with the pixels around it.
fn make_tiles(device: &wgpu::Device, queue: &wgpu::Queue, pixels: &RgbaImage) -> Vec<Tile> {
    let (width, height) = pixels.dimensions();
    let mut tiles = Vec::new();
    for y in (0..height).step_by(TILE_SIZE as usize) {
        for x in (0..width).step_by(TILE_SIZE as usize) {
            let (end_x, end_y) = ((x + TILE_SIZE).min(width), (y + TILE_SIZE).min(height));
            let core = [
                if x == 0 { -UNBOUNDED } else { x as i32 },
                if y == 0 { -UNBOUNDED } else { y as i32 },
                if end_x == width { UNBOUNDED } else { end_x as i32 },
                if end_y == height { UNBOUNDED } else { end_y as i32 },
            ];
            let (from_x, from_y) = (x.saturating_sub(TILE_MARGIN), y.saturating_sub(TILE_MARGIN));
            let (to_x, to_y) = ((end_x + TILE_MARGIN).min(width), (end_y + TILE_MARGIN).min(height));
            let (tile_width, tile_height) = (to_x - from_x, to_y - from_y);
            let mut data = Vec::with_capacity((tile_width * tile_height * 4) as usize);
            for row in from_y..to_y {
                let start = ((row * width + from_x) * 4) as usize;
                data.extend_from_slice(&pixels.as_raw()[start..start + (tile_width * 4) as usize]);
            }
            let texture = device.create_texture_with_data(
                queue,
                &wgpu::TextureDescriptor {
                    label: Some("resample"),
                    size: wgpu::Extent3d { width: tile_width, height: tile_height, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                &data,
            );
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            tiles.push(Tile { core, texture_origin: [from_x as i32, from_y as i32], view });
        }
    }
    tiles
}

impl egui_wgpu::CallbackTrait for PreviewCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        callback_resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(resources) = callback_resources.get::<Resources>() else {
            return Vec::new();
        };
        let tiles = self.tiles.get_or_init(|| make_tiles(device, queue, &self.pixels));
        self.bind_groups.get_or_init(|| {
            let bind = |tile: &Tile| {
                let bytes = self.params.bytes(tile.core, tile.texture_origin);
                let texture = wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&tile.view) };
                uniform_bind_group(device, &resources.layout, &bytes, "resample", &[texture])
            };
            tiles.iter().map(bind).collect()
        });
        Vec::new()
    }

    fn paint(
        &self,
        _info: eframe::egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        callback_resources: &egui_wgpu::CallbackResources,
    ) {
        let (Some(resources), Some(bind_groups)) = (callback_resources.get::<Resources>(), self.bind_groups.get()) else {
            return;
        };
        render_pass.set_pipeline(&resources.pipeline);
        for bind_group in bind_groups {
            render_pass.set_bind_group(0, bind_group, &[]);
            render_pass.draw(0..3, 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shader_is_valid() {
        let module = naga::front::wgsl::parse_str(include_str!("resample.wgsl")).unwrap();
        let flags = naga::valid::ValidationFlags::all();
        naga::valid::Validator::new(flags, naga::valid::Capabilities::empty()).validate(&module).unwrap();
        let params = module.global_variables.iter().find(|(_, variable)| variable.name.as_deref() == Some("params"));
        let (_, params) = params.unwrap();
        let size = module.types[params.ty].inner.size(module.to_ctx());
        assert_eq!(u64::from(size), PARAMS_SIZE);
    }

    #[test]
    fn params_are_laid_out_as_the_shader_reads_them() {
        let params =
            Params { view: [0.0; 4], map_x: [0.0; 4], map_y: [0.0; 4], map_w: [0.0; 4], size: [1, 1], mode: [1, 0] };
        assert_eq!(params.bytes([0; 4], [0; 2]).len() as u64, PARAMS_SIZE);
    }
}

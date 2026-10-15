//! The image as its layers blend together, worked out on the GPU by `composite.wgsl`
//! as `power_paint_core::composite` does on the CPU, which would be too slow to do
//! again each time a layer's opacity or blend mode changes, or a stroke goes on.
//!
//! Each layer is kept on the GPU, cut into tiles: a GPU takes no texture wider or
//! taller than it allows, which an image can easily be. The layers of a tile are
//! blended one over the other in turn, between two textures, and the result is put
//! into the tile's texture that egui draws, with copies of it shrunk again and again
//! to show it zoomed out.

use std::{
    num::NonZeroU64,
    sync::{Arc, Mutex},
};

use eframe::{
    egui::{Color32, Painter, Pos2, Rect, TextureId, Vec2, mutex::RwLock},
    egui_wgpu::{self, wgpu, wgpu::util::DeviceExt},
};
use power_paint_core::{Layer, MAX_OPACITY, PixelRect, RgbaImage};

/// Widest and tallest a tile is. Every GPU takes textures this large.
const TILE_SIZE: u32 = 2048;
/// As the layers are kept on the CPU, a byte a channel, their alpha not multiplied in.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// Size of what `composite.wgsl` is given for each layer, in bytes.
const PARAMS_SIZE: u64 = 16;

/// The GPU, and what it blends layers with, shared by all the images.
#[derive(Clone)]
pub struct Gpu(Arc<Shared>);

struct Shared {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Arc<RwLock<egui_wgpu::Renderer>>,
    blend: wgpu::RenderPipeline,
    premultiply: wgpu::RenderPipeline,
    halve: wgpu::RenderPipeline,
    blend_layout: wgpu::BindGroupLayout,
    copy_layout: wgpu::BindGroupLayout,
    /// Where the layers of a tile are blended over those under them, from one into the
    /// other in turn.
    scratch: [wgpu::TextureView; 2],
    /// How far apart the parameters of each layer are, as the GPU reads them.
    params_stride: u64,
    /// Textures egui no longer has to draw, which the frame being drawn may still: they
    /// are let go of at the start of the next one.
    released: Mutex<Vec<TextureId>>,
}

impl Gpu {
    pub fn new(render_state: &egui_wgpu::RenderState) -> Self {
        Self::with(render_state.device.clone(), render_state.queue.clone(), Arc::clone(&render_state.renderer))
    }

    fn with(device: wgpu::Device, queue: wgpu::Queue, renderer: Arc<RwLock<egui_wgpu::Renderer>>) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("composite"),
            source: wgpu::ShaderSource::Wgsl(include_str!("composite.wgsl").into()),
        });
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let blend_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("composite blend"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: NonZeroU64::new(PARAMS_SIZE),
                    },
                    count: None,
                },
                texture_entry(1),
                texture_entry(2),
            ],
        });
        let copy_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("composite copy"),
            entries: &[texture_entry(1)],
        });
        let pipeline = |layout: &wgpu::BindGroupLayout, entry_point: &str| {
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(entry_point),
                bind_group_layouts: &[Some(layout)],
                immediate_size: 0,
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry_point),
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
                    entry_point: Some(entry_point),
                    targets: &[Some(wgpu::ColorTargetState { format: FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let blend = pipeline(&blend_layout, "fs_blend");
        let premultiply = pipeline(&copy_layout, "fs_premultiply");
        let halve = pipeline(&copy_layout, "fs_halve");
        let scratch = [0, 1].map(|_| {
            let usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT;
            texture(&device, "composite scratch", TILE_SIZE, TILE_SIZE, 1, usage).create_view(&Default::default())
        });
        let alignment = u64::from(device.limits().min_uniform_buffer_offset_alignment);
        let params_stride = PARAMS_SIZE.div_ceil(alignment) * alignment;
        Self(Arc::new(Shared {
            device,
            queue,
            renderer,
            blend,
            premultiply,
            halve,
            blend_layout,
            copy_layout,
            scratch,
            params_stride,
            released: Mutex::default(),
        }))
    }

    /// Lets go of the textures egui drew the images with that are gone, which the frame
    /// before could still draw. To be called before anything is drawn in a frame.
    pub fn free_released(&self) {
        let released = std::mem::take(&mut *self.0.released.lock().unwrap());
        if released.is_empty() {
            return;
        }
        let mut renderer = self.0.renderer.write();
        for id in released {
            renderer.free_texture(&id);
        }
    }
}

/// The layers of an image on the GPU, and the image they blend into.
pub struct Composite {
    gpu: Gpu,
    size: (u32, u32),
    tiles: Vec<Tile>,
    /// Each layer, in the same order as the document's, as a texture for each tile.
    layers: Vec<Vec<wgpu::Texture>>,
}

/// A part of the image, blended.
struct Tile {
    /// Where it is, in pixels of the image.
    rect: PixelRect,
    /// The blended pixels, their alpha multiplied in, at their full size then at half
    /// each time, as mip levels of one texture: each level, as it is drawn to and as
    /// egui draws it.
    levels: Vec<(wgpu::TextureView, TextureId)>,
}

impl Composite {
    /// Sends the layers to the GPU and blends them. They must all be the same size.
    pub fn new(gpu: &Gpu, layers: &[Layer]) -> Self {
        let shared = &gpu.0;
        let size = layers[0].pixels.dimensions();
        let rects = tile_rects(size);
        let tiles = rects.iter().map(|&rect| Tile::new(shared, rect)).collect();
        let textures = layers
            .iter()
            .map(|layer| {
                rects
                    .iter()
                    .map(|rect| {
                        let usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
                        let texture = texture(&shared.device, "layer", rect.width() as u32, rect.height() as u32, 1, usage);
                        upload(&shared.queue, &texture, &layer.pixels, *rect, *rect);
                        texture
                    })
                    .collect()
            })
            .collect();
        let composite = Self { gpu: gpu.clone(), size, tiles, layers: textures };
        composite.blend(layers, composite.whole());
        composite
    }

    /// Sends the layers to the GPU again, in place of those there, after layers were
    /// added, removed, moved or changed, or the image's size changed.
    pub fn replace_layers(&mut self, layers: &[Layer]) {
        *self = Self::new(&self.gpu.clone(), layers);
    }

    /// Size in pixels.
    pub fn size(&self) -> Vec2 {
        Vec2::new(self.size.0 as f32, self.size.1 as f32)
    }

    /// Sends the layer at `index` to the GPU again after its pixels changed over `area`,
    /// or anywhere without one, and blends the image again there.
    pub fn update_layer(&mut self, layers: &[Layer], index: usize, area: Option<PixelRect>) {
        let area = area.map_or(self.whole(), |area| intersect(area, self.whole()));
        if area.is_empty() {
            return;
        }
        let shared = &self.gpu.0;
        for (tile, texture) in self.tiles.iter().zip(&self.layers[index]) {
            let part = intersect(tile.rect, area);
            if !part.is_empty() {
                upload(&shared.queue, texture, &layers[index].pixels, tile.rect, part);
            }
        }
        self.blend(layers, area);
    }

    /// Blends the layers again after how they are blended changed, not their pixels.
    pub fn reblend(&self, layers: &[Layer]) {
        self.blend(layers, self.whole());
    }

    /// Draws the whole, stretched over `rect`, from the copy closest to that size
    /// without being smaller.
    pub fn paint(&self, painter: &Painter, rect: Rect) {
        let scale = rect.size() / self.size();
        let screen_scale = scale.x * painter.pixels_per_point();
        let level = if screen_scale >= 1.0 { 0 } else { (1.0 / screen_scale).log2().floor() as usize };
        let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        for tile in &self.tiles {
            let PixelRect { left, top, .. } = tile.rect;
            let size = Vec2::new(tile.rect.width() as f32, tile.rect.height() as f32);
            let tile_rect = Rect::from_min_size(rect.min + Vec2::new(left as f32, top as f32) * scale, size * scale);
            let (_, id) = &tile.levels[level.min(tile.levels.len() - 1)];
            painter.image(*id, tile_rect, uv, Color32::WHITE);
        }
    }

    fn whole(&self) -> PixelRect {
        PixelRect { left: 0, top: 0, right: self.size.0 as i32, bottom: self.size.1 as i32 }
    }

    /// Blends the layers that show over an area of the image, which must be on it.
    fn blend(&self, layers: &[Layer], area: PixelRect) {
        debug_assert_eq!(layers.len(), self.layers.len());
        let shared = &self.gpu.0;
        let shown: Vec<usize> = (0..layers.len()).filter(|&index| layers[index].shows()).collect();
        // How each layer is blended, the mode by its place among `BlendMode`'s. With no
        // layer to show, a layer of nothing is blended, for the image to be clear.
        let mut entries: Vec<(f32, u32, bool)> = shown
            .iter()
            .enumerate()
            .map(|(order, &index)| {
                let layer = &layers[index];
                (layer.opacity as f32 / MAX_OPACITY as f32, layer.blend_mode as u32, order == 0)
            })
            .collect();
        if entries.is_empty() {
            entries.push((0.0, 0, true));
        }
        let mut params = Vec::new();
        for (opacity, mode, bottom) in entries {
            let start = params.len();
            params.extend(opacity.to_ne_bytes());
            params.extend(mode.to_ne_bytes());
            params.extend(u32::from(bottom).to_ne_bytes());
            params.resize(start + shared.params_stride as usize, 0);
        }
        let params = shared.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("composite params"),
            contents: &params,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let params_binding = wgpu::BufferBinding { buffer: &params, offset: 0, size: NonZeroU64::new(PARAMS_SIZE) };

        let mut encoder = shared.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("composite") });
        for (index, tile) in self.tiles.iter().enumerate() {
            let part = intersect(tile.rect, area);
            if part.is_empty() {
                continue;
            }
            let scissor = PixelRect {
                left: part.left - tile.rect.left,
                top: part.top - tile.rect.top,
                right: part.right - tile.rect.left,
                bottom: part.bottom - tile.rect.top,
            };
            let mut target = 0;
            let passes = shown.len().max(1);
            for pass in 0..passes {
                let under = &shared.scratch[1 - target];
                let view = shown.get(pass).map(|&layer| self.layers[layer][index].create_view(&Default::default()));
                let layer = view.as_ref().unwrap_or(under);
                let bind_group = shared.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("composite blend"),
                    layout: &shared.blend_layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Buffer(params_binding.clone()) },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(under) },
                        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(layer) },
                    ],
                });
                let offset = (pass as u64 * shared.params_stride) as u32;
                draw(&mut encoder, &shared.blend, &bind_group, &[offset], &shared.scratch[target], scissor);
                target = 1 - target;
            }
            let blended = &shared.scratch[1 - target];
            draw(&mut encoder, &shared.premultiply, &copy_bind_group(shared, blended), &[], &tile.levels[0].0, scissor);
            for level in 1..tile.levels.len() {
                let (width, height) = level_size(tile.rect, level);
                let halved = PixelRect {
                    left: scissor.left >> level,
                    top: scissor.top >> level,
                    right: (scissor.right as u32).div_ceil(1 << level).min(width) as i32,
                    bottom: (scissor.bottom as u32).div_ceil(1 << level).min(height) as i32,
                };
                if halved.is_empty() {
                    break;
                }
                let bind_group = copy_bind_group(shared, &tile.levels[level - 1].0);
                draw(&mut encoder, &shared.halve, &bind_group, &[], &tile.levels[level].0, halved);
            }
        }
        shared.queue.submit([encoder.finish()]);
    }
}

impl Drop for Composite {
    fn drop(&mut self) {
        let ids = self.tiles.iter().flat_map(|tile| tile.levels.iter().map(|(_, id)| *id));
        self.gpu.0.released.lock().unwrap().extend(ids);
    }
}

impl Tile {
    fn new(shared: &Shared, rect: PixelRect) -> Self {
        let (width, height) = (rect.width() as u32, rect.height() as u32);
        // Down to a single pixel along the longest side.
        let level_count = u32::BITS - width.max(height).leading_zeros();
        // Copied from, for the tests to read it back.
        let usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC;
        let texture = texture(&shared.device, "composite", width, height, level_count, usage);
        // Enlarged pixels stay sharp; a reduced image is smoothed.
        let sampler = wgpu::SamplerDescriptor {
            label: Some("composite"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        };
        let mut renderer = shared.renderer.write();
        let levels = (0..level_count)
            .map(|level| {
                let view = texture.create_view(&wgpu::TextureViewDescriptor {
                    base_mip_level: level,
                    mip_level_count: Some(1),
                    ..Default::default()
                });
                let id = renderer.register_native_texture_with_sampler_options(&shared.device, &view, sampler.clone());
                (view, id)
            })
            .collect();
        Self { rect, levels }
    }
}

/// The image of that size cut into tiles, left to right then top to bottom.
fn tile_rects((width, height): (u32, u32)) -> Vec<PixelRect> {
    let mut rects = Vec::new();
    for top in (0..height).step_by(TILE_SIZE as usize) {
        for left in (0..width).step_by(TILE_SIZE as usize) {
            let (right, bottom) = ((left + TILE_SIZE).min(width), (top + TILE_SIZE).min(height));
            rects.push(PixelRect { left: left as i32, top: top as i32, right: right as i32, bottom: bottom as i32 });
        }
    }
    rects
}

/// Size of a tile's texture at a mip level, which rounds halves down.
fn level_size(rect: PixelRect, level: usize) -> (u32, u32) {
    ((rect.width() as u32 >> level).max(1), (rect.height() as u32 >> level).max(1))
}

fn intersect(a: PixelRect, b: PixelRect) -> PixelRect {
    let rect = PixelRect {
        left: a.left.max(b.left),
        top: a.top.max(b.top),
        right: a.right.min(b.right),
        bottom: a.bottom.min(b.bottom),
    };
    if rect.is_empty() { PixelRect { left: 0, top: 0, right: 0, bottom: 0 } } else { rect }
}

fn texture(
    device: &wgpu::Device,
    label: &str,
    width: u32,
    height: u32,
    mip_level_count: u32,
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage,
        view_formats: &[],
    })
}

/// Copies the pixels of `part` of the image into the texture of the tile at `tile`.
fn upload(queue: &wgpu::Queue, texture: &wgpu::Texture, image: &RgbaImage, tile: PixelRect, part: PixelRect) {
    let offset = (part.top as usize * image.width() as usize + part.left as usize) * 4;
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d { x: (part.left - tile.left) as u32, y: (part.top - tile.top) as u32, z: 0 },
            aspect: wgpu::TextureAspect::All,
        },
        &image.as_raw()[offset..],
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(image.width() * 4), rows_per_image: None },
        wgpu::Extent3d { width: part.width() as u32, height: part.height() as u32, depth_or_array_layers: 1 },
    );
}

fn copy_bind_group(shared: &Shared, source: &wgpu::TextureView) -> wgpu::BindGroup {
    shared.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("composite copy"),
        layout: &shared.copy_layout,
        entries: &[wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(source) }],
    })
}

/// Draws over the `scissor` part of `target` alone, the rest of it kept as it is.
fn draw(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::RenderPipeline,
    bind_group: &wgpu::BindGroup,
    offsets: &[u32],
    target: &wgpu::TextureView,
    scissor: PixelRect,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("composite"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, offsets);
    let PixelRect { left, top, .. } = scissor;
    pass.set_scissor_rect(left as u32, top as u32, scissor.width() as u32, scissor.height() as u32);
    pass.draw(0..3, 0..1);
}

#[cfg(test)]
mod tests {
    use eframe::egui::Color32;
    use power_paint_core::{BlendMode, Rgba, composite};

    use super::*;

    #[test]
    fn the_shader_is_valid() {
        let module = naga::front::wgsl::parse_str(include_str!("composite.wgsl")).unwrap();
        let flags = naga::valid::ValidationFlags::all();
        naga::valid::Validator::new(flags, naga::valid::Capabilities::empty()).validate(&module).unwrap();
        let params = module.global_variables.iter().find(|(_, variable)| variable.name.as_deref() == Some("params"));
        let (_, params) = params.unwrap();
        let size = module.types[params.ty].inner.size(module.to_ctx());
        assert_eq!(u64::from(size), PARAMS_SIZE);
    }

    /// The GPU to test on, if there is one.
    fn test_gpu() -> Option<Gpu> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()?;
        let renderer = egui_wgpu::Renderer::new(&device, FORMAT, egui_wgpu::RendererOptions::default());
        Some(Gpu::with(device, queue, Arc::new(RwLock::new(renderer))))
    }

    /// The image blended at its full size, its alpha multiplied in, from the GPU.
    fn read_back(composite: &Composite) -> RgbaImage {
        let shared = &composite.gpu.0;
        let mut image = RgbaImage::new(composite.size.0, composite.size.1);
        for tile in &composite.tiles {
            let (width, height) = (tile.rect.width() as u32, tile.rect.height() as u32);
            let row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
            let buffer = shared.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: u64::from(row * height),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = shared.device.create_command_encoder(&Default::default());
            encoder.copy_texture_to_buffer(
                tile.levels[0].0.texture().as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None },
                },
                wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            );
            shared.queue.submit([encoder.finish()]);
            buffer.map_async(wgpu::MapMode::Read, .., |result| result.unwrap());
            shared.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            let data = buffer.get_mapped_range(..).unwrap();
            for y in 0..height {
                for x in 0..width {
                    let start = (y * row + x * 4) as usize;
                    let pixel = Rgba(data[start..start + 4].try_into().unwrap());
                    image.put_pixel(tile.rect.left as u32 + x, tile.rect.top as u32 + y, pixel);
                }
            }
        }
        image
    }

    /// The layers blended on the CPU, their alpha multiplied in as egui does.
    fn expected(layers: &[Layer]) -> RgbaImage {
        let mut image = composite(layers);
        for pixel in image.pixels_mut() {
            let [r, g, b, a] = pixel.0;
            *pixel = Rgba(Color32::from_rgba_unmultiplied(r, g, b, a).to_array());
        }
        image
    }

    /// Exact on the GPUs tried, but a GPU may divide a little less exactly than the CPU,
    /// which can tip a channel's rounding.
    fn assert_close(gpu: &RgbaImage, cpu: &RgbaImage) {
        assert_eq!(gpu.dimensions(), cpu.dimensions());
        for (x, y, pixel) in gpu.enumerate_pixels() {
            let want = cpu[(x, y)];
            let close = pixel.0.iter().zip(want.0).all(|(&got, want)| got.abs_diff(want) <= 1);
            assert!(close, "at {x}, {y}: the GPU gives {:?}, the CPU {:?}", pixel.0, want.0);
        }
    }

    /// A layer of colours and alphas of all sorts, the same nowhere twice.
    fn layer(seed: u32, width: u32, height: u32) -> Layer {
        let pixels = RgbaImage::from_fn(width, height, |x, y| {
            let value = |salt: u32| ((x * 37 + y * 101 + seed * 53 + salt * 71) ^ (seed * 29 + salt)) as u8;
            Rgba([value(0), value(1), value(2), if x % 7 == 0 { 255 } else { value(3) }])
        });
        Layer::new("", pixels)
    }

    #[test]
    fn layers_blend_as_on_the_cpu() {
        let Some(gpu) = test_gpu() else {
            eprintln!("no GPU to test on");
            return;
        };
        // Wider than a tile, for the layers to be cut.
        let (width, height) = (TILE_SIZE + 5, 40);
        let mut layers: Vec<Layer> = (0..BlendMode::ALL.len() as u32 + 2).map(|seed| layer(seed, width, height)).collect();
        for (index, layer) in layers.iter_mut().enumerate().skip(1) {
            layer.blend_mode = BlendMode::ALL[(index - 1) % BlendMode::ALL.len()];
            layer.opacity = [255, 200, 90][index % 3];
        }
        layers[3].visible = false;
        let mut composite = Composite::new(&gpu, &layers);
        assert_close(&read_back(&composite), &expected(&layers));

        // Changed over an area across both tiles, the image only blends again there.
        let area = PixelRect { left: TILE_SIZE as i32 - 10, top: 5, right: TILE_SIZE as i32 + 3, bottom: 20 };
        for x in area.left..area.right {
            for y in area.top..area.bottom {
                layers[2].pixels.put_pixel(x as u32, y as u32, Rgba([10, 200, 30, 180]));
            }
        }
        composite.update_layer(&layers, 2, Some(area));
        assert_close(&read_back(&composite), &expected(&layers));

        layers[5].blend_mode = BlendMode::Difference;
        layers[0].opacity = 100;
        composite.reblend(&layers);
        assert_close(&read_back(&composite), &expected(&layers));
    }

    #[test]
    fn nothing_shown_is_clear() {
        let Some(gpu) = test_gpu() else {
            return;
        };
        let mut layers = vec![layer(1, 3, 3)];
        let composite = Composite::new(&gpu, &layers);
        layers[0].visible = false;
        composite.reblend(&layers);
        assert!(read_back(&composite).pixels().all(|pixel| pixel.0 == [0, 0, 0, 0]));
    }
}

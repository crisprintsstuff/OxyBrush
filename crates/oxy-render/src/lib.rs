//! Matcap viewport renderer.
//!
//! One global vertex buffer mirrors the mesh; because every chunk owns a
//! contiguous vertex range, a dirty chunk is re-uploaded with a single
//! `write_buffer` call. The index buffer is rebuilt only when the mesh's
//! topology version changes.

mod camera;
pub mod matcap;

pub use camera::Camera;
pub use matcap::MatcapPreset;

use bytemuck::{Pod, Zeroable};
use oxy_mesh::Mesh;
use wgpu::util::DeviceExt;

pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const MATCAP_SIZE: u32 = 256;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuVertex {
    position: [f32; 3],
    normal: [f32; 3],
    mask: f32,
    _pad: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    view_proj: [[f32; 4]; 4],
    view: [[f32; 4]; 4],
    params: [f32; 4],
    mask_color: [f32; 4],
}

/// Per-frame display options.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewSettings {
    pub show_mask: bool,
    pub background: [f64; 3],
}

impl Default for ViewSettings {
    fn default() -> Self {
        Self { show_mask: true, background: [0.012, 0.013, 0.016] }
    }
}

/// Where to draw: a color view, its size, and the viewport rectangle in
/// physical pixels (x, y, width, height).
#[derive(Clone, Copy)]
pub struct RenderTarget<'a> {
    pub view: &'a wgpu::TextureView,
    pub size: (u32, u32),
    pub viewport: [f32; 4],
}

struct GpuMesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    topology_version: u64,
}

pub struct ViewportRenderer {
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    matcap: wgpu::Texture,
    mesh: Option<GpuMesh>,
    depth: Option<(wgpu::TextureView, u32, u32)>,
}

impl ViewportRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, color_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("oxy viewport"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("oxy uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let matcap = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("oxy matcap"),
            size: wgpu::Extent3d { width: MATCAP_SIZE, height: MATCAP_SIZE, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("oxy matcap sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("oxy viewport layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let matcap_view = matcap.create_view(&Default::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("oxy viewport bind group"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&matcap_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&sampler) },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("oxy viewport pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("oxy viewport pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<GpuVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attributes,
                })],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let mut r = Self { pipeline, uniforms, bind_group, matcap, mesh: None, depth: None };
        r.set_matcap(queue, MatcapPreset::Terracotta);
        r
    }

    pub fn set_matcap(&mut self, queue: &wgpu::Queue, preset: MatcapPreset) {
        let pixels = matcap::generate(preset, MATCAP_SIZE);
        queue.write_texture(
            self.matcap.as_image_copy(),
            &pixels,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(MATCAP_SIZE * 4), rows_per_image: None },
            wgpu::Extent3d { width: MATCAP_SIZE, height: MATCAP_SIZE, depth_or_array_layers: 1 },
        );
    }

    /// Upload changes: full rebuild on topology change, otherwise dirty chunks only.
    /// Returns the number of chunks uploaded.
    pub fn sync(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, mesh: &mut Mesh) -> usize {
        let rebuild = self.mesh.as_ref().is_none_or(|g| g.topology_version != mesh.topology_version());
        if rebuild {
            let verts: Vec<GpuVertex> = (0..mesh.vertex_count()).map(|i| vertex(mesh, i)).collect();
            let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("oxy vertices"),
                contents: bytemuck::cast_slice(&verts),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            });
            let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("oxy indices"),
                contents: bytemuck::cast_slice(&mesh.triangles),
                usage: wgpu::BufferUsages::INDEX,
            });
            self.mesh = Some(GpuMesh {
                vertices,
                indices,
                index_count: mesh.triangle_count() as u32 * 3,
                topology_version: mesh.topology_version(),
            });
            let n = mesh.take_dirty().len();
            return n;
        }
        let gpu = self.mesh.as_ref().expect("checked above");
        let dirty = mesh.take_dirty();
        let mut staging = Vec::new();
        for &c in &dirty {
            let r = mesh.chunks[c as usize].vertices.clone();
            if r.is_empty() {
                continue;
            }
            staging.clear();
            staging.extend((r.start as usize..r.end as usize).map(|i| vertex(mesh, i)));
            queue.write_buffer(
                &gpu.vertices,
                r.start as u64 * std::mem::size_of::<GpuVertex>() as u64,
                bytemuck::cast_slice(&staging),
            );
        }
        dirty.len()
    }

    fn ensure_depth(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if self.depth.as_ref().is_some_and(|d| d.1 == width && d.2 == height) {
            return;
        }
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("oxy depth"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        self.depth = Some((tex.create_view(&Default::default()), width, height));
    }

    /// Clear the target and draw the mesh into its viewport rectangle.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &RenderTarget<'_>,
        camera: &Camera,
        settings: &ViewSettings,
    ) {
        let RenderTarget { view: target, size: target_size, viewport } = *target;
        self.ensure_depth(device, target_size.0.max(1), target_size.1.max(1));
        let aspect = (viewport[2] / viewport[3].max(1.0)).max(1e-3);
        let u = Uniforms {
            view_proj: camera.view_projection(aspect).to_cols_array_2d(),
            view: camera.view().to_cols_array_2d(),
            params: [0.0, if settings.show_mask { 1.0 } else { 0.0 }, 0.0, 0.0],
            mask_color: [0.25, 0.3, 0.45, 1.0],
        };
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&u));

        let [r, g, b] = settings.background;
        let depth = &self.depth.as_ref().expect("ensured").0;
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("oxy viewport pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a: 1.0 }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let Some(gpu) = &self.mesh else { return };
        let (w, h) = (target_size.0 as f32, target_size.1 as f32);
        let x = viewport[0].clamp(0.0, w - 1.0);
        let y = viewport[1].clamp(0.0, h - 1.0);
        let vw = viewport[2].min(w - x).max(1.0);
        let vh = viewport[3].min(h - y).max(1.0);
        pass.set_viewport(x, y, vw, vh, 0.0, 1.0);
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, gpu.vertices.slice(..));
        pass.set_index_buffer(gpu.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..gpu.index_count, 0, 0..1);
    }
}

fn vertex(mesh: &Mesh, i: usize) -> GpuVertex {
    GpuVertex {
        position: mesh.positions[i].to_array(),
        normal: mesh.normals[i].to_array(),
        mask: mesh.mask[i],
        _pad: 0.0,
    }
}

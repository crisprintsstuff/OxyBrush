//! Image planes are composited with the viewport and do not contribute to mesh depth.

use super::{DEPTH_FORMAT, NORMAL_DEPTH_FORMAT, ViewportRenderer};
use bytemuck::{Pod, Zeroable};
use std::sync::Arc;

pub struct ReferencePlane {
    pub pixels: Arc<[u8]>,
    pub size: [u32; 2],
    /// Upper left, upper right, lower left, lower right, in clip space.
    pub corners: [[f32; 4]; 4],
    /// Zero keeps the texture cached while the plane is hidden.
    pub opacity: f32,
    pub overlay: bool,
}

#[derive(Clone, Copy, PartialEq, Pod, Zeroable)]
#[repr(C)]
struct Params {
    corners: [[f32; 4]; 4],
    opacity: [f32; 4],
}

pub(super) struct GpuReference {
    pixels: Arc<[u8]>,
    size: [u32; 2],
    uniform: wgpu::Buffer,
    bind: wgpu::BindGroup,
    params: Params,
    overlay: bool,
}

pub(super) struct ReferencePass {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    planes: Vec<GpuReference>,
}

impl ReferencePass {
    pub(super) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("reference image layout"),
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
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("reference image pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("reference image shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("reference.wgsl").into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("reference image pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[
                    Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        format: NORMAL_DEPTH_FORMAT,
                        blend: None,
                        write_mask: wgpu::ColorWrites::empty(),
                    }),
                ],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self { layout, pipeline, sampler, planes: Vec::new() }
    }

    fn upload(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        plane: ReferencePlane,
        params: Params,
    ) -> GpuReference {
        let size = wgpu::Extent3d { width: plane.size[0], height: plane.size[1], depth_or_array_layers: 1 };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("reference image"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            texture.as_image_copy(),
            &plane.pixels,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(size.width * 4), rows_per_image: None },
            size,
        );
        let view = texture.create_view(&Default::default());
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("reference placement"),
            size: std::mem::size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&uniform, 0, bytemuck::bytes_of(&params));
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("reference image"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        GpuReference { pixels: plane.pixels, size: plane.size, uniform, bind, params, overlay: plane.overlay }
    }

    pub(super) fn visible(&self, overlay: bool) -> bool {
        self.planes.iter().any(|p| p.overlay == overlay && p.params.opacity[0] > 0.0)
    }

    pub(super) fn draw<'p>(&'p self, pass: &mut wgpu::RenderPass<'p>, overlay: bool) {
        if !self.visible(overlay) {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        for plane in self.planes.iter().filter(|p| p.overlay == overlay && p.params.opacity[0] > 0.0) {
            pass.set_bind_group(0, &plane.bind, &[]);
            pass.draw(0..6, 0..1);
        }
    }
}

impl ViewportRenderer {
    /// Keep textures for the displayed project and update only changed placement uniforms.
    pub fn set_reference_planes(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        planes: impl ExactSizeIterator<Item = ReferencePlane>,
    ) {
        let refs = &mut self.references;
        if planes.len() == 0 {
            refs.planes.clear();
            return;
        }
        let mut old = std::mem::take(&mut refs.planes);
        refs.planes = planes
            .map(|plane| {
                let params = Params { corners: plane.corners, opacity: [plane.opacity, 0.0, 0.0, 0.0] };
                if let Some(i) = old.iter().position(|p| p.size == plane.size && Arc::ptr_eq(&p.pixels, &plane.pixels))
                {
                    let mut gpu = old.swap_remove(i);
                    if gpu.params != params {
                        queue.write_buffer(&gpu.uniform, 0, bytemuck::bytes_of(&params));
                        gpu.params = params;
                    }
                    gpu.overlay = plane.overlay;
                    gpu
                } else {
                    refs.upload(device, queue, plane, params)
                }
            })
            .collect();
    }
}

//! Matcap viewport renderer.
//!
//! Each piece has its own GPU mesh. One vertex buffer mirrors the mesh;
//! because every chunk owns a contiguous vertex range, a dirty chunk is
//! re-uploaded with a single `write_buffer` call. Index and face-color
//! buffers are rebuilt only when topology, face groups or visibility change
//! (hidden triangles are left out of the index buffer).

mod camera;
pub mod matcap;

pub use camera::Camera;
pub use matcap::MatcapPreset;

use bytemuck::{Pod, Zeroable};
use oxy_mesh::Mesh;
use std::collections::HashMap;
use wgpu::util::DeviceExt;

pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// View normal (xyz) and linear depth (w) for the screen-space shading passes.
const NORMAL_DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Float;
const AO_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;
const MATCAP_SIZE: u32 = 256;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuVertex {
    position: [f32; 3],
    normal: [f32; 3],
    mask: f32,
    group_color: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    view_proj: [[f32; 4]; 4],
    view: [[f32; 4]; 4],
    /// x: unused, y: mask overlay strength, z: viewport aspect (w / h)
    params: [f32; 4],
    mask_color: [f32; 4],
    /// Background gradient: color at the center and at the edges (linear).
    bg_center: [f32; 4],
    bg_edge: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PostUniforms {
    /// 1/P[0][0], 1/P[1][1], orthographic flag, unused
    proj: [f32; 4],
    /// Viewport rectangle in framebuffer pixels.
    viewport: [f32; 4],
    /// Obscurance strength, cavity strength, obscurance radius (world), unused.
    params: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PieceUniforms {
    flags: [f32; 4],
}

/// Per-frame display options.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewSettings {
    pub show_mask: bool,
    pub show_groups: bool,
    /// Background color at the viewport edges (linear RGB).
    pub background: [f64; 3],
    /// Background color at the center (linear RGB); a soft radial gradient
    /// to `background` keeps the model readable against the backdrop.
    pub background_center: [f64; 3],
    /// Ambient occlusion strength, 0 (off) to 1: darkens areas that other
    /// parts of the surface shade, such as the inside of folds.
    pub ambient_occlusion: f32,
    /// Cavity strength, 0 (off) to 1: darkens creases and pores and
    /// brightens edges, so fine detail reads clearly.
    pub cavity: f32,
}

impl Default for ViewSettings {
    fn default() -> Self {
        Self {
            show_mask: true,
            show_groups: false,
            background: [0.010, 0.011, 0.013],
            background_center: [0.062, 0.064, 0.070],
            ambient_occlusion: 0.7,
            cavity: 0.6,
        }
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

/// One piece to draw this frame.
#[derive(Debug, Clone, Copy)]
pub struct DrawItem {
    pub id: u64,
    pub active: bool,
}

struct GpuMesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    piece_uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    topology_version: u64,
    face_version: u64,
}

pub struct ViewportRenderer {
    pipeline: wgpu::RenderPipeline,
    background_pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    piece_layout: wgpu::BindGroupLayout,
    matcap: wgpu::Texture,
    meshes: HashMap<u64, GpuMesh>,
    depth: Option<(wgpu::TextureView, u32, u32)>,
    exact_face_colors: bool,
    color_format: wgpu::TextureFormat,
    post_uniforms: wgpu::Buffer,
    post_layout: wgpu::BindGroupLayout,
    ao_pipeline: wgpu::RenderPipeline,
    composite_pipeline: wgpu::RenderPipeline,
    /// 1x1 stand-in bound as the obscurance input while the obscurance pass
    /// itself renders (a texture can't be read and written in one pass).
    dummy_ao: wgpu::TextureView,
    targets: Option<ShadingTargets>,
}

/// Offscreen targets for the shading passes, sized to the window.
struct ShadingTargets {
    size: (u32, u32),
    color: wgpu::TextureView,
    normal_depth: wgpu::TextureView,
    ao: wgpu::TextureView,
    ao_group: wgpu::BindGroup,
    composite_group: wgpu::BindGroup,
}

/// Features the renderer can use if the adapter offers them.
pub fn optional_features() -> wgpu::Features {
    wgpu::Features::PRIMITIVE_INDEX
}

/// Stable, well-spread color for a face group (packed RGBA8, little-endian).
pub fn group_color(group: u32) -> u32 {
    // Golden-ratio hue steps give distinct neighbors; medium saturation keeps
    // the matcap shading readable.
    let h = (group as f32 * 0.618_034 + 0.11).fract();
    let s = 0.55;
    let v = 0.95;
    let k = |n: f32| {
        let k = (n + h * 6.0) % 6.0;
        v - v * s * k.min(4.0 - k).clamp(0.0, 1.0)
    };
    let (r, g, b) = (k(5.0), k(3.0), k(1.0));
    (r * 255.0) as u32 | ((g * 255.0) as u32) << 8 | ((b * 255.0) as u32) << 16 | 0xff << 24
}

impl ViewportRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, color_format: wgpu::TextureFormat) -> Self {
        let exact_face_colors = device.features().contains(wgpu::Features::PRIMITIVE_INDEX);
        let src = include_str!("shader.wgsl");
        let src = if exact_face_colors { src.replace("//PRIM ", "") } else { src.to_string() };
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("oxy viewport"),
            source: wgpu::ShaderSource::Wgsl(src.into()),
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

        let uniform_entry = |binding, visibility| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("oxy viewport layout"),
            entries: &[
                uniform_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT),
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
        let piece_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("oxy piece layout"),
            entries: &[
                uniform_entry(0, wgpu::ShaderStages::FRAGMENT),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
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
            bind_group_layouts: &[Some(&layout), Some(&piece_layout)],
            immediate_size: 0,
        });
        // Background: a fullscreen triangle drawn first, behind everything.
        let background_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("oxy background pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let background_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("oxy background pipeline"),
            layout: Some(&background_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_background"),
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
                entry_point: Some("fs_background"),
                compilation_options: Default::default(),
                targets: &[
                    Some(wgpu::ColorTargetState {
                        format: color_format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        format: NORMAL_DEPTH_FORMAT,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                ],
            }),
            multiview_mask: None,
            cache: None,
        });
        let attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32, 3 => Uint32];
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
                targets: &[
                    Some(wgpu::ColorTargetState {
                        format: color_format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        format: NORMAL_DEPTH_FORMAT,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                ],
            }),
            multiview_mask: None,
            cache: None,
        });

        // Screen-space shading passes (obscurance, then blur + cavity + composite).
        let post_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("oxy shading"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shading.wgsl").into()),
        });
        let post_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("oxy post uniforms"),
            size: std::mem::size_of::<PostUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
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
        let post_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("oxy post layout"),
            entries: &[
                uniform_entry(0, wgpu::ShaderStages::FRAGMENT),
                texture_entry(1),
                texture_entry(2),
                texture_entry(3),
            ],
        });
        let post_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("oxy post pipeline layout"),
            bind_group_layouts: &[Some(&post_layout)],
            immediate_size: 0,
        });
        let post_pipeline = |label, entry, format| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&post_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &post_shader,
                    entry_point: Some("vs_fullscreen"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &post_shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let ao_pipeline = post_pipeline("oxy ambient occlusion", "fs_ao", AO_FORMAT);
        let composite_pipeline = post_pipeline("oxy composite", "fs_composite", color_format);
        let dummy_ao = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("oxy dummy ao"),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: AO_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());

        let mut r = Self {
            pipeline,
            background_pipeline,
            color_format,
            post_uniforms,
            post_layout,
            ao_pipeline,
            composite_pipeline,
            dummy_ao,
            targets: None,
            uniforms,
            bind_group,
            piece_layout,
            matcap,
            meshes: HashMap::new(),
            depth: None,
            exact_face_colors,
        };
        r.set_matcap(queue, MatcapPreset::Terracotta);
        r
    }

    /// Whether face-group colors are exact per triangle (vs. per vertex).
    pub fn exact_face_colors(&self) -> bool {
        self.exact_face_colors
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

    /// Drop GPU meshes for pieces that no longer exist.
    pub fn retain(&mut self, alive: impl Fn(u64) -> bool) {
        self.meshes.retain(|id, _| alive(*id));
    }

    /// Upload changes for one piece: full rebuild on topology/face change,
    /// otherwise dirty chunks only. Returns the number of chunks uploaded.
    pub fn sync(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, id: u64, mesh: &mut Mesh) -> usize {
        let rebuild = self
            .meshes
            .get(&id)
            .is_none_or(|g| g.topology_version != mesh.topology_version() || g.face_version != mesh.face_version());
        if rebuild {
            let verts: Vec<GpuVertex> = (0..mesh.vertex_count()).map(|i| vertex(mesh, i)).collect();
            let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("oxy vertices"),
                contents: bytemuck::cast_slice(&verts),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            });
            // Visible triangles only; face colors in the same order.
            let mut indices: Vec<[u32; 3]> = Vec::with_capacity(mesh.triangle_count());
            let mut face_colors: Vec<u32> = Vec::with_capacity(mesh.triangle_count());
            for (t, tri) in mesh.triangles.iter().enumerate() {
                if !mesh.hidden()[t] {
                    indices.push(*tri);
                    face_colors.push(group_color(mesh.groups()[t]));
                }
            }
            if face_colors.is_empty() {
                face_colors.push(0);
            }
            let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("oxy indices"),
                contents: if indices.is_empty() { &[0u8; 12] } else { bytemuck::cast_slice(&indices) },
                usage: wgpu::BufferUsages::INDEX,
            });
            let faces = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("oxy face colors"),
                contents: bytemuck::cast_slice(&face_colors),
                usage: wgpu::BufferUsages::STORAGE,
            });
            let piece_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("oxy piece uniforms"),
                size: std::mem::size_of::<PieceUniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("oxy piece bind group"),
                layout: &self.piece_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: piece_uniforms.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: faces.as_entire_binding() },
                ],
            });
            self.meshes.insert(
                id,
                GpuMesh {
                    vertices,
                    indices: index_buffer,
                    index_count: indices.len() as u32 * 3,
                    piece_uniforms,
                    bind_group,
                    topology_version: mesh.topology_version(),
                    face_version: mesh.face_version(),
                },
            );
            return mesh.take_dirty().len();
        }
        let gpu = &self.meshes[&id];
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

    fn ensure_targets(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if self.targets.as_ref().is_some_and(|t| t.size == (width, height)) {
            return;
        }
        let make = |label, format| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let color = make("oxy scene color", self.color_format);
        let normal_depth = make("oxy normal depth", NORMAL_DEPTH_FORMAT);
        let ao = make("oxy ambient occlusion", AO_FORMAT);
        let group = |ao_view: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("oxy post bind group"),
                layout: &self.post_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.post_uniforms.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&color) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&normal_depth) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(ao_view) },
                ],
            })
        };
        let ao_group = group(&self.dummy_ao);
        let composite_group = group(&ao);
        self.targets =
            Some(ShadingTargets { size: (width, height), color, normal_depth, ao, ao_group, composite_group });
    }

    /// Clear the target and draw the given pieces into its viewport rectangle.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &RenderTarget<'_>,
        camera: &Camera,
        settings: &ViewSettings,
        items: &[DrawItem],
    ) {
        let RenderTarget { view: target, size: target_size, viewport } = *target;
        self.ensure_depth(device, target_size.0.max(1), target_size.1.max(1));
        let aspect = (viewport[2] / viewport[3].max(1.0)).max(1e-3);
        let u = Uniforms {
            view_proj: camera.view_projection(aspect).to_cols_array_2d(),
            view: camera.view().to_cols_array_2d(),
            params: [0.0, if settings.show_mask { 1.0 } else { 0.0 }, aspect, 0.0],
            mask_color: [0.25, 0.3, 0.45, 1.0],
            bg_center: rgba(settings.background_center),
            bg_edge: rgba(settings.background),
        };
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&u));
        let multiple = items.len() > 1;
        for item in items {
            if let Some(gpu) = self.meshes.get(&item.id) {
                let flags = [
                    if settings.show_groups { 1.0 } else { 0.0 },
                    if item.active || !multiple { 1.0 } else { 0.0 },
                    0.0,
                    0.0,
                ];
                queue.write_buffer(&gpu.piece_uniforms, 0, bytemuck::bytes_of(&PieceUniforms { flags }));
            }
        }

        let (w, h) = (target_size.0 as f32, target_size.1 as f32);
        let x = viewport[0].clamp(0.0, w - 1.0);
        let y = viewport[1].clamp(0.0, h - 1.0);
        let vw = viewport[2].min(w - x).max(1.0);
        let vh = viewport[3].min(h - y).max(1.0);

        let proj = camera.projection(aspect).to_cols_array_2d();
        let shading = [settings.ambient_occlusion.clamp(0.0, 1.0), settings.cavity.clamp(0.0, 1.0)];
        let post = PostUniforms {
            proj: [1.0 / proj[0][0], 1.0 / proj[1][1], if camera.orthographic { 1.0 } else { 0.0 }, 0.0],
            viewport: [x, y, vw, vh],
            // The obscurance radius follows the camera distance, so it picks
            // up broad forms when zoomed out and fine detail when zoomed in.
            params: [shading[0], shading[1], camera.distance * 0.035, 0.0],
        };
        queue.write_buffer(&self.post_uniforms, 0, bytemuck::bytes_of(&post));
        self.ensure_targets(device, target_size.0.max(1), target_size.1.max(1));
        let t = self.targets.as_ref().expect("ensured");

        let [r, g, b] = settings.background;
        let bg = wgpu::Color { r, g, b, a: 1.0 };
        let depth = &self.depth.as_ref().expect("ensured").0;
        let attachment = |view, clear| {
            Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(clear), store: wgpu::StoreOp::Store },
            })
        };
        // With shading off, the matcap pass draws straight into the window
        // (no offscreen copy), so turning shading off costs nothing.
        let shaded = shading[0] > 0.0 || shading[1] > 0.0;
        let scene_color = if shaded { &t.color } else { target };
        {
            // 1. Matcap shading into the color + normal/depth targets.
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("oxy viewport pass"),
                color_attachments: &[
                    attachment(scene_color, bg),
                    attachment(&t.normal_depth, wgpu::Color::TRANSPARENT),
                ],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_viewport(x, y, vw, vh, 0.0, 1.0);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_pipeline(&self.background_pipeline);
            pass.draw(0..3, 0..1);
            pass.set_pipeline(&self.pipeline);
            for item in items {
                let Some(gpu) = self.meshes.get(&item.id) else { continue };
                if gpu.index_count == 0 {
                    continue;
                }
                pass.set_bind_group(1, &gpu.bind_group, &[]);
                pass.set_vertex_buffer(0, gpu.vertices.slice(..));
                pass.set_index_buffer(gpu.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..gpu.index_count, 0, 0..1);
            }
        }
        if shading[0] > 0.0 {
            // 2. Ambient obscurance (skipped when switched off).
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("oxy ambient occlusion pass"),
                color_attachments: &[attachment(&t.ao, wgpu::Color::WHITE)],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_viewport(x, y, vw, vh, 0.0, 1.0);
            pass.set_pipeline(&self.ao_pipeline);
            pass.set_bind_group(0, &t.ao_group, &[]);
            pass.draw(0..3, 0..1);
        }
        if !shaded {
            return;
        }
        // 3. Blur + cavity + composite into the window.
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("oxy composite pass"),
            color_attachments: &[attachment(target, bg)],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_viewport(x, y, vw, vh, 0.0, 1.0);
        pass.set_pipeline(&self.composite_pipeline);
        pass.set_bind_group(0, &t.composite_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

fn rgba(c: [f64; 3]) -> [f32; 4] {
    [c[0] as f32, c[1] as f32, c[2] as f32, 1.0]
}

fn vertex(mesh: &Mesh, i: usize) -> GpuVertex {
    // Fallback per-vertex group color: the vertex's first triangle's group.
    let group_color = mesh.vertex_triangles(i as u32).first().map_or(0, |&t| group_color(mesh.groups()[t as usize]));
    GpuVertex {
        position: mesh.positions[i].to_array(),
        normal: mesh.normals[i].to_array(),
        mask: mesh.mask[i],
        group_color,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validate(name: &str, src: &str) {
        let module = naga::front::wgsl::parse_str(src).unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(src)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
            .validate(&module)
            .unwrap_or_else(|e| panic!("{name}: {e:?}"));
    }

    #[test]
    fn shaders_are_valid_wgsl() {
        // Both variants of the viewport shader: with exact per-face colors and without.
        let src = include_str!("shader.wgsl");
        validate("shader.wgsl", src);
        validate("shader.wgsl (primitive_index)", &src.replace("//PRIM ", ""));
        validate("shading.wgsl", include_str!("shading.wgsl"));
    }

    #[test]
    fn uniform_layouts_match_wgsl() {
        // std140-style sizes the shaders expect (vec4/mat4 aligned).
        assert_eq!(std::mem::size_of::<Uniforms>(), 2 * 64 + 4 * 16);
        assert_eq!(std::mem::size_of::<PostUniforms>(), 3 * 16);
    }

    #[test]
    fn group_colors_are_distinct_and_opaque() {
        let colors: Vec<u32> = (0..12).map(group_color).collect();
        for (i, a) in colors.iter().enumerate() {
            assert_eq!(a >> 24, 0xff);
            for b in &colors[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }
}

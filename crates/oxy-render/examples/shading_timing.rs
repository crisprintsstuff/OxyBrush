//! GPU time of the viewport passes with and without screen-space shading.
//!
//! Renders a dense sphere into an offscreen 1440x900 target (the default
//! window size) and reports the average time per frame, waiting for the
//! GPU after each frame. Run on the machine you want numbers for:
//!
//!     cargo run --release -p oxy-render --example shading_timing

use oxy_render::{Camera, DrawItem, RenderTarget, ViewSettings, ViewportRenderer};
use std::time::Instant;

fn main() {
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .expect("no GPU adapter");
    let info = adapter.get_info();
    println!("GPU: {} ({:?}, {})", info.name, info.backend, info.driver);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("shading timing"),
        required_limits: adapter.limits(),
        required_features: adapter.features() & oxy_render::optional_features(),
        ..Default::default()
    }))
    .expect("device");

    let (w, h) = (1440u32, 900u32);
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("target"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());

    let mut renderer = ViewportRenderer::new(&device, &queue, format);
    let mut mesh = oxy_mesh::primitives::icosphere(7, 1.0);
    renderer.sync(&device, &queue, 1, &mut mesh);
    let mut camera = Camera::default();
    camera.frame(mesh.bounds());
    let items = [DrawItem { id: 1, active: true, preview: false, material: 0 }];
    // The viewport between the default panels.
    let rt = RenderTarget { view: &view, size: (w, h), viewport: [206.0, 70.0, 844.0, 800.0] };
    println!("{} triangles, viewport {}x{}", mesh.triangle_count(), 844, 800);

    let frames = 60;
    let lit = |shadows: bool| oxy_render::lighting::Lighting { enabled: true, shadows, ..Default::default() };
    let bounds = mesh.bounds();
    let scene_bounds = Some((bounds.min.to_array(), bounds.max.to_array()));
    for (label, ao, cavity, lighting) in [
        ("plain", 0.0, 0.0, oxy_render::lighting::Lighting::default()),
        ("cavity only", 0.0, 0.6, oxy_render::lighting::Lighting::default()),
        ("ambient occlusion + cavity", 0.7, 0.6, oxy_render::lighting::Lighting::default()),
        ("lights (no shadows) + AO + cavity", 0.7, 0.6, lit(false)),
        ("lights + 1 shadow map + AO + cavity", 0.7, 0.6, lit(true)),
    ] {
        let settings = ViewSettings { ambient_occlusion: ao, cavity, lighting, scene_bounds, ..Default::default() };
        let mut run = |n: usize| {
            let t = Instant::now();
            for _ in 0..n {
                let mut encoder = device.create_command_encoder(&Default::default());
                renderer.render(&device, &queue, &mut encoder, &rt, &camera, &settings, &items);
                queue.submit([encoder.finish()]);
                device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
            }
            t.elapsed().as_secs_f64() * 1000.0 / n as f64
        };
        run(5); // warm up
        println!("{label:>36}: {:7.2} ms/frame", run(frames));
    }
}

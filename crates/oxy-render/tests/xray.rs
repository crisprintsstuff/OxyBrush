use glam::{Mat4, Vec3};
use oxy_render::{Camera, DrawItem, ReferencePlane, RenderTarget, ViewSettings, ViewportRenderer, XrayScope};

#[test]
#[ignore = "requires a wgpu adapter"]
fn xray_and_glass_respect_opaque_depth() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        required_features: adapter.features() & oxy_render::optional_features(),
        ..Default::default()
    }))
    .unwrap();

    const SIDE: u32 = 64;
    let size = wgpu::Extent3d { width: SIDE, height: SIDE, depth_or_array_layers: 1 };
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("X-ray test target"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let target = RenderTarget { view: &view, size: (SIDE, SIDE), viewport: [4.0, 4.0, 56.0, 56.0], clear: true };
    let next_view = std::cell::Cell::new(None);
    let mut viewport = ViewportRenderer::new(&device, &queue, format);
    let mut front = oxy_mesh::primitives::icosphere(3, 0.85);
    let mut back = front.to_data();
    for p in &mut back.positions {
        *p -= Vec3::Z * 0.4;
    }
    let mut back = oxy_mesh::Mesh::from_data(back);
    let mut back_wire = back.clone();
    for (id, mesh) in [(1, &mut front), (2, &mut back), (3, &mut back_wire)] {
        viewport.sync(&device, &queue, id, mesh, oxy_render::DEFAULT_SPLIT_ANGLE_DEG, true);
    }
    viewport.set_instances(&device, &queue, 1, &[Mat4::IDENTITY]);
    let camera = Camera::default();
    let item = DrawItem {
        id: 1,
        active: true,
        preview: false,
        material: 0,
        instances: None,
        wireframe: false,
        color_map: false,
        glass: None,
    };
    let rear = DrawItem { id: 2, active: false, ..item };
    let settings = ViewSettings { ambient_occlusion: 0.0, cavity: 0.0, ..Default::default() };
    let mut render = |settings: &ViewSettings, items: &[DrawItem], plane: Option<ReferencePlane>| -> Vec<u8> {
        viewport.set_reference_planes(&device, &queue, plane.into_iter());
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(SIDE * SIDE * 4),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        let (target, camera) = next_view.take().unwrap_or((target, camera));
        viewport.render(&device, &queue, &mut encoder, &target, &camera, settings, items);
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(SIDE * 4), rows_per_image: None },
            },
            size,
        );
        queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        rx.recv().unwrap().unwrap();
        slice.get_mapped_range().unwrap().to_vec()
    };
    fn sample(frame: &[u8], x: u32, y: u32) -> &[u8] {
        let offset = ((y * SIDE + x) * 4) as usize;
        &frame[offset..offset + 4]
    }
    let bg = render(&settings, &[], None);
    let solid = render(&settings, &[item], None);
    let mut xray = ViewSettings { xray: true, xray_scope: XrayScope::AllVisible, ..settings };
    let ghost = render(&xray, &[item], None);
    let center = |frame: &[u8]| sample(frame, SIDE / 2, SIDE / 2).to_vec();
    assert_ne!(center(&solid), center(&ghost));
    assert_ne!(center(&bg), center(&ghost));
    let srgb_to_linear = |byte: u8| {
        let value = f32::from(byte) / 255.0;
        if value <= 0.04045 { value / 12.92 } else { ((value + 0.055) / 1.055).powf(2.4) }
    };
    for channel in 0..3 {
        let expected = srgb_to_linear(center(&bg)[channel]) * 0.65 + srgb_to_linear(center(&solid)[channel]) * 0.35;
        assert!((srgb_to_linear(center(&ghost)[channel]) - expected).abs() < 0.01);
    }
    assert_eq!(sample(&bg, 1, 1), sample(&ghost, 1, 1));
    let overlap = render(&xray, &[DrawItem { instances: Some(1), ..rear }, item], None);
    assert_eq!(center(&overlap), center(&ghost));

    xray.xray_scope = XrayScope::Inactive;
    let behind_active = render(&xray, &[item, DrawItem { id: 3, wireframe: true, ..rear }], None);
    assert_eq!(center(&behind_active), center(&solid));
    assert_eq!(
        center(&render(&xray, &[rear], None)),
        center(&render(&ViewSettings { xray_scope: XrayScope::AllVisible, ..xray }, &[rear], None))
    );
    let opaque_rear = render(&settings, &[rear], None);
    xray.xray_scope = XrayScope::Active;
    assert_eq!(render(&xray, &[rear], None), opaque_rear);
    assert_eq!(center(&render(&xray, &[item], None)), center(&ghost));
    let invisible_active = ViewSettings { xray_opacity: 0.0, inactive_strength: 0.0, ..xray };
    assert_eq!(render(&invisible_active, &[item, rear], None), opaque_rear);

    xray.xray_scope = XrayScope::AllVisible;
    xray.xray_opacity = 0.0;
    assert_eq!(render(&xray, &[item], None), bg);

    xray.xray_opacity = 0.35;
    xray.ambient_occlusion = 0.7;
    xray.cavity = 0.6;
    xray.wireframe = true;
    xray.lighting.enabled = true;
    let combined = render(&xray, &[item, rear], None);
    assert_ne!(center(&combined), center(&bg));
    let reference = |overlay| ReferencePlane {
        pixels: [255, 0, 0, 255].into(),
        size: [1, 1],
        corners: [[-1.0, 1.0, 0.5, 1.0], [1.0, 1.0, 0.5, 1.0], [-1.0, -1.0, 0.5, 1.0], [1.0, -1.0, 0.5, 1.0]],
        opacity: 1.0,
        overlay,
    };
    assert_eq!(center(&render(&settings, &[item], Some(reference(false)))), center(&solid));
    let image_only = render(&settings, &[], Some(reference(false)));
    assert_eq!(center(&image_only), [255, 0, 0, 255]);
    let faded = ViewSettings { xray: true, xray_scope: XrayScope::AllVisible, ..settings };
    let through_mesh = render(&faded, &[item], Some(reference(false)));
    assert!(center(&through_mesh)[0] > center(&ghost)[0]);
    assert_eq!(center(&render(&settings, &[item], Some(reference(true)))), center(&image_only));
    for (ambient_occlusion, cavity) in [(1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
        let shaded = ViewSettings { ambient_occlusion, cavity, ..settings };
        let shaded_ghost = ViewSettings { xray: true, xray_scope: XrayScope::AllVisible, ..shaded };
        let through = render(&shaded_ghost, &[item], Some(reference(false)));
        assert_eq!(center(&through), center(&through_mesh));
        assert_eq!(sample(&through, 5, 5), sample(&through_mesh, 5, 5));

        // Ghosts behind an opaque piece must not change its shading or depth.
        let shaded_solid = render(&shaded, &[item], None);
        let inactive = ViewSettings { xray: true, xray_scope: XrayScope::Inactive, ..shaded };
        let mixed = render(&inactive, &[item, rear], None);
        assert_eq!(center(&mixed), center(&shaded_solid));
        assert!(center(&mixed)[..3].iter().any(|&channel| channel > 0));
        assert_eq!(sample(&mixed, 5, 5), sample(&bg, 5, 5));
    }
    let transparent = ReferencePlane { pixels: [255, 0, 0, 128].into(), opacity: 0.5, ..reference(true) };
    let alpha = 0.5 * 128.0 / 255.0;
    let over_mesh = center(&render(&settings, &[item], Some(transparent)));
    for (channel, (out, original)) in over_mesh.into_iter().zip(center(&solid)).take(3).enumerate() {
        let source = if channel == 0 { alpha } else { 0.0 };
        let expected = srgb_to_linear(original) * (1.0 - alpha) + source;
        assert!((srgb_to_linear(out) - expected).abs() < 0.01);
    }
    // A second pane preserves the first pane's output, but starts with
    // empty X-ray scratch textures even when its geometry misses the pane.
    let flat = ViewSettings { background_style: oxy_render::BackgroundStyle::Flat, ..faded };
    let flat_bg = render(&flat, &[], None);
    let seeded = render(&flat, &[item], None);
    let offset_camera = Camera { target: Vec3::X * 20.0, ..camera };
    next_view.set(Some((RenderTarget { viewport: [28.0, 4.0, 32.0, 56.0], clear: false, ..target }, offset_camera)));
    let second = render(&flat, &[item], None);
    assert_eq!(sample(&second, 25, 32), sample(&seeded, 25, 32));
    assert_eq!(sample(&second, 32, 32), sample(&flat_bg, 32, 32));

    // Refraction must sample the available background, respect opaque
    // depth, and leave the viewport outside the glass unchanged.
    use oxy_render::GlassMaterial;
    let board = || ReferencePlane {
        size: [16, 16],
        pixels: (0..256)
            .flat_map(|i| {
                let c = if (i % 16 + i / 16) % 2 == 0 { 28 } else { 230 };
                [c, c, c, 255]
            })
            .collect::<Vec<_>>()
            .into(),
        ..reference(false)
    };
    let plain = render(&settings, &[], Some(board()));
    let clear = render(&settings, &[DrawItem { glass: Some(GlassMaterial::CLEAR), ..item }], Some(board()));
    let frosted = render(&settings, &[DrawItem { glass: Some(GlassMaterial::FROSTED), ..item }], Some(board()));
    let neutral = render(
        &settings,
        &[DrawItem { glass: Some(GlassMaterial { ior: 1.0, roughness: 0.0, ..GlassMaterial::CLEAR }), ..item }],
        Some(board()),
    );
    let gradient = |frame: &[u8]| {
        (23..41)
            .flat_map(|y| (23..40).map(move |x| u32::from(sample(frame, x, y)[0].abs_diff(sample(frame, x + 1, y)[0]))))
            .sum::<u32>()
    };
    assert_eq!(plain, neutral);
    assert_ne!(clear, plain);
    assert!(gradient(&frosted) < gradient(&clear));
    assert_eq!(sample(&plain, 5, 5), sample(&clear, 5, 5));
    let occluded = render(&settings, &[item, DrawItem { glass: Some(GlassMaterial::CLEAR), ..rear }], Some(board()));
    assert_eq!(center(&solid), center(&occluded));
    let shaded_glass = render(
        &ViewSettings { ambient_occlusion: 0.7, cavity: 0.6, ..settings },
        &[DrawItem { glass: Some(GlassMaterial::CLEAR), ..item }],
        Some(board()),
    );
    assert_eq!(clear, shaded_glass, "without opaque meshes the background has no AO");
}

//! Window, GPU and event-loop glue.

use crate::editor::{Editor, Modifiers, PointerButton};
use crate::ui::{self, UiAction};
use glam::Vec2;
use oxy_render::{MatcapPreset, ViewportRenderer};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, KeyCode, NamedKey, PhysicalKey};
use winit::window::{Window, WindowId};

struct Gfx {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    view_format: wgpu::TextureFormat,
    viewport: ViewportRenderer,
    egui_renderer: egui_wgpu::Renderer,
    egui_state: egui_winit::State,
}

/// Options for automated runs (CI screenshots, demos).
#[derive(Default)]
pub struct AutoRun {
    /// Write a PNG of the window to this path, then exit.
    pub screenshot: Option<PathBuf>,
    /// Perform a few scripted strokes first.
    pub demo: bool,
}

pub struct App {
    gfx: Option<Gfx>,
    egui_ctx: egui::Context,
    editor: Editor,
    applied_matcap: Option<MatcapPreset>,
    applied_theme: Option<(bool, f32)>,
    last_frame: Instant,
    initial_file: Option<PathBuf>,
    auto: AutoRun,
    frames: u32,
}

impl App {
    pub fn new(initial_file: Option<PathBuf>, auto: AutoRun) -> Self {
        Self {
            gfx: None,
            egui_ctx: egui::Context::default(),
            editor: Editor::default(),
            applied_matcap: None,
            applied_theme: None,
            last_frame: Instant::now(),
            initial_file,
            auto,
            frames: 0,
        }
    }

    fn init_gfx(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        let attrs = Window::default_attributes()
            .with_title("OxyBrush")
            .with_inner_size(winit::dpi::LogicalSize::new(1440.0, 900.0))
            .with_min_inner_size(winit::dpi::LogicalSize::new(640.0, 400.0));
        let window = Arc::new(event_loop.create_window(attrs).map_err(|e| e.to_string())?);

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(
            event_loop.owned_display_handle(),
        )));
        let surface = instance.create_surface(window.clone()).map_err(|e| e.to_string())?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .map_err(|e| format!("no compatible GPU adapter: {e}"))?;
        log::info!("GPU adapter: {:?}", adapter.get_info());
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("oxybrush"),
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .map_err(|e| e.to_string())?;

        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or("surface not supported by adapter")?;
        let caps = surface.get_capabilities(&adapter);
        // Prefer an sRGB view so matcap colors are displayed correctly.
        let view_format = config.format.add_srgb_suffix();
        for f in [view_format, config.format.remove_srgb_suffix()] {
            if f != config.format {
                config.view_formats.push(f);
            }
        }
        if caps.usages.contains(wgpu::TextureUsages::COPY_SRC) {
            config.usage |= wgpu::TextureUsages::COPY_SRC;
        }
        config.present_mode = if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
            wgpu::PresentMode::Mailbox
        } else {
            wgpu::PresentMode::AutoVsync
        };
        surface.configure(&device, &config);

        let viewport = ViewportRenderer::new(&device, &queue, view_format);
        // egui does its own gamma handling and prefers a non-sRGB target.
        let egui_renderer = egui_wgpu::Renderer::new(
            &device,
            config.format.remove_srgb_suffix(),
            egui_wgpu::RendererOptions::default(),
        );
        let egui_state = egui_winit::State::new(
            self.egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            Some(device.limits().max_texture_dimension_2d as usize),
        );
        self.gfx =
            Some(Gfx { window, surface, device, queue, config, view_format, viewport, egui_renderer, egui_state });
        Ok(())
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        let Some(g) = &mut self.gfx else { return };
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32() * 1000.0;
        self.last_frame = now;
        self.editor.frame_ms = self.editor.frame_ms * 0.9 + dt * 0.1;

        let theme = (self.editor.high_contrast, self.editor.ui_scale);
        if self.applied_theme != Some(theme) {
            ui::apply_theme(&self.egui_ctx, theme.0, theme.1);
            self.applied_theme = Some(theme);
        }

        let raw = g.egui_state.take_egui_input(&g.window);
        let mut action = None;
        let mut canvas = egui::Rect::NOTHING;
        let ed = &mut self.editor;
        let mut out = self.egui_ctx.run_ui(raw, |ui| {
            (action, canvas) = ui::draw(ui, ed);
        });
        // Apply texture uploads now so skipped frames never lose them.
        let mut textures = std::mem::take(&mut out.textures_delta);
        for (id, deltas) in textures.set.drain() {
            for delta in deltas {
                g.egui_renderer.update_texture(&g.device, &g.queue, id, &delta);
            }
        }
        let to_free: Vec<egui::TextureId> = textures.free.drain().collect();
        g.egui_state.handle_platform_output(&g.window, out.platform_output);
        let ppp = out.pixels_per_point;
        ed.pixels_per_point = ppp;
        ed.canvas = [canvas.min.x * ppp, canvas.min.y * ppp, canvas.width() * ppp, canvas.height() * ppp];

        if self.applied_matcap != Some(ed.matcap) {
            g.viewport.set_matcap(&g.queue, ed.matcap);
            self.applied_matcap = Some(ed.matcap);
        }
        g.viewport.sync(&g.device, &g.queue, &mut ed.mesh);

        let frame = match g.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                g.surface.configure(&g.device, &g.config);
                g.window.request_redraw();
                to_free.iter().for_each(|id| g.egui_renderer.free_texture(id));
                return;
            }
            _ => {
                g.window.request_redraw();
                to_free.iter().for_each(|id| g.egui_renderer.free_texture(id));
                return;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor { format: Some(g.view_format), ..Default::default() });
        let ui_view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(g.config.format.remove_srgb_suffix()),
            ..Default::default()
        });

        let tris = self.egui_ctx.tessellate(out.shapes, ppp);
        let screen =
            egui_wgpu::ScreenDescriptor { size_in_pixels: [g.config.width, g.config.height], pixels_per_point: ppp };

        let mut encoder = g.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        g.viewport.render(
            &g.device,
            &g.queue,
            &mut encoder,
            &oxy_render::RenderTarget { view: &view, size: (g.config.width, g.config.height), viewport: ed.canvas },
            &ed.camera,
            &ed.view,
        );
        let extra = g.egui_renderer.update_buffers(&g.device, &g.queue, &mut encoder, &tris, &screen);
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("egui"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &ui_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            g.egui_renderer.render(&mut pass, &tris, &screen);
        }

        self.frames += 1;
        let want_shot = self.auto.screenshot.is_some() && self.frames >= 4;
        let shot = want_shot.then(|| crate::snapshot::copy_to_buffer(&g.device, &mut encoder, &frame.texture));
        g.queue.submit(extra.into_iter().chain([encoder.finish()]));
        if let (Some(shot), Some(path)) = (shot.flatten(), &self.auto.screenshot) {
            match crate::snapshot::save_png(&g.device, shot, path) {
                Ok(()) => log::info!("wrote screenshot {}", path.display()),
                Err(e) => log::error!("screenshot failed: {e}"),
            }
            event_loop.exit();
        } else if want_shot {
            log::error!("surface does not support COPY_SRC; cannot take screenshot");
            event_loop.exit();
        }
        g.window.pre_present_notify();
        g.queue.present(frame);
        for id in &to_free {
            g.egui_renderer.free_texture(id);
        }

        let repaint_now = out.viewport_output.get(&egui::ViewportId::ROOT).is_some_and(|v| v.repaint_delay.is_zero());
        if repaint_now || self.auto.screenshot.is_some() {
            g.window.request_redraw();
        }

        if let Some(a) = action {
            self.run_action(a);
        }
        if self.editor.quit_requested {
            event_loop.exit();
        }
    }

    fn run_action(&mut self, action: UiAction) {
        let ed = &mut self.editor;
        match action {
            UiAction::Open => {
                if let Some(p) = rfd::FileDialog::new().add_filter("Meshes", &["obj", "stl", "OBJ", "STL"]).pick_file()
                {
                    ed.open(&p);
                }
            }
            UiAction::Export => {
                let name = ed
                    .path
                    .as_ref()
                    .and_then(|p| p.file_stem())
                    .map(|s| format!("{}.obj", s.to_string_lossy()))
                    .unwrap_or_else(|| "sculpt.obj".into());
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("Wavefront OBJ", &["obj"])
                    .add_filter("STL (3D printing)", &["stl"])
                    .set_file_name(name)
                    .save_file()
                {
                    ed.export(&p);
                }
            }
        }
        if let Some(g) = &self.gfx {
            g.window.request_redraw();
        }
    }

    fn handle_key(&mut self, event: &KeyEvent) -> Option<UiAction> {
        if event.state != ElementState::Pressed {
            return None;
        }
        let ed = &mut self.editor;
        let m = ed.modifiers;
        let ctrl = m.ctrl;
        if let PhysicalKey::Code(code) = event.physical_key {
            match code {
                KeyCode::Numpad1 => ed.camera.snap(0.0, 0.0),
                KeyCode::Numpad3 => ed.camera.snap(std::f32::consts::FRAC_PI_2, 0.0),
                KeyCode::Numpad7 => ed.camera.snap(0.0, 1.55),
                _ => {}
            }
        }
        match &event.logical_key {
            Key::Named(NamedKey::Tab) => ed.show_panels = !ed.show_panels,
            Key::Named(NamedKey::F1) => ed.show_shortcuts = !ed.show_shortcuts,
            Key::Named(NamedKey::Escape) => ed.pointer_up(),
            Key::Character(c) => {
                let c = c.to_lowercase();
                match (c.as_str(), ctrl, m.shift, m.alt) {
                    ("z", true, false, _) => ed.undo(),
                    ("z", true, true, _) | ("y", true, _, _) => ed.redo(),
                    ("o", true, _, _) => return Some(UiAction::Open),
                    ("e", true, _, _) => return Some(UiAction::Export),
                    ("i", true, _, _) => ed.invert_mask(),
                    ("m", false, _, true) => ed.clear_mask(),
                    ("m", false, _, false) => ed.view.show_mask = !ed.view.show_mask,
                    ("f", false, ..) => ed.frame(),
                    ("o", false, ..) => ed.camera.orthographic = !ed.camera.orthographic,
                    ("x", false, ..) => {
                        ed.symmetry.x = !ed.symmetry.x;
                        ed.status = format!("X symmetry {}", if ed.symmetry.x { "on" } else { "off" });
                    }
                    ("d", false, ..) => ed.subdivide(true),
                    ("[", false, ..) => ed.adjust_radius(1.0 / 1.15),
                    ("]", false, ..) => ed.adjust_radius(1.15),
                    ("-", false, ..) => ed.adjust_strength(-0.05),
                    ("=", false, ..) | ("+", false, ..) => ed.adjust_strength(0.05),
                    (d, false, _, false) if d.len() == 1 && d.as_bytes()[0].is_ascii_digit() => {
                        let i = (d.as_bytes()[0] - b'0') as usize;
                        if (1..=oxy_sculpt::BrushKind::ALL.len()).contains(&i) {
                            ed.brush_kind = oxy_sculpt::BrushKind::ALL[i - 1];
                            ed.status = format!("{} brush", ed.brush_kind.label());
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        None
    }

    fn run_demo(&mut self) {
        // A few deterministic strokes so automated screenshots show sculpting.
        let ed = &mut self.editor;
        let [x, y, w, h] = ed.canvas;
        let c = Vec2::new(x + w * 0.5, y + h * 0.5);
        let s = h * 0.25;
        let strokes: [(oxy_sculpt::BrushKind, Vec2, Vec2); 4] = [
            (oxy_sculpt::BrushKind::Clay, Vec2::new(-0.6, -0.2), Vec2::new(0.6, -0.3)),
            (oxy_sculpt::BrushKind::Draw, Vec2::new(-0.5, 0.35), Vec2::new(0.5, 0.35)),
            (oxy_sculpt::BrushKind::Inflate, Vec2::new(-0.35, -0.6), Vec2::new(-0.3, -0.55)),
            (oxy_sculpt::BrushKind::Mask, Vec2::new(-0.2, 0.7), Vec2::new(0.3, 0.75)),
        ];
        for (kind, a, b) in strokes {
            ed.brush_kind = kind;
            ed.pointer = Some(c + a * s);
            ed.pointer_down(PointerButton::Primary);
            for i in 1..=40 {
                ed.pointer_moved(c + a.lerp(b, i as f32 / 40.0) * s);
            }
            ed.pointer_up();
        }
        ed.brush_kind = oxy_sculpt::BrushKind::Draw;
        ed.pointer = Some(c + Vec2::new(0.9, 0.1) * s);
        ed.status = "Demo strokes applied".into();
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        if let Err(e) = self.init_gfx(event_loop) {
            log::error!("failed to initialize graphics: {e}");
            rfd::MessageDialog::new()
                .set_title("OxyBrush")
                .set_description(format!("OxyBrush could not initialize graphics:\n{e}"))
                .show();
            event_loop.exit();
            return;
        }
        if let Some(p) = self.initial_file.take() {
            self.editor.open(&p);
        }
        if let Some(g) = &self.gfx {
            g.window.request_redraw();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(g) = &mut self.gfx else { return };
        let response = g.egui_state.on_window_event(&g.window, &event);
        let over_egui = self.egui_ctx.is_pointer_over_egui() || self.egui_ctx.egui_is_using_pointer();
        let mut redraw = response.repaint;

        match &event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                g.config.width = size.width.max(1);
                g.config.height = size.height.max(1);
                g.surface.configure(&g.device, &g.config);
                redraw = true;
            }
            WindowEvent::RedrawRequested => {
                self.redraw(event_loop);
                if self.auto.demo && self.frames == 2 {
                    self.auto.demo = false;
                    self.run_demo();
                }
                return;
            }
            WindowEvent::ModifiersChanged(m) => {
                let s = m.state();
                self.editor.modifiers =
                    Modifiers { shift: s.shift_key(), ctrl: s.control_key() || s.super_key(), alt: s.alt_key() };
                redraw = true;
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.editor.pressure = 1.0;
                self.editor.pointer_moved(Vec2::new(position.x as f32, position.y as f32));
                redraw = true;
            }
            WindowEvent::CursorLeft { .. } => {
                if !self.editor.is_stroking() {
                    self.editor.pointer = None;
                }
                redraw = true;
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let b = match button {
                    MouseButton::Left => Some(PointerButton::Primary),
                    MouseButton::Middle => Some(PointerButton::Middle),
                    MouseButton::Right => Some(PointerButton::Secondary),
                    _ => None,
                };
                if let Some(b) = b {
                    match state {
                        ElementState::Pressed if !over_egui && self.editor.pointer_in_canvas() => {
                            self.editor.pointer_down(b)
                        }
                        ElementState::Released => self.editor.pointer_up(),
                        _ => {}
                    }
                }
                redraw = true;
            }
            WindowEvent::Touch(t) => {
                // Pen and touch input; pressure comes from `force` where the platform reports it.
                self.editor.pressure = t.force.map_or(1.0, |f| f.normalized() as f32);
                let pos = Vec2::new(t.location.x as f32, t.location.y as f32);
                match t.phase {
                    TouchPhase::Started => {
                        self.editor.pointer = Some(pos);
                        if !over_egui && self.editor.pointer_in_canvas() {
                            self.editor.pointer_down(PointerButton::Primary);
                        }
                    }
                    TouchPhase::Moved => {
                        self.editor.pointer_moved(pos);
                    }
                    TouchPhase::Ended | TouchPhase::Cancelled => self.editor.pointer_up(),
                }
                redraw = true;
            }
            WindowEvent::MouseWheel { delta, .. } if !over_egui => {
                let steps = match delta {
                    MouseScrollDelta::LineDelta(_, y) => *y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                };
                self.editor.scroll(steps);
                redraw = true;
            }
            WindowEvent::KeyboardInput { event, .. } if !self.egui_ctx.egui_wants_keyboard_input() => {
                if let Some(a) = self.handle_key(event) {
                    self.run_action(a);
                }
                redraw = true;
            }
            _ => {}
        }
        if redraw && let Some(g) = &self.gfx {
            g.window.request_redraw();
        }
    }
}

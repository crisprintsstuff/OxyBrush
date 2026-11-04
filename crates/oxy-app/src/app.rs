//! Window, GPU and event-loop glue.

use crate::editor::{Editor, Modifiers, PointerButton};
use crate::keymap::{self, Chord, CommandEffect, CommandId, KeyId};
use crate::ui::{self, UiAction};
use glam::Vec2;
use oxy_render::{MatcapPreset, ViewportRenderer};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, StartCause, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
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
    /// Start with this layout (`--layout`); remembered like choosing it in Settings.
    pub layout: Option<crate::settings::UiLayout>,
    /// `--set key=value` lines applied on top of the saved settings.
    pub overrides: Vec<String>,
    /// Open the keymap editor on startup. `Some` selects that navigation preset.
    pub open_keymap: Option<crate::keymap::Preset>,
}

pub struct App {
    gfx: Option<Gfx>,
    egui_ctx: egui::Context,
    editor: Editor,
    /// Material last uploaded: a built-in preset, or a custom matcap's name.
    applied_matcap: Option<(MatcapPreset, Option<String>)>,
    /// Middle button held over a panel: last pointer position, for drag-scrolling.
    mmb_scroll: Option<Vec2>,
    applied_theme: Option<(bool, f32, crate::settings::UiLayout)>,
    applied_title: String,
    last_frame: Instant,
    initial_file: Option<PathBuf>,
    auto: AutoRun,
    frames: u32,
    /// Where preferences are remembered (None for automated runs).
    settings_path: Option<PathBuf>,
    keymap_path: Option<PathBuf>,
    saved_settings: crate::settings::Settings,
    /// Canonical keymap text last written, so an unchanged keymap is not rewritten.
    saved_keymap: String,
    /// When the preferences last changed without being saved yet.
    settings_changed_at: Option<Instant>,
}

impl App {
    pub fn new(initial_file: Option<PathBuf>, auto: AutoRun, log_path: Option<PathBuf>) -> Self {
        // Automated runs (CI screenshots) neither read nor write the user's settings.
        let settings_path = if auto.screenshot.is_none() { crate::settings::Settings::default_path() } else { None };
        let keymap_path = if settings_path.is_some() { keymap::Keymap::default_path() } else { None };
        let mut keymap = keymap_path
            .as_deref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|t| keymap::Keymap::from_text(&t))
            .unwrap_or_default();
        if let Some(preset) = auto.open_keymap {
            keymap.apply_navigation_preset(preset);
        }
        let saved_keymap = keymap.to_text();
        let loaded = settings_path.as_deref().and_then(crate::settings::Settings::load);
        let first_start = settings_path.is_some() && loaded.is_none();
        let settings = loaded.unwrap_or_default();
        Self {
            gfx: None,
            egui_ctx: egui::Context::default(),
            editor: {
                let mut e = Editor::default();
                // Automated runs (CI screenshots) don't touch the user's autosave folder.
                if auto.screenshot.is_none() {
                    e.enable_autosave();
                }
                e.system_info.log_path = log_path;
                // Brush presets: read the user's library (automated runs use none).
                if auto.screenshot.is_none() {
                    e.preset_dir = crate::presets::library_dir();
                    if let Some(dir) = &e.preset_dir {
                        e.presets = crate::presets::load_all(dir);
                    }
                    e.matcap_dir = crate::matcaps::library_dir();
                    if let Some(dir) = &e.matcap_dir {
                        e.custom_matcaps = crate::matcaps::load_all(dir);
                    }
                }
                e.apply_settings(&settings);
                if !auto.overrides.is_empty() {
                    let text = format!("{}\n{}", e.settings().to_text(), auto.overrides.join("\n"));
                    e.apply_settings(&crate::settings::Settings::from_text(&text));
                }
                if let Some(l) = auto.layout {
                    e.layout = l;
                }
                // Automated runs (CI screenshots) never connect anywhere.
                if auto.screenshot.is_some() {
                    e.updater.mode = crate::settings::UpdateMode::Off;
                }
                e.keymap = keymap;
                e.show_welcome = crate::settings::show_welcome(
                    first_start,
                    settings.welcome_at_startup,
                    settings_path.is_none(),
                    initial_file.is_some() || auto.demo,
                );
                if auto.open_keymap.is_some() {
                    e.show_welcome = false;
                    e.show_keymap = true;
                }
                log::info!(
                    "layout {:?}, welcome window {}",
                    e.layout,
                    if e.show_welcome { "shown" } else { "skipped" }
                );
                e
            },
            applied_matcap: None,
            mmb_scroll: None,
            applied_theme: None,
            applied_title: String::new(),
            last_frame: Instant::now(),
            initial_file,
            auto,
            frames: 0,
            saved_settings: settings,
            settings_path,
            keymap_path,
            saved_keymap,
            // Write the file on the first start so the welcome window isn't shown again.
            settings_changed_at: first_start.then(Instant::now),
        }
    }

    /// Save preferences once they've been stable for a moment (sliders change
    /// every frame while dragged), or right away when `now` is set.
    fn persist_settings(&mut self, now: bool) {
        let Some(path) = &self.settings_path else { return };
        let current = self.editor.settings();
        if current != self.saved_settings && self.settings_changed_at.is_none() {
            self.settings_changed_at = Some(Instant::now());
        }
        let due = self.settings_changed_at.is_some_and(|t| now || t.elapsed().as_secs_f32() > 1.0);
        if due {
            match current.save(path) {
                Ok(()) => log::info!("saved settings to {}", path.display()),
                Err(e) => log::warn!("could not save settings to {}: {e}", path.display()),
            }
            self.saved_settings = current;
            self.settings_changed_at = None;
        }
        self.persist_keymap();
    }

    fn persist_keymap(&mut self) {
        let Some(path) = &self.keymap_path else { return };
        let text = self.editor.keymap.to_text();
        if text == self.saved_keymap {
            return;
        }
        match self.editor.keymap.save(path) {
            Ok(()) => log::info!("saved keymap to {}", path.display()),
            Err(e) => log::warn!("could not save keymap to {}: {e}", path.display()),
        }
        self.saved_keymap = text;
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
        let info = adapter.get_info();
        log::info!("GPU adapter: {info:?}");
        let si = &mut self.editor.system_info;
        si.gpu = Some(info.name.clone());
        si.gpu_backend = Some(format!("{:?}", info.backend));
        si.gpu_driver = Some(format!("{} {}", info.driver, info.driver_info).trim().to_string());
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("oxybrush"),
            required_limits: adapter.limits(),
            required_features: adapter.features() & oxy_render::optional_features(),
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
        log::info!("exact face-group colors: {}", viewport.exact_face_colors());
        self.editor.system_info.exact_face_colors = Some(viewport.exact_face_colors());
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

        let theme = (self.editor.high_contrast, self.editor.ui_scale, self.editor.layout);
        if self.applied_theme != Some(theme) {
            ui::apply_theme(&self.egui_ctx, theme.0, theme.1, theme.2);
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

        let custom = ed.custom_matcap.and_then(|i| ed.custom_matcaps.get(i));
        let want = (ed.matcap, custom.map(|m| m.name.clone()));
        if self.applied_matcap.as_ref() != Some(&want) {
            match custom {
                Some(m) => g.viewport.set_matcap_pixels(&g.queue, &m.pixels),
                None => g.viewport.set_matcap(&g.queue, ed.matcap),
            }
            self.applied_matcap = Some(want);
        }
        ed.poll_job();
        ed.tick_autosave();
        ed.updater.tick(crate::updates::now_secs());
        let title = ed.title();
        if self.applied_title != title {
            g.window.set_title(&title);
            self.applied_title = title;
        }
        let ids: Vec<u64> = ed.scene.pieces.iter().map(|p| p.id).collect();
        g.viewport.retain(|id| ids.contains(&id));
        let active = ed.scene.active().id;
        let mut items = Vec::new();
        for p in ed.scene.pieces.iter_mut() {
            g.viewport.sync(&g.device, &g.queue, p.id, &mut p.mesh);
            if p.visible {
                items.push(oxy_render::DrawItem { id: p.id, active: p.id == active });
            }
        }

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
            &items,
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
        self.persist_settings(self.editor.quit_now);
        if self.editor.quit_now {
            self.editor.autosaver.finish();
            event_loop.exit();
        }
    }

    fn run_action(&mut self, action: UiAction) {
        let ed = &mut self.editor;
        let project_dialog = || rfd::FileDialog::new().add_filter("OxyBrush project", &[oxy_doc::format::EXTENSION]);
        match action {
            UiAction::Open => {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("OxyBrush project or mesh", &["oxb", "obj", "stl", "OXB", "OBJ", "STL"])
                    .pick_file()
                {
                    ed.open(&p);
                }
            }
            UiAction::Import => {
                if let Some(p) = rfd::FileDialog::new().add_filter("Meshes", &["obj", "stl", "OBJ", "STL"]).pick_file()
                {
                    ed.import(&p);
                }
            }
            UiAction::Save | UiAction::SaveThenContinue => {
                let path = ed.path.clone().or_else(|| project_dialog().set_file_name("sculpt.oxb").save_file());
                let saved = path.is_some_and(|p| ed.save(&p));
                if action == UiAction::SaveThenContinue {
                    if saved {
                        if let Some(pending) = ed.pending.take() {
                            ed.perform(pending);
                        }
                    } else {
                        // Cancelled or failed: keep the document open.
                        ed.pending = None;
                    }
                }
            }
            UiAction::LoadMatcap => {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("Matcap image", &["png", "jpg", "jpeg", "PNG", "JPG", "JPEG"])
                    .pick_file()
                {
                    // Copy into the library so it is there next time; without a
                    // library folder, just use it for this session.
                    let loaded = match &ed.matcap_dir {
                        Some(dir) => crate::matcaps::import(dir, &p),
                        None => crate::matcaps::decode(&p).map(|(w, h, rgba)| crate::matcaps::CustomMatcap {
                            name: p.file_stem().map_or("Matcap".into(), |s| s.to_string_lossy().into_owned()),
                            pixels: std::sync::Arc::new(crate::matcaps::to_matcap(w, h, &rgba)),
                        }),
                    };
                    match loaded {
                        Ok(m) => ed.add_custom_matcap(m),
                        Err(e) => ed.status = format!("Could not load {}: {e}", p.display()),
                    }
                }
            }
            UiAction::LoadAlpha => {
                if let Some(p) = rfd::FileDialog::new().add_filter("PNG image", &["png", "PNG"]).pick_file() {
                    match crate::support::load_alpha_png(&p) {
                        Ok(alpha) => ed.add_alpha(alpha),
                        Err(e) => ed.status = format!("Could not load {}: {e}", p.display()),
                    }
                }
            }
            UiAction::ImportBrush => {
                let ext = oxy_sculpt::preset::EXTENSION;
                if let Some(p) = rfd::FileDialog::new().add_filter("OxyBrush brush preset", &[ext]).pick_file() {
                    match crate::presets::read_file(&p) {
                        Ok(preset) => {
                            ed.apply_preset(&preset);
                            ed.add_preset(preset);
                        }
                        Err(e) => ed.status = format!("Could not import {}: {e}", p.display()),
                    }
                }
            }
            UiAction::ExportBrush => {
                if let Some(preset) = ed.request_export_preset.take() {
                    let ext = oxy_sculpt::preset::EXTENSION;
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("OxyBrush brush preset", &[ext])
                        .set_file_name(crate::presets::file_name(&preset.name))
                        .save_file()
                    {
                        let p = if p.extension().is_none() { p.with_extension(ext) } else { p };
                        ed.status = match crate::presets::write_file(&preset, &p) {
                            Ok(()) => format!("Exported brush preset to {}", p.display()),
                            Err(e) => format!("Could not export: {e}"),
                        };
                    }
                }
            }
            UiAction::SaveAs => {
                if let Some(p) = project_dialog().set_file_name("sculpt.oxb").save_file() {
                    ed.save(&p);
                }
            }
            UiAction::ImportKeymap => {
                if let Some(p) =
                    rfd::FileDialog::new().add_filter("OxyBrush keymap", &[keymap::EXTENSION, "txt"]).pick_file()
                {
                    ed.status = match std::fs::read_to_string(&p) {
                        Ok(text) => match keymap::Keymap::import_text(&text) {
                            Ok(map) => {
                                let n = map.conflicts().len() + map.nav_conflicts().len();
                                ed.keymap = map;
                                ed.keymap_capture = None;
                                if n == 0 {
                                    format!("Imported keymap from {}", p.display())
                                } else {
                                    format!("Imported keymap from {} ({n} conflicts to resolve)", p.display())
                                }
                            }
                            Err(e) => e.to_string(),
                        },
                        Err(e) => format!("Could not import {}: {e}", p.display()),
                    };
                }
            }
            UiAction::ExportKeymap => {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("OxyBrush keymap", &[keymap::EXTENSION])
                    .set_file_name("oxybrush.oxkeys")
                    .save_file()
                {
                    let p = if p.extension().is_none() { p.with_extension(keymap::EXTENSION) } else { p };
                    ed.status = match ed.keymap.save(&p) {
                        Ok(()) => format!("Exported keymap to {}", p.display()),
                        Err(e) => format!("Could not export: {e}"),
                    };
                }
            }
            UiAction::Export => {
                let name = ed
                    .path
                    .as_ref()
                    .and_then(|p| p.file_stem())
                    .map(|s| format!("{}.glb", s.to_string_lossy()))
                    .unwrap_or_else(|| "sculpt.glb".into());
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("glTF binary (games, engines, Blender)", &["glb"])
                    .add_filter("glTF (single text file)", &["gltf"])
                    .add_filter("Wavefront OBJ", &["obj"])
                    .add_filter("STL (3D printing)", &["stl"])
                    .add_filter("PLY", &["ply"])
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
        let key = key_id(event);
        if self.editor.keymap_capture.is_some() {
            if event.state == ElementState::Pressed && !event.repeat {
                self.capture_key(key);
            }
            return None;
        }
        // The quick menu is held: it opens on the press and closes when that key is released.
        if let Some(key) = key {
            let chord = self.chord(key);
            if event.state == ElementState::Released && self.editor.quick_menu_key == Some(key) {
                self.editor.quick_menu = None;
                self.editor.quick_menu_key = None;
                return None;
            }
            if event.state == ElementState::Pressed
                && !event.repeat
                && self.editor.keymap.chords(CommandId::QuickMenu).contains(&chord)
            {
                if let Some(pos) = self.editor.pointer {
                    self.editor.quick_menu = Some(crate::editor::QuickMenu { pos, held: true });
                    self.editor.quick_menu_key = Some(key);
                }
                return None;
            }
        }
        if event.state != ElementState::Pressed {
            return None;
        }
        let key = key?;
        let chord = self.chord(key);
        let cmd = self.editor.keymap.command_for(chord)?;
        if cmd == CommandId::QuickMenu || (cmd == CommandId::BrushFinder && self.editor.palette.is_some()) {
            return None;
        }
        let effect = self.editor.perform_command(cmd);
        effect_to_action(effect)
    }

    fn chord(&self, key: KeyId) -> Chord {
        let m = self.editor.modifiers;
        Chord { ctrl: m.ctrl, shift: m.shift, alt: m.alt, key }
    }

    fn capture_key(&mut self, key: Option<KeyId>) {
        let Some((id, slot)) = self.editor.keymap_capture else { return };
        let bare_esc = key == Some(KeyId::Escape)
            && !self.editor.modifiers.ctrl
            && !self.editor.modifiers.shift
            && !self.editor.modifiers.alt;
        if bare_esc {
            self.editor.keymap_capture = None;
            self.editor.status = "Key unchanged".into();
            return;
        }
        let Some(key) = key else { return };
        let chord = self.chord(key);
        self.editor.keymap.set_chord(id, slot, chord);
        self.editor.keymap_capture = None;
        let overlaps = self.editor.keymap.overlaps_with(id);
        self.editor.status = if overlaps.is_empty() {
            format!("{} is now {}", keymap::command_name(id), chord.label())
        } else {
            let names: Vec<&str> = overlaps.iter().map(|o| keymap::command_name(*o)).collect();
            format!("{} is now {} (also used by {})", keymap::command_name(id), chord.label(), names.join(", "))
        };
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
        // An alpha stamp (Scales) dragged out on the lower right.
        ed.alpha = ed.alphas.iter().position(|a| a.name == "Scales");
        ed.stroke_mode = crate::editor::StrokeMode::Stamp;
        ed.brush_kind = oxy_sculpt::BrushKind::Draw;
        ed.pointer = Some(c + Vec2::new(0.45, 0.05) * s);
        ed.pointer_down(PointerButton::Primary);
        ed.pointer_moved(c + Vec2::new(0.95, 0.05) * s);
        ed.pointer_up();
        ed.stroke_mode = crate::editor::StrokeMode::Freehand;
        // Face groups from the mask, shown in color.
        ed.edit_faces("Group from mask", oxy_mesh::Mesh::group_from_mask);
        ed.clear_mask();
        ed.view.show_groups = true;
        // A second piece beside the first: a ring of radial-symmetry dabs on a cube.
        ed.add_primitive("cube");
        let piece = ed.scene.active_mut();
        let mut data = piece.mesh.to_data();
        data.positions.iter_mut().for_each(|p| *p = *p * 0.55 + glam::Vec3::new(1.7, -0.4, 0.0));
        piece.mesh = oxy_mesh::Mesh::from_data(data);
        ed.scene.set_active(0);
        ed.frame();
        ed.brush_kind = oxy_sculpt::BrushKind::Draw;
        ed.pointer = Some(c + Vec2::new(0.9, 0.1) * s);
        ed.status = "Demo strokes applied".into();
    }
}

impl ApplicationHandler for App {
    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        if matches!(cause, StartCause::ResumeTimeReached { .. })
            && let Some(g) = &self.gfx
        {
            g.window.request_redraw();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Save preferences once they settle, even if nothing redraws.
        self.persist_settings(false);
        // Wake up regularly to finish background jobs, pending settings and
        // autosave even when idle.
        let wait = if self.editor.job.is_some() || self.editor.updater.busy() {
            Duration::from_millis(200)
        } else if self.settings_changed_at.is_some() {
            Duration::from_millis(500)
        } else {
            Duration::from_secs(20)
        };
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + wait));
    }

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
            WindowEvent::CloseRequested => {
                if self.auto.screenshot.is_some() {
                    event_loop.exit();
                } else {
                    self.editor.request(crate::editor::Pending::Quit);
                    if self.editor.quit_now {
                        self.persist_settings(true);
                        self.editor.autosaver.finish();
                        event_loop.exit();
                    }
                    redraw = true;
                }
            }
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
            WindowEvent::CursorMoved { position, .. } if self.mmb_scroll.is_some() => {
                // Middle-drag over a panel scrolls it, like grabbing the page.
                let pos = Vec2::new(position.x as f32, position.y as f32);
                let last = self.mmb_scroll.replace(pos).unwrap_or(pos);
                let d = (pos - last) / self.editor.pixels_per_point.max(0.1);
                g.egui_state.egui_input_mut().events.push(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(d.x, d.y),
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Unknown,
                    modifiers: Default::default(),
                });
                self.editor.pointer = Some(pos);
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
                if *button == MouseButton::Middle {
                    self.mmb_scroll = match state {
                        ElementState::Pressed if over_egui => self.editor.pointer,
                        _ => None,
                    };
                }
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
            // A keymap capture, or releasing the quick-menu key, wins over a focused
            // slider so the key the user pressed is the one that gets bound.
            WindowEvent::KeyboardInput { event, .. }
                if self.editor.keymap_capture.is_some()
                    || (event.state == ElementState::Released && self.editor.quick_menu.is_some_and(|q| q.held)) =>
            {
                if let Some(a) = self.handle_key(event) {
                    self.run_action(a);
                }
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

/// The key a keymap chord is built from. Numpad, function and symbol keys use
/// the physical key so they stay distinct (Numpad 1 is not the number row).
/// Letters and digits use the character the layout produces.
fn effect_to_action(effect: CommandEffect) -> Option<UiAction> {
    match effect {
        CommandEffect::None => None,
        CommandEffect::Open => Some(UiAction::Open),
        CommandEffect::Import => Some(UiAction::Import),
        CommandEffect::Save => Some(UiAction::Save),
        CommandEffect::SaveAs => Some(UiAction::SaveAs),
        CommandEffect::Export => Some(UiAction::Export),
    }
}

fn key_id(event: &KeyEvent) -> Option<KeyId> {
    if let PhysicalKey::Code(code) = event.physical_key {
        if is_modifier(code) {
            return None;
        }
        if let Some(id) = physical_key(code) {
            return Some(id);
        }
    }
    match &event.logical_key {
        Key::Named(named) => named_key(*named),
        Key::Character(s) => character_key(s),
        _ => None,
    }
}

fn is_modifier(code: KeyCode) -> bool {
    matches!(
        code,
        KeyCode::ShiftLeft
            | KeyCode::ShiftRight
            | KeyCode::ControlLeft
            | KeyCode::ControlRight
            | KeyCode::AltLeft
            | KeyCode::AltRight
            | KeyCode::SuperLeft
            | KeyCode::SuperRight
    )
}

fn physical_key(code: KeyCode) -> Option<KeyId> {
    let numpad = match code {
        KeyCode::Numpad0 => Some(0),
        KeyCode::Numpad1 => Some(1),
        KeyCode::Numpad2 => Some(2),
        KeyCode::Numpad3 => Some(3),
        KeyCode::Numpad4 => Some(4),
        KeyCode::Numpad5 => Some(5),
        KeyCode::Numpad6 => Some(6),
        KeyCode::Numpad7 => Some(7),
        KeyCode::Numpad8 => Some(8),
        KeyCode::Numpad9 => Some(9),
        _ => None,
    };
    if let Some(n) = numpad {
        return Some(KeyId::Numpad(n));
    }
    let function = match code {
        KeyCode::F1 => Some(1),
        KeyCode::F2 => Some(2),
        KeyCode::F3 => Some(3),
        KeyCode::F4 => Some(4),
        KeyCode::F5 => Some(5),
        KeyCode::F6 => Some(6),
        KeyCode::F7 => Some(7),
        KeyCode::F8 => Some(8),
        KeyCode::F9 => Some(9),
        KeyCode::F10 => Some(10),
        KeyCode::F11 => Some(11),
        KeyCode::F12 => Some(12),
        _ => None,
    };
    if let Some(n) = function {
        return Some(KeyId::F(n));
    }
    Some(match code {
        KeyCode::Tab => KeyId::Tab,
        KeyCode::Space => KeyId::Space,
        KeyCode::Escape => KeyId::Escape,
        KeyCode::ArrowUp => KeyId::Up,
        KeyCode::ArrowDown => KeyId::Down,
        KeyCode::ArrowLeft => KeyId::Left,
        KeyCode::ArrowRight => KeyId::Right,
        KeyCode::Home => KeyId::Home,
        KeyCode::End => KeyId::End,
        KeyCode::PageUp => KeyId::PageUp,
        KeyCode::PageDown => KeyId::PageDown,
        KeyCode::Insert => KeyId::Insert,
        KeyCode::Delete => KeyId::Delete,
        KeyCode::Backspace => KeyId::Backspace,
        KeyCode::Minus => KeyId::Char('-'),
        KeyCode::Equal => KeyId::Char('='),
        KeyCode::BracketLeft => KeyId::Char('['),
        KeyCode::BracketRight => KeyId::Char(']'),
        _ => return None,
    })
}

fn named_key(named: NamedKey) -> Option<KeyId> {
    Some(match named {
        NamedKey::Tab => KeyId::Tab,
        NamedKey::Space => KeyId::Space,
        NamedKey::Escape => KeyId::Escape,
        NamedKey::ArrowUp => KeyId::Up,
        NamedKey::ArrowDown => KeyId::Down,
        NamedKey::ArrowLeft => KeyId::Left,
        NamedKey::ArrowRight => KeyId::Right,
        NamedKey::Home => KeyId::Home,
        NamedKey::End => KeyId::End,
        NamedKey::PageUp => KeyId::PageUp,
        NamedKey::PageDown => KeyId::PageDown,
        NamedKey::Insert => KeyId::Insert,
        NamedKey::Delete => KeyId::Delete,
        NamedKey::Backspace => KeyId::Backspace,
        NamedKey::F1 => KeyId::F(1),
        NamedKey::F2 => KeyId::F(2),
        NamedKey::F3 => KeyId::F(3),
        NamedKey::F4 => KeyId::F(4),
        NamedKey::F5 => KeyId::F(5),
        NamedKey::F6 => KeyId::F(6),
        NamedKey::F7 => KeyId::F(7),
        NamedKey::F8 => KeyId::F(8),
        NamedKey::F9 => KeyId::F(9),
        NamedKey::F10 => KeyId::F(10),
        NamedKey::F11 => KeyId::F(11),
        NamedKey::F12 => KeyId::F(12),
        _ => return None,
    })
}

/// Map a shifted symbol back to the key that produced it, so Shift+1 is the 1 key.
fn character_key(s: &str) -> Option<KeyId> {
    let c = s.chars().next()?;
    let c = match c {
        '!' => '1',
        '@' => '2',
        '#' => '3',
        '$' => '4',
        '%' => '5',
        '^' => '6',
        '&' => '7',
        '*' => '8',
        '(' => '9',
        ')' => '0',
        '+' => '=',
        '_' => '-',
        '{' => '[',
        '}' => ']',
        other => other,
    };
    if c.is_ascii_alphanumeric() || matches!(c, '[' | ']' | '-' | '=') {
        Some(KeyId::Char(c.to_ascii_lowercase()))
    } else {
        None
    }
}

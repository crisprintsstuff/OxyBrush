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
use winit::event_loop::EventLoopProxy;
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

type AppliedThemeKey = (
    crate::settings::ThemePreset,
    crate::settings::AccentPreset,
    [u8; 3],
    crate::settings::CornerStyle,
    bool,
    f32,
    crate::settings::UiLayout,
);

pub struct App {
    gfx: Option<Gfx>,
    egui_ctx: egui::Context,
    editor: Editor,
    /// Material last uploaded: a built-in preset, or a custom matcap's name.
    applied_matcap: Option<(MatcapPreset, Option<String>)>,
    /// Middle button held over a panel: last pointer position, for drag-scrolling.
    mmb_scroll: Option<Vec2>,
    applied_theme: Option<AppliedThemeKey>,
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
    proxy: Option<EventLoopProxy<()>>,
    /// Local MCP listener. None unless the user opted in.
    agent: Option<crate::agent::Inbox>,
    /// Screenshot tool waiting for this frame's viewport.
    shot_reply: Option<std::sync::mpsc::Sender<oxy_mcp::ToolResult>>,
    /// Running Python plugin process.
    plugin_process: Option<crate::plugins::PluginProcess>,
    /// When egui requested a delayed repaint (e.g. for hover tooltips).
    egui_repaint_at: Option<Instant>,
    /// Active multi-touch points for gestures and tablet input.
    touches: std::collections::HashMap<u64, TouchPoint>,
    touch_two_finger_start: Option<Instant>,
    touch_prev_dist: Option<f32>,
    touch_prev_mid: Option<Vec2>,
    dialog_rx: std::sync::mpsc::Receiver<DialogReply>,
    dialog_tx: std::sync::mpsc::Sender<DialogReply>,
    dialog_active: bool,
}

#[derive(Debug)]
enum DialogReply {
    Open(Option<PathBuf>),
    Import(Option<PathBuf>),
    Save { path: Option<PathBuf>, and_continue: bool },
    SaveAs(Option<PathBuf>),
    LoadMatcap(Option<PathBuf>),
    LoadAlpha(Option<PathBuf>),
    ImportBrush(Option<PathBuf>),
    ExportBrush { preset: oxy_sculpt::BrushPreset, path: Option<PathBuf> },
    ImportKeymap(Option<PathBuf>),
    ExportKeymap(Option<PathBuf>),
    Export(Option<PathBuf>),
    RunScript(Option<PathBuf>),
}

#[derive(Debug, Clone, Copy)]
struct TouchPoint {
    start_pos: Vec2,
    pos: Vec2,
    _start_time: Instant,
}

impl App {
    pub fn new(initial_file: Option<PathBuf>, auto: AutoRun, log_path: Option<PathBuf>) -> Self {
        let (dialog_tx, dialog_rx) = std::sync::mpsc::channel();
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
                    e.coalesce_strokes = true;
                    e.dab_budget_ms =
                        std::env::var("OXYBRUSH_DAB_BUDGET_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(10.0);
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
                    e.agent_enabled = false;
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
            proxy: None,
            agent: None,
            shot_reply: None,
            plugin_process: None,
            egui_repaint_at: None,
            touches: std::collections::HashMap::new(),
            touch_two_finger_start: None,
            touch_prev_dist: None,
            touch_prev_mid: None,
            dialog_rx,
            dialog_tx,
            dialog_active: false,
        }
    }

    /// The event-loop proxy is created in `main`, before the loop starts
    /// (`ActiveEventLoop` cannot build one). The agent thread uses it to wake the UI.
    pub(crate) fn set_event_proxy(&mut self, proxy: EventLoopProxy<()>) {
        let p = proxy.clone();
        self.egui_ctx.set_request_repaint_callback(move |info| {
            if info.delay.is_zero() {
                let _ = p.send_event(());
            }
        });
        self.proxy = Some(proxy);
    }

    /// Start or stop the loopback agent server to match the setting, and
    /// refresh the token and the "connected" indicator.
    fn sync_agent(&mut self) {
        if self.auto.screenshot.is_some() {
            self.editor.agent_enabled = false;
        }
        if self.editor.agent_disconnect {
            self.editor.agent_disconnect = false;
            if let Some(agent) = &self.agent {
                agent.disconnect();
                self.editor.agent_token = agent.token();
                self.editor.agent_connected = false;
                self.editor.status = "Agent disconnected. The new token is in Settings.".into();
            }
        }
        let want = self.editor.agent_enabled && self.auto.screenshot.is_none();
        if want && self.agent.is_none() {
            let Some(proxy) = self.proxy.clone() else { return };
            let wake = Arc::new(move || {
                let _ = proxy.send_event(());
            });
            match crate::agent::Inbox::listen(wake) {
                Ok(inbox) => {
                    log::info!("local agent listening on {} (127.0.0.1 only)", inbox.endpoint);
                    self.editor.agent_endpoint = inbox.endpoint.clone();
                    self.editor.agent_token = inbox.token();
                    self.agent = Some(inbox);
                }
                Err(e) => {
                    log::warn!("could not start the local agent server: {e}");
                    self.editor.agent_enabled = false;
                    self.editor.status = format!("Could not start the local agent server: {e}");
                }
            }
        } else if !want && self.agent.is_some() {
            self.agent = None;
            self.editor.agent_endpoint.clear();
            self.editor.agent_token.clear();
            self.editor.agent_connected = false;
        }
        if let Some(agent) = &self.agent {
            self.editor.agent_connected = agent.connected();
            self.editor.agent_token = agent.token();
        }
    }

    /// Apply one queued agent call. A screenshot is answered after the viewport is drawn.
    fn poll_agent(&mut self) {
        if self.shot_reply.is_some() {
            return;
        }
        let Some(agent) = &mut self.agent else { return };
        if let Some(reply) = agent.poll(&mut self.editor) {
            self.shot_reply = Some(reply);
        }
    }

    /// Check if a background Python plugin process has completed.
    fn poll_plugin(&mut self) {
        let Some(proc) = &mut self.plugin_process else { return };
        match proc.rx.try_recv() {
            Ok(Ok(stdout)) => {
                let name = proc.name.clone();
                self.plugin_process = None;
                if stdout.is_empty() {
                    self.editor.status = format!("Plugin '{name}' finished");
                } else {
                    let first_line = stdout.lines().next().unwrap_or("finished");
                    self.editor.status = format!("Plugin '{name}': {first_line}");
                }
            }
            Ok(Err(stderr)) => {
                let name = proc.name.clone();
                self.plugin_process = None;
                // For Python exceptions and tracebacks, the meaningful diagnostic
                // (e.g. `AttributeError: ...`) is on the last non-empty line of stderr.
                let err_msg = stderr.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("failed").trim();
                self.editor.status = format!("Plugin '{name}' failed: {err_msg}");
                log::warn!("Plugin '{name}' failed:\n{stderr}");
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.plugin_process = None;
            }
        }
    }

    /// Execute a Python plugin script against the local agent server.
    fn run_python_script(&mut self, path: &std::path::Path) {
        if self.plugin_process.is_some() {
            self.editor.status = "Another plugin is already running".into();
            return;
        }
        if self.agent.is_none() {
            self.editor.agent_enabled = true;
            self.sync_agent();
        }
        let (endpoint, token) = if let Some(agent) = &self.agent {
            (agent.endpoint.clone(), agent.token())
        } else {
            self.editor.status = "Could not start local agent server for Python script".into();
            return;
        };
        let Some(proxy) = self.proxy.clone() else { return };
        let wake = Arc::new(move || {
            let _ = proxy.send_event(());
        });
        match crate::plugins::run_plugin(path, &endpoint, &token, wake) {
            Ok(proc) => {
                let name = proc.name.clone();
                self.plugin_process = Some(proc);
                self.editor.status = format!("Running plugin '{name}'…");
            }
            Err(e) => {
                self.editor.status = format!("Failed to run plugin: {e}");
            }
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
            .with_min_inner_size(winit::dpi::LogicalSize::new(640.0, 400.0))
            // The title bar / window list icon. Without it, Windows shows a
            // generic icon there even though the exe has one.
            .with_window_icon(window_icon(include_bytes!("../../../packaging/icons/oxybrush-32.png")));
        // The taskbar and Alt+Tab use the large icon.
        #[cfg(target_os = "windows")]
        let attrs = winit::platform::windows::WindowAttributesExtWindows::with_taskbar_icon(
            attrs,
            window_icon(include_bytes!("../../../packaging/icons/oxybrush-256.png")),
        );
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
        self.editor.flush_drag();
        self.poll_dialog();
        // Before borrowing the GPU state, so a listener started last frame shows its address.
        self.sync_agent();
        let Some(g) = &mut self.gfx else { return };
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32() * 1000.0;
        self.last_frame = now;
        self.editor.frame_ms = self.editor.frame_ms * 0.9 + dt * 0.1;

        let theme_key = (
            self.editor.theme,
            self.editor.accent,
            self.editor.custom_accent,
            self.editor.corner_style,
            self.editor.high_contrast,
            self.editor.ui_scale,
            self.editor.layout,
        );
        if self.applied_theme != Some(theme_key) {
            ui::apply_theme(
                &self.egui_ctx,
                theme_key.0,
                theme_key.1,
                theme_key.2,
                theme_key.3,
                theme_key.4,
                theme_key.5,
                theme_key.6,
            );
            self.applied_theme = Some(theme_key);
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

        let (active_matcap, active_custom) = ed.preview_matcap.unwrap_or((ed.matcap, ed.custom_matcap));
        let custom = active_custom.and_then(|i| ed.custom_matcaps.get(i));
        let want = (active_matcap, custom.map(|m| m.name.clone()));
        if self.applied_matcap.as_ref() != Some(&want) {
            match custom {
                Some(m) => g.viewport.set_matcap_pixels(&g.queue, &m.pixels),
                None => g.viewport.set_matcap(&g.queue, active_matcap),
            }
            self.applied_matcap = Some(want);
        }
        ed.poll_job();
        ed.tick_autosave();
        ed.updater.tick(crate::updates::now_secs());
        // Agent calls run after the user's finished job and before the mesh is uploaded.
        if self.shot_reply.is_none()
            && let Some(reply) = self.agent.as_mut().and_then(|agent| agent.poll(ed))
        {
            self.shot_reply = Some(reply);
        }
        let title = ed.title();
        if self.applied_title != title {
            g.window.set_title(&title);
            self.applied_title = title;
        }
        // The Insert Primitive and mask extract previews are drawn like a
        // piece under a reserved id.
        const PREVIEW_ID: u64 = u64::MAX;
        ed.update_extract_preview();
        ed.tick_hold(Instant::now());
        ed.update_curve();
        ed.update_tube();
        ed.check_link_topology();
        let ids: Vec<u64> = ed.scene.pieces.iter().map(|p| p.id).collect();
        let previewing =
            ed.insert_preview.is_some() || ed.extract_preview.is_some() || ed.curve.is_some() || ed.tube.is_some();
        // A curve stroke's preview is the whole piece with the stroke
        // applied: draw it in the piece's place, looking like the piece.
        let curve_piece = ed.curve.as_ref().filter(|_| ed.tube.is_none()).map(|c| c.piece);
        g.viewport.retain(|id| ids.contains(&id) || (previewing && id == PREVIEW_ID));
        let active = ed.scene.active().id;
        let mut items = Vec::new();
        for p in ed.scene.pieces.iter_mut() {
            g.viewport.sync(&g.device, &g.queue, p.id, &mut p.mesh);
            if p.visible && Some(p.id) != curve_piece {
                items.push(oxy_render::DrawItem { id: p.id, active: p.id == active, preview: false });
            }
        }
        if let Some(preview) = ed.preview_mesh_mut() {
            g.viewport.sync(&g.device, &g.queue, PREVIEW_ID, preview);
            items.push(oxy_render::DrawItem { id: PREVIEW_ID, active: true, preview: curve_piece.is_none() });
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
            &oxy_render::ViewSettings {
                floor: ed.floor_grid.then(|| ed.floor()).flatten(),
                scene_bounds: {
                    let b = ed.scene.bounds();
                    (!b.is_empty()).then(|| (b.min.to_array(), b.max.to_array()))
                },
                ..ed.view
            },
            &items,
        );
        // The agent's screenshot is the viewport without the panels, so copy
        // before egui draws. Reading the pixels back submits this encoder;
        // the UI is recorded into a fresh one afterwards.
        if self.shot_reply.is_some() {
            let copied = crate::snapshot::copy_to_buffer(&g.device, &mut encoder, &frame.texture);
            match copied {
                Some(pending) => {
                    g.queue.submit([encoder.finish()]);
                    let canvas = ed.canvas;
                    let crop = (
                        canvas[0].round().max(0.0) as u32,
                        canvas[1].round().max(0.0) as u32,
                        canvas[2].round().max(1.0) as u32,
                        canvas[3].round().max(1.0) as u32,
                    );
                    let png = crate::snapshot::png_bytes(&g.device, pending, Some(crop));
                    if let Some(reply) = self.shot_reply.take() {
                        let result = match png {
                            Ok(bytes) => oxy_mcp::ToolResult::with_image(
                                format!(
                                    "viewport {}x{} pixels. Screen-space stroke points use these pixels, origin at the top-left of the viewport.",
                                    crop.2, crop.3
                                ),
                                bytes,
                            ),
                            Err(e) => oxy_mcp::ToolResult::error(format!("screenshot failed: {e}")),
                        };
                        let _ = reply.send(result);
                    }
                    encoder =
                        g.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame-ui") });
                }
                None => {
                    if let Some(reply) = self.shot_reply.take() {
                        let _ = reply.send(oxy_mcp::ToolResult::error("this GPU surface can't be read back"));
                    }
                }
            }
        }
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

        let repaint_delay = out.viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.repaint_delay);
        if repaint_delay == Some(Duration::ZERO) || self.auto.screenshot.is_some() {
            g.window.request_redraw();
            self.egui_repaint_at = None;
        } else if let Some(delay) = repaint_delay {
            if delay < Duration::from_secs(3600) {
                self.egui_repaint_at = Some(Instant::now() + delay);
            } else {
                self.egui_repaint_at = None;
            }
        } else {
            self.egui_repaint_at = None;
        }

        if let Some(a) = action {
            self.run_action(a);
        }
        if let Some(p) = self.editor.request_run_plugin.take() {
            self.run_python_script(&p);
        }
        if std::mem::take(&mut self.editor.request_open_plugins_dir)
            && let Some(dir) = crate::plugins::plugins_dir()
        {
            crate::plugins::open_folder(&dir);
        }
        if std::mem::take(&mut self.editor.request_reload_plugins) {
            self.editor.reload_plugins();
            self.editor.status = format!("Loaded {} plugins", self.editor.plugins.len());
        }
        self.persist_settings(self.editor.quit_now);
        if self.editor.quit_now {
            self.editor.autosaver.finish();
            event_loop.exit();
        }
    }

    fn spawn_dialog<F>(&mut self, future: F, map: impl FnOnce(Option<PathBuf>) -> DialogReply + Send + 'static)
    where
        F: std::future::Future<Output = Option<rfd::FileHandle>> + Send + 'static,
    {
        self.dialog_active = true;
        let tx = self.dialog_tx.clone();
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let handle = pollster::block_on(future);
            let path = handle.map(|h| h.path().to_path_buf());
            let _ = tx.send(map(path));
            if let Some(proxy) = proxy {
                let _ = proxy.send_event(());
            }
        });
    }

    fn run_action(&mut self, action: UiAction) {
        let ed = &mut self.editor;
        match action {
            UiAction::Save | UiAction::SaveThenContinue => {
                if let Some(path) = ed.path.clone() {
                    let saved = ed.save(&path);
                    if action == UiAction::SaveThenContinue {
                        if saved {
                            if let Some(pending) = ed.pending.take() {
                                ed.perform(pending);
                            }
                        } else {
                            ed.pending = None;
                        }
                    }
                    if let Some(g) = &self.gfx {
                        g.window.request_redraw();
                    }
                    return;
                }
                if self.dialog_active {
                    return;
                }
                let mut dialog = rfd::AsyncFileDialog::new()
                    .add_filter("OxyBrush project (*.oxb)", &[oxy_doc::format::EXTENSION])
                    .set_file_name("sculpt.oxb");
                if let Some(g) = &self.gfx {
                    dialog = dialog.set_parent(&g.window);
                }
                let and_continue = action == UiAction::SaveThenContinue;
                self.spawn_dialog(dialog.save_file(), move |path| DialogReply::Save { path, and_continue });
            }
            UiAction::SaveAs => {
                if self.dialog_active {
                    return;
                }
                let mut dialog = rfd::AsyncFileDialog::new()
                    .add_filter("OxyBrush project (*.oxb)", &[oxy_doc::format::EXTENSION])
                    .set_file_name("sculpt.oxb");
                if let Some(g) = &self.gfx {
                    dialog = dialog.set_parent(&g.window);
                }
                self.spawn_dialog(dialog.save_file(), DialogReply::SaveAs);
            }
            UiAction::Open => {
                if self.dialog_active {
                    return;
                }
                let mut dialog = rfd::AsyncFileDialog::new()
                    .add_filter(
                        "OxyBrush project or mesh",
                        &["oxb", "obj", "stl", "ztl", "zpr", "OXB", "OBJ", "STL", "ZTL", "ZPR"],
                    )
                    .add_filter("OxyBrush project (*.oxb)", &["oxb", "OXB"])
                    .add_filter("Wavefront OBJ (*.obj)", &["obj", "OBJ"])
                    .add_filter("STL mesh (*.stl)", &["stl", "STL"])
                    .add_filter("ZBrush tool or project (*.ztl, *.zpr)", &["ztl", "zpr", "ZTL", "ZPR"]);
                if let Some(g) = &self.gfx {
                    dialog = dialog.set_parent(&g.window);
                }
                self.spawn_dialog(dialog.pick_file(), DialogReply::Open);
            }
            UiAction::Import => {
                if self.dialog_active {
                    return;
                }
                let mut dialog = rfd::AsyncFileDialog::new()
                    .add_filter("Meshes", &["obj", "stl", "ztl", "zpr", "OBJ", "STL", "ZTL", "ZPR"])
                    .add_filter("Wavefront OBJ (*.obj)", &["obj", "OBJ"])
                    .add_filter("STL mesh (*.stl)", &["stl", "STL"])
                    .add_filter("ZBrush tool or project (*.ztl, *.zpr)", &["ztl", "zpr", "ZTL", "ZPR"]);
                if let Some(g) = &self.gfx {
                    dialog = dialog.set_parent(&g.window);
                }
                self.spawn_dialog(dialog.pick_file(), DialogReply::Import);
            }
            UiAction::LoadMatcap => {
                if self.dialog_active {
                    return;
                }
                let mut dialog = rfd::AsyncFileDialog::new()
                    .add_filter("Matcap image", &["png", "jpg", "jpeg", "PNG", "JPG", "JPEG"]);
                if let Some(g) = &self.gfx {
                    dialog = dialog.set_parent(&g.window);
                }
                self.spawn_dialog(dialog.pick_file(), DialogReply::LoadMatcap);
            }
            UiAction::LoadAlpha => {
                if self.dialog_active {
                    return;
                }
                let mut dialog = rfd::AsyncFileDialog::new().add_filter("PNG image", &["png", "PNG"]);
                if let Some(g) = &self.gfx {
                    dialog = dialog.set_parent(&g.window);
                }
                self.spawn_dialog(dialog.pick_file(), DialogReply::LoadAlpha);
            }
            UiAction::ImportBrush => {
                if self.dialog_active {
                    return;
                }
                let ext = oxy_sculpt::preset::EXTENSION;
                let mut dialog = rfd::AsyncFileDialog::new().add_filter("OxyBrush brush preset", &[ext]);
                if let Some(g) = &self.gfx {
                    dialog = dialog.set_parent(&g.window);
                }
                self.spawn_dialog(dialog.pick_file(), DialogReply::ImportBrush);
            }
            UiAction::ExportBrush => {
                if self.dialog_active {
                    return;
                }
                if let Some(preset) = ed.request_export_preset.take() {
                    let ext = oxy_sculpt::preset::EXTENSION;
                    let mut dialog = rfd::AsyncFileDialog::new()
                        .add_filter("OxyBrush brush preset", &[ext])
                        .set_file_name(crate::presets::file_name(&preset.name));
                    if let Some(g) = &self.gfx {
                        dialog = dialog.set_parent(&g.window);
                    }
                    self.spawn_dialog(dialog.save_file(), move |path| DialogReply::ExportBrush { preset, path });
                }
            }
            UiAction::ImportKeymap => {
                if self.dialog_active {
                    return;
                }
                let mut dialog = rfd::AsyncFileDialog::new().add_filter("OxyBrush keymap", &[keymap::EXTENSION, "txt"]);
                if let Some(g) = &self.gfx {
                    dialog = dialog.set_parent(&g.window);
                }
                self.spawn_dialog(dialog.pick_file(), DialogReply::ImportKeymap);
            }
            UiAction::ExportKeymap => {
                if self.dialog_active {
                    return;
                }
                let mut dialog = rfd::AsyncFileDialog::new()
                    .add_filter("OxyBrush keymap", &[keymap::EXTENSION])
                    .set_file_name("oxybrush.oxkeys");
                if let Some(g) = &self.gfx {
                    dialog = dialog.set_parent(&g.window);
                }
                self.spawn_dialog(dialog.save_file(), DialogReply::ExportKeymap);
            }
            UiAction::Export => {
                if self.dialog_active {
                    return;
                }
                let name = ed
                    .path
                    .as_ref()
                    .and_then(|p| p.file_stem())
                    .map(|s| format!("{}.glb", s.to_string_lossy()))
                    .unwrap_or_else(|| "sculpt.glb".into());
                let mut dialog = rfd::AsyncFileDialog::new()
                    .add_filter("glTF binary (games, engines, Blender)", &["glb"])
                    .add_filter("glTF (single text file)", &["gltf"])
                    .add_filter("Wavefront OBJ", &["obj"])
                    .add_filter("STL (3D printing)", &["stl"])
                    .add_filter("PLY", &["ply"])
                    .set_file_name(name);
                if let Some(g) = &self.gfx {
                    dialog = dialog.set_parent(&g.window);
                }
                self.spawn_dialog(dialog.save_file(), DialogReply::Export);
            }
            UiAction::RunScript => {
                if self.dialog_active {
                    return;
                }
                let mut dialog = rfd::AsyncFileDialog::new().add_filter("Python script", &["py", "PY"]);
                if let Some(g) = &self.gfx {
                    dialog = dialog.set_parent(&g.window);
                }
                self.spawn_dialog(dialog.pick_file(), DialogReply::RunScript);
            }
        }
        if let Some(g) = &self.gfx {
            g.window.request_redraw();
        }
    }

    fn poll_dialog(&mut self) {
        while let Ok(reply) = self.dialog_rx.try_recv() {
            self.dialog_active = false;
            match reply {
                DialogReply::Open(Some(p)) => {
                    self.editor.open(&p);
                }
                DialogReply::Import(Some(p)) => {
                    self.editor.import(&p);
                }
                DialogReply::Save { path: Some(p), and_continue } => {
                    let p = if p.extension().is_none() { p.with_extension(oxy_doc::format::EXTENSION) } else { p };
                    let saved = self.editor.save(&p);
                    if and_continue {
                        if saved {
                            if let Some(pending) = self.editor.pending.take() {
                                self.editor.perform(pending);
                            }
                        } else {
                            self.editor.pending = None;
                        }
                    }
                }
                DialogReply::Save { path: None, and_continue } => {
                    if and_continue {
                        self.editor.pending = None;
                    }
                }
                DialogReply::SaveAs(Some(p)) => {
                    let p = if p.extension().is_none() { p.with_extension(oxy_doc::format::EXTENSION) } else { p };
                    self.editor.save(&p);
                }
                DialogReply::LoadMatcap(Some(p)) => {
                    let loaded = match &self.editor.matcap_dir {
                        Some(dir) => crate::matcaps::import(dir, &p),
                        None => crate::matcaps::decode(&p).map(|(w, h, rgba)| crate::matcaps::CustomMatcap {
                            name: p.file_stem().map_or("Matcap".into(), |s| s.to_string_lossy().into_owned()),
                            pixels: std::sync::Arc::new(crate::matcaps::to_matcap(w, h, &rgba)),
                        }),
                    };
                    match loaded {
                        Ok(m) => self.editor.add_custom_matcap(m),
                        Err(e) => self.editor.status = format!("Could not load {}: {e}", p.display()),
                    }
                }
                DialogReply::LoadAlpha(Some(p)) => match crate::support::load_alpha_png(&p) {
                    Ok(alpha) => self.editor.add_alpha(alpha),
                    Err(e) => self.editor.status = format!("Could not load {}: {e}", p.display()),
                },
                DialogReply::ImportBrush(Some(p)) => match crate::presets::read_file(&p) {
                    Ok(preset) => {
                        self.editor.apply_preset(&preset);
                        self.editor.add_preset(preset);
                    }
                    Err(e) => self.editor.status = format!("Could not import {}: {e}", p.display()),
                },
                DialogReply::ExportBrush { preset, path: Some(p) } => {
                    let ext = oxy_sculpt::preset::EXTENSION;
                    let p = if p.extension().is_none() { p.with_extension(ext) } else { p };
                    self.editor.status = match crate::presets::write_file(&preset, &p) {
                        Ok(()) => format!("Exported brush preset to {}", p.display()),
                        Err(e) => format!("Could not export: {e}"),
                    };
                }
                DialogReply::ImportKeymap(Some(p)) => {
                    self.editor.status = match std::fs::read_to_string(&p) {
                        Ok(text) => match keymap::Keymap::import_text(&text) {
                            Ok(map) => {
                                let n = map.conflicts().len() + map.nav_conflicts().len();
                                self.editor.keymap = map;
                                self.editor.keymap_capture = None;
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
                DialogReply::ExportKeymap(Some(p)) => {
                    let p = if p.extension().is_none() { p.with_extension(keymap::EXTENSION) } else { p };
                    self.editor.status = match self.editor.keymap.save(&p) {
                        Ok(()) => format!("Exported keymap to {}", p.display()),
                        Err(e) => format!("Could not export: {e}"),
                    };
                }
                DialogReply::Export(Some(p)) => {
                    self.editor.export(&p);
                }
                DialogReply::RunScript(Some(p)) => {
                    self.run_python_script(&p);
                }
                _ => {}
            }
            if let Some(g) = &self.gfx {
                g.window.request_redraw();
            }
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
    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        self.editor.force_autosave();
        self.editor.autosaver.finish();
    }

    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        if matches!(cause, StartCause::ResumeTimeReached { .. })
            && let Some(g) = &self.gfx
        {
            g.window.request_redraw();
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: ()) {
        // The agent thread woke us. Apply the call and draw it.
        self.poll_agent();
        self.poll_plugin();
        self.poll_dialog();
        if let Some(g) = &self.gfx {
            g.window.request_redraw();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.sync_agent();
        self.poll_agent();
        self.poll_plugin();
        self.poll_dialog();
        if self.shot_reply.is_some()
            && let Some(g) = &self.gfx
        {
            g.window.request_redraw();
        }
        // Save preferences once they settle, even if nothing redraws.
        self.persist_settings(false);
        // Wake up regularly to finish background jobs, pending settings,
        // autosave and a waiting agent even when the window is idle.
        let mut wait = if self.editor.job.is_some()
            || self.shot_reply.is_some()
            || self.editor.updater.busy()
            || self.plugin_process.is_some()
            || self.dialog_active
        {
            Duration::from_millis(50)
        } else if self.agent.is_some() {
            Duration::from_millis(200)
        } else if self.settings_changed_at.is_some() {
            Duration::from_millis(500)
        } else {
            Duration::from_secs(20)
        };
        if let Some(target) = self.egui_repaint_at {
            let now = Instant::now();
            if now >= target {
                if let Some(g) = &self.gfx {
                    g.window.request_redraw();
                }
                self.egui_repaint_at = None;
            } else {
                wait = wait.min(target.saturating_duration_since(now));
            }
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + wait));
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            self.sync_agent();
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
        self.sync_agent();
        if let Some(g) = &self.gfx {
            g.window.request_redraw();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if self.gfx.is_none() {
            return;
        }
        // Tab is OxyBrush's shortcut (show / hide panels) unless a text field
        // is being typed in. egui would otherwise use it to move keyboard
        // focus between controls: that scrolls the panels to the focused
        // control, and every later Tab goes to egui instead of the shortcut.
        if let WindowEvent::KeyboardInput { event: key, is_synthetic: false, .. } = &event
            && key.physical_key == PhysicalKey::Code(KeyCode::Tab)
            && !self.egui_ctx.text_edit_focused()
            && self.editor.keymap_capture.is_none()
        {
            if let Some(id) = self.egui_ctx.memory(|m| m.focused()) {
                self.egui_ctx.memory_mut(|m| m.surrender_focus(id));
            }
            if key.state == ElementState::Pressed
                && let Some(a) = self.handle_key(key)
            {
                self.run_action(a);
            }
            if let Some(g) = &self.gfx {
                g.window.request_redraw();
            }
            return;
        }
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
            WindowEvent::Focused(focused) => {
                if !*focused {
                    self.editor.modifiers = Modifiers::default();
                    self.editor.space_held = false;
                    if self.editor.is_stroking() {
                        self.editor.pointer_up();
                    }
                    self.editor.force_autosave();
                    self.editor.pointer = None;
                    self.editor.quick_menu = None;
                    self.editor.quick_menu_key = None;
                    self.mmb_scroll = None;
                    self.egui_ctx.memory_mut(|m| {
                        if let Some(id) = m.focused() {
                            m.surrender_focus(id);
                        }
                        m.stop_text_input();
                    });
                }
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
                if !self.editor.is_stroking() || self.editor.pressure <= 0.0 {
                    self.editor.pressure = 1.0;
                }
                self.editor.pointer_moved(Vec2::new(position.x as f32, position.y as f32));
                redraw = true;
            }
            WindowEvent::TouchpadPressure { pressure, .. } => {
                self.editor.pressure = *pressure;
                self.editor.pen_seen = true;
                redraw = true;
            }
            WindowEvent::PinchGesture { delta, .. } if !over_egui => {
                self.editor.scroll(*delta as f32 * 20.0);
                redraw = true;
            }
            WindowEvent::RotationGesture { delta, .. } if !over_egui => {
                self.editor.camera.orbit(Vec2::new(*delta * 2.0, 0.0));
                redraw = true;
            }
            WindowEvent::AxisMotion { value, .. } => {
                if self.editor.is_stroking() && *value > 0.0 && *value <= 1.0 {
                    self.editor.pressure = *value as f32;
                    self.editor.pen_seen = true;
                }
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
                let pos = Vec2::new(t.location.x as f32, t.location.y as f32);
                let force_pressure = t.force.map_or(1.0, |f| f.normalized() as f32);
                if t.force.is_some() {
                    self.editor.pen_seen = true;
                }
                match t.phase {
                    TouchPhase::Started => {
                        self.touches.insert(t.id, TouchPoint { start_pos: pos, pos, _start_time: Instant::now() });
                        if self.touches.len() == 1 {
                            self.editor.pressure = force_pressure;
                            self.editor.pointer = Some(pos);
                            if !over_egui && self.editor.pointer_in_canvas() {
                                self.editor.pointer_down(PointerButton::Primary);
                            }
                        } else if self.touches.len() == 2 {
                            if self.editor.is_stroking() {
                                self.editor.pointer_up();
                            }
                            self.touch_two_finger_start = Some(Instant::now());
                            let pts: Vec<Vec2> = self.touches.values().map(|tp| tp.pos).collect();
                            if pts.len() >= 2 {
                                self.touch_prev_dist = Some(pts[0].distance(pts[1]));
                                self.touch_prev_mid = Some((pts[0] + pts[1]) * 0.5);
                            }
                        }
                    }
                    TouchPhase::Moved => {
                        if let Some(tp) = self.touches.get_mut(&t.id) {
                            tp.pos = pos;
                        }
                        if self.touches.len() == 1 {
                            self.editor.pressure = force_pressure;
                            self.editor.pointer_moved(pos);
                        } else if self.touches.len() == 2 {
                            let pts: Vec<Vec2> = self.touches.values().map(|tp| tp.pos).collect();
                            if pts.len() >= 2 {
                                let d = pts[0].distance(pts[1]);
                                let m = (pts[0] + pts[1]) * 0.5;
                                if let (Some(prev_d), Some(prev_m)) = (self.touch_prev_dist, self.touch_prev_mid) {
                                    let zoom_delta = (d - prev_d) / 40.0;
                                    if zoom_delta.abs() > 0.01 {
                                        self.editor.scroll(zoom_delta * 4.0);
                                    }
                                    let pan_delta = m - prev_m;
                                    if pan_delta.length() > 0.5 {
                                        self.editor.camera.orbit(pan_delta * 0.5);
                                    }
                                }
                                self.touch_prev_dist = Some(d);
                                self.touch_prev_mid = Some(m);
                            }
                        }
                    }
                    TouchPhase::Ended | TouchPhase::Cancelled => {
                        if self.touches.len() == 2 && self.touch_two_finger_start.is_some() {
                            let start_time = self.touch_two_finger_start.take().unwrap();
                            let elapsed = start_time.elapsed();
                            let moved_far = self.touches.values().any(|tp| tp.start_pos.distance(tp.pos) > 15.0);
                            if elapsed.as_millis() <= 300 && !moved_far {
                                self.editor.undo();
                                self.editor.status = "Two-finger tap: Undo".into();
                            }
                        }
                        self.touches.remove(&t.id);
                        if self.touches.is_empty() {
                            self.touch_two_finger_start = None;
                            self.touch_prev_dist = None;
                            self.touch_prev_mid = None;
                            self.editor.pointer_up();
                        }
                    }
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
            WindowEvent::KeyboardInput { event, is_synthetic: false, .. } => {
                if let PhysicalKey::Code(KeyCode::Space) = event.physical_key {
                    self.editor.space_held = event.state == ElementState::Pressed;
                }
                let capture_or_quick = self.editor.keymap_capture.is_some()
                    || (event.state == ElementState::Released && self.editor.quick_menu.is_some_and(|q| q.held));
                if capture_or_quick || !self.egui_ctx.egui_wants_keyboard_input() {
                    if let Some(a) = self.handle_key(event) {
                        self.run_action(a);
                    }
                    redraw = true;
                }
            }
            _ => {}
        }
        if redraw && let Some(g) = &self.gfx {
            g.window.request_redraw();
        }
    }
}

/// A window icon from one of the generated PNGs (`None` if it can't be
/// decoded, which only leaves the platform's default icon).
fn window_icon(png: &[u8]) -> Option<winit::window::Icon> {
    let (w, h, rgba) = crate::matcaps::decode_png(png).ok()?;
    winit::window::Icon::from_rgba(rgba, w, h).ok()
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

#[cfg(test)]
mod tests {
    #[test]
    fn window_icons_decode() {
        for png in [
            &include_bytes!("../../../packaging/icons/oxybrush-32.png")[..],
            &include_bytes!("../../../packaging/icons/oxybrush-256.png")[..],
        ] {
            assert!(super::window_icon(png).is_some());
        }
    }
}

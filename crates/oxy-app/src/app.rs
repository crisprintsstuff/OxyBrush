//! Window, GPU and event-loop glue.

use crate::editor::{Editor, ExportFormat, Modifiers, PointerButton};
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
    (crate::settings::ThemePreset, crate::settings::ThemeMix),
    crate::settings::AccentPreset,
    [u8; 3],
    crate::settings::CornerStyle,
    crate::settings::SliderSize,
    Option<[u8; 3]>,
    bool,
    f32,
    crate::settings::UiLayout,
    Option<[u8; 3]>,
    u32,
    u32,
);

pub struct App {
    gfx: Option<Gfx>,
    egui_ctx: egui::Context,
    editor: Editor,
    /// Material last uploaded: a built-in preset, or a custom matcap's name.
    applied_matcap: Option<(MatcapPreset, Option<String>)>,
    /// Material keys in the renderer's piece material slots (slot `i + 1`),
    /// so a slot is only uploaded again when what it holds changes.
    material_slots: Vec<Option<String>>,
    /// The generation of each surface-instance buffer last sent to the
    /// renderer, so transforms are uploaded only when they change.
    instance_uploaded: std::collections::HashMap<u64, u64>,
    fur_preview: crate::fur::Preview,
    /// Middle button held over a panel: last pointer position, for drag-scrolling.
    mmb_scroll: Option<Vec2>,
    /// Latest pressure event from platforms that deliver the pen as a mouse.
    last_pressure: Option<(Instant, f32)>,
    /// A pen press on a panel (RFC 0015): it scrolls the panel once it moves,
    /// or opens the control's menu once held still.
    pen_press: Option<PenPress>,
    applied_theme: Option<AppliedThemeKey>,
    applied_title: String,
    /// The window was just handed to the system to move or resize (own
    /// title bar, RFC 0028). The system keeps the button release, so egui
    /// is told the button went up on the next frame.
    release_after_window_drag: bool,
    /// The macOS menu bar (RFC 0028).
    #[cfg(target_os = "macos")]
    native_menu: Option<crate::native_menu::NativeMenu>,
    last_frame: Instant,
    initial_files: Vec<PathBuf>,
    /// Saved project tabs from the previous session, to open on `resumed`.
    pending_session: Option<(Vec<PathBuf>, usize)>,
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
    pub(crate) bridge_inbox: Option<crate::bridge::Inbox>,
    /// Local MCP listener. None unless the user opted in.
    agent: Option<crate::agent::Inbox>,
    /// Screenshot tool waiting for this frame's viewport.
    shot_reply: Option<std::sync::mpsc::Sender<oxy_mcp::ToolResult>>,
    screenshot_request: Option<crate::snapshot::Request>,
    screenshot_result: Option<std::sync::mpsc::Receiver<String>>,
    /// Running Python plugin process.
    plugin_process: Option<crate::plugins::PluginProcess>,
    /// The running Python plugin's own MCP listener (not the agent's).
    plugin_inbox: Option<crate::agent::Inbox>,
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
    /// Files from one drop. winit reports each path as its own event;
    /// they are imported together once the event loop is idle.
    pending_drops: Vec<PathBuf>,
}

#[derive(Debug)]
enum DialogReply {
    Open(Option<PathBuf>),
    Import(Option<Vec<PathBuf>>),
    Save { path: Option<PathBuf>, and_continue: bool },
    SaveAs(Option<PathBuf>),
    LoadMatcap(Option<PathBuf>),
    LoadReference { doc_id: u64, imported: Option<Result<oxy_doc::ReferenceImage, String>> },
    LoadAlpha { doc_id: u64, imported: Option<Result<crate::alpha_library::Imported, String>> },
    ExportAlpha(Option<PathBuf>),
    ImportBrush(Option<PathBuf>),
    ExportBrush { preset: oxy_sculpt::BrushPreset, path: Option<PathBuf> },
    ImportKeymap(Option<PathBuf>),
    ExportKeymap(Option<PathBuf>),
    Export(Option<PathBuf>),
    ExportScreenshot { options: crate::snapshot::Options, path: Option<PathBuf> },
    RunScript(Option<PathBuf>),
    InstallAddon(Option<PathBuf>),
}

#[derive(Debug, Clone, Copy)]
struct TouchPoint {
    start_pos: Vec2,
    pos: Vec2,
    _start_time: Instant,
    force: Option<f32>,
}

impl TouchPoint {
    /// Some pen drivers omit force in individual movement events. Keep
    /// the last measured force for this contact, including a valid zero.
    fn pressure(&mut self, force: Option<f32>) -> f32 {
        self.force = force.or(self.force);
        self.force.unwrap_or(1.0)
    }
}

/// A paired pressure event arrives shortly before the button press. A
/// regular mouse press after the pen is put away starts at full opacity.
fn pointer_start_pressure(last: Option<(Instant, f32)>, now: Instant) -> f32 {
    last.filter(|(at, _)| now.saturating_duration_since(*at) <= Duration::from_millis(150))
        .map_or(1.0, |(_, pressure)| pressure)
}

fn valid_pressure(pressure: f32) -> Option<f32> {
    pressure.is_finite().then(|| pressure.clamp(0.0, 1.0))
}

impl App {
    pub fn new(initial_files: Vec<PathBuf>, auto: AutoRun, log_path: Option<PathBuf>) -> Self {
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
        let pending_session = if settings.restore_session
            && !settings.session_files.is_empty()
            && initial_files.is_empty()
            && auto.screenshot.is_none()
            && !auto.demo
            && auto.open_keymap.is_none()
        {
            Some((settings.session_files.clone(), settings.session_active))
        } else {
            None
        };
        Self {
            gfx: None,
            egui_ctx: {
                let ctx = egui::Context::default();
                // Ctrl+plus/minus/0 are OxyBrush's view zoom and frame keys;
                // the interface size is View > Theme > Size, not a key.
                ctx.options_mut(|o| o.zoom_with_keyboard = false);
                ctx
            },
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
                    if let Some(dir) = crate::alpha_library::library_dir() {
                        e.open_alpha_library(dir);
                    }
                    e.parts_dir = crate::parts::library_dir();
                    if let Some(dir) = &e.parts_dir {
                        e.parts_library = crate::parts::PartsLibrary::load_all(dir);
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
                    !initial_files.is_empty() || auto.demo || pending_session.is_some(),
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
            material_slots: vec![None; oxy_render::MATERIAL_SLOTS as usize],
            instance_uploaded: Default::default(),
            fur_preview: Default::default(),
            mmb_scroll: None,
            last_pressure: None,
            pen_press: None,
            applied_theme: None,
            applied_title: String::new(),
            release_after_window_drag: false,
            #[cfg(target_os = "macos")]
            native_menu: None,
            last_frame: Instant::now(),
            initial_files,
            pending_session,
            auto,
            frames: 0,
            saved_settings: settings,
            settings_path,
            keymap_path,
            saved_keymap,
            // Write the file on the first start so the welcome window isn't shown again.
            settings_changed_at: first_start.then(Instant::now),
            proxy: None,
            bridge_inbox: None,
            agent: None,
            shot_reply: None,
            screenshot_request: None,
            screenshot_result: None,
            plugin_process: None,
            plugin_inbox: None,
            egui_repaint_at: None,
            touches: std::collections::HashMap::new(),
            touch_two_finger_start: None,
            touch_prev_dist: None,
            touch_prev_mid: None,
            dialog_rx,
            dialog_tx,
            dialog_active: false,
            pending_drops: Vec::new(),
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
        crate::agent::poll_inboxes(
            &mut self.editor,
            self.agent.as_mut(),
            self.plugin_inbox.as_mut(),
            &mut self.shot_reply,
        );
    }

    /// Check if a background Python plugin process has completed.
    fn poll_plugin(&mut self) {
        let Some(proc) = &mut self.plugin_process else { return };
        match proc.rx.try_recv() {
            Ok(Ok(stdout)) => {
                let name = proc.name.clone();
                self.plugin_process = None;
                self.plugin_inbox = None;
                self.editor.plugin_running = None;
                self.editor.plugin_running_doc = None;
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
                self.plugin_inbox = None;
                self.editor.plugin_running = None;
                self.editor.plugin_running_doc = None;
                // For Python exceptions and tracebacks, the meaningful diagnostic
                // (e.g. `AttributeError: ...`) is on the last non-empty line of stderr.
                let err_msg = stderr.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("failed").trim();
                self.editor.status = format!("Plugin '{name}' failed: {err_msg}");
                log::warn!("Plugin '{name}' failed:\n{stderr}");
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.plugin_process = None;
                self.plugin_inbox = None;
                self.editor.plugin_running = None;
                self.editor.plugin_running_doc = None;
            }
        }
    }

    /// Run a Python plugin. It talks to OxyBrush over MCP like an agent,
    /// but on a listener of its own, opened for this run and closed when it
    /// ends: running a plugin doesn't turn on Settings > Local AI agent,
    /// the status bar names the plugin, and its edits are labelled with it.
    fn run_python_script(&mut self, path: &std::path::Path) {
        if self.plugin_process.is_some() {
            self.editor.status = "Another plugin is already running".into();
            return;
        }
        let Some(proxy) = self.proxy.clone() else { return };
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            let _ = proxy.send_event(());
        });
        let name = crate::plugins::plugin_name(path);
        let doc_id = self.editor.documents[self.editor.active_document].id;
        let inbox = match crate::agent::Inbox::for_plugin(&name, doc_id, Arc::clone(&wake)) {
            Ok(inbox) => inbox,
            Err(e) => {
                self.editor.status = format!("Could not start plugin '{name}': {e}");
                return;
            }
        };
        match crate::plugins::run_plugin(path, &inbox.endpoint, &inbox.token(), wake) {
            Ok(proc) => {
                // The status bar shows "Plugin '…' running" until it ends.
                self.editor.status.clear();
                self.editor.plugin_running = Some(proc.name.clone());
                self.editor.plugin_running_doc = Some(doc_id);
                self.plugin_process = Some(proc);
                self.plugin_inbox = Some(inbox);
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
        // The window as it was last time (issue #95), except in automated
        // runs, which keep the fixed default size.
        let saved = self.editor.window_geometry.filter(|_| self.auto.screenshot.is_none());
        let (w, h) = saved.map_or((1440.0, 900.0), |g| (g.width, g.height));
        let mut attrs = Window::default_attributes()
            .with_title("OxyBrush")
            .with_inner_size(winit::dpi::LogicalSize::new(w, h))
            .with_maximized(saved.is_some_and(|g| g.maximized));
        // Reuse the position only if the title bar would be on a monitor
        // that's connected now, so a window last on an unplugged screen
        // doesn't open out of reach.
        if let Some((x, y)) = saved.and_then(|g| g.position) {
            let monitors: Vec<crate::settings::MonitorRect> = event_loop
                .available_monitors()
                .map(|m| ((m.position().x, m.position().y), (m.size().width, m.size().height)))
                .collect();
            let scale = event_loop.primary_monitor().map_or(1.0, |m| m.scale_factor());
            let size = ((w as f64 * scale) as u32, (h as f64 * scale) as u32);
            if crate::settings::WindowGeometry::top_is_visible((x, y), size, &monitors) {
                attrs = attrs.with_position(winit::dpi::PhysicalPosition::new(x, y));
            }
        }
        let (min_w, min_h) = crate::settings::WindowGeometry::MIN;
        // OxyBrush's own title bar (RFC 0028) on Windows and Linux, unless
        // the preference asks for the system's. macOS keeps its own for now.
        let own_title_bar = cfg!(not(target_os = "macos")) && !self.editor.system_title_bar;
        self.editor.own_title_bar = own_title_bar;
        let attrs = attrs
            .with_decorations(!own_title_bar)
            .with_min_inner_size(winit::dpi::LogicalSize::new(min_w, min_h))
            // The title bar / window list icon. Without it, Windows shows a
            // generic icon there even though the exe has one.
            .with_window_icon(window_icon(include_bytes!("../../../packaging/icons/oxybrush-32.png")));
        // The taskbar and Alt+Tab use the large icon.
        #[cfg(target_os = "windows")]
        let attrs = winit::platform::windows::WindowAttributesExtWindows::with_taskbar_icon(
            attrs,
            window_icon(include_bytes!("../../../packaging/icons/oxybrush-256.png")),
        );
        // Keep the drop shadow a window without the system frame would lose.
        #[cfg(target_os = "windows")]
        let attrs = winit::platform::windows::WindowAttributesExtWindows::with_undecorated_shadow(attrs, true);
        let window = Arc::new(event_loop.create_window(attrs).map_err(|e| e.to_string())?);
        // The menus go in the macOS menu bar; a choice wakes the event loop.
        #[cfg(target_os = "macos")]
        if self.native_menu.is_none() {
            let proxy = self.proxy.clone();
            self.native_menu = Some(crate::native_menu::NativeMenu::install(&self.editor, move || {
                if let Some(p) = &proxy {
                    let _ = p.send_event(());
                }
            }));
            self.editor.native_menu_bar = true;
        }

        let select_adapter = |backends| {
            let mut desc =
                wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(event_loop.owned_display_handle()));
            if let Some(backends) = backends {
                desc.backends = backends;
            }
            let instance = wgpu::Instance::new(desc);
            let surface = instance.create_surface(window.clone()).map_err(|e| e.to_string())?;
            let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            }))
            .map_err(|e| format!("no compatible GPU adapter: {e}"))?;
            Ok::<_, String>((surface, adapter))
        };
        let (surface, adapter) = select_adapter(None)?;
        let candidate = adapter.get_info();
        let (surface, adapter) = if cfg!(target_os = "windows")
            && needs_intel_dx12(
                candidate.backend,
                candidate.vendor,
                candidate.device,
                std::env::var_os("WGPU_BACKEND").is_some(),
            ) {
            log::warn!("Using Direct3D 12 for Intel UHD Graphics 600 to avoid a reported Vulkan startup crash");
            drop(surface);
            drop(adapter);
            select_adapter(Some(wgpu::Backends::DX12))
                .map_err(|e| format!("could not start in Direct3D 12 compatibility mode: {e}"))?
        } else {
            (surface, adapter)
        };
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
        // A pen held still on a panel: a right-click there, which opens the
        // control's menu, and pins its tooltip (RFC 0015).
        if let Some(press) = &mut self.pen_press
            && press.held(now)
            && let Some(p) = self.editor.pointer
        {
            let at = p / self.editor.pixels_per_point.max(0.1);
            let pos = egui::pos2(at.x, at.y);
            let input = g.egui_state.egui_input_mut();
            for pressed in [true, false] {
                input.events.push(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Secondary,
                    pressed,
                    modifiers: Default::default(),
                });
            }
            crate::tips::mark_pen_hold(&self.egui_ctx);
        }
        let dt = now.duration_since(self.last_frame).as_secs_f32() * 1000.0;
        self.last_frame = now;
        self.editor.frame_ms = self.editor.frame_ms * 0.9 + dt * 0.1;

        let theme_key = (
            (self.editor.theme, self.editor.theme_mix),
            self.editor.accent,
            self.editor.custom_accent,
            self.editor.corner_style,
            self.editor.slider_size,
            self.editor.button_color,
            self.editor.high_contrast,
            self.editor.ui_scale,
            self.editor.layout,
            self.editor.ui_tint_color,
            self.editor.ui_tint_intensity.to_bits(),
            self.editor.ui_brightness.to_bits(),
        );
        if self.applied_theme != Some(theme_key) {
            ui::apply_theme(
                &self.egui_ctx,
                theme_key.0.0,
                theme_key.0.1,
                theme_key.1,
                theme_key.2,
                theme_key.3,
                theme_key.4,
                theme_key.5,
                theme_key.6,
                theme_key.7,
                theme_key.8,
                theme_key.9,
                f32::from_bits(theme_key.10),
                f32::from_bits(theme_key.11),
            );
            self.applied_theme = Some(theme_key);
        }

        // A redraw can arrive after the window changed size but before its
        // resize event. Use one size for the surface and egui's layout, and
        // avoid waiting for the GPU on every intermediate resize event.
        let size = g.window.inner_size();
        let (frame_size, screen_rect) = frame_dimensions(size, egui_winit::pixels_per_point(&self.egui_ctx, &g.window));
        if [g.config.width, g.config.height] != frame_size {
            g.config.width = frame_size[0];
            g.config.height = frame_size[1];
            g.surface.configure(&g.device, &g.config);
        }
        // What was chosen in the macOS menu bar since the last frame.
        #[cfg(target_os = "macos")]
        let menu_action = match self.native_menu.as_mut() {
            Some(m) => {
                // The in-app plugins load the first time a Plugins menu is
                // drawn; the menu bar's isn't drawn by egui, so load them
                // here, or its Plugins menu stays empty until Settings >
                // Plugins is opened (#448).
                if !self.editor.wasm_loaded {
                    self.editor.load_wasm_plugins();
                }
                let a = m.run_chosen(&mut self.editor, &self.egui_ctx);
                m.refresh(&self.editor);
                a
            }
            None => None,
        };
        #[cfg(not(target_os = "macos"))]
        let menu_action: Option<UiAction> = None;
        let mut raw = g.egui_state.take_egui_input(&g.window);
        if std::mem::take(&mut self.release_after_window_drag) {
            let (pos, modifiers) = self.egui_ctx.input(|i| (i.pointer.latest_pos().unwrap_or_default(), i.modifiers));
            raw.events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers,
            });
        }
        self.editor.window_maximized = g.window.is_maximized();
        raw.screen_rect = screen_rect;
        let mut action = None;
        let mut canvas = egui::Rect::NOTHING;
        let ed = &mut self.editor;
        let mut out = self.egui_ctx.run_ui(raw, |ui| {
            (action, canvas) = ui::draw(ui, ed);
        });
        if let Some(request) = ed.window_request.take() {
            use crate::title_bar::WindowRequest as W;
            match request {
                W::Drag => {
                    if let Err(e) = g.window.drag_window() {
                        log::warn!("could not move the window: {e}");
                    }
                    self.release_after_window_drag = true;
                }
                W::Resize(edge) => {
                    if let Err(e) = g.window.drag_resize_window(edge.to_winit()) {
                        log::warn!("could not resize the window: {e}");
                    }
                    self.release_after_window_drag = true;
                }
                W::Minimize => g.window.set_minimized(true),
                W::ToggleMaximize => g.window.set_maximized(!g.window.is_maximized()),
                // As the system close button does: ask about unsaved tabs.
                W::Close => ed.request(crate::editor::Pending::Quit),
                W::Menu(pos) => g.window.show_window_menu(winit::dpi::LogicalPosition::new(pos.x, pos.y)),
            }
        }
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
        ed.canvas_full = [canvas.min.x * ppp, canvas.min.y * ppp, canvas.width() * ppp, canvas.height() * ppp];
        ed.apply_pane_canvas();

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
        ed.deform_poll();
        ed.sync_model_radius();
        ed.living_sync();
        ed.sync_links();
        ed.sync_brush_look();
        ed.sync_lighter();
        ed.enforce_shared_undo_cap();
        ed.watch_uvs();
        ed.tick_gizmo_session();
        ed.tick_autosave();
        ed.updater.tick(crate::updates::now_secs());
        // Agent calls run after the user's finished job and before the mesh is uploaded.
        crate::agent::poll_inboxes(ed, self.agent.as_mut(), self.plugin_inbox.as_mut(), &mut self.shot_reply);
        let title = ed.title();
        if self.applied_title != title {
            g.window.set_title(&title);
            self.applied_title = title;
        }
        // The Insert Primitive and mask extract previews are drawn like a
        // piece under a reserved id.
        const PREVIEW_ID: u64 = u64::MAX;
        const MIX_ID: u64 = u64::MAX - 1;
        // Lowest-level copies of pieces (Lighter inactive pieces) are drawn
        // under their piece's id with this bit set.
        const PROXY_FLAG: u64 = 1 << 62;
        ed.update_extract_preview();
        ed.tick_hold(Instant::now());
        ed.update_curve();
        ed.update_tube();
        ed.update_spheres();
        ed.update_primitive();
        ed.update_poly_smooth();
        ed.check_link_topology();
        ed.oxymorph_refresh_comparison();
        let compare = ed.oxymorph_comparison().map(|(_, pair)| pair);
        let shown = |p: &oxy_doc::Piece| compare.map_or(p.visible, |pair| pair.contains(&p.id));
        let ids: Vec<u64> = ed.scene.pieces.iter().filter(|p| shown(p)).map(|p| p.id).collect();
        // A hidden source still supplies the geometry of copies on a shown host.
        let sources: Vec<u64> = if compare.is_some() {
            Vec::new()
        } else {
            ed.scene.pieces.iter().filter(|p| p.visible).flat_map(|p| p.instancers.iter().map(|i| i.source)).collect()
        };
        let previewing = ed.insert_preview.is_some()
            || ed.extract_preview.is_some()
            || ed.curve.is_some()
            || ed.tube.is_some()
            || ed.spheres.is_some()
            || ed.primitive_edit.is_some()
            || ed.preview_in_place().is_some();
        // A curve stroke's preview is the whole piece with the stroke
        // applied: draw it in the piece's place, looking like the piece.
        let curve_piece = ed.preview_in_place();
        let morph_pair = ed.oxymorph_preview_pair();
        let morph_map = morph_pair.is_some() && ed.oxymorph_show_map();
        let single_mix = compare.is_none() && morph_pair.is_some();
        // While another piece is edited, pieces with levels are drawn at
        // their lowest level (Lighter inactive pieces), from a copy kept on
        // the GPU under its own id next to the full mesh, so switching back
        // at the end of a stroke costs nothing.
        let proxied: Vec<u64> = if compare.is_some() {
            Vec::new()
        } else {
            ids.iter().copied().filter(|&id| ed.lighter_proxy_mut(id).is_some()).collect()
        };
        let proxy_id = |id: u64| id | PROXY_FLAG;
        let kept_proxies: Vec<u64> = ed.documents[ed.active_document].proxies.keys().map(|&id| proxy_id(id)).collect();
        g.viewport.retain(|id| {
            ids.contains(&id)
                || sources.contains(&id)
                || (previewing && !single_mix && id == PREVIEW_ID)
                || (morph_pair.is_some() && id == MIX_ID)
                || (compare.is_some() && ed.scene.get(id).is_some())
                || kept_proxies.contains(&id)
                || crate::fur::strand_host(id).is_some_and(|host| ed.scene.get(host).is_some_and(|p| p.fur.is_some()))
        });
        let active = ed.scene.try_active().map(|p| p.id).unwrap_or(0);
        // Pieces with their own material: give each distinct one a slot.
        let piece_key = |p: &oxy_doc::Piece| -> Option<String> {
            if p.id == active && ed.preview_piece_material.is_some() {
                return ed.preview_piece_material.clone();
            }
            p.material.clone()
        };
        let mut wanted: Vec<String> = Vec::new();
        for p in ed.scene.pieces.iter().filter(|p| shown(p) || Some(p.id) == curve_piece || sources.contains(&p.id)) {
            if let Some(k) = piece_key(p)
                && ed.resolve_material(&k).is_some()
                && !wanted.contains(&k)
                && wanted.len() < oxy_render::MATERIAL_SLOTS as usize
            {
                wanted.push(k);
            }
        }
        for k in &wanted {
            if self.material_slots.iter().any(|s| s.as_ref() == Some(k)) {
                continue;
            }
            // A slot nothing wanted uses now.
            let Some(i) = self.material_slots.iter().position(|s| s.as_ref().is_none_or(|s| !wanted.contains(s)))
            else {
                break;
            };
            let pixels = match ed.resolve_material(k) {
                Some(Ok(m)) => oxy_render::matcap::generate(m, oxy_render::MATCAP_SIZE),
                Some(Err(px)) => px.to_vec(),
                None => continue,
            };
            g.viewport.set_material_slot(&g.queue, i as u32 + 1, &pixels);
            self.material_slots[i] = Some(k.clone());
        }
        let slots = &self.material_slots;
        let slot_map: std::collections::HashMap<u64, u32> = ed
            .scene
            .pieces
            .iter()
            .filter_map(|p| {
                let k = piece_key(p)?;
                let i = slots.iter().position(|s| s.as_ref() == Some(&k))?;
                Some((p.id, i as u32 + 1))
            })
            .collect();
        let slot_of = |id: u64| slot_map.get(&id).copied().unwrap_or(0);
        let glass: std::collections::HashMap<_, _> = ed
            .scene
            .pieces
            .iter()
            .filter_map(|p| {
                let key = if p.id == active {
                    ed.preview_piece_material.as_deref().or(p.material.as_deref())
                } else {
                    p.material.as_deref()
                };
                Some((p.id, oxy_render::GlassMaterial::from_key(key?)?))
            })
            .collect();
        let hard_edge_angle = ed.view.hard_edge_angle;
        let mut items = Vec::new();
        let mut compare_items = [None; 3];
        // The retopology tool draws its piece as an overlay, on top of the surface.
        let retopo_piece = ed.retopo.as_ref().and_then(|r| r.cage_piece.filter(|&id| id != r.surface));
        // Poly Edit shows the piece's polygon edges as a depth-tested
        // wireframe, so the surface in front hides the edges behind it.
        let poly_wire = ed.poly_wire_piece();
        let all_wires = ed.view.wireframe && ed.view.wireframe_opacity > 0.001;
        for p in ed.scene.pieces.iter_mut() {
            if !shown(p) && !sources.contains(&p.id) {
                continue;
            }
            let piece_wire = shown(p)
                && (all_wires || Some(p.id) == poly_wire)
                && p.mesh.triangle_count() <= oxy_render::MAX_WIREFRAME_TRIANGLES;
            g.viewport.sync(&g.device, &g.queue, p.id, &mut p.mesh, hard_edge_angle, piece_wire);
            if let Some([a, _]) = compare {
                compare_items[if p.id == a { 0 } else { 2 }] = Some(oxy_render::DrawItem {
                    id: p.id,
                    active: true,
                    preview: false,
                    material: slot_of(p.id),
                    instances: None,
                    wireframe: false,
                    color_map: false,
                    glass: glass.get(&p.id).copied(),
                });
            } else if p.visible
                && Some(p.id) != curve_piece
                && Some(p.id) != retopo_piece
                && !morph_pair.is_some_and(|pair| pair.contains(&p.id))
            {
                let material = slot_of(p.id);
                let shown = if proxied.contains(&p.id) { proxy_id(p.id) } else { p.id };
                items.push(oxy_render::DrawItem {
                    id: shown,
                    active: p.id == active,
                    preview: false,
                    material,
                    instances: None,
                    wireframe: Some(p.id) == poly_wire,
                    color_map: false,
                    glass: glass.get(&p.id).copied(),
                });
            }
        }
        for &id in &proxied {
            if let Some(mesh) = ed.lighter_proxy_mut(id) {
                g.viewport.sync(&g.device, &g.queue, proxy_id(id), mesh, hard_edge_angle, false);
            }
        }
        // Surface instances: the source's mesh, drawn once per copy. The
        // transforms are uploaded only when they change.
        let mut draws = if compare.is_some() { Vec::new() } else { ed.instance_draws() };
        let hosts: Vec<_> = ed
            .scene
            .pieces
            .iter()
            .filter(|p| {
                compare.is_none()
                    && p.fur.is_some()
                    && p.visible
                    && p.multires.is_none()
                    && Some(p.id) != curve_piece
                    && Some(p.id) != retopo_piece
                    && !morph_pair.is_some_and(|pair| pair.contains(&p.id))
            })
            .collect();
        let fur_draws = self.fur_preview.draws(&hosts);
        for draw in &fur_draws {
            g.viewport.sync(&g.device, &g.queue, draw.source, self.fur_preview.strand_mut(), hard_edge_angle, false);
        }
        draws.extend(fur_draws);
        let scatter_pending = ed.instance_scatter_pending();
        for d in &draws {
            if morph_pair.is_some_and(|pair| pair.contains(&d.host)) {
                continue;
            }
            if self.instance_uploaded.get(&d.key) != Some(&d.generation) {
                g.viewport.set_instances(&g.device, &g.queue, d.key, &d.transforms);
                self.instance_uploaded.insert(d.key, d.generation);
            }
            items.push(oxy_render::DrawItem {
                id: d.source,
                active: d.host == active,
                preview: false,
                material: slot_of(crate::fur::strand_host(d.source).unwrap_or(d.source)),
                instances: Some(d.key),
                wireframe: false,
                color_map: false,
                glass: glass.get(&d.source).copied(),
            });
        }
        if compare.is_none() {
            self.instance_uploaded.retain(|k, _| draws.iter().any(|d| d.key == *k));
            g.viewport.retain_instances(|k| draws.iter().any(|d| d.key == k));
        }
        // Add Primitive / Insert Primitive previews always show edges so
        // divisions are visible while adjusting (#568). Other previews follow
        // the global wireframe toggle. Compute before borrowing the mesh.
        let force_preview_wire = ed.preview_forces_wireframe();
        if let Some([a, _]) = compare
            && let Some(preview) = ed.oxymorph_preview_mesh_mut()
        {
            let wire = all_wires && preview.triangle_count() <= oxy_render::MAX_WIREFRAME_TRIANGLES;
            g.viewport.sync(&g.device, &g.queue, MIX_ID, preview, hard_edge_angle, wire);
            compare_items[1] = Some(oxy_render::DrawItem {
                id: MIX_ID,
                active: true,
                preview: false,
                material: slot_of(a),
                instances: None,
                wireframe: false,
                color_map: morph_map,
                glass: glass.get(&a).copied(),
            });
        }
        if let Some(preview) = ed.preview_mesh_mut() {
            let preview_id = if single_mix { MIX_ID } else { PREVIEW_ID };
            let preview_wire =
                (all_wires || force_preview_wire) && preview.triangle_count() <= oxy_render::MAX_WIREFRAME_TRIANGLES;
            g.viewport.sync(&g.device, &g.queue, preview_id, preview, hard_edge_angle, preview_wire);
            // A piece shown in place (a curve stroke) keeps its material.
            let material = curve_piece.map_or(0, slot_of);
            let item = oxy_render::DrawItem {
                id: preview_id,
                active: true,
                preview: curve_piece.is_none(),
                material,
                instances: None,
                wireframe: force_preview_wire,
                color_map: single_mix && morph_map,
                glass: curve_piece.and_then(|id| glass.get(&id).copied()),
            };
            if let Some([a, b]) = compare {
                if curve_piece == Some(a) {
                    compare_items[0] = Some(item);
                } else if curve_piece == Some(b) {
                    compare_items[2] = Some(item);
                }
            } else {
                items.push(item);
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
        let mut view_settings = oxy_render::ViewSettings {
            scene_bounds: {
                let b = ed.oxymorph_compare_bounds().unwrap_or_else(|| ed.scene.bounds());
                (!b.is_empty()).then(|| (b.min.to_array(), b.max.to_array()))
            },
            ..ed.view
        };
        let pane_cams = ed.pane_cameras();
        let compare_floor = (ed.floor_grid && compare.is_some()).then(|| ed.floor(&ed.camera));
        let pane_rects = if compare.is_some() {
            crate::oxymorph::ComparePane::ALL.map(|pane| pane.rect(ed.canvas_full)).to_vec()
        } else if ed.split.is_split() {
            crate::split_view::pane_rects(ed.split, ed.canvas_full, ed.split_ratio).to_vec()
        } else {
            vec![ed.canvas]
        };
        for (i, rect) in pane_rects.iter().enumerate() {
            if i > 0 {
                // Submit before updating the shared viewport uniforms for the next camera.
                g.queue.submit([encoder.finish()]);
                encoder =
                    g.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("split pane") });
            }
            let (camera, draw_items) = if compare.is_some() {
                (&ed.camera, compare_items[i].as_slice())
            } else {
                (pane_cams[i], items.as_slice())
            };
            view_settings.floor = compare_floor.unwrap_or_else(|| ed.floor_grid.then(|| ed.floor(camera)).flatten());
            g.viewport.set_reference_planes(
                &g.device,
                &g.queue,
                crate::references::planes(&self.egui_ctx, ed, camera, *rect),
            );
            g.viewport.render(
                &g.device,
                &g.queue,
                &mut encoder,
                &oxy_render::RenderTarget {
                    view: &view,
                    size: (g.config.width, g.config.height),
                    viewport: *rect,
                    clear: i == 0,
                },
                camera,
                &view_settings,
                draw_items,
            );
        }
        use crate::snapshot::{self, Area, Format};
        let export = self.screenshot_request.take();
        let export_area = export.as_ref().map(|request| request.options.area);
        let mut export_pixels = None;
        if export_area == Some(Area::Viewport) {
            let crop = snapshot::viewport_crop(ed.canvas_full);
            export_pixels = snapshot::copy_to_buffer(&g.device, &mut encoder, &frame.texture, Some(crop));
        }
        // The agent receives the sculpt canvas before egui draws.
        if self.shot_reply.is_some() {
            let crop = snapshot::viewport_crop(ed.canvas_full);
            let copied = snapshot::copy_to_buffer(&g.device, &mut encoder, &frame.texture, Some(crop));
            match copied {
                Some(pending) => {
                    g.queue.submit([encoder.finish()]);
                    let (width, height) = pending.dimensions();
                    let png = pending.encode(&g.device, Format::Png);
                    if let Some(reply) = self.shot_reply.take() {
                        let result = match png {
                            Ok(bytes) => oxy_mcp::ToolResult::with_image(
                                format!(
                                    "viewport {width}x{height} pixels. Screen-space stroke points use these pixels, origin at the top-left of the viewport."
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
        let shot = want_shot.then(|| snapshot::copy_to_buffer(&g.device, &mut encoder, &frame.texture, None));
        if export_area == Some(Area::Window) {
            export_pixels = snapshot::copy_to_buffer(&g.device, &mut encoder, &frame.texture, None);
        }
        g.queue.submit(extra.into_iter().chain([encoder.finish()]));
        if let (Some(shot), Some(path)) = (shot.flatten(), &self.auto.screenshot) {
            match shot.save(&g.device, path, Format::Png) {
                Ok(()) => log::info!("wrote screenshot {}", path.display()),
                Err(e) => log::error!("screenshot failed: {e}"),
            }
            event_loop.exit();
        } else if want_shot {
            log::error!("surface does not support COPY_SRC; cannot take screenshot");
            event_loop.exit();
        }
        if let Some(request) = export {
            if let Some(pixels) = export_pixels {
                let (width, height) = pixels.dimensions();
                let (tx, rx) = std::sync::mpsc::channel();
                self.screenshot_result = Some(rx);
                let device = g.device.clone();
                let proxy = self.proxy.clone();
                std::thread::spawn(move || {
                    let path = &request.path;
                    let status = match pixels.save(&device, path, request.options.format) {
                        Ok(()) => format!("Saved screenshot ({width} × {height}) to {}", path.display()),
                        Err(e) => format!("Could not save screenshot to {}: {e}", path.display()),
                    };
                    let _ = tx.send(status);
                    if let Some(proxy) = proxy {
                        let _ = proxy.send_event(());
                    }
                });
            } else {
                ed.status = "This graphics surface does not support screenshots".into();
                g.window.request_redraw();
            }
        }
        g.window.pre_present_notify();
        g.queue.present(frame);
        for id in &to_free {
            g.egui_renderer.free_texture(id);
        }

        let repaint_delay = out
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map(|v| v.repaint_delay)
            .into_iter()
            .chain(scatter_pending.then_some(Duration::from_millis(30)))
            .min();
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

        if let Some(a) = action.or(menu_action) {
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
        if self.editor.quit_now
            && let Some(g) = &self.gfx
        {
            // Where the window is now, in case no move event said so.
            remember_window(&mut self.editor, &g.window);
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

    /// Several files from one dialog. `pick_files` is rfd's multi-select,
    /// so Shift-click and Ctrl/Cmd-click work in the native panel.
    fn spawn_files<F>(&mut self, future: F, map: impl FnOnce(Option<Vec<PathBuf>>) -> DialogReply + Send + 'static)
    where
        F: std::future::Future<Output = Option<Vec<rfd::FileHandle>>> + Send + 'static,
    {
        self.dialog_active = true;
        let tx = self.dialog_tx.clone();
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let handles = pollster::block_on(future);
            let paths = handles.map(|hs| hs.into_iter().map(|h| h.path().to_path_buf()).collect());
            let _ = tx.send(map(paths));
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
                        &[
                            "oxb", "obj", "glb", "gltf", "stl", "ply", "ztl", "zpr", "fbx", "OXB", "OBJ", "GLB",
                            "GLTF", "STL", "PLY", "ZTL", "ZPR", "FBX",
                        ],
                    )
                    .add_filter("OxyBrush project (*.oxb)", &["oxb", "OXB"])
                    .add_filter("glTF (*.glb, *.gltf)", &["glb", "gltf", "GLB", "GLTF"])
                    .add_filter("Wavefront OBJ (*.obj)", &["obj", "OBJ"])
                    .add_filter("Autodesk FBX (*.fbx)", &["fbx", "FBX"])
                    .add_filter("STL mesh (*.stl)", &["stl", "STL"])
                    .add_filter("PLY mesh, with vertex color (*.ply)", &["ply", "PLY"])
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
                    .add_filter(
                        "Meshes",
                        &[
                            "obj", "glb", "gltf", "stl", "ply", "ztl", "zpr", "fbx", "OBJ", "GLB", "GLTF", "STL",
                            "PLY", "ZTL", "ZPR", "FBX",
                        ],
                    )
                    .add_filter("glTF (*.glb, *.gltf)", &["glb", "gltf", "GLB", "GLTF"])
                    .add_filter("Wavefront OBJ (*.obj)", &["obj", "OBJ"])
                    .add_filter("Autodesk FBX (*.fbx)", &["fbx", "FBX"])
                    .add_filter("STL mesh (*.stl)", &["stl", "STL"])
                    .add_filter("PLY mesh, with vertex color (*.ply)", &["ply", "PLY"])
                    .add_filter("ZBrush tool or project (*.ztl, *.zpr)", &["ztl", "zpr", "ZTL", "ZPR"]);
                if let Some(g) = &self.gfx {
                    dialog = dialog.set_parent(&g.window);
                }
                dialog = dialog.set_title("Import meshes");
                self.spawn_files(dialog.pick_files(), DialogReply::Import);
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
            UiAction::LoadReference => {
                if self.dialog_active {
                    return;
                }
                let mut dialog = rfd::AsyncFileDialog::new()
                    .add_filter("Reference image", &["png", "jpg", "jpeg", "PNG", "JPG", "JPEG"]);
                if let Some(g) = &self.gfx {
                    dialog = dialog.set_parent(&g.window);
                }
                let doc_id = self.editor.documents[self.editor.active_document].id;
                self.spawn_dialog(dialog.pick_file(), move |path| DialogReply::LoadReference {
                    doc_id,
                    imported: path.map(|path| crate::references::load(&path)),
                });
            }
            UiAction::LoadAlpha => {
                if self.dialog_active {
                    return;
                }
                let mut dialog = rfd::AsyncFileDialog::new().add_filter(
                    "Alpha image",
                    &[
                        "png", "jpg", "jpeg", "tif", "tiff", "webp", "psd", "PNG", "JPG", "JPEG", "TIF", "TIFF",
                        "WEBP", "PSD",
                    ],
                );
                if let Some(g) = &self.gfx {
                    dialog = dialog.set_parent(&g.window);
                }
                let doc_id = self.editor.documents[self.editor.active_document].id;
                let dir = self.editor.alpha_dir.clone();
                self.spawn_dialog(dialog.pick_file(), move |path| DialogReply::LoadAlpha {
                    doc_id,
                    imported: path.map(|path| crate::alpha_library::import(&path, dir.as_deref())),
                });
            }
            UiAction::ExportAlpha => {
                if self.dialog_active {
                    return;
                }
                let Some(alpha) = ed.alpha.and_then(|i| ed.alphas.get(i)) else {
                    ed.status = "Select an alpha to export first".into();
                    return;
                };
                let mut dialog = rfd::AsyncFileDialog::new()
                    .add_filter("PNG image", &["png", "PNG"])
                    .set_file_name(format!("{}.png", alpha.name));
                if let Some(g) = &self.gfx {
                    dialog = dialog.set_parent(&g.window);
                }
                self.spawn_dialog(dialog.save_file(), DialogReply::ExportAlpha);
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
                let format = ed.export_format;
                let name = ed
                    .path
                    .as_ref()
                    .and_then(|p| p.file_stem())
                    .map(|s| format!("{}.{}", s.to_string_lossy(), format.extension()))
                    .unwrap_or_else(|| format!("sculpt.{}", format.extension()));
                let mut dialog = rfd::AsyncFileDialog::new();
                if cfg!(target_os = "macos") {
                    // macOS save panels have no file type list: offer only the
                    // type chosen in File > Export type.
                    dialog = dialog.add_filter(format.label(), &[format.extension()]);
                } else {
                    for f in std::iter::once(format).chain(ExportFormat::ALL.into_iter().filter(|&f| f != format)) {
                        dialog = dialog.add_filter(f.label(), &[f.extension()]);
                    }
                }
                dialog = dialog.set_file_name(name);
                if let Some(g) = &self.gfx {
                    dialog = dialog.set_parent(&g.window);
                }
                self.spawn_dialog(dialog.save_file(), DialogReply::Export);
            }
            UiAction::ExportScreenshot(options) => {
                if self.dialog_active || self.screenshot_request.is_some() || self.screenshot_result.is_some() {
                    ed.status = "Wait for the current file operation to finish".into();
                    return;
                }
                let name = ed.path.as_ref().and_then(|p| p.file_stem()).unwrap_or_else(|| "sculpt".as_ref());
                let extensions = options.format.extensions();
                let mut dialog = rfd::AsyncFileDialog::new()
                    .set_title("Save screenshot")
                    .add_filter(options.format.label(), extensions)
                    .set_file_name(format!("{}.{}", name.to_string_lossy(), extensions[0]));
                if let Some(g) = &self.gfx {
                    dialog = dialog.set_parent(&g.window);
                }
                self.spawn_dialog(dialog.save_file(), move |path| DialogReply::ExportScreenshot { options, path });
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
            UiAction::InstallAddon => {
                if self.dialog_active {
                    return;
                }
                let mut dialog = rfd::AsyncFileDialog::new().add_filter("OxyBrush add-on", &["oxyaddon"]);
                if let Some(g) = &self.gfx {
                    dialog = dialog.set_parent(&g.window);
                }
                self.spawn_dialog(dialog.pick_file(), DialogReply::InstallAddon);
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
                DialogReply::Import(Some(paths)) if !paths.is_empty() => {
                    self.editor.import_paths(&paths);
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
                DialogReply::ExportAlpha(Some(p)) => self.editor.export_alpha(&p),
                DialogReply::LoadReference { doc_id, imported: Some(result) } => match result {
                    Ok(image) => crate::references::insert(&mut self.editor, doc_id, image),
                    Err(err) => self.editor.status = err,
                },
                DialogReply::LoadAlpha { doc_id, imported: Some(result) } => match result {
                    Ok(imported) => self.editor.finish_alpha_import(doc_id, imported),
                    Err(err) => self.editor.status = err,
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
                    let p = self.editor.export_format.with_default_extension(&p);
                    self.editor.export(&p);
                }
                DialogReply::ExportScreenshot { options, path: Some(path) } => match options.format.export_path(path) {
                    Ok(path) => self.screenshot_request = Some(crate::snapshot::Request { path, options }),
                    Err(e) => self.editor.status = e,
                },
                DialogReply::RunScript(Some(p)) => {
                    self.run_python_script(&p);
                }
                DialogReply::InstallAddon(Some(p)) => {
                    self.editor.install_addon(&p);
                }
                _ => {}
            }
            if let Some(g) = &self.gfx {
                g.window.request_redraw();
            }
        }
    }

    /// Import or open every file queued from the current drop.
    fn flush_dropped_files(&mut self) {
        if self.pending_drops.is_empty() {
            return;
        }
        let paths = std::mem::take(&mut self.pending_drops);
        if paths.len() == 1 {
            log::info!("opening {} (dropped or opened by the system)", paths[0].display());
        } else {
            log::info!("importing {} dropped files", paths.len());
        }
        self.editor.open_dropped(&paths);
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
        if let Some(brush) = self.editor.brush_key_capture {
            if event.state == ElementState::Pressed && !event.repeat {
                self.capture_brush_key(brush, key);
            }
            return None;
        }
        // Space moves a box, lasso or cut line being drawn instead.
        if key == Some(KeyId::Space) && self.editor.space_moves_drag() {
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
        // The macOS menu bar handles its own shortcuts.
        #[cfg(target_os = "macos")]
        if self.native_menu.as_ref().is_some_and(|m| m.takes(chord)) {
            return None;
        }
        // A key given to a brush from the shelf wins over a command (#259).
        if let Some(brush) = self.editor.keymap.brush_for(chord) {
            self.editor.pick_brush_from_key(brush);
            return None;
        }
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

    fn capture_brush_key(&mut self, brush: oxy_sculpt::BrushKind, key: Option<KeyId>) {
        let m = self.editor.modifiers;
        if key == Some(KeyId::Escape) && !m.ctrl && !m.shift && !m.alt {
            self.editor.brush_key_capture = None;
            self.editor.status = "Key unchanged".into();
            return;
        }
        // A modifier on its own isn't a key yet: wait for the rest.
        let Some(key) = key else { return };
        self.editor.brush_key_capture = None;
        let chord = self.chord(key);
        self.editor.give_brush_key(brush, chord);
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
        // A screenshot also opens Edit on the last stroke, so the image shows
        // the stroke settings panel.
        if self.auto.screenshot.is_some() {
            self.editor.set_living_strokes(true);
            self.editor.living.show = true;
        }
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
        if ed.living.show
            && let Some(id) = ed
                .living
                .strokes
                .iter()
                .rev()
                .find(|s| s.recipe.is_some() && s.piece == ed.scene.pieces[0].id)
                .map(|s| s.id)
        {
            let _ = ed.living_open_edit(id);
        }
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
        // A drop of several files arrives as one DroppedFile event per
        // path, in a single burst. Import them together once that burst
        // has been delivered, instead of one undo step per file.
        self.flush_dropped_files();
        self.sync_agent();
        self.poll_agent();
        self.poll_plugin();
        self.poll_dialog();
        if let Some(result) = &self.screenshot_result {
            use std::sync::mpsc::TryRecvError;
            let status = match result.try_recv() {
                Ok(status) => Some(status),
                Err(TryRecvError::Disconnected) => Some("Screenshot export ended before saving the image".into()),
                Err(TryRecvError::Empty) => None,
            };
            if let Some(status) = status {
                self.screenshot_result = None;
                self.editor.status = status;
                if let Some(g) = &self.gfx {
                    g.window.request_redraw();
                }
            }
        }
        let accept_bridge = self.gfx.is_some()
            && !self.dialog_active
            && self.editor.pending.is_none()
            && self.editor.save_error.is_none()
            && self.editor.recoverable.is_empty()
            && !self.editor.quit_now
            && !self.editor.is_stroking()
            && self.editor.job.is_none();
        if accept_bridge && let Some(path) = self.bridge_inbox.as_mut().and_then(|inbox| inbox.pop(Instant::now())) {
            self.editor.receive_bridge_link(path);
            if let Some(g) = &self.gfx {
                g.window.set_minimized(false);
                g.window.focus_window();
                g.window.request_redraw();
            }
        }
        if (self.shot_reply.is_some() || self.screenshot_request.is_some())
            && let Some(g) = &self.gfx
        {
            g.window.request_redraw();
        }
        // Save preferences once they settle, even if nothing redraws.
        self.persist_settings(false);
        // Wake up regularly to finish background jobs, pending settings,
        // autosave and a waiting agent even when the window is idle.
        let mut wait = if self.editor.deform_busy() || self.editor.scene_stamp_busy() {
            // A deform or stamp preview is catching up: look for it often.
            Duration::from_millis(15)
        } else if self.editor.documents.iter().any(|d| d.job.is_some())
            || self.shot_reply.is_some()
            || self.screenshot_request.is_some()
            || self.screenshot_result.is_some()
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
        // A pen press on a panel becomes a right-click once held long enough.
        if let Some(due) = self.pen_press.as_ref().and_then(PenPress::hold_due) {
            let now = Instant::now();
            if now >= due {
                if let Some(g) = &self.gfx {
                    g.window.request_redraw();
                }
            } else {
                wait = wait.min(due - now);
            }
        }
        if accept_bridge && let Some(inbox) = &self.bridge_inbox {
            wait = wait.min(inbox.wait(Instant::now()));
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
        if !self.initial_files.is_empty() {
            self.pending_session = None;
            let files = std::mem::take(&mut self.initial_files);
            self.editor.open_dropped(&files);
        } else if let Some((paths, active)) = self.pending_session.take() {
            self.editor.restore_session(&paths, active);
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
        // Held keys first: the Tab branch below returns early, and the
        // retopology tool's Tab+drag needs to know Tab is down.
        if let WindowEvent::KeyboardInput { event: key, is_synthetic: false, .. } = &event {
            note_held_key(&mut self.editor, key.physical_key, key.state);
        }
        // Tab is OxyBrush's shortcut (show / hide panels) unless a text field
        // is being typed in. egui would otherwise use it to move keyboard
        // focus between controls: that scrolls the panels to the focused
        // control, and every later Tab goes to egui instead of the shortcut.
        if let WindowEvent::KeyboardInput { event: key, is_synthetic: false, .. } = &event
            && key.physical_key == PhysicalKey::Code(KeyCode::Tab)
            && !self.egui_ctx.text_edit_focused()
            && self.editor.keymap_capture.is_none()
            && self.editor.screenshot_options.is_none()
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
                        remember_window(&mut self.editor, &g.window);
                        self.persist_settings(true);
                        self.editor.autosaver.finish();
                        event_loop.exit();
                    }
                    redraw = true;
                }
            }
            // A file dropped on the window, or (macOS) one the system asked
            // OxyBrush to open: a double-click in Finder, Open With, the Dock.
            // Several files in one drop are queued and imported together
            // when the event loop goes idle (`flush_dropped_files`).
            WindowEvent::DroppedFile(path) => {
                log::info!("queued {} (dropped or opened by the system)", path.display());
                self.pending_drops.push(path.clone());
                redraw = true;
            }
            WindowEvent::Resized(_) => {
                redraw = true;
                remember_window(&mut self.editor, &g.window);
            }
            WindowEvent::Moved(_) => remember_window(&mut self.editor, &g.window),
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
                let alt_was = self.editor.modifiers.alt;
                self.editor.modifiers =
                    Modifiers { shift: s.shift_key(), ctrl: s.control_key() || s.super_key(), alt: s.alt_key() };
                // Alt taps add points to a curve selection being drawn (#446).
                if self.editor.modifiers.alt != alt_was {
                    self.editor.select_curve_alt(self.editor.modifiers.alt);
                }
                if self.editor.modifiers.ctrl {
                    self.editor.tab_held = false;
                }
                redraw = true;
            }
            WindowEvent::Focused(focused) => {
                if !*focused {
                    self.editor.modifiers = Modifiers::default();
                    self.editor.space_held = false;
                    self.editor.tab_held = false;
                    if self.editor.is_stroking() {
                        self.editor.pointer_up();
                    }
                    self.editor.force_autosave();
                    self.last_pressure = None;
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
                if let Some(press) = &mut self.pen_press {
                    let p = Vec2::new(position.x as f32, position.y as f32);
                    let ppp = self.editor.pixels_per_point.max(0.1);
                    let widget_drag = self.egui_ctx.dragged_id().is_some();
                    if let Some(d) = press.moved(p, ppp, widget_drag) {
                        g.egui_state.egui_input_mut().events.push(egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: egui::vec2(d.x, d.y),
                            phase: egui::TouchPhase::Move,
                            source: egui::MouseWheelSource::Unknown,
                            modifiers: Default::default(),
                        });
                    }
                }
                self.editor.pointer_moved(Vec2::new(position.x as f32, position.y as f32));
                redraw = true;
            }
            WindowEvent::TouchpadPressure { pressure, .. } => {
                let Some(pressure) = valid_pressure(*pressure) else { return };
                if !self.editor.pen_seen {
                    // Once per session, so a bug report's log shows whether
                    // pen pressure reaches OxyBrush at all.
                    log::info!("pen pressure is coming in (first reading {pressure:.3})");
                }
                if self.touches.is_empty() {
                    self.editor.pressure = pressure;
                }
                self.editor.pen_seen = true;
                self.last_pressure = Some((Instant::now(), pressure));
                redraw = true;
            }
            WindowEvent::PinchGesture { delta, .. } if !over_egui => {
                self.editor.scroll(*delta as f32 * 20.0);
                redraw = true;
            }
            WindowEvent::RotationGesture { delta, .. } if !over_egui => {
                self.editor.orbit_by(Vec2::new(*delta * 2.0, 0.0));
                redraw = true;
            }
            // Pen pressure on X11 arrives as TouchpadPressure, scaled to 0..=1
            // by the vendored winit (third_party/README.md). Other raw axes
            // (tilt, for example) used to be taken for pressure when their
            // value happened to fall in 0..=1; they are ignored now.
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
                // Middle- or right-drag over a panel scrolls it, like grabbing
                // the page: a pen's side buttons send these (RFC 0015). A
                // right-click that doesn't move still opens a context menu.
                if matches!(button, MouseButton::Middle | MouseButton::Right) {
                    self.mmb_scroll = match state {
                        ElementState::Pressed if over_egui => self.editor.pointer,
                        _ => None,
                    };
                }
                // A pen press on a panel (RFC 0015): dragging it scrolls the
                // panel unless a control (a slider, a brush tile) takes the
                // drag, and holding it still opens the control's menu.
                if *button == MouseButton::Left {
                    let since = self.last_pressure.map(|(at, _)| at.elapsed());
                    match state {
                        ElementState::Pressed if over_egui && self.editor.press_counts_as_touch(since) => {
                            self.pen_press = self.editor.pointer.map(PenPress::new);
                        }
                        ElementState::Released => self.pen_press = None,
                        _ => {}
                    }
                }
                if let Some(b) = b {
                    match state {
                        ElementState::Pressed if !over_egui && self.editor.pointer_in_full_canvas() => {
                            if b == PointerButton::Primary && self.touches.is_empty() {
                                self.editor.pressure = pointer_start_pressure(self.last_pressure, Instant::now());
                            }
                            self.editor.pointer_down(b)
                        }
                        ElementState::Released
                            if self.editor.part_placement.as_ref().is_some_and(|p| p.moving.is_none())
                                && (b != PointerButton::Primary || self.egui_ctx.is_pointer_over_egui()) => {}
                        ElementState::Released => self.editor.pointer_released(),
                        _ => {}
                    }
                }
                redraw = true;
            }
            WindowEvent::Touch(t) => {
                let pos = Vec2::new(t.location.x as f32, t.location.y as f32);
                let force_pressure = t.force.and_then(|f| valid_pressure(f.normalized() as f32));
                if force_pressure.is_some() {
                    self.editor.pen_seen = true;
                }
                match t.phase {
                    TouchPhase::Started => {
                        let mut contact = TouchPoint { start_pos: pos, pos, _start_time: Instant::now(), force: None };
                        let pressure = contact.pressure(force_pressure);
                        self.touches.insert(t.id, contact);
                        if self.touches.len() == 1 {
                            self.editor.pressure = pressure;
                            self.editor.pointer = Some(pos);
                            if !over_egui && self.editor.pointer_in_full_canvas() {
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
                        let pressure = self.touches.get_mut(&t.id).map(|contact| {
                            contact.pos = pos;
                            contact.pressure(force_pressure)
                        });
                        if self.touches.is_empty() {
                            // A pen hovering above the tablet: on Windows,
                            // winit reports it as touch moves with no touch
                            // started (and no mouse moves), so the brush
                            // cursor must follow these or it stays where the
                            // pen last touched.
                            self.editor.pointer_moved(pos);
                        } else if self.touches.len() == 1 {
                            if let Some(pressure) = pressure {
                                self.editor.pressure = pressure;
                                self.editor.pointer_moved(pos);
                            }
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
                                        self.editor.orbit_by(pan_delta * 0.5);
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
                            self.editor.pointer_released();
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
                let capture_or_quick = self.editor.keymap_capture.is_some()
                    || (event.state == ElementState::Released && self.editor.quick_menu.is_some_and(|q| q.held));
                if self.editor.screenshot_options.is_none()
                    && (capture_or_quick || !self.egui_ctx.egui_wants_keyboard_input())
                {
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

/// Keep track of the keys that change what a click or drag does while held:
/// Space (the quick menu) and Tab (retopology: pull a quad out of an edge).
pub(crate) fn note_held_key(ed: &mut crate::editor::Editor, key: PhysicalKey, state: ElementState) {
    let down = state == ElementState::Pressed;
    match key {
        PhysicalKey::Code(KeyCode::Space) => ed.space_held = down,
        PhysicalKey::Code(KeyCode::Tab) => ed.tab_held = down && !ed.modifiers.ctrl,
        _ => {}
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

/// A pen press on a panel (RFC 0015), in physical pixels. A pen reaches
/// OxyBrush as a mouse without a wheel, so egui can't scroll a panel with
/// it or tell a long press. Once the press moves past a click's distance it
/// scrolls the panel under it (as middle-drag does), unless egui gave the
/// drag to a control; held still for egui's longest click it becomes a
/// right-click there, which opens the control's menu and pins its tooltip,
/// and egui no longer counts the press as a click when it's released.
pub(crate) struct PenPress {
    start: Vec2,
    last: Vec2,
    at: Instant,
    scrolling: bool,
    /// Moved, scrolled or already turned into a right-click.
    done_holding: bool,
}

/// A press held this long becomes a right-click (egui's `max_click_duration`,
/// past which the release is no longer a click).
pub(crate) const PEN_HOLD: Duration = Duration::from_millis(800);

impl PenPress {
    pub(crate) fn new(at: Vec2) -> Self {
        PenPress { start: at, last: at, at: Instant::now(), scrolling: false, done_holding: false }
    }

    /// The pen moved to `p`: the scroll to apply, in points, if this press
    /// scrolls. `widget_drag`: egui is dragging a control with it.
    pub(crate) fn moved(&mut self, p: Vec2, pixels_per_point: f32, widget_drag: bool) -> Option<Vec2> {
        let delta = (p - self.last) / pixels_per_point;
        self.last = p;
        if !self.done_holding && (p - self.start).length() / pixels_per_point > 6.0 {
            self.done_holding = true;
            self.scrolling = !widget_drag;
        }
        self.scrolling.then_some(delta)
    }

    /// When the press becomes a right-click, if it still can.
    pub(crate) fn hold_due(&self) -> Option<Instant> {
        (!self.done_holding).then_some(self.at + PEN_HOLD)
    }

    /// Whether the press has now been held still long enough to be a
    /// right-click (true once).
    pub(crate) fn held(&mut self, now: Instant) -> bool {
        if self.done_holding || now.duration_since(self.at) < PEN_HOLD {
            return false;
        }
        self.done_holding = true;
        true
    }
}

fn frame_dimensions(size: winit::dpi::PhysicalSize<u32>, pixels_per_point: f32) -> ([u32; 2], Option<egui::Rect>) {
    let pixels = [size.width.max(1), size.height.max(1)];
    if size.width == 0 || size.height == 0 {
        return (pixels, None);
    }
    let points = egui::vec2(size.width as f32, size.height as f32) / pixels_per_point;
    (pixels, Some(egui::Rect::from_min_size(egui::Pos2::ZERO, points)))
}

/// Note the window's size, position and maximized state for next time.
/// While it's maximized (or full screen) the normal size and position are
/// kept, so un-maximizing next session goes back to them.
fn remember_window(ed: &mut crate::editor::Editor, window: &Window) {
    let maximized = window.is_maximized();
    let mut g = ed.window_geometry.unwrap_or(crate::settings::WindowGeometry {
        width: 1440.0,
        height: 900.0,
        position: None,
        maximized,
    });
    g.maximized = maximized;
    if !maximized && window.fullscreen().is_none() && !window.is_minimized().unwrap_or(false) {
        let size: winit::dpi::LogicalSize<f32> = window.inner_size().to_logical(window.scale_factor());
        let (min_w, min_h) = crate::settings::WindowGeometry::MIN;
        g.width = size.width.max(min_w);
        g.height = size.height.max(min_h);
        // Wayland doesn't tell windows where they are.
        g.position = window.outer_position().ok().map(|p| (p.x, p.y)).or(g.position);
    }
    ed.window_geometry = Some(g);
}

/// The #577 report reaches Vulkan adapter selection, then faults inside
/// Intel's user-mode driver. Switch before requesting the device.
fn needs_intel_dx12(backend: wgpu::Backend, vendor: u32, device: u32, explicit_backend: bool) -> bool {
    !explicit_backend && backend == wgpu::Backend::Vulkan && vendor == 0x8086 && device == 0x3185
}

#[cfg(test)]
mod tests {
    use super::{
        PEN_HOLD, PenPress, TouchPoint, frame_dimensions, note_held_key, pointer_start_pressure, valid_pressure,
    };
    use glam::Vec2;
    use std::time::{Duration, Instant};
    use winit::dpi::PhysicalSize;
    use winit::event::ElementState::{Pressed, Released};
    use winit::keyboard::{KeyCode, PhysicalKey};

    #[test]
    fn windows_compatibility_matches_the_reported_adapter_and_respects_an_override() {
        use wgpu::Backend::{Dx12, Vulkan};
        assert!(super::needs_intel_dx12(Vulkan, 0x8086, 0x3185, false));
        assert!(!super::needs_intel_dx12(Vulkan, 0x8086, 0x3185, true));
        assert!(!super::needs_intel_dx12(Dx12, 0x8086, 0x3185, false));
        assert!(!super::needs_intel_dx12(Vulkan, 0x8086, 0x4680, false));
    }

    #[test]
    fn pen_press_starts_at_the_fresh_pressure_and_regular_mouse_at_full() {
        let at = Instant::now();
        assert_eq!(pointer_start_pressure(Some((at, 0.04)), at), 0.04);
        assert_eq!(pointer_start_pressure(Some((at, 0.0)), at + Duration::from_millis(5)), 0.0);
        assert_eq!(pointer_start_pressure(Some((at, 0.04)), at + Duration::from_secs(2)), 1.0);
        assert_eq!(pointer_start_pressure(None, at), 1.0);
    }

    #[test]
    fn a_contact_keeps_its_force_when_a_pen_packet_drops_it() {
        let mut contact =
            TouchPoint { start_pos: Vec2::ZERO, pos: Vec2::ZERO, _start_time: Instant::now(), force: None };
        assert_eq!(contact.pressure(None), 1.0, "an unpressured contact can draw");
        assert_eq!(contact.pressure(valid_pressure(0.08)), 0.08);
        assert_eq!(contact.pressure(None), 0.08);
        assert_eq!(contact.pressure(valid_pressure(f32::NAN)), 0.08);
        assert_eq!(contact.pressure(valid_pressure(0.0)), 0.0);
        assert_eq!(contact.pressure(None), 0.0);
    }

    #[test]
    fn frame_dimensions_keep_layout_in_step_with_surface() {
        for (size, scale) in [
            (PhysicalSize::new(1440, 900), 1.0),
            (PhysicalSize::new(1439, 900), 1.5),
            (PhysicalSize::new(1600, 1000), 2.0),
        ] {
            let (pixels, rect) = frame_dimensions(size, scale);
            let rect = rect.unwrap();
            assert_eq!(pixels, [size.width, size.height]);
            assert_eq!(rect.min, egui::Pos2::ZERO);
            assert!((rect.width() * scale - size.width as f32).abs() < 0.001);
            assert!((rect.height() * scale - size.height as f32).abs() < 0.001);
        }
        assert_eq!(frame_dimensions(PhysicalSize::new(0, 900), 1.0), ([1, 900], None));
        assert_eq!(frame_dimensions(PhysicalSize::new(1440, 0), 1.0), ([1440, 1], None));
    }

    #[test]
    fn tab_and_space_are_tracked_while_held() {
        // The retopology tool's Tab+drag reads `tab_held`; Tab presses take an
        // early path in `window_event`, so they're noted before it.
        let mut ed = crate::editor::Editor::default();
        let tab = PhysicalKey::Code(KeyCode::Tab);
        note_held_key(&mut ed, tab, Pressed);
        assert!(ed.tab_held);
        note_held_key(&mut ed, PhysicalKey::Code(KeyCode::KeyA), Pressed);
        assert!(ed.tab_held, "other keys leave it alone");
        note_held_key(&mut ed, tab, Released);
        assert!(!ed.tab_held);

        // Ctrl+Tab does not set tab_held (isolated so retopology drag isn't activated)
        ed.modifiers.ctrl = true;
        note_held_key(&mut ed, tab, Pressed);
        assert!(!ed.tab_held, "ctrl+tab does not track tab_held");
        ed.modifiers.ctrl = false;

        note_held_key(&mut ed, PhysicalKey::Code(KeyCode::Space), Pressed);
        assert!(ed.space_held);
        note_held_key(&mut ed, PhysicalKey::Code(KeyCode::Space), Released);
        assert!(!ed.space_held);
    }

    #[test]
    fn a_pen_press_scrolls_once_it_moves_unless_a_control_takes_it() {
        // RFC 0015. Physical pixels at 2 px per point: 6 pt is 12 px.
        let mut p = PenPress::new(Vec2::new(100.0, 100.0));
        assert_eq!(p.moved(Vec2::new(100.0, 108.0), 2.0, false), None, "within a click's distance");
        assert_eq!(
            p.moved(Vec2::new(100.0, 120.0), 2.0, false),
            Some(Vec2::new(0.0, 6.0)),
            "past it: scroll by the move, in points"
        );
        assert_eq!(p.moved(Vec2::new(100.0, 100.0), 2.0, false), Some(Vec2::new(0.0, -10.0)));
        assert!(p.hold_due().is_none(), "a press that moved never becomes a right-click");
        // A slider took the drag: no scrolling, ever, for this press.
        let mut p = PenPress::new(Vec2::new(100.0, 100.0));
        assert_eq!(p.moved(Vec2::new(140.0, 100.0), 2.0, true), None);
        assert_eq!(p.moved(Vec2::new(180.0, 100.0), 2.0, false), None);
    }

    #[test]
    fn a_pen_held_still_becomes_one_right_click() {
        let mut p = PenPress::new(Vec2::new(10.0, 10.0));
        let start = p.at;
        assert!(!p.held(start + PEN_HOLD / 2), "not yet");
        assert!(p.moved(Vec2::new(12.0, 11.0), 1.0, false).is_none(), "a tremble isn't a move");
        assert_eq!(p.hold_due(), Some(start + PEN_HOLD));
        assert!(p.held(start + PEN_HOLD));
        assert!(!p.held(start + PEN_HOLD * 2), "only once");
        assert!(p.hold_due().is_none());
    }

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

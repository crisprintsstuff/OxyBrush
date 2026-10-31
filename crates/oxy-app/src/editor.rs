//! Editor state and pointer/keyboard interaction, independent of the GPU and
//! window so it can be unit-tested.

use glam::Vec2;
use oxy_history::History;
use oxy_mesh::{Mesh, primitives, topo};
use oxy_render::{Camera, MatcapPreset, ViewSettings};
use oxy_sculpt::{BrushKind, BrushSettings, Falloff, Stroke, StrokeSample, Symmetry};
use std::path::{Path, PathBuf};

/// Hard limit for subdivision so a click can't exhaust memory.
pub const MAX_TRIANGLES: usize = 24_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerButton {
    Primary,
    Middle,
    Secondary,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

pub enum Drag {
    None,
    Orbit,
    Pan,
    Stroke { stroke: Box<Stroke>, grab_plane: (glam::Vec3, glam::Vec3) },
}

pub struct Editor {
    pub mesh: Mesh,
    pub history: History,
    pub camera: Camera,
    pub brush_kind: BrushKind,
    /// Brush radius in logical points (screen space).
    pub brush_radius_pt: f32,
    pub strengths: [f32; BrushKind::ALL.len()],
    pub falloff: Falloff,
    pub spacing: f32,
    pub pressure_strength: bool,
    pub pressure_radius: bool,
    pub front_faces_only: bool,
    pub auto_smooth: f32,
    pub symmetry: Symmetry,
    pub view: ViewSettings,
    pub matcap: MatcapPreset,
    pub ui_scale: f32,
    pub show_panels: bool,
    pub show_shortcuts: bool,
    pub show_about: bool,
    pub high_contrast: bool,
    pub pixels_per_point: f32,
    /// Canvas rectangle in physical pixels: x, y, w, h.
    pub canvas: [f32; 4],
    /// Pointer position in physical pixels.
    pub pointer: Option<Vec2>,
    pub modifiers: Modifiers,
    pub pressure: f32,
    pub drag: Drag,
    pub status: String,
    pub path: Option<PathBuf>,
    pub frame_ms: f32,
    pub quit_requested: bool,
}

impl Default for Editor {
    fn default() -> Self {
        let mesh = primitives::icosphere(6, 1.0);
        let mut camera = Camera { yaw: 0.0, pitch: 0.15, ..Default::default() };
        camera.frame(mesh.bounds());
        let mut strengths = [0.0; BrushKind::ALL.len()];
        for (i, k) in BrushKind::ALL.iter().enumerate() {
            strengths[i] = k.default_strength();
        }
        Self {
            mesh,
            history: History::default(),
            camera,
            brush_kind: BrushKind::Draw,
            brush_radius_pt: 50.0,
            strengths,
            falloff: Falloff::Smooth,
            spacing: 0.15,
            pressure_strength: true,
            pressure_radius: false,
            front_faces_only: true,
            auto_smooth: 0.0,
            symmetry: Symmetry { x: true, ..Default::default() },
            view: ViewSettings::default(),
            matcap: MatcapPreset::Terracotta,
            ui_scale: 1.0,
            show_panels: true,
            show_shortcuts: false,
            show_about: false,
            high_contrast: false,
            pixels_per_point: 1.0,
            canvas: [0.0, 0.0, 1.0, 1.0],
            pointer: None,
            modifiers: Modifiers::default(),
            pressure: 1.0,
            drag: Drag::None,
            status: "Welcome to OxyBrush. Drag on the model to sculpt, drag on empty space to orbit. Press F1 for shortcuts.".into(),
            path: None,
            frame_ms: 0.0,
            quit_requested: false,
        }
    }
}

fn kind_index(k: BrushKind) -> usize {
    BrushKind::ALL.iter().position(|&x| x == k).unwrap_or(0)
}

impl Editor {
    pub fn strength(&self, k: BrushKind) -> f32 {
        self.strengths[kind_index(k)]
    }

    pub fn strength_mut(&mut self, k: BrushKind) -> &mut f32 {
        &mut self.strengths[kind_index(k)]
    }

    fn canvas_size(&self) -> Vec2 {
        Vec2::new(self.canvas[2].max(1.0), self.canvas[3].max(1.0))
    }

    fn canvas_local(&self, p: Vec2) -> Vec2 {
        p - Vec2::new(self.canvas[0], self.canvas[1])
    }

    pub fn pointer_in_canvas(&self) -> bool {
        self.pointer.is_some_and(|p| {
            let l = self.canvas_local(p);
            l.x >= 0.0 && l.y >= 0.0 && l.x < self.canvas[2] && l.y < self.canvas[3]
        })
    }

    pub fn is_stroking(&self) -> bool {
        matches!(self.drag, Drag::Stroke { .. })
    }

    /// The brush that a primary-button press would use right now.
    pub fn effective_brush(&self) -> (BrushKind, bool) {
        let m = self.modifiers;
        if m.ctrl {
            (BrushKind::Mask, m.alt)
        } else if m.shift {
            (BrushKind::Smooth, false)
        } else {
            (self.brush_kind, m.alt)
        }
    }

    pub fn pointer_down(&mut self, button: PointerButton) {
        let Some(p) = self.pointer else { return };
        match button {
            PointerButton::Primary => {
                let (o, d) = self.camera.ray(self.canvas_local(p), self.canvas_size());
                if let Some(hit) = self.mesh.raycast(o, d) {
                    let (kind, invert) = self.effective_brush();
                    let radius = self.brush_radius_pt
                        * self.pixels_per_point
                        * self.camera.world_per_pixel(hit.point, self.canvas_size().y);
                    let settings = BrushSettings {
                        kind,
                        radius,
                        strength: self.strength(kind),
                        falloff: self.falloff,
                        spacing: self.spacing,
                        pressure_strength: self.pressure_strength,
                        pressure_radius: self.pressure_radius,
                        front_faces_only: self.front_faces_only,
                        auto_smooth: self.auto_smooth,
                    };
                    let mut stroke = Box::new(Stroke::new(settings, self.symmetry, invert));
                    self.history.begin(kind.label(), &self.mesh);
                    let normal = if hit.normal.dot(d) > 0.0 { -hit.normal } else { hit.normal };
                    let sample = StrokeSample { position: hit.point, normal, pressure: self.pressure };
                    let (mesh, history) = (&mut self.mesh, &mut self.history);
                    stroke.add_sample(mesh, sample, &mut |m, c| history.touch(m, c));
                    self.drag = Drag::Stroke { stroke, grab_plane: (hit.point, self.camera.forward()) };
                } else if self.modifiers.alt {
                    self.drag = Drag::Pan;
                } else {
                    self.drag = Drag::Orbit;
                }
            }
            PointerButton::Middle => {
                self.drag = if self.modifiers.shift { Drag::Pan } else { Drag::Orbit };
            }
            PointerButton::Secondary => self.drag = Drag::Pan,
        }
    }

    /// Returns true if anything changed that needs a redraw.
    pub fn pointer_moved(&mut self, pos: Vec2) -> bool {
        let prev = self.pointer.replace(pos);
        let delta = prev.map_or(Vec2::ZERO, |p| pos - p);
        let size = self.canvas_size();
        match &mut self.drag {
            Drag::None => false,
            Drag::Orbit => {
                self.camera.orbit(delta);
                true
            }
            Drag::Pan => {
                self.camera.pan(delta, size.y);
                true
            }
            Drag::Stroke { stroke, grab_plane } => {
                let local = pos - Vec2::new(self.canvas[0], self.canvas[1]);
                let (o, d) = self.camera.ray(local, size);
                let sample = if stroke.settings().kind == BrushKind::Grab {
                    let (pp, pn) = *grab_plane;
                    let denom = d.dot(pn);
                    if denom.abs() < 1e-6 {
                        return false;
                    }
                    let t = (pp - o).dot(pn) / denom;
                    StrokeSample { position: o + d * t, normal: -pn, pressure: self.pressure }
                } else {
                    match self.mesh.raycast(o, d) {
                        Some(hit) => {
                            let normal = if hit.normal.dot(d) > 0.0 { -hit.normal } else { hit.normal };
                            StrokeSample { position: hit.point, normal, pressure: self.pressure }
                        }
                        None => return false,
                    }
                };
                let (mesh, history) = (&mut self.mesh, &mut self.history);
                stroke.add_sample(mesh, sample, &mut |m, c| history.touch(m, c));
                true
            }
        }
    }

    pub fn pointer_up(&mut self) {
        if let Drag::Stroke { .. } = self.drag {
            self.history.commit();
        }
        self.drag = Drag::None;
    }

    pub fn scroll(&mut self, steps: f32) {
        self.camera.zoom(steps);
    }

    pub fn undo(&mut self) {
        self.pointer_up();
        self.status = match self.history.undo(&mut self.mesh) {
            Some(l) => format!("Undid {l}"),
            None => "Nothing to undo".into(),
        };
    }

    pub fn redo(&mut self) {
        self.pointer_up();
        self.status = match self.history.redo(&mut self.mesh) {
            Some(l) => format!("Redid {l}"),
            None => "Nothing to redo".into(),
        };
    }

    fn replace_mesh(&mut self, label: &str, mesh: Mesh, frame: bool) {
        self.pointer_up();
        let prev = std::mem::replace(&mut self.mesh, mesh);
        self.history.record_topology(label, prev);
        if frame {
            self.camera.frame(self.mesh.bounds());
        }
    }

    pub fn new_primitive(&mut self, which: &str) {
        let mesh = match which {
            "cube" => primitives::cube(64, 1.6),
            "plane" => primitives::plane(200, 2.0),
            _ => primitives::icosphere(6, 1.0),
        };
        self.replace_mesh(&format!("New {which}"), mesh, true);
        self.path = None;
        self.status = format!("New {which}: {} triangles", self.mesh.triangle_count());
    }

    pub fn subdivide(&mut self, smooth: bool) {
        let n = self.mesh.triangle_count() * 4;
        if n > MAX_TRIANGLES {
            self.status = format!("Subdivision would create {n} triangles (limit {MAX_TRIANGLES})");
            return;
        }
        let mesh = topo::subdivide(&self.mesh, if smooth { 1 } else { 0 });
        self.replace_mesh(if smooth { "Smooth subdivide" } else { "Subdivide" }, mesh, false);
        self.status = format!("Subdivided: {} triangles", self.mesh.triangle_count());
    }

    fn edit_mask(&mut self, label: &str, f: impl Fn(f32) -> f32) {
        self.pointer_up();
        self.history.begin(label, &self.mesh);
        for c in 0..self.mesh.chunks.len() as u32 {
            self.history.touch(&self.mesh, c);
        }
        self.mesh.mask.iter_mut().for_each(|m| *m = f(*m));
        self.mesh.mark_all_dirty();
        self.history.commit();
        self.status = label.into();
    }

    pub fn invert_mask(&mut self) {
        self.edit_mask("Invert mask", |m| 1.0 - m);
    }

    pub fn clear_mask(&mut self) {
        self.edit_mask("Clear mask", |_| 0.0);
    }

    pub fn open(&mut self, path: &Path) {
        match oxy_io::import(path) {
            Ok(mesh) => {
                let tris = mesh.triangle_count();
                self.replace_mesh("Open", mesh, true);
                self.path = Some(path.to_path_buf());
                self.status = format!("Opened {} ({tris} triangles)", path.display());
            }
            Err(e) => self.status = format!("Could not open {}: {e}", path.display()),
        }
    }

    pub fn export(&mut self, path: &Path) {
        self.status = match oxy_io::export(&self.mesh, path) {
            Ok(()) => format!("Exported {}", path.display()),
            Err(e) => format!("Export failed: {e}"),
        };
    }

    pub fn frame(&mut self) {
        self.camera.frame(self.mesh.bounds());
    }

    pub fn adjust_radius(&mut self, factor: f32) {
        self.brush_radius_pt = (self.brush_radius_pt * factor).clamp(2.0, 1000.0);
    }

    pub fn adjust_strength(&mut self, delta: f32) {
        let k = self.brush_kind;
        let s = self.strength_mut(k);
        *s = (*s + delta).clamp(0.01, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor() -> Editor {
        let mut e = Editor { mesh: primitives::icosphere(4, 1.0), ..Default::default() };
        e.canvas = [0.0, 0.0, 800.0, 600.0];
        e.camera = Camera::default();
        e.camera.frame(e.mesh.bounds());
        e
    }

    #[test]
    fn stroke_on_model_then_undo() {
        let mut e = editor();
        let before = e.mesh.positions.clone();
        e.pointer = Some(Vec2::new(400.0, 300.0));
        e.pointer_down(PointerButton::Primary);
        assert!(e.is_stroking());
        e.pointer_moved(Vec2::new(430.0, 300.0));
        e.pointer_up();
        assert_ne!(before, e.mesh.positions);
        e.undo();
        assert_eq!(before, e.mesh.positions);
    }

    #[test]
    fn click_on_empty_space_orbits() {
        let mut e = editor();
        e.pointer = Some(Vec2::new(5.0, 5.0));
        e.pointer_down(PointerButton::Primary);
        assert!(matches!(e.drag, Drag::Orbit));
        let yaw = e.camera.yaw;
        e.pointer_moved(Vec2::new(50.0, 5.0));
        assert_ne!(yaw, e.camera.yaw);
    }

    #[test]
    fn ctrl_paints_mask_and_invert_works() {
        let mut e = editor();
        e.modifiers.ctrl = true;
        e.pointer = Some(Vec2::new(400.0, 300.0));
        e.pointer_down(PointerButton::Primary);
        e.pointer_up();
        assert!(e.mesh.mask.iter().any(|&m| m > 0.0));
        e.clear_mask();
        assert!(e.mesh.mask.iter().all(|&m| m == 0.0));
        e.undo();
        assert!(e.mesh.mask.iter().any(|&m| m > 0.0));
    }

    #[test]
    fn subdivide_is_undoable() {
        let mut e = editor();
        let n = e.mesh.triangle_count();
        e.subdivide(true);
        assert_eq!(e.mesh.triangle_count(), n * 4);
        e.undo();
        assert_eq!(e.mesh.triangle_count(), n);
    }
}

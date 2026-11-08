//! Parametric primitives: Pieces > + Add opens a window with the shape's
//! parameters and a live preview; Accept adds it as a piece, optionally
//! voxel-remeshed for even triangles.

use crate::editor::{Editor, JobResult};
use egui::RichText;
use oxy_doc::Piece;
use oxy_mesh::{Mesh, primitives};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Sphere,
    Cube,
    Plane,
    Cylinder,
    Cone,
    Torus,
}

impl Shape {
    pub fn from_key(which: &str) -> Shape {
        match which {
            "cube" => Shape::Cube,
            "plane" => Shape::Plane,
            "cylinder" => Shape::Cylinder,
            "cone" => Shape::Cone,
            "torus" => Shape::Torus,
            _ => Shape::Sphere,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Shape::Sphere => "Sphere",
            Shape::Cube => "Cube",
            Shape::Plane => "Plane",
            Shape::Cylinder => "Cylinder",
            Shape::Cone => "Cone",
            Shape::Torus => "Torus",
        }
    }
}

/// The primitive being set up.
#[derive(Debug, Clone, PartialEq)]
pub struct PrimitiveParams {
    pub shape: Shape,
    /// Radius (sphere, cylinder, cone, torus ring) or edge length (cube, plane).
    pub size: f32,
    /// Height (cylinder, cone) or tube radius (torus).
    pub second: f32,
    /// Segments around (cylinder, cone, torus ring) or divisions per side
    /// (cube, plane), or subdivisions (sphere).
    pub segments: u32,
    /// Rows up the side (cylinder, cone) or segments around the tube (torus).
    pub rows: u32,
    /// Closed ends (cylinder, cone).
    pub caps: bool,
    /// Voxel-remesh for even triangles, ready to sculpt.
    pub remesh: bool,
    pub resolution: u32,
}

impl PrimitiveParams {
    /// Low-poly defaults: the clean shape, not a sculpting mesh.
    pub fn new(shape: Shape) -> Self {
        let p = |size, second, segments, rows| PrimitiveParams {
            shape,
            size,
            second,
            segments,
            rows,
            caps: true,
            remesh: false,
            resolution: 110,
        };
        match shape {
            Shape::Sphere => p(1.0, 0.0, 3, 0),
            Shape::Cube => p(1.6, 0.0, 1, 0),
            Shape::Plane => p(2.0, 0.0, 4, 0),
            Shape::Cylinder => p(0.7, 1.8, 24, 1),
            Shape::Cone => p(0.8, 1.8, 24, 1),
            Shape::Torus => p(0.75, 0.3, 32, 12),
        }
    }

    /// The shape as set (before any remesh).
    pub fn mesh(&self) -> Mesh {
        let (s, t) = (self.size.max(1e-3), self.second.max(1e-3));
        match self.shape {
            Shape::Sphere => primitives::icosphere(self.segments.min(7), s),
            Shape::Cube => primitives::cube(self.segments.clamp(1, 256), s),
            Shape::Plane => primitives::plane(self.segments.clamp(1, 512), s),
            Shape::Cylinder => primitives::cylinder_with(self.segments.max(3), self.rows.max(1), s, t, self.caps),
            Shape::Cone => primitives::cone_with(self.segments.max(3), self.rows.max(1), s, t, self.caps),
            Shape::Torus => primitives::torus(self.segments.max(3), self.rows.max(3), s, t.min(s * 0.99)),
        }
    }
}

pub struct PrimitiveEdit {
    pub params: PrimitiveParams,
    pub preview: Mesh,
    built: Option<PrimitiveParams>,
}

impl Editor {
    /// Open the parametric window for `which` (sphere, cube, plane,
    /// cylinder, cone, torus).
    pub fn open_primitive(&mut self, which: &str) {
        let params = PrimitiveParams::new(Shape::from_key(which));
        let preview = params.mesh();
        self.primitive_edit = Some(PrimitiveEdit { built: Some(params.clone()), params, preview });
        self.status = "Set the shape, then Accept (Enter) to add it".into();
    }

    pub fn update_primitive(&mut self) {
        if let Some(p) = &mut self.primitive_edit
            && p.built.as_ref() != Some(&p.params)
        {
            p.preview = p.params.mesh();
            p.built = Some(p.params.clone());
        }
    }

    pub fn cancel_primitive(&mut self) {
        self.primitive_edit = None;
    }

    /// Add the primitive as a new piece (one undo step); remeshed on a
    /// worker thread when asked.
    pub fn accept_primitive(&mut self) {
        let Some(p) = self.primitive_edit.take() else { return };
        let name = p.params.shape.label().to_string();
        if p.params.remesh {
            let (mesh, res) = (p.params.mesh(), p.params.resolution);
            self.start_job(format!("Add {name}"), Vec::new(), move || {
                Ok(JobResult::Primitive(name, crate::editor::sculptable_at(mesh, res)))
            });
        } else {
            let name = self.scene.unique_name(&name);
            let tris = p.preview.triangle_count();
            self.add_piece(Piece::new(name.clone(), p.preview), &format!("Add {name}"));
            self.status = format!("Added {name} ({tris} triangles)");
        }
    }
}

/// The primitive's parameters, with Accept and Cancel (Enter / Esc).
pub fn window(ctx: &egui::Context, ed: &mut Editor) {
    let Some(edit) = &mut ed.primitive_edit else { return };
    let (enter, esc) = ctx.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
    let mut accept = enter && !ctx.egui_wants_keyboard_input();
    let mut cancel = esc;
    let tris = edit.preview.triangle_count();
    let p = &mut edit.params;
    let screen = ctx.content_rect();
    egui::Window::new(format!("Add {}", p.shape.label()))
        .collapsible(false)
        .resizable(false)
        .pivot(egui::Align2::LEFT_BOTTOM)
        .default_pos(egui::pos2(screen.left() + 150.0, screen.bottom() - 48.0))
        .show(ctx, |ui| {
            egui::Grid::new("primitive_grid").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                let slider_f = |ui: &mut egui::Ui, label: &str, v: &mut f32, max: f32| {
                    ui.label(label);
                    ui.add(egui::Slider::new(v, 0.01..=max).logarithmic(true));
                    ui.end_row();
                };
                match p.shape {
                    Shape::Sphere => {
                        slider_f(ui, "Radius", &mut p.size, 10.0);
                        ui.label("Subdivisions");
                        ui.add(egui::Slider::new(&mut p.segments, 0..=7));
                        ui.end_row();
                    }
                    Shape::Cube | Shape::Plane => {
                        slider_f(ui, "Size", &mut p.size, 20.0);
                        ui.label("Divisions per side");
                        ui.add(egui::Slider::new(&mut p.segments, 1..=256).logarithmic(true));
                        ui.end_row();
                    }
                    Shape::Cylinder | Shape::Cone => {
                        slider_f(ui, "Radius", &mut p.size, 10.0);
                        slider_f(ui, "Height", &mut p.second, 20.0);
                        ui.label("Segments around");
                        ui.add(egui::Slider::new(&mut p.segments, 3..=256).logarithmic(true));
                        ui.end_row();
                        ui.label("Rows up the side");
                        ui.add(egui::Slider::new(&mut p.rows, 1..=128).logarithmic(true));
                        ui.end_row();
                        ui.label("Ends");
                        ui.checkbox(&mut p.caps, if p.shape == Shape::Cone { "Closed base" } else { "Closed caps" });
                        ui.end_row();
                    }
                    Shape::Torus => {
                        slider_f(ui, "Ring radius", &mut p.size, 10.0);
                        slider_f(ui, "Tube radius", &mut p.second, 5.0);
                        ui.label("Segments around the ring");
                        ui.add(egui::Slider::new(&mut p.segments, 3..=512).logarithmic(true));
                        ui.end_row();
                        ui.label("Segments around the tube");
                        ui.add(egui::Slider::new(&mut p.rows, 3..=256).logarithmic(true));
                        ui.end_row();
                    }
                }
                ui.label("Sculpting");
                ui.checkbox(&mut p.remesh, "Even triangles (voxel remesh)").on_hover_text(
                    "Remesh into evenly sized triangles, ready to sculpt. Off keeps the clean, low-poly shape.",
                );
                ui.end_row();
                if p.remesh {
                    ui.label("Remesh resolution");
                    ui.add(egui::Slider::new(&mut p.resolution, 16..=320));
                    ui.end_row();
                }
            });
            ui.label(
                RichText::new(if p.remesh {
                    format!("{tris} triangles before remeshing")
                } else {
                    format!("{tris} triangles")
                })
                .weak()
                .small(),
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                accept |= ui.button("Accept").on_hover_text("Add it as a new piece (Enter)").clicked();
                cancel |= ui.button("Cancel").on_hover_text("Discard it (Esc)").clicked();
            });
        });
    if cancel {
        ed.cancel_primitive();
    } else if accept {
        ed.accept_primitive();
    }
}

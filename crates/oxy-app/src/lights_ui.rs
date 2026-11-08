//! The Lights window and the light markers drawn over the viewport.

use crate::editor::Editor;
use crate::tips::{self, Cmd};
use egui::{Color32, RichText, Stroke, Ui};
use glam::{Vec2, Vec3};
use oxy_render::lighting::{self, LightKind, Lighting};

/// Tooltips for this window.
mod tip {
    pub const LIGHT_ON: &str = "Turn this light on or off.";
    pub const KIND: &str = "Sun: parallel light from a direction. Point: light from a spot in every direction. Spot: a cone of light aimed at a point.";
    pub const FOLLOW: &str = "Keep this light where it is relative to the view, so the model turns under it as you orbit. Off: it stays put in the scene.";
    pub const INTENSITY: &str = "How bright this light is.";
    pub const COLOR: &str = "Light color.";
    pub const WARMTH: &str = "Set the color to a warm (3200 K), daylight (5600 K) or cool (9000 K) white.";
    pub const AZIMUTH: &str = "Which way the sun shines from, around the model (0 = from the front).";
    pub const ELEVATION: &str = "How high the sun is above the horizon.";
    pub const FALLOFF: &str = "How fast the light dims with distance (2 is natural, 0 never dims).";
    pub const POSITION: &str = "Where the light is, in model sizes from the middle of the scene.";
    pub const CONE: &str = "How wide the spot's cone is (half-angle, degrees).";
    pub const CONE_EDGE: &str = "How much of the cone fades out softly at its edge.";
    pub const AIM: &str = "Aim the spot at the middle of the scene.";
    pub const SHADOWS: &str = "This light casts shadows (suns and spots; point lights don't yet).";
    pub const SOFTNESS: &str = "How soft the shadow edges are.";
    pub const SHADOWS_ALL: &str = "Shadows at all. Each light that casts them redraws the model once more per frame.";
    pub const MAP_SIZE: &str = "Shadow detail: larger is sharper but uses more video memory (16 MB per light at 2048).";
    pub const EXPOSURE: &str = "Overall brightness, in stops.";
    pub const AMBIENT: &str = "Light from the sky above and the ground below, everywhere.";
    pub const SKY: &str = "Color of the light from above.";
    pub const GROUND: &str = "Color of the light from below.";
    pub const RIM: &str = "Brighten edges that turn away from you, to separate the model from the background.";
    pub const SURFACE: &str = "The model's color under the lights (painted color and face groups still show).";
    pub const ROUGHNESS: &str = "Rough surfaces spread highlights wide; smooth ones keep them small and sharp.";
    pub const SPECULAR: &str = "How strong highlights are.";
    pub const RESET: &str = "Put every light and setting back to the defaults.";
    pub const MARKERS: &str = "While this window is open, numbered dots show the lights in the viewport: drag one to move its light. A sun's dot sets its direction; a spot's diamond sets where it aims.";
}

fn color_edit(ui: &mut Ui, c: &mut [f32; 3]) -> egui::Response {
    ui.color_edit_button_rgb(c)
}

/// The Lights window (View > Shading > Lights…).
pub fn window(ctx: &egui::Context, ed: &mut Editor) {
    if !ed.show_lights {
        return;
    }
    let mut open = true;
    egui::Window::new("Lights")
        .open(&mut open)
        .resizable(false)
        .collapsible(true)
        .default_pos(egui::pos2(300.0, 110.0))
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().max_height(ctx.content_rect().height() * 0.75).show(ui, |ui| {
                controls(ui, ed);
            });
        });
    ed.show_lights = open;
}

fn controls(ui: &mut Ui, ed: &mut Editor) {
    let l = &mut ed.view.lighting;
    tips::check(ui, &mut l.enabled, "Light the viewport", Cmd::LightViewport);
    ui.label(RichText::new(tip::MARKERS).weak().small());
    ui.add_space(4.0);
    egui::Grid::new("lights_global").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
        ui.label("Exposure");
        tips::hover(ui.add(egui::Slider::new(&mut l.exposure, -3.0..=3.0)), tip::EXPOSURE);
        ui.end_row();
        ui.label("Sky and ground");
        ui.horizontal(|ui| {
            tips::hover(ui.add(egui::Slider::new(&mut l.ambient, 0.0..=1.5)), tip::AMBIENT);
            tips::hover(color_edit(ui, &mut l.sky), tip::SKY);
            tips::hover(color_edit(ui, &mut l.ground), tip::GROUND);
        });
        ui.end_row();
        ui.label("Rim light");
        tips::hover(ui.add(egui::Slider::new(&mut l.rim, 0.0..=1.0)), tip::RIM);
        ui.end_row();
        ui.label("Surface");
        tips::hover(color_edit(ui, &mut l.surface), tip::SURFACE);
        ui.end_row();
        ui.label("Roughness");
        tips::hover(ui.add(egui::Slider::new(&mut l.roughness, 0.05..=1.0)), tip::ROUGHNESS);
        ui.end_row();
        ui.label("Specular");
        tips::hover(ui.add(egui::Slider::new(&mut l.specular, 0.0..=1.0)), tip::SPECULAR);
        ui.end_row();
        ui.label("Shadows");
        ui.horizontal(|ui| {
            tips::hover(ui.checkbox(&mut l.shadows, "On"), tip::SHADOWS_ALL);
            egui::ComboBox::from_id_salt("shadow_size").selected_text(format!("{} px", l.shadow_size)).show_ui(
                ui,
                |ui| {
                    for s in [1024, 2048, 4096] {
                        tips::hover(ui.selectable_value(&mut l.shadow_size, s, format!("{s} px")), tip::MAP_SIZE);
                    }
                },
            );
        });
        ui.end_row();
    });
    ui.add_space(6.0);
    for i in 0..lighting::MAX_LIGHTS {
        let light = &mut l.lights[i];
        let title = format!(
            "{}  Light {}: {}{}",
            if light.on { "●" } else { "○" },
            i + 1,
            light.kind.label(),
            if light.on { "" } else { " (off)" }
        );
        egui::CollapsingHeader::new(title).id_salt(("light", i)).default_open(i == 0).show(ui, |ui| {
            egui::Grid::new(("light_grid", i)).num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label("On");
                ui.horizontal(|ui| {
                    tips::hover(ui.checkbox(&mut light.on, ""), tip::LIGHT_ON);
                    for k in LightKind::ALL {
                        tips::hover(ui.selectable_value(&mut light.kind, k, k.label()), tip::KIND);
                    }
                });
                ui.end_row();
                ui.label("Follow the view");
                tips::hover(ui.checkbox(&mut light.follow_camera, ""), tip::FOLLOW);
                ui.end_row();
                ui.label("Intensity");
                tips::hover(ui.add(egui::Slider::new(&mut light.intensity, 0.0..=10.0)), tip::INTENSITY);
                ui.end_row();
                ui.label("Color");
                ui.horizontal(|ui| {
                    tips::hover(color_edit(ui, &mut light.color), tip::COLOR);
                    for (name, k) in [("Warm", 3200.0), ("Daylight", 5600.0), ("Cool", 9000.0)] {
                        if tips::hover(ui.small_button(name), tip::WARMTH).clicked() {
                            light.color = lighting::kelvin_to_rgb(k);
                        }
                    }
                });
                ui.end_row();
                match light.kind {
                    LightKind::Sun => {
                        ui.label("Direction");
                        ui.horizontal(|ui| {
                            tips::hover(
                                ui.add(
                                    egui::DragValue::new(&mut light.azimuth)
                                        .range(-180.0..=180.0)
                                        .suffix("°")
                                        .speed(1.0),
                                ),
                                tip::AZIMUTH,
                            );
                            tips::hover(
                                ui.add(
                                    egui::DragValue::new(&mut light.elevation)
                                        .range(-89.0..=89.0)
                                        .suffix("° up")
                                        .speed(1.0),
                                ),
                                tip::ELEVATION,
                            );
                        });
                        ui.end_row();
                    }
                    LightKind::Point | LightKind::Spot => {
                        ui.label("Position");
                        ui.horizontal(|ui| {
                            for (axis, v) in ["x", "y", "z"].iter().zip(light.position.iter_mut()) {
                                tips::hover(ui.add(egui::DragValue::new(v).speed(0.01).prefix(*axis)), tip::POSITION);
                            }
                        });
                        ui.end_row();
                        ui.label("Falloff");
                        tips::hover(ui.add(egui::Slider::new(&mut light.falloff, 0.0..=3.0)), tip::FALLOFF);
                        ui.end_row();
                    }
                }
                if light.kind == LightKind::Spot {
                    ui.label("Cone");
                    ui.horizontal(|ui| {
                        tips::hover(ui.add(egui::Slider::new(&mut light.cone, 2.0..=80.0).suffix("°")), tip::CONE);
                        if tips::hover(ui.small_button("Aim at the model"), tip::AIM).clicked() {
                            light.aim = [0.0, 0.0, 0.0];
                            light.follow_camera = false;
                        }
                    });
                    ui.end_row();
                    ui.label("Soft edge");
                    tips::hover(ui.add(egui::Slider::new(&mut light.cone_edge, 0.0..=1.0)), tip::CONE_EDGE);
                    ui.end_row();
                }
                if light.kind != LightKind::Point {
                    ui.label("Shadows");
                    ui.horizontal(|ui| {
                        tips::hover(ui.checkbox(&mut light.shadows, ""), tip::SHADOWS);
                        ui.add_enabled_ui(light.shadows, |ui| {
                            tips::hover(
                                ui.add(egui::Slider::new(&mut light.softness, 0.0..=1.0).text("Soft")),
                                tip::SOFTNESS,
                            );
                        });
                    });
                    ui.end_row();
                }
            });
        });
    }
    ui.add_space(6.0);
    if tips::hover(ui.button("Reset lights"), tip::RESET).clicked() {
        *l = Lighting { enabled: l.enabled, ..Lighting::default() };
    }
}

/// What a marker drag moves.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Handle {
    Light(usize),
    Aim(usize),
}

/// Numbered dots for the lights over the viewport (while the Lights window
/// is open and lighting is on). Dragging one moves its light; they sit on
/// egui's top layer, so a drag never reaches the model.
pub fn markers(ctx: &egui::Context, ed: &mut Editor, canvas: egui::Rect) {
    if !(ed.show_lights && ed.view.lighting.enabled) {
        return;
    }
    let (scene, cam) = ed.light_frames();
    let accent = ed.accent_color();
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("light_markers")));
    let painter = painter.with_clip_rect(canvas);
    let center = crate::ui::project_point(ed, scene.center);
    let mut handles: Vec<(Handle, egui::Pos2)> = Vec::new();
    for (i, light) in ed.view.lighting.lights.iter().enumerate() {
        if !light.on {
            continue;
        }
        let Some(p) = crate::ui::project_point(ed, light.marker(&scene, &cam)) else { continue };
        let p = canvas.shrink(10.0).clamp(p);
        let col = Color32::from_rgb(
            (light.color[0].powf(1.0 / 2.2) * 255.0) as u8,
            (light.color[1].powf(1.0 / 2.2) * 255.0) as u8,
            (light.color[2].powf(1.0 / 2.2) * 255.0) as u8,
        );
        match light.kind {
            LightKind::Sun => {
                if let Some(c) = center {
                    painter.line_segment([p, c], Stroke::new(1.0, col.gamma_multiply(0.35)));
                }
            }
            LightKind::Spot => {
                if let Some(a) = crate::ui::project_point(ed, light.world_aim(&scene, &cam)) {
                    let a = canvas.shrink(10.0).clamp(a);
                    painter.line_segment([p, a], Stroke::new(1.0, col.gamma_multiply(0.6)));
                    let d = 6.0;
                    painter.add(egui::Shape::convex_polygon(
                        vec![
                            a + egui::vec2(0.0, -d),
                            a + egui::vec2(d, 0.0),
                            a + egui::vec2(0.0, d),
                            a + egui::vec2(-d, 0.0),
                        ],
                        col.gamma_multiply(0.8),
                        Stroke::new(1.2, Color32::from_black_alpha(180)),
                    ));
                    handles.push((Handle::Aim(i), a));
                }
            }
            LightKind::Point => {}
        }
        painter.circle_filled(p, 10.0, Color32::from_black_alpha(170));
        painter.circle_filled(p, 8.0, col);
        painter.circle_stroke(p, 10.0, Stroke::new(1.5, accent));
        let text_col = if light.color.iter().sum::<f32>() > 1.6 { Color32::BLACK } else { Color32::WHITE };
        painter.text(p, egui::Align2::CENTER_CENTER, format!("{}", i + 1), egui::FontId::proportional(11.0), text_col);
        handles.push((Handle::Light(i), p));
    }
    // Interaction: an invisible area per handle, on top of the viewport.
    egui::Area::new(egui::Id::unique("light_marker_area")).order(egui::Order::Foreground).fixed_pos(canvas.min).show(
        ctx,
        |ui| {
            for (h, pos) in handles.iter().rev() {
                let rect = egui::Rect::from_center_size(*pos, egui::vec2(22.0, 22.0));
                let r = ui.interact(rect, egui::Id::unique(("light_handle", format!("{h:?}"))), egui::Sense::drag());
                let r = r.on_hover_text(match h {
                    Handle::Light(i) => format!("Light {}: drag to move it", i + 1),
                    Handle::Aim(i) => format!("Light {}: drag to change where it aims", i + 1),
                });
                if r.hovered() || r.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                }
                if r.dragged()
                    && let Some(pointer) = r.interact_pointer_pos()
                {
                    drag_handle(ed, *h, pointer);
                }
            }
        },
    );
}

/// Move a light's handle to follow the pointer (logical points).
fn drag_handle(ed: &mut Editor, handle: Handle, pointer: egui::Pos2) {
    let (scene, cam) = ed.light_frames();
    let ppp = ed.pixels_per_point.max(0.1);
    let px = Vec2::new(pointer.x * ppp - ed.canvas[0], pointer.y * ppp - ed.canvas[1]);
    let (o, d) = ed.camera.ray(px, ed.canvas_size());
    let forward = ed.camera.forward();
    // The point under the pointer on the view plane through `at`.
    let on_plane = |at: Vec3| {
        let denom = d.dot(forward);
        if denom.abs() < 1e-6 {
            return None;
        }
        let t = (at - o).dot(forward) / denom;
        (t > 0.0).then(|| o + d * t)
    };
    let light = match handle {
        Handle::Light(i) | Handle::Aim(i) => &mut ed.view.lighting.lights[i],
    };
    match (handle, light.kind) {
        (Handle::Light(_), LightKind::Sun) => {
            // The sun's dot moves on a sphere around the scene; its
            // direction is from the middle to the dot.
            let r = scene.radius() * 1.3;
            let oc = o - scene.center;
            let b = oc.dot(d);
            let c = oc.length_squared() - r * r;
            let disc = b * b - c;
            let hit = if disc >= 0.0 {
                o + d * (-b - disc.sqrt())
            } else {
                // Past the edge of the sphere: the nearest point on the ray.
                o + d * (-b)
            };
            light.set_sun_direction((hit - scene.center).normalize_or(Vec3::Y), &cam);
        }
        (Handle::Light(_), _) => {
            let at = light.world_position(&scene, &cam);
            if let Some(p) = on_plane(at) {
                light.set_world_position(p, &scene, &cam);
            }
        }
        (Handle::Aim(_), _) => {
            let at = light.world_aim(&scene, &cam);
            if let Some(p) = on_plane(at) {
                light.set_world_aim(p, &scene, &cam);
            }
        }
    }
}

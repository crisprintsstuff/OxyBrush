//! The Lights window and the light markers drawn over the viewport.

use crate::editor::Editor;
use crate::editor::LightHandle;
use crate::tips::{self, Cmd};
use egui::{Color32, RichText, Stroke, Ui};
use glam::Vec2;
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
        .default_pos(egui::pos2(ctx.content_rect().right() - 760.0, 110.0))
        .show(ctx, |ui| {
            {
                let h = (ctx.content_rect().height() - 200.0).max(240.0);
                egui::ScrollArea::vertical().min_scrolled_height(h).max_height(h)
            }
            .show(ui, |ui| {
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
        tips::hover(ui.add(crate::widgets::Bar(egui::Slider::new(&mut l.exposure, -3.0..=3.0))), tip::EXPOSURE);
        ui.end_row();
        ui.label("Sky and ground");
        ui.horizontal(|ui| {
            tips::hover(ui.add(crate::widgets::Bar(egui::Slider::new(&mut l.ambient, 0.0..=1.5))), tip::AMBIENT);
            tips::hover(color_edit(ui, &mut l.sky), tip::SKY);
            tips::hover(color_edit(ui, &mut l.ground), tip::GROUND);
        });
        ui.end_row();
        ui.label("Rim light");
        tips::hover(ui.add(crate::widgets::Bar(egui::Slider::new(&mut l.rim, 0.0..=1.0))), tip::RIM);
        ui.end_row();
        ui.label("Surface");
        tips::hover(color_edit(ui, &mut l.surface), tip::SURFACE);
        ui.end_row();
        ui.label("Roughness");
        tips::hover(ui.add(crate::widgets::Bar(egui::Slider::new(&mut l.roughness, 0.05..=1.0))), tip::ROUGHNESS);
        ui.end_row();
        ui.label("Specular");
        tips::hover(ui.add(crate::widgets::Bar(egui::Slider::new(&mut l.specular, 0.0..=1.0))), tip::SPECULAR);
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
        let title = format!("Light {}: {}{}", i + 1, light.kind.label(), if light.on { "" } else { " (off)" });
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
                tips::hover(
                    ui.add(crate::widgets::Bar(egui::Slider::new(&mut light.intensity, 0.0..=10.0))),
                    tip::INTENSITY,
                );
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
                        tips::hover(
                            ui.add(crate::widgets::Bar(egui::Slider::new(&mut light.falloff, 0.0..=3.0))),
                            tip::FALLOFF,
                        );
                        ui.end_row();
                    }
                }
                if light.kind == LightKind::Spot {
                    ui.label("Cone");
                    ui.horizontal(|ui| {
                        tips::hover(
                            ui.add(crate::widgets::Bar(egui::Slider::new(&mut light.cone, 2.0..=80.0).suffix("°"))),
                            tip::CONE,
                        );
                        if tips::hover(ui.small_button("Aim at the model"), tip::AIM).clicked() {
                            light.aim = [0.0, 0.0, 0.0];
                            light.follow_camera = false;
                        }
                    });
                    ui.end_row();
                    ui.label("Soft edge");
                    tips::hover(
                        ui.add(crate::widgets::Bar(egui::Slider::new(&mut light.cone_edge, 0.0..=1.0))),
                        tip::CONE_EDGE,
                    );
                    ui.end_row();
                }
                if light.kind != LightKind::Point {
                    ui.label("Shadows");
                    ui.horizontal(|ui| {
                        tips::hover(ui.checkbox(&mut light.shadows, ""), tip::SHADOWS);
                        ui.add_enabled_ui(light.shadows, |ui| {
                            tips::hover(
                                ui.add(crate::widgets::Bar(
                                    egui::Slider::new(&mut light.softness, 0.0..=1.0).text("Soft"),
                                )),
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

/// Numbered dots for the lights over the viewport, while the Lights window
/// is open and lighting is on. The editor handles dragging them (see
/// `Editor::light_handles`); they are drawn in the viewport's own layer, so
/// windows stay on top.
pub fn markers(ui: &Ui, ed: &Editor, canvas: egui::Rect) {
    let handles = ed.light_handles();
    if handles.is_empty() {
        return;
    }
    let (scene, _) = ed.light_frames();
    let painter = ui.painter_at(canvas);
    let accent = ed.accent_color();
    let ppp = ed.pixels_per_point.max(0.1);
    let to_ui = |v: Vec2| egui::pos2(v.x / ppp, v.y / ppp);
    let center = crate::ui::project_point(ed, scene.center);
    let held = match ed.drag {
        crate::editor::Drag::LightHandle(h) => Some(h),
        _ => None,
    };
    let at = |h: LightHandle| handles.iter().find(|(k, _)| *k == h).map(|(_, p)| to_ui(*p));
    for (h, p) in &handles {
        let p = to_ui(*p);
        let (LightHandle::Light(i) | LightHandle::Aim(i)) = *h;
        let light = &ed.view.lighting.lights[i];
        let col = Color32::from_rgb(
            (light.color[0].clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8,
            (light.color[1].clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8,
            (light.color[2].clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8,
        );
        let big = held == Some(*h);
        match h {
            LightHandle::Light(_) => {
                if light.kind == LightKind::Sun
                    && let Some(c) = center
                {
                    painter.line_segment([p, c], Stroke::new(1.0, col.gamma_multiply(0.4)));
                }
                if light.kind == LightKind::Spot
                    && let Some(a) = at(LightHandle::Aim(i))
                {
                    painter.line_segment([p, a], Stroke::new(1.0, col.gamma_multiply(0.6)));
                }
                let r = if big { 11.0 } else { 9.0 };
                painter.circle_filled(p, r + 2.0, Color32::from_black_alpha(170));
                painter.circle_filled(p, r, col);
                painter.circle_stroke(p, r + 2.0, Stroke::new(1.5, accent));
                let text = if light.color.iter().sum::<f32>() > 1.6 { Color32::BLACK } else { Color32::WHITE };
                painter.text(
                    p,
                    egui::Align2::CENTER_CENTER,
                    format!("{}", i + 1),
                    egui::FontId::proportional(11.0),
                    text,
                );
            }
            LightHandle::Aim(_) => {
                let d = if big { 8.0 } else { 6.0 };
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        p + egui::vec2(0.0, -d),
                        p + egui::vec2(d, 0.0),
                        p + egui::vec2(0.0, d),
                        p + egui::vec2(-d, 0.0),
                    ],
                    col,
                    Stroke::new(1.5, accent),
                ));
            }
        }
    }
}

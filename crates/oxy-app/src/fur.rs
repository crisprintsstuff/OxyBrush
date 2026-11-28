//! Non-destructive viewport fur on a piece's displayed surface.

use crate::editor::{Editor, JobResult};
use crate::instances::InstanceDraw;
use glam::{Mat4, Vec3};
use oxy_doc::Piece;
use oxy_history::Step;
use oxy_mesh::Mesh;
use oxy_mesh::fur::{Coat, Region, Settings};
use std::collections::HashMap;
use std::sync::Arc;

const STRAND_FLAG: u64 = 1 << 61;
const INSTANCE_FLAG: u64 = 1 << 63;

pub fn strand_host(id: u64) -> Option<u64> {
    (id >> 61 == 1).then_some(id & !STRAND_FLAG)
}

fn strand_mesh() -> Mesh {
    let mut points = Vec::new();
    for (height, half_width) in [(0.0f32, 0.50f32), (0.4, 0.39), (0.72, 0.22), (1.0, 0.015)] {
        let bend = 0.32 * height * height;
        points.push(Vec3::new(-half_width, height, bend));
        points.push(Vec3::new(half_width, height, bend));
    }
    let triangles = (0..6).step_by(2).flat_map(|i| [[i, i + 1, i + 2], [i + 1, i + 3, i + 2]]).collect();
    Mesh::new(points, triangles)
}

struct DrawCache {
    coat: Arc<Coat>,
    versions: [u64; 3],
    bound: bool,
    transforms: Arc<Vec<Mat4>>,
    generation: u64,
}

#[derive(Default)]
pub struct Preview {
    strand: Option<Mesh>,
    cache: HashMap<u64, DrawCache>,
    generation: u64,
}

impl Preview {
    pub fn strand_mut(&mut self) -> &mut Mesh {
        self.strand.get_or_insert_with(strand_mesh)
    }

    pub fn draws(&mut self, hosts: &[&Piece]) -> Vec<InstanceDraw> {
        self.cache.retain(|id, _| hosts.iter().any(|p| p.id == *id));
        let mut draws = Vec::new();
        for host in hosts {
            let Some(coat) = &host.fur else { continue };
            let versions = versions(&host.mesh);
            let old = self.cache.get(&host.id);
            let same_binding =
                old.is_some_and(|cached| Arc::ptr_eq(&cached.coat, coat) && cached.versions[0] == versions[0]);
            if !same_binding || old.is_some_and(|cached| cached.versions != versions) {
                let bound = if same_binding { old.expect("binding was checked").bound } else { coat.fits(&host.mesh) };
                let transforms = if bound { coat.transforms(&host.mesh) } else { Vec::new() };
                self.generation = self.generation.wrapping_add(1);
                self.cache.insert(
                    host.id,
                    DrawCache {
                        coat: Arc::clone(coat),
                        versions,
                        bound,
                        transforms: Arc::new(transforms),
                        generation: self.generation,
                    },
                );
            }
            let cached = &self.cache[&host.id];
            if !cached.transforms.is_empty() {
                draws.push(InstanceDraw {
                    key: INSTANCE_FLAG | host.id,
                    host: host.id,
                    source: STRAND_FLAG | host.id,
                    transforms: Arc::clone(&cached.transforms),
                    generation: cached.generation,
                });
            }
        }
        draws
    }
}

pub fn versions(mesh: &Mesh) -> [u64; 3] {
    [mesh.topology_version(), mesh.edit_version(), mesh.face_version()]
}

impl Editor {
    pub fn generate_fur(&mut self, settings: Settings) {
        self.pointer_up();
        let Some(piece) = self.scene.try_active() else { return };
        if piece.is_spheres() || piece.multires.is_some() || piece.link.is_some() || !settings.valid() {
            self.status = "Grow fur on a sculpt piece at a single subdivision level".into();
            return;
        }
        if let Some(old) = &piece.fur
            && old.settings != settings
            && old.settings.region == settings.region
            && old.settings.spacing == settings.spacing
            && old.settings.seed == settings.seed
            && old.fits(&piece.mesh)
        {
            let id = piece.id;
            let previous = Arc::clone(old);
            let mut resized = (**old).clone();
            resized.settings = settings;
            self.scene.active_mut().fur = Some(Arc::new(resized));
            self.history.record("Change fur dimensions", Step::Fur { piece: id, previous: Some(previous) });
            self.status = "Changed fur dimensions".into();
            return;
        }
        let mesh = piece.mesh.to_data();
        let source = versions(&piece.mesh);
        self.start_job("Grow fur coat".into(), Vec::new(), move || {
            let coat = Coat::generate(&mesh, settings);
            if coat.roots.is_empty() {
                return Err("The fur region has no visible surface".into());
            }
            Ok(JobResult::Fur { coat: Arc::new(coat), source })
        });
    }

    fn remove_fur(&mut self) {
        self.pointer_up();
        let Some(piece) = self.scene.try_active_mut() else { return };
        let id = piece.id;
        if let Some(previous) = piece.fur.take() {
            self.history.record("Remove fur coat", Step::Fur { piece: id, previous: Some(previous) });
            self.status = "Removed fur coat".into();
        }
    }
}

pub fn section(ui: &mut egui::Ui, ed: &mut Editor, busy: bool) {
    let Some(piece) = ed.scene.try_active() else { return };
    let stored = piece.fur.as_ref().map(|coat| coat.settings);
    let editable = !piece.is_spheres() && piece.multires.is_none() && piece.link.is_none();
    if !editable && stored.is_none() {
        ui.label("Select a sculpt piece at a single subdivision level to grow fur.");
        return;
    }
    let span = piece.mesh.bounds().extent().max_element().max(0.001);
    let key = ui.scope_id().with(("fur_settings", piece.id));
    let mut draft = ui
        .data_mut(|data| data.get_temp::<(Option<Settings>, Settings)>(key))
        .filter(|(original, _)| *original == stored)
        .map_or_else(|| stored.unwrap_or_else(|| Settings::for_span(span)), |(_, draft)| draft);
    let coat_needs_binding = piece.fur.as_ref().is_some_and(|coat| {
        if !editable {
            return true;
        }
        let topology = piece.mesh.topology_version();
        let binding = key.with("binding");
        ui.data_mut(|data| {
            if let Some((previous, version, fits)) = data.get_temp::<(Arc<Coat>, u64, bool)>(binding)
                && version == topology
                && Arc::ptr_eq(&previous, coat)
            {
                return !fits;
            }
            let fits = coat.fits(&piece.mesh);
            data.insert_temp(binding, (Arc::clone(coat), topology, fits));
            !fits
        })
    });
    let count = piece.fur.as_ref().map_or(0, |coat| coat.roots.len());
    ui.add_enabled_ui(!busy && editable, |ui| {
        let selected = match draft.region {
            Region::Whole => "Whole piece",
            Region::Masked => "Masked",
            Region::Group(_) => "Face group",
        };
        egui::ComboBox::from_id_salt("fur_region").selected_text(selected).show_ui(ui, |ui| {
            ui.selectable_value(&mut draft.region, Region::Whole, "Whole piece");
            ui.selectable_value(&mut draft.region, Region::Masked, "Masked");
            if ui.selectable_label(matches!(draft.region, Region::Group(_)), "Face group").clicked() {
                draft.region = Region::Group(1);
            }
        });
        if let Region::Group(group) = &mut draft.region {
            ui.horizontal(|ui| {
                ui.label("Face group");
                ui.add(egui::DragValue::new(group));
            });
        }
        ui.add(crate::widgets::Bar(
            egui::Slider::new(&mut draft.spacing, span * 0.001..=span * 0.15).logarithmic(true).text("Spacing"),
        ))
        .on_hover_text("Smaller spacing grows a denser coat.");
        ui.add(crate::widgets::Bar(egui::Slider::new(&mut draft.length, span * 0.005..=span * 0.2).text("Length")));
        ui.add(crate::widgets::Bar(
            egui::Slider::new(&mut draft.width, span * 0.0002..=span * 0.02).logarithmic(true).text("Width"),
        ));
        ui.horizontal(|ui| {
            ui.label("Seed");
            ui.add(egui::DragValue::new(&mut draft.seed));
        });
    });
    if coat_needs_binding {
        ui.label("The saved coat is paused until its surface is restored or you grow it again.");
    } else if stored.is_some() {
        ui.label(format!("{count} strands"));
    }
    ui.horizontal(|ui| {
        if ui.add_enabled(!busy && editable, egui::Button::new("Grow fur")).clicked() {
            ed.generate_fur(draft);
        }
        if stored.is_some() && ui.add_enabled(!busy, egui::Button::new("Remove coat")).clicked() {
            ed.remove_fur();
        }
    });
    ui.data_mut(|data| data.insert_temp(key, (stored, draft)));
}

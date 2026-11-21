//! Surface instances (RFC 0022, phase 1): copies of one piece (the source)
//! on the faces, points or a face group of another (the host).
//!
//! The scene keeps only the rule ([`oxy_doc::Instancer`] on the host); the
//! copies are worked out here, from the host's and the source's meshes as
//! they are now, and cached until either changes. The renderer draws them
//! with instancing, from the source's own vertex buffer.
//!
//! The source is an ordinary piece kept hidden. **Edit source** shows it
//! alone so every tool works on it as usual; leaving puts the scene back.

use crate::editor::Editor;
use glam::Mat4;
use oxy_doc::{Instancer, Piece};
use oxy_history::Step;
use oxy_mesh::instances::{self, Anchor, Settings};
use oxy_mesh::{Mesh, MeshData};
use std::sync::{Arc, mpsc};

/// Make real refuses above this many triangles (a mesh too big to sculpt).
pub const MAKE_REAL_MAX_TRIANGLES: usize = 40_000_000;

/// What the copies of one instancer were worked out from.
#[derive(Clone, PartialEq)]
struct Inputs {
    host_topology: u64,
    host_faces: u64,
    host_edits: u64,
    source: u64,
    source_topology: u64,
    source_edits: u64,
    settings: Settings,
}

#[derive(Clone, Copy, PartialEq)]
struct AnchorInputs {
    topology: u64,
    faces: u64,
    place: instances::Place,
    orient: instances::Orient,
    scatter_seed: Option<u32>,
}

/// Cached anchors and transforms of one instancer.
#[derive(Default)]
pub struct InstanceCache {
    anchors_for: Option<AnchorInputs>,
    pending: Option<(AnchorInputs, mpsc::Receiver<Vec<Anchor>>)>,
    anchors: Vec<Anchor>,
    inputs: Option<Inputs>,
    transforms: Arc<Vec<Mat4>>,
    /// Bumped whenever `transforms` changes, so the app uploads only then.
    pub generation: u64,
}

/// One instanced draw for the renderer.
pub struct InstanceDraw {
    /// Key of the renderer's instance buffer.
    pub key: u64,
    pub host: u64,
    pub source: u64,
    pub transforms: Arc<Vec<Mat4>>,
    pub generation: u64,
}

/// Edit source: the instancer whose source is shown alone, and what each
/// piece's visibility was, to put back.
pub struct SourceEdit {
    pub host: u64,
    pub source: u64,
    shown: Vec<(u64, bool)>,
}

/// The renderer key for instancer `index` of piece `host`.
pub fn draw_key(host: u64, index: usize) -> u64 {
    (host << 16) | (index as u64 & 0xffff)
}

impl Editor {
    /// Pieces that could be copied onto the active piece: any other piece
    /// that doesn't have copies of its own (no copies of copies) and isn't
    /// already copied somewhere (each source belongs to one instancer, so
    /// Edit source and Remove know which copies they change).
    pub fn instance_sources(&self) -> Vec<(u64, String)> {
        let host = self.scene.active().id;
        self.scene
            .pieces
            .iter()
            .filter(|p| {
                p.id != host && p.instancers.is_empty() && p.mesh.triangle_count() > 0 && !self.is_instance_source(p.id)
            })
            .map(|p| (p.id, p.name.clone()))
            .collect()
    }

    /// Whether piece `id` is the source of any instancer.
    pub fn is_instance_source(&self, id: u64) -> bool {
        self.scene.pieces.iter().any(|p| p.instancers.iter().any(|i| i.source == id))
    }

    /// The host whose instancer uses piece `id` as source, if any.
    pub fn instance_host_of(&self, id: u64) -> Option<&Piece> {
        self.scene.pieces.iter().find(|p| p.instancers.iter().any(|i| i.source == id))
    }

    /// Copy piece `source` onto the active piece's surface: a new
    /// instancer with default settings, sized for the host. The source is
    /// hidden. One undo step.
    pub fn add_instancer(&mut self, source: u64) {
        self.pointer_up();
        let host = self.scene.active();
        let host_id = host.id;
        if source == host_id || self.scene.get(source).is_none() {
            return;
        }
        if self.is_instance_source(source) {
            self.status = "That piece is already copied; Duplicate it to copy it somewhere else".into();
            return;
        }
        if self.is_instance_source(host_id) {
            self.status = "A source of copies can't have copies of its own".into();
            return;
        }
        if self.scene.get(source).is_some_and(|p| !p.instancers.is_empty()) {
            self.status = "That piece has copies of its own; copies of copies aren't supported".into();
            return;
        }
        let previous = host.instancers.clone();
        let settings = Settings { size_mode: instances::SizeMode::Face, ..Settings::default() };
        let source_piece = self.scene.get_mut(source).expect("checked");
        let was_visible = source_piece.visible;
        source_piece.visible = false;
        let source_name = source_piece.name.clone();
        let host = self.scene.active_mut();
        host.instancers.push(Instancer { source, settings });
        let host_name = host.name.clone();
        self.history.record_compound(
            "Add surface instances",
            vec![Step::Visible { piece: source, previous: was_visible }, Step::Instancers { piece: host_id, previous }],
        );
        let n = self.instance_count(host_id, self.scene.active().instancers.len() - 1);
        self.status = format!("Copied {source_name} onto {host_name}: {} copies", crate::ui::fmt_count(n));
    }

    /// Change instancer `index` of the active piece. The change is applied
    /// at once; the undo step is recorded when the edit ends
    /// ([`Self::commit_instancer_edit`]), so dragging a slider is one step.
    pub fn set_instancer_settings(&mut self, index: usize, settings: Settings) {
        let host = self.scene.active();
        let id = host.id;
        let Some(current) = host.instancers.get(index) else { return };
        if current.settings == settings {
            return;
        }
        if self.instancer_before.as_ref().is_none_or(|(h, _)| *h != id) {
            let before = host.instancers.clone();
            self.commit_instancer_edit();
            self.instancer_before = Some((id, before));
        }
        self.scene.active_mut().instancers[index].settings = settings;
    }

    /// Record the settings edit in progress, if any, as one undo step.
    pub fn commit_instancer_edit(&mut self) {
        let Some((piece, previous)) = self.instancer_before.take() else { return };
        let changed = self.scene.get(piece).is_some_and(|p| p.instancers != previous);
        if changed {
            self.history.record("Change surface instances", Step::Instancers { piece, previous });
        }
    }

    /// Remove instancer `index` of the active piece. Its source piece is
    /// shown again, so nothing disappears. One undo step.
    pub fn remove_instancer(&mut self, index: usize) {
        self.pointer_up();
        if self.editing_source.is_some() {
            self.end_edit_source();
        }
        let host = self.scene.active();
        let host_id = host.id;
        let Some(ins) = host.instancers.get(index) else { return };
        let source = ins.source;
        let previous = host.instancers.clone();
        self.scene.active_mut().instancers.remove(index);
        let mut steps = vec![Step::Instancers { piece: host_id, previous }];
        // Show the source again unless another instancer still uses it.
        if !self.is_instance_source(source)
            && let Some(p) = self.scene.get_mut(source)
        {
            steps.push(Step::Visible { piece: source, previous: p.visible });
            p.visible = true;
        }
        self.history.record_compound("Remove surface instances", steps);
        self.status = "Removed the copies; their source is a piece again".into();
    }

    /// Show only the source of instancer `index` of the active piece, and
    /// make it the active piece, to sculpt, paint or reshape it. Every
    /// copy follows. [`Self::end_edit_source`] puts the scene back.
    pub fn begin_edit_source(&mut self, index: usize) {
        self.pointer_up();
        if self.editing_source.is_some() {
            self.end_edit_source();
        }
        let host = self.scene.active();
        let Some(ins) = host.instancers.get(index) else { return };
        let (host, source) = (host.id, ins.source);
        let Some(si) = self.scene.index_of(source) else { return };
        let shown = self.scene.pieces.iter().map(|p| (p.id, p.visible)).collect();
        for p in self.scene.pieces.iter_mut() {
            p.visible = p.id == source;
        }
        self.scene.set_active(si);
        self.editing_source = Some(SourceEdit { host, source, shown });
        self.status = format!("Editing {}: every copy follows. Done returns to the scene.", self.scene.active().name);
    }

    /// Leave Edit source: pieces shown as before, the source hidden again,
    /// and the host active.
    pub fn end_edit_source(&mut self) {
        let Some(edit) = self.editing_source.take() else { return };
        self.pointer_up();
        for p in self.scene.pieces.iter_mut() {
            if let Some(&(_, v)) = edit.shown.iter().find(|(id, _)| *id == p.id) {
                p.visible = v;
            }
            // Pieces added while editing stay shown.
        }
        if self.is_instance_source(edit.source)
            && let Some(p) = self.scene.get_mut(edit.source)
        {
            p.visible = false;
        }
        if let Some(i) = self.scene.index_of(edit.host) {
            self.scene.set_active(i);
        }
        self.status = "Back to the scene".into();
    }

    /// Copies of instancer `index` of piece `host`, as the renderer draws them.
    pub fn instance_count(&mut self, host: u64, index: usize) -> usize {
        self.instance_transforms(host, index).map_or(0, |t| t.len())
    }

    /// Each copy's transform for instancer `index` of piece `host`, worked
    /// out again only when the host, the source or the settings changed.
    pub fn instance_transforms(&mut self, host: u64, index: usize) -> Option<Arc<Vec<Mat4>>> {
        let doc = &mut self.documents[self.active_document];
        let hp = doc.scene.get(host)?;
        let ins = hp.instancers.get(index)?;
        let sp = doc.scene.get(ins.source)?;
        let inputs = Inputs {
            host_topology: hp.mesh.topology_version(),
            host_faces: hp.mesh.face_version(),
            host_edits: hp.mesh.edit_version(),
            source: ins.source,
            source_topology: sp.mesh.topology_version(),
            source_edits: sp.mesh.edit_version(),
            settings: ins.settings,
        };
        let cache = doc.instance_cache.entry(draw_key(host, index)).or_default();
        if cache.inputs.as_ref() == Some(&inputs) {
            return Some(cache.transforms.clone());
        }
        let s = &ins.settings;
        let scatter = if let instances::Place::Scatter(spacing) = s.place { Some(spacing) } else { None };
        let anchors_for = AnchorInputs {
            topology: inputs.host_topology,
            faces: inputs.host_faces,
            // The current mask is sampled in transforms, so a threshold
            // change can reuse the same polygon anchors.
            place: if matches!(s.place, instances::Place::Mask(_)) { instances::Place::Faces } else { s.place },
            orient: s.orient,
            scatter_seed: scatter.map(|_| s.seed),
        };
        if let Some(spacing) = scatter {
            if let Some((key, result)) = &cache.pending {
                match result.try_recv() {
                    Ok(anchors) if *key == anchors_for => {
                        cache.anchors = anchors;
                        cache.anchors_for = Some(anchors_for);
                    }
                    Ok(_) | Err(mpsc::TryRecvError::Disconnected) => {}
                    Err(mpsc::TryRecvError::Empty) => return None,
                }
                cache.pending = None;
            }
            if cache.anchors_for != Some(anchors_for) {
                let (sender, result) = mpsc::channel();
                let positions = hp.mesh.positions.clone();
                let triangles = hp.mesh.triangles.clone();
                let hidden = hp.mesh.hidden().to_vec();
                let (seed, orient) = (s.seed, s.orient);
                rayon::spawn(move || {
                    let anchors = instances::scatter_anchors(&positions, &triangles, &hidden, spacing, seed, orient);
                    let _ = sender.send(anchors);
                });
                cache.pending = Some((anchors_for, result));
                cache.inputs = None;
                return None;
            }
        } else {
            cache.pending = None;
            if cache.anchors_for != Some(anchors_for) {
                cache.anchors = instances::anchors(&hp.mesh, s);
                cache.anchors_for = Some(anchors_for);
            }
        }
        cache.transforms = Arc::new(instances::transforms(&hp.mesh, &sp.mesh, &cache.anchors, s));
        cache.inputs = Some(inputs);
        cache.generation += 1;
        Some(cache.transforms.clone())
    }

    /// Everything the renderer should draw instanced this frame: the
    /// copies on every shown host, from sources that still exist.
    pub fn instance_draws(&mut self) -> Vec<InstanceDraw> {
        let list: Vec<(u64, usize, u64)> = self
            .scene
            .pieces
            .iter()
            .filter(|p| p.visible)
            .flat_map(|p| p.instancers.iter().enumerate().map(move |(i, ins)| (p.id, i, ins.source)))
            .collect();
        let mut out = Vec::with_capacity(list.len());
        for (host, index, source) in list {
            let Some(transforms) = self.instance_transforms(host, index) else { continue };
            let key = draw_key(host, index);
            let generation = self.instance_cache.get(&key).map_or(0, |c| c.generation);
            out.push(InstanceDraw { key, host, source, transforms, generation });
        }
        // Forget copies of pieces that are gone.
        let doc = &mut self.documents[self.active_document];
        let scene = &doc.scene;
        doc.instance_cache.retain(|key, _| {
            scene.get(key >> 16).is_some_and(|piece| piece.visible && (key & 0xffff) < piece.instancers.len() as u64)
        });
        out
    }

    pub fn instance_scatter_pending(&self) -> bool {
        self.documents[self.active_document].instance_cache.values().any(|cache| cache.pending.is_some())
    }

    /// Triangles drawn for instancer `index` of the active piece.
    pub fn instance_triangles(&mut self, index: usize) -> usize {
        let host = self.scene.active();
        let Some(source) = host.instancers.get(index).and_then(|i| self.scene.get(i.source)) else { return 0 };
        let per = source.mesh.triangle_count();
        let id = host.id;
        per * self.instance_count(id, index)
    }

    /// Turn the copies of instancer `index` of the active piece into a new
    /// piece of ordinary mesh, one face group per copy. The instancer and
    /// its source stay, so it can be made real again after changes.
    pub fn make_instances_real(&mut self, index: usize) {
        self.pointer_up();
        let host = self.scene.active();
        let host_id = host.id;
        let host_name = host.name.clone();
        let Some(ins) = host.instancers.get(index) else { return };
        let source_id = ins.source;
        let Some(per) = self.scene.get(source_id).map(|s| s.mesh.triangle_count()) else { return };
        let Some(transforms) = self.instance_transforms(host_id, index) else { return };
        let source = self.scene.get(source_id).expect("checked");
        let source_name = source.name.clone();
        let total = per * transforms.len();
        if transforms.is_empty() {
            self.status = "There are no copies to make real".into();
            return;
        }
        if total > MAKE_REAL_MAX_TRIANGLES {
            self.status = format!(
                "Too many triangles to make real ({}); use fewer copies or a lighter source",
                crate::ui::fmt_count(total)
            );
            return;
        }
        let source_data: MeshData = source.mesh.to_data();
        let name = format!("{source_name} on {host_name}");
        self.start_job("Make real".into(), Vec::new(), move || {
            let source = Mesh::from_data(source_data);
            let copies = transforms.len();
            let mesh = Mesh::from_data(instances::realize(&source, &transforms));
            Ok(crate::editor::JobResult::MadeReal(name, mesh, copies))
        });
    }
}

//! Linked copies (#327): a piece that is a copy of another (the original),
//! moved, turned, scaled or mirrored, and kept so. Sculpt, paint or remesh
//! the original and every copy follows; move a copy with the transform
//! gizmo and it stays linked, in its new place.
//!
//! A copy is an ordinary piece with its own mesh, so drawing, picking,
//! saving and exporting need nothing new; [`Editor::sync_links`] rebuilds
//! it from the original whenever the original changes. The link is
//! [`oxy_doc::PieceLink`]: the original's id and the transform from the
//! original to the copy. When only the copy changed (a gizmo move, or undo
//! of one), the transform is worked out again from the two meshes.
//!
//! Copies can't be sculpted themselves (the brushes say so): every change
//! comes from the original, so there's one shape to edit. **Unlink** makes
//! a copy an ordinary piece. Moving the original moves its copies too
//! (they are its shape, wherever it is); a mirrored copy mirrors the move.

use crate::editor::Editor;
use glam::{DMat4, DVec3, DVec4, Mat4, Vec3};
use oxy_doc::{Piece, PieceLink};
use oxy_history::Step;
use oxy_mesh::Mesh;
use oxy_sculpt::Axis;
use rayon::prelude::*;

/// What a copy was last synced from: the versions of both meshes then,
/// and which point of the original each point of the copy is.
#[derive(Debug, Clone, Default)]
pub(crate) struct LinkSeen {
    source: (u64, u64, u64),
    copy: (u64, u64, u64),
    map: Vec<u32>,
    /// The copy's point for each point of the original (inverse of `map`).
    inverse: Vec<u32>,
    /// The original's points, mask and color as last synced, to find what
    /// a stroke changed and update only that part of the copy.
    last: Vec<Vec3>,
    last_mask: Vec<f32>,
    last_color: Vec<Vec3>,
}

impl LinkSeen {
    fn new(source: &Mesh, copy: &Mesh, map: Vec<u32>) -> Self {
        let mut inverse = vec![u32::MAX; source.vertex_count()];
        for (c, &s) in map.iter().enumerate() {
            if let Some(slot) = inverse.get_mut(s as usize) {
                *slot = c as u32;
            }
        }
        LinkSeen {
            source: versions(source),
            copy: versions(copy),
            map,
            inverse,
            last: source.positions.clone(),
            last_mask: source.mask.clone(),
            last_color: source.color.clone(),
        }
    }
}

/// A mesh's (topology, edit, face) versions.
fn versions(m: &Mesh) -> (u64, u64, u64) {
    (m.topology_version(), m.edit_version(), m.face_version())
}

/// The copy's mesh for `source` moved by `t` (winding flipped when `t`
/// mirrors, so the copy faces out), and which source point each of its
/// points is.
pub(crate) fn build_copy(source: &Mesh, t: Mat4) -> (Mesh, Vec<u32>) {
    let mut data = source.to_data();
    data.positions.par_iter_mut().for_each(|p| *p = t.transform_point3(*p));
    if t.determinant() < 0.0 {
        data.flip_winding();
    }
    let mesh = Mesh::from_data(data);
    let map = mesh.source_vertices().to_vec();
    (mesh, map)
}

/// The affine transform that best takes `from` to `to` (least squares),
/// and the largest distance it leaves between them. None when the points
/// don't pin one down (fewer than four, or all in a plane).
pub(crate) fn fit_affine(from: &[Vec3], to: &[Vec3]) -> Option<(Mat4, f32)> {
    if from.len() < 4 || from.len() != to.len() {
        return None;
    }
    // Normal equations about the centroids, in f64: M = Σ d dᵀ (3×3),
    // N = Σ d eᵀ, A = M⁻¹ N, then the translation.
    let n = from.len() as f64;
    let ca = from.iter().map(|p| p.as_dvec3()).sum::<DVec3>() / n;
    let cb = to.iter().map(|p| p.as_dvec3()).sum::<DVec3>() / n;
    let (mut m, mut nn) = (glam::DMat3::ZERO, glam::DMat3::ZERO);
    for (a, b) in from.iter().zip(to) {
        let (d, e) = (a.as_dvec3() - ca, b.as_dvec3() - cb);
        m += glam::DMat3::from_cols(d * d.x, d * d.y, d * d.z);
        nn += glam::DMat3::from_cols(e * d.x, e * d.y, e * d.z);
    }
    let scale = m.x_axis.x + m.y_axis.y + m.z_axis.z;
    if m.determinant().abs() <= 1e-12 * scale.powi(3).max(1e-300) {
        return None;
    }
    let linear = nn * m.inverse();
    let translation = cb - linear * ca;
    let t = DMat4::from_cols(
        linear.x_axis.extend(0.0),
        linear.y_axis.extend(0.0),
        linear.z_axis.extend(0.0),
        DVec4::new(translation.x, translation.y, translation.z, 1.0),
    )
    .as_mat4();
    let worst = from.par_iter().zip(to).map(|(a, b)| t.transform_point3(*a).distance(*b)).reduce(|| 0.0, f32::max);
    Some((t, worst))
}

impl Editor {
    /// Add a linked copy of the active piece (mirrored across the piece's
    /// symmetry plane when an axis is given), one undo step.
    pub fn duplicate_linked(&mut self, mirror_axis: Option<Axis>) {
        if !self.need_piece() {
            return;
        }
        let src = self.scene.active();
        // A copy of a copy links to the same original.
        let (source_id, base) = match src.link {
            Some(l) => (l.source, l.transform),
            None => (src.id, Mat4::IDENTITY),
        };
        let transform = match mirror_axis {
            Some(axis) => {
                let centre = src.symmetry_center.unwrap_or(Vec3::ZERO);
                let scale = Vec3::ONE - 2.0 * axis.vector();
                Mat4::from_translation(centre) * Mat4::from_scale(scale) * Mat4::from_translation(-centre) * base
            }
            None => base,
        };
        let Some(source) = self.scene.get(source_id) else { return };
        let (mesh, map) = build_copy(&source.mesh, transform);
        let base_name = if mirror_axis.is_some() {
            format!("{} (linked mirror)", source.name)
        } else {
            format!("{} (linked)", source.name)
        };
        let name = self.scene.unique_name(&base_name);
        let piece = Piece {
            parent: src.parent,
            symmetry_center: src.symmetry_center,
            material: source.material.clone(),
            link: Some(PieceLink { source: source_id, transform }),
            ..Piece::new(name.clone(), mesh)
        };
        let (id, seen) = (piece.id, LinkSeen::new(&source.mesh, &piece.mesh, map));
        self.link_seen.insert(id, seen);
        self.add_piece(piece, if mirror_axis.is_some() { "Mirror duplicate linked" } else { "Duplicate linked" });
        self.status = format!(
            "Created {name}: it follows {} until you unlink it",
            self.scene.get(source_id).map_or("", |p| p.name.as_str())
        );
    }

    /// Make the active piece an ordinary piece again, one undo step.
    pub fn unlink_active(&mut self) {
        let Some(piece) = self.scene.try_active() else { return };
        let Some(link) = piece.link else {
            self.status = "This piece isn't a linked copy".into();
            return;
        };
        let (id, name) = (piece.id, piece.name.clone());
        self.scene.active_mut().link = None;
        self.link_seen.remove(&id);
        self.history.record(format!("Unlink {name}"), Step::Link { piece: id, previous: Some(link) });
        self.status = format!("{name} is an ordinary piece now");
    }

    /// The original the active piece is a linked copy of, by name.
    pub fn active_link_source(&self) -> Option<String> {
        let link = self.scene.try_active()?.link?;
        Some(self.scene.get(link.source)?.name.clone())
    }

    /// Bring every linked copy up to date (every frame, and after edits in
    /// tests): rebuild it from its original when that changed, or work its
    /// transform out again when only the copy moved.
    pub fn sync_links(&mut self) {
        let doc = &mut self.documents[self.active_document];
        let scene = &mut doc.scene;
        let seen = &mut doc.link_seen;
        seen.retain(|id, _| scene.get(*id).is_some_and(|p| p.link.is_some()));
        for i in 0..scene.pieces.len() {
            let Some(link) = scene.pieces[i].link else { continue };
            // While the original is gone (deleted, until undo brings it
            // back), the copy stays as it is.
            let Some(si) = scene.index_of(link.source) else { continue };
            if si == i {
                continue;
            }
            let id = scene.pieces[i].id;
            let (src_v, copy_v) = (versions(&scene.pieces[si].mesh), versions(&scene.pieces[i].mesh));
            let state = seen.get(&id);
            if state.is_some_and(|s| s.source == src_v && s.copy == copy_v) {
                continue;
            }
            let source_changed = state.is_none_or(|s| s.source != src_v);
            // Neither mesh's points or faces were rebuilt since the last sync.
            let same_points = state.is_some_and(|s| {
                s.map.len() == scene.pieces[i].mesh.vertex_count()
                    && (s.source.0, s.source.2) == (src_v.0, src_v.2)
                    && s.copy.0 == copy_v.0
            });
            if !source_changed && same_points {
                // Only the copy changed: it was moved (or the move undone).
                let map = &state.expect("same_points").map;
                let from: Vec<Vec3> = map.iter().map(|&v| scene.pieces[si].mesh.positions[v as usize]).collect();
                let size = scene.pieces[si].mesh.bounds().extent().max_element().max(1e-6);
                if let Some((t, worst)) = fit_affine(&from, &scene.pieces[i].mesh.positions)
                    && worst <= size * 1e-3
                {
                    scene.pieces[i].link = Some(PieceLink { transform: t, ..link });
                    seen.get_mut(&id).expect("same_points").copy = copy_v;
                    continue;
                }
                // Changed in shape, not just moved: the original wins.
                self.status = format!(
                    "{} is a linked copy and follows its original: unlink it to change it on its own",
                    scene.pieces[i].name
                );
            }
            let t = link.transform;
            if same_points {
                // Same points on both: move just the copy's points whose
                // original changed (a stroke touches a small part).
                let state = seen.get_mut(&id).expect("same_points");
                let (src, copy) = two(&mut scene.pieces, si, i);
                let m = &src.mesh;
                let same_color = m.color.len() == state.last_color.len();
                let changed: Vec<u32> = (0..m.vertex_count() as u32)
                    .into_par_iter()
                    .filter(|&v| {
                        let k = v as usize;
                        // (A point no face uses has no copy.)
                        state.inverse[k] != u32::MAX
                            && (m.positions[k] != state.last[k]
                                || m.mask[k] != state.last_mask[k]
                                || (same_color && m.color.get(k) != state.last_color.get(k)))
                    })
                    .collect();
                if !same_color {
                    copy.mesh.color = if m.color.is_empty() {
                        Vec::new()
                    } else {
                        state.map.iter().map(|&v| m.color[v as usize]).collect()
                    };
                    state.last_color = m.color.clone();
                }
                let moved: Vec<u32> = changed.iter().map(|&v| state.inverse[v as usize]).collect();
                for (&v, &c) in changed.iter().zip(&moved) {
                    let (k, c) = (v as usize, c as usize);
                    copy.mesh.positions[c] = t.transform_point3(m.positions[k]);
                    copy.mesh.mask[c] = m.mask[k];
                    if same_color && !m.color.is_empty() {
                        copy.mesh.color[c] = m.color[k];
                    }
                    state.last[k] = m.positions[k];
                    state.last_mask[k] = m.mask[k];
                    if same_color && !m.color.is_empty() {
                        state.last_color[k] = m.color[k];
                    }
                }
                copy.mesh.update_after_move(&moved);
                if !same_color {
                    copy.mesh.mark_all_dirty();
                }
                state.source = src_v;
                state.copy = versions(&copy.mesh);
            } else {
                let (mesh, map) = build_copy(&scene.pieces[si].mesh, t);
                scene.pieces[i].mesh = mesh;
                scene.pieces[i].multires = None;
                seen.insert(id, LinkSeen::new(&scene.pieces[si].mesh, &scene.pieces[i].mesh, map));
            }
        }
    }
}

/// Two different elements of `v`, mutably for the second.
fn two(v: &mut [Piece], a: usize, b: usize) -> (&Piece, &mut Piece) {
    if a < b {
        let (x, y) = v.split_at_mut(b);
        (&x[a], &mut y[0])
    } else {
        let (x, y) = v.split_at_mut(a);
        (&y[0], &mut x[b])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::PointerButton;
    use glam::Vec2;
    use oxy_doc::Scene;
    use oxy_mesh::primitives;

    /// A sphere at x = 2, so its mirror image across x = 0 is elsewhere.
    fn editor() -> Editor {
        let mut e = Editor::default();
        let mut d = primitives::icosphere(4, 1.0).to_data();
        d.positions.iter_mut().for_each(|p| p.x += 2.0);
        e.scene = Scene::new(Piece::new("Ear", Mesh::from_data(d)));
        e.scene.active_mut().symmetry_center = Some(Vec3::ZERO);
        e.canvas = [0.0, 0.0, 800.0, 600.0];
        e.symmetry = oxy_sculpt::Symmetry::default();
        e
    }

    fn frame(e: &mut Editor) {
        let bounds = e.scene.bounds();
        e.camera.frame(bounds);
    }

    /// A short stroke on the middle of piece `i`, which becomes active.
    fn stroke_on(e: &mut Editor, i: usize) {
        e.scene.set_active(i);
        let c = e.scene.pieces[i].mesh.bounds().center() + Vec3::Z;
        let at = e.camera.project(c, e.canvas_size()).unwrap();
        e.pointer = Some(at);
        e.pointer_down(PointerButton::Primary);
        for k in 1..=8 {
            e.pointer_moved(at + Vec2::new(4.0 * k as f32, 0.0));
        }
        e.pointer_up();
    }

    /// Whether the copy is exactly the original moved by its link.
    fn follows(e: &Editor) -> bool {
        let (src, copy) = (&e.scene.pieces[0].mesh, &e.scene.pieces[1].mesh);
        let t = e.scene.pieces[1].link.unwrap().transform;
        let mut want: Vec<[i64; 3]> =
            src.positions.iter().map(|p| (t.transform_point3(*p) * 1e4).round().as_i64vec3().to_array()).collect();
        let mut got: Vec<[i64; 3]> =
            copy.positions.iter().map(|p| (*p * 1e4).round().as_i64vec3().to_array()).collect();
        want.sort_unstable();
        got.sort_unstable();
        want == got
    }

    #[test]
    fn a_mirrored_linked_copy_follows_sculpting_on_the_original_and_faces_out() {
        let mut e = editor();
        e.duplicate_linked(Some(Axis::X));
        frame(&mut e);
        assert_eq!(e.scene.pieces.len(), 2);
        assert_eq!(e.scene.pieces[1].link.map(|l| l.source), Some(e.scene.pieces[0].id));
        let copy = &e.scene.pieces[1].mesh;
        assert!(copy.bounds().center().x < -1.5, "mirrored across x = 0");
        // Normals point out of the copy (the winding was flipped).
        let c = copy.bounds().center();
        let out = copy.positions.iter().zip(&copy.normals).filter(|(p, n)| (**p - c).dot(**n) > 0.0).count();
        assert_eq!(out, copy.vertex_count(), "faces out");

        let before = e.scene.pieces[1].mesh.positions.clone();
        stroke_on(&mut e, 0);
        e.sync_links();
        assert_ne!(e.scene.pieces[1].mesh.positions, before, "the copy changed with the original");
        assert!(follows(&e));
        // Undo on the original takes the copy back too.
        e.undo();
        e.sync_links();
        assert!(follows(&e));
        assert_eq!(e.scene.pieces[1].mesh.positions, before);
    }

    #[test]
    fn linked_mirrors_use_the_chosen_axis_through_the_symmetry_center() {
        for (axis, expected) in [(Axis::Y, Vec3::new(2.0, 6.0, 0.0)), (Axis::Z, Vec3::new(2.0, 0.0, 8.0))] {
            let mut e = editor();
            let center = Vec3::new(1.0, 3.0, 4.0);
            e.scene.active_mut().symmetry_center = Some(center);
            e.duplicate_linked(Some(axis));
            let copy = e.scene.active();
            assert_eq!(copy.symmetry_center, Some(center));
            assert!((copy.mesh.bounds().center() - expected).length() < 1e-5);
            assert!(copy.mesh.signed_volume() > 0.0);
            assert!(follows(&e));

            // A reflected copy's transform is also used when the original changes.
            let original = &mut e.scene.pieces[0].mesh;
            original.positions[0] += Vec3::new(0.1, 0.2, 0.3);
            original.refresh_all();
            e.sync_links();
            assert!(follows(&e));
        }
    }

    #[test]
    fn moving_a_copy_keeps_it_linked_where_it_was_put() {
        let mut e = editor();
        e.duplicate_linked(None);
        e.sync_links();
        // Move the copy up (as the gizmo does: positions, then update).
        let copy = &mut e.scene.pieces[1].mesh;
        copy.positions.iter_mut().for_each(|p| p.y += 3.0);
        let all: Vec<u32> = (0..copy.vertex_count() as u32).collect();
        copy.update_after_move(&all);
        e.sync_links();
        let t = e.scene.pieces[1].link.unwrap().transform;
        assert!(t.transform_point3(Vec3::ZERO).distance(Vec3::new(0.0, 3.0, 0.0)) < 1e-4, "{t}");
        frame(&mut e);
        stroke_on(&mut e, 0);
        e.sync_links();
        assert!(follows(&e), "follows from its new place");
        assert!(e.scene.pieces[1].mesh.bounds().center().y > 2.5);
    }

    #[test]
    fn copies_cant_be_sculpted_and_unlink_frees_them() {
        let mut e = editor();
        e.duplicate_linked(None);
        let copy = e.scene.pieces[1].id;
        // Put it beside the original so it can be clicked.
        let m = &mut e.scene.pieces[1].mesh;
        m.positions.iter_mut().for_each(|p| p.x -= 4.0);
        m.refresh_all();
        e.sync_links();
        frame(&mut e);
        let before = e.scene.pieces[1].mesh.positions.clone();
        stroke_on(&mut e, 1);
        assert_eq!(e.scene.pieces[1].mesh.positions, before, "{}", e.status);
        assert!(e.status.contains("linked copy"), "{}", e.status);

        e.unlink_active();
        assert!(e.scene.get(copy).unwrap().link.is_none());
        stroke_on(&mut e, 1);
        assert_ne!(e.scene.pieces[1].mesh.positions, before, "an ordinary piece now");
        e.undo();
        e.undo();
        assert!(e.scene.get(copy).unwrap().link.is_some(), "undo links it again");
    }

    #[test]
    fn a_new_topology_on_the_original_rebuilds_the_copy_and_a_save_keeps_the_link() {
        let mut e = editor();
        e.duplicate_linked(Some(Axis::X));
        e.sync_links();
        let src = &mut e.scene.pieces[0];
        src.mesh = Mesh::from_data(primitives::icosphere(2, 0.5).to_data());
        e.sync_links();
        assert_eq!(e.scene.pieces[1].mesh.vertex_count(), e.scene.pieces[0].mesh.vertex_count());
        assert!(follows(&e));

        let back = Scene::from_data(e.scene.to_data()).unwrap();
        let link = back.pieces[1].link.expect("saved");
        assert_eq!(link.source, back.pieces[0].id);
        assert!(link.transform.determinant() < 0.0, "still mirrored");
    }

    #[test]
    fn paint_and_mask_on_the_original_reach_the_copy() {
        let mut e = editor();
        e.duplicate_linked(Some(Axis::X));
        e.sync_links();
        let m = &mut e.scene.pieces[0].mesh;
        let n = m.vertex_count();
        m.color = vec![oxy_mesh::WHITE; n];
        m.color[5] = Vec3::new(1.0, 0.0, 0.0);
        m.mask[7] = 1.0;
        m.mark_all_dirty();
        e.sync_links();
        let copy = &e.scene.pieces[1].mesh;
        assert_eq!(copy.color.iter().filter(|c| c.y == 0.0).count(), 1, "painted");
        assert_eq!(copy.mask.iter().filter(|&&x| x == 1.0).count(), 1, "masked");
        // A later dab changes one more point.
        let m = &mut e.scene.pieces[0].mesh;
        m.color[9] = Vec3::new(0.0, 0.0, 1.0);
        m.mark_dirty(0);
        e.sync_links();
        assert_eq!(e.scene.pieces[1].mesh.color.iter().filter(|c| c.x == 0.0).count(), 1);
    }

    #[test]
    fn fitting_finds_the_transform_and_refuses_flat_points() {
        let from: Vec<Vec3> = (0..20).map(|i| Vec3::new(i as f32, (i * i % 7) as f32, (i % 3) as f32)).collect();
        let t = Mat4::from_scale_rotation_translation(
            Vec3::new(-1.5, 1.5, 1.5),
            glam::Quat::from_rotation_y(0.7),
            Vec3::new(1.0, 2.0, 3.0),
        );
        let to: Vec<Vec3> = from.iter().map(|p| t.transform_point3(*p)).collect();
        let (fit, worst) = fit_affine(&from, &to).unwrap();
        assert!(worst < 1e-3 && fit.abs_diff_eq(t, 1e-4), "{fit} {worst}");
        let flat: Vec<Vec3> = from.iter().map(|p| Vec3::new(p.x, p.y, 0.0)).collect();
        assert!(fit_affine(&flat, &flat).is_none());
    }
}

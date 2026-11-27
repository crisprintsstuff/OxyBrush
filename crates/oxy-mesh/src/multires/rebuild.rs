//! Insert a recovered Catmull–Clark level beneath a mesh or existing stack.

use super::{Attributes, CcRefinement, Level, Multires, OwnedBasePolygons, Polys, Refinement, Scheme};
use crate::unsubdivide::{QuadTriangles, RebuildError, mesh_quads, recover_cc, triangulate_quads};
use crate::{Mesh, Vec3};
use std::collections::{HashMap, HashSet};

use RebuildError::{NoSubdivision, NotAllQuads, NotCatmullClark, SplitFaceAttributes, UnsupportedAttributes};

pub(crate) fn from_mesh(mesh: &Mesh) -> Result<(Mesh, Multires), RebuildError> {
    if !mesh.uvs().is_empty() {
        return Err(UnsupportedAttributes);
    }
    let faces = mesh_quads(mesh)?;
    let (mut stack, vertices) = recover_base(&mesh.positions, &faces)?;
    let attrs = Attributes {
        mask: mesh.mask.clone(),
        color: if mesh.is_painted() { mesh.color.clone() } else { Vec::new() },
        groups: mesh.groups().to_vec(),
        hidden: mesh.hidden().to_vec(),
    };
    stack.record_recovered_paint(&remap(&attrs.color, &vertices));
    let triangles = triangle_map(&stack.levels[1], &faces, &mesh.triangles, &attrs, &vertices)?;
    Ok(stack.finish_rebuild(attrs, &vertices, triangles))
}

impl Multires {
    /// Cheap eligibility check for the controls. The worker checks manifoldness
    /// and whether a complete inverse subdivision exists.
    pub fn can_rebuild_lower_level(&self) -> bool {
        let base = &self.levels[0];
        self.scheme == Scheme::CatmullClark
            && self.creases.is_empty()
            && base.uv.is_empty()
            && base.polys.as_ref().is_some_and(|p| p.len() * 4 == p.corner_total())
    }

    /// Add one level below the current base, keeping the upper levels and
    /// the current view. The receiver is a working snapshot of the stack.
    pub fn rebuild_lower_level(mut self, mesh: &Mesh) -> Result<(Mesh, Self), RebuildError> {
        if self.scheme != Scheme::CatmullClark || !self.is_bound_to(mesh) {
            return Err(NotCatmullClark);
        }
        if !mesh.uvs().is_empty() || !self.levels[0].uv.is_empty() || !self.creases.is_empty() {
            return Err(UnsupportedAttributes);
        }
        let attrs = self.prepare(mesh).0;
        let base = &self.levels[0];
        let base_faces = quad_triangles(base)?;
        let (mut rebuilt, mut vertices) = recover_base(&base.positions, &base_faces)?;

        // Each finer level keeps the prefix of vertices inherited from its
        // parent. The original base's colors are in that prefix.
        rebuilt.record_recovered_paint(&remap(&attrs.color, &vertices));

        let mut active_vertices = None;
        for (index, source) in self.levels.iter().enumerate() {
            if index > 0 {
                let parent = &rebuilt.levels[index];
                let child = rebuilt.refine_level(index, source.smooth);
                vertices = lift_vertices(&self.levels[index - 1], source, parent, &child, vertices)?;
                rebuilt.levels.push(child);
                rebuilt.active = index + 1;
                rebuilt.install_active_positions(remap(&source.positions, &vertices));
                rebuilt.levels[index + 1].paint = remap(&source.paint, &vertices);
            }
            if index == self.active {
                active_vertices = Some(vertices.clone());
            }
        }

        let vertices = active_vertices.ok_or(NoSubdivision)?;
        rebuilt.active = self.active + 1;
        let source = &self.levels[self.active];
        let faces = if self.active == 0 { base_faces } else { quad_triangles(source)? };
        let triangles = triangle_map(&rebuilt.levels[rebuilt.active], &faces, &source.triangles, &attrs, &vertices)?;
        Ok(rebuilt.finish_rebuild(attrs, &vertices, triangles))
    }

    fn record_recovered_paint(&mut self, color: &[Vec3]) {
        if !color.is_empty() {
            let base = self.levels[0].positions.len();
            self.keep_paint(1, color, &color[..base]);
        }
    }

    fn finish_rebuild(mut self, attrs: Attributes, vertices: &[u32], triangles: Vec<u32>) -> (Mesh, Self) {
        let mut color = remap(&attrs.color, vertices);
        self.clamp_active_paint(&mut color);
        let data = self.level_data(Attributes {
            mask: remap(&attrs.mask, vertices),
            color,
            groups: remap(&attrs.groups, &triangles),
            hidden: remap(&attrs.hidden, &triangles),
        });
        let mesh = Mesh::from_data(data);
        self.bind(&mesh);
        // All recovered levels above the new base have recorded quads.
        mesh.set_ngon_edges(HashSet::new());
        (mesh, self)
    }
}

fn quad_triangles(level: &Level) -> Result<Vec<QuadTriangles>, RebuildError> {
    let polys = level.polys.as_ref().ok_or(NotAllQuads)?;
    match polys {
        Polys::Quads(quads) => Ok(quads.iter().enumerate().map(|(f, &q)| (q, [2 * f, 2 * f + 1])).collect()),
        Polys::Mixed { tri_offsets, tris, .. } => (0..polys.len())
            .map(|f| {
                let q = polys.corners(f).try_into().map_err(|_| NotAllQuads)?;
                let [a, b] = tris[tri_offsets[f] as usize..tri_offsets[f + 1] as usize] else {
                    return Err(NotAllQuads);
                };
                Ok((q, [a as usize, b as usize]))
            })
            .collect(),
    }
}

fn recover_base(positions: &[Vec3], faces: &[QuadTriangles]) -> Result<(Multires, Vec<u32>), RebuildError> {
    let quads: Vec<_> = faces.iter().map(|&(q, _)| q).collect();
    let (mut coarse, old_vertices, old_edges, old_faces) = recover_cc(&quads, positions.len())?;

    // The inverse produces oriented rings, but the two inverses can reverse
    // the global orientation. Compare one child corner against the input.
    let [a, b, _, d] = coarse[0].map(|v| old_vertices[v as usize]);
    let ab = *old_edges.get(&key([a, b])).ok_or(NoSubdivision)?;
    let da = *old_edges.get(&key([d, a])).ok_or(NoSubdivision)?;
    let corner = [a, ab, old_faces[0], da];
    let original = quads.iter().find(|&&q| key(q) == key(corner)).ok_or(NoSubdivision)?;
    if oriented(*original) != oriented(corner) {
        for q in &mut coarse {
            q.swap(1, 3);
        }
    }

    let n = coarse.len() as u32;
    let base = OwnedBasePolygons {
        offsets: (0..=n).map(|f| 4 * f).collect(),
        corners: coarse.iter().flatten().copied().collect(),
        tri_offsets: (0..=n).map(|f| 2 * f).collect(),
        tris: (0..2 * n).collect(),
        creases: Vec::new(),
    };
    let mut stack =
        Multires::from_catmull_clark_parts(triangulate_quads(&coarse), base, vec![remap(positions, &old_vertices)], 0)
            .ok_or(NoSubdivision)?;
    let child = stack.refine_level(0, true);
    let edges = cc_refinement(&child)?
        .edges
        .iter()
        .map(|e| old_edges.get(&key([old_vertices[e.a as usize], old_vertices[e.b as usize]])).copied())
        .collect::<Option<Vec<_>>>()
        .ok_or(NoSubdivision)?;
    let mut vertices = old_vertices;
    vertices.extend(edges);
    vertices.extend(old_faces);
    if !is_permutation(&vertices, positions.len()) {
        return Err(NoSubdivision);
    }
    let old_quads: HashSet<_> = quads.into_iter().map(oriented).collect();
    let Some(Polys::Quads(quads)) = &child.polys else { return Err(NoSubdivision) };
    if old_quads.len() != quads.len()
        || !quads.iter().all(|q| old_quads.contains(&oriented(q.map(|v| vertices[v as usize]))))
    {
        return Err(NoSubdivision);
    }
    stack.levels.push(child);
    stack.active = 1;
    stack.install_active_positions(remap(positions, &vertices));
    Ok((stack, vertices))
}

/// Lift the new-to-old map through one known subdivision, using the old
/// edge endpoints and face corners to identify the old child points.
fn lift_vertices(
    old_parent: &Level,
    old_child: &Level,
    new_parent: &Level,
    new_child: &Level,
    mut vertices: Vec<u32>,
) -> Result<Vec<u32>, RebuildError> {
    let old = cc_refinement(old_child)?;
    let new = cc_refinement(new_child)?;
    let edges: HashMap<_, _> =
        old.edges.iter().enumerate().map(|(i, e)| (key([e.a, e.b]), (old.n_prev + i) as u32)).collect();
    let old_polys = old_parent.polys.as_ref().ok_or(NoSubdivision)?;
    let new_polys = new_parent.polys.as_ref().ok_or(NoSubdivision)?;
    let f_start = old.n_prev + old.edges.len();
    if old_polys.len() != new_polys.len() {
        return Err(NoSubdivision);
    }
    let faces: HashMap<_, _> = (0..old_polys.len())
        .map(|f| {
            let q = old_polys.corners(f).try_into().map_err(|_| NoSubdivision)?;
            Ok((oriented(q), (f_start + f) as u32))
        })
        .collect::<Result<_, RebuildError>>()?;
    let added_edges = new
        .edges
        .iter()
        .map(|e| edges.get(&key([vertices[e.a as usize], vertices[e.b as usize]])).copied())
        .collect::<Option<Vec<_>>>()
        .ok_or(NoSubdivision)?;
    let added_faces = (0..new_polys.len())
        .map(|f| {
            let q: [u32; 4] = new_polys.corners(f).try_into().ok()?;
            faces.get(&oriented(q.map(|v| vertices[v as usize]))).copied()
        })
        .collect::<Option<Vec<_>>>()
        .ok_or(NoSubdivision)?;
    vertices.extend(added_edges);
    vertices.extend(added_faces);
    if !is_permutation(&vertices, old_child.positions.len()) {
        return Err(NoSubdivision);
    }
    Ok(vertices)
}

fn triangle_map(
    level: &Level,
    old_faces: &[QuadTriangles],
    old_triangles: &[[u32; 3]],
    attrs: &Attributes,
    vertices: &[u32],
) -> Result<Vec<u32>, RebuildError> {
    let old_faces: HashMap<_, _> = old_faces.iter().map(|&(q, pair)| (oriented(q), pair)).collect();
    let Some(Polys::Quads(quads)) = &level.polys else { return Err(NoSubdivision) };
    let mut mapped = Vec::with_capacity(level.triangles.len());
    for (&q, tris) in quads.iter().zip(level.triangles.as_chunks::<2>().0) {
        let [a, b] = *old_faces.get(&oriented(q.map(|v| vertices[v as usize]))).ok_or(NoSubdivision)?;
        // Imported quads can use the opposite diagonal. A face group or
        // hide boundary across that diagonal cannot move to the new split.
        let triangle = key(tris[0].map(|v| vertices[v as usize]));
        if triangle == key(old_triangles[b]) {
            mapped.extend([b as u32, a as u32]);
        } else if triangle == key(old_triangles[a])
            || (attrs.groups[a] == attrs.groups[b] && attrs.hidden[a] == attrs.hidden[b])
        {
            mapped.extend([a as u32, b as u32]);
        } else {
            return Err(SplitFaceAttributes);
        }
    }
    Ok(mapped)
}

fn cc_refinement(level: &Level) -> Result<&CcRefinement, RebuildError> {
    match level.refinement.as_ref() {
        Some(Refinement::Cc(refinement)) => Ok(refinement),
        _ => Err(NoSubdivision),
    }
}

fn oriented<const N: usize>(mut q: [u32; N]) -> [u32; N] {
    let start = q.iter().enumerate().min_by_key(|&(_, v)| v).map_or(0, |(i, _)| i);
    q.rotate_left(start);
    q
}

fn key<const N: usize>(mut vertices: [u32; N]) -> [u32; N] {
    vertices.sort_unstable();
    vertices
}

fn is_permutation(map: &[u32], len: usize) -> bool {
    let mut seen = vec![false; len];
    map.len() == len && map.iter().all(|&v| seen.get_mut(v as usize).is_some_and(|slot| !std::mem::replace(slot, true)))
}

fn remap<T: Copy>(source: &[T], map: &[u32]) -> Vec<T> {
    if source.is_empty() { Vec::new() } else { map.iter().map(|&v| source[v as usize]).collect() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::unsubdivide::tests::dense_cc_cube;

    /// Match points by exact position and triangles by their oriented corners.
    fn vertex_order(before: &Mesh, after: &Mesh) -> Vec<u32> {
        let point = |p: Vec3| p.to_array().map(f32::to_bits);
        let ids: HashMap<_, _> = before.positions.iter().enumerate().map(|(v, &p)| (point(p), v as u32)).collect();
        after.positions.iter().map(|&p| ids[&point(p)]).collect()
    }

    #[test]
    fn rebuild_preserves_sculpted_upper_levels_and_their_attributes() {
        let base = dense_cc_cube();
        let mut source = Multires::new_catmull_clark(&base, None);
        let middle = Mesh::from_data(source.add_level_linear(&base));
        source.bind(&middle);
        let mut top = Mesh::from_data(source.add_level(&middle));
        source.bind(&top);

        // Deliberately move points beyond their neighbouring CC face
        // centroids. Connectivity alone identifies them after recovery.
        top.positions[11] += Vec3::new(0.8, -0.23, 0.12);
        top.update_after_move(&[11]);
        top.mask[11] = 0.72;
        top.color[11] = Vec3::new(0.21, 0.72, 0.33);
        let mut groups = top.groups().to_vec();
        groups[3] = 12;
        top.set_groups(groups);
        let mut hidden = top.hidden().to_vec();
        hidden[9] = true;
        top.set_hidden(hidden);

        let (mesh, mut recovered) = source.rebuild_lower_level(&top).unwrap();
        assert_eq!(recovered.level_count(), 4);
        assert_eq!(recovered.active(), 3);
        assert_eq!(recovered.level_smooth(), [true, true, false, true]);
        let vertices = vertex_order(&top, &mesh);
        assert!(is_permutation(&vertices, top.vertex_count()));
        assert_eq!(mesh.mask, remap(&top.mask, &vertices));
        assert_eq!(mesh.color, remap(&top.color, &vertices));
        let triangles: HashMap<_, _> = top.triangles.iter().enumerate().map(|(i, &t)| (oriented(t), i)).collect();
        assert_eq!(mesh.triangle_count(), triangles.len());
        for (i, tri) in mesh.triangles.iter().enumerate() {
            let old = triangles[&oriented(tri.map(|v| vertices[v as usize]))];
            assert_eq!(mesh.groups()[i], top.groups()[old]);
            assert_eq!(mesh.hidden()[i], top.hidden()[old]);
        }

        let coarse = Mesh::from_data(recovered.set_level(&mesh, 0));
        recovered.bind(&coarse);
        let roundtrip = Mesh::from_data(recovered.set_level(&coarse, 3));
        let roundtrip_vertices = vertex_order(&mesh, &roundtrip);
        assert!(is_permutation(&roundtrip_vertices, mesh.vertex_count()));
        for (&actual, expected) in roundtrip.color.iter().zip(remap(&mesh.color, &roundtrip_vertices)) {
            assert!(actual.abs_diff_eq(expected, 1e-6));
        }
    }
}

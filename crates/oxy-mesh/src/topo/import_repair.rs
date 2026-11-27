//! Conservative repairs for the import inspector.

use super::{ImportTopologyReport, boundary_loops, fill_boundary_loops, hole_patch_rings, inspect_import, zero_area};
use crate::{Mesh, MeshData};
use glam::Vec3;
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportRepairAction {
    Faces,
    CloseHoles,
}

pub struct ImportRepair {
    pub mesh: Option<Mesh>,
    pub report: ImportTopologyReport,
    pub summary: String,
}

pub fn repair_import(
    mesh: &Mesh,
    action: ImportRepairAction,
    max_triangles: usize,
) -> Result<ImportRepair, &'static str> {
    let before = inspect_import(mesh);
    let (data, summary) = match action {
        ImportRepairAction::Faces => repair_faces(mesh, &before)?,
        ImportRepairAction::CloseHoles => patch_holes(mesh, &before, max_triangles)?,
    };
    let Some(data) = data else {
        return Ok(ImportRepair { mesh: None, report: before, summary });
    };
    let candidate = Mesh::from_data(data);
    let report = inspect_import(&candidate);
    if action == ImportRepairAction::CloseHoles && report.has_findings() {
        return Err("The hole patches did not produce clean edge and face topology");
    }
    preserve_polygon_edges(mesh, &candidate);
    Ok(ImportRepair { mesh: Some(candidate), report, summary })
}

fn repair_faces(mesh: &Mesh, before: &ImportTopologyReport) -> Result<(Option<MeshData>, String), &'static str> {
    let mut keep = vec![true; mesh.triangle_count()];
    let mut degenerate = 0;
    let mut duplicates = 0;
    for (i, &tri) in mesh.triangles.iter().enumerate() {
        if zero_area(&mesh.positions, tri) {
            keep[i] = false;
            degenerate += 1;
        } else if before.duplicate_triangles != 0 {
            let anchor = *tri.iter().min().unwrap();
            if mesh.vertex_triangles(anchor).iter().any(|&other| {
                let other = other as usize;
                other < i && keep[other] && tri.iter().all(|v| mesh.triangles[other].contains(v))
            }) {
                keep[i] = false;
                duplicates += 1;
            }
        }
    }
    let flips = if before.inconsistent_winding_edges > 0 || degenerate + duplicates > 0 {
        orient_faces(mesh, &keep)
    } else {
        Vec::new()
    };
    if degenerate + duplicates + flips.len() == 0 {
        return Ok((None, "No faces could be repaired by this action".into()));
    }
    if degenerate + duplicates == mesh.triangle_count() {
        return Err("Repair would remove every face");
    }
    let summary = format!(
        "Repaired faces: {duplicates} duplicate and {degenerate} degenerate triangles removed, {} faces reoriented",
        flips.len()
    );
    let mut data = mesh.to_data();
    for i in flips {
        data.triangles[i].swap(1, 2);
        if let Some(uv) = data.uv.get_mut(i) {
            uv.swap(1, 2);
        }
    }
    data.retain_triangles(&keep);
    Ok((Some(data), summary))
}

/// Follow only edges shared by two surviving triangles. Propagate relative
/// flips, then choose the sign that changes the fewest faces in the region.
/// A contradictory region has no consistent sign and is left untouched.
fn orient_faces(mesh: &Mesh, keep: &[bool]) -> Vec<usize> {
    let mut parity = vec![None; keep.len()];
    let mut component = Vec::new();
    let mut flips = Vec::new();
    for start in 0..keep.len() {
        if !keep[start] || parity[start].is_some() {
            continue;
        }
        parity[start] = Some(false);
        component.clear();
        component.push(start);
        let mut consistent = true;
        let mut next = 0;
        while next < component.len() {
            let i = component[next];
            next += 1;
            let tri = mesh.triangles[i];
            for edge in 0..3 {
                let (a, b) = (tri[edge], tri[(edge + 1) % 3]);
                let mut incident = mesh
                    .vertex_triangles(a)
                    .iter()
                    .map(|&j| j as usize)
                    .filter(|&j| j != i && keep[j] && mesh.triangles[j].contains(&b));
                let Some(j) = incident.next() else { continue };
                if incident.next().is_some() {
                    continue;
                }
                let other = mesh.triangles[j];
                let corner = other.iter().position(|&v| v == a).unwrap();
                let expected = parity[i].unwrap() ^ (other[(corner + 1) % 3] == b);
                if let Some(value) = parity[j] {
                    consistent &= value == expected;
                } else {
                    parity[j] = Some(expected);
                    component.push(j);
                }
            }
        }
        if consistent {
            let reversed = component.iter().filter(|&&i| parity[i] == Some(true)).count();
            let invert = reversed > component.len() - reversed;
            flips.extend(component.iter().copied().filter(|&i| parity[i] == Some(!invert)));
        }
    }
    flips
}

fn patch_holes(
    mesh: &Mesh,
    before: &ImportTopologyReport,
    max_triangles: usize,
) -> Result<(Option<MeshData>, String), &'static str> {
    if before.topology.non_manifold_edge_count > 0
        || before.topology.degenerate_triangle_count > 0
        || before.inconsistent_winding_edges > 0
        || before.duplicate_triangles > 0
        || before.zero_area_triangles > 0
    {
        return Err("Resolve the face and non-manifold edge findings before closing holes");
    }
    if before.topology.boundary_edge_count == 0 {
        return Ok((None, "No open boundaries to close".into()));
    }
    let loops = boundary_loops(&mesh.triangles);
    let mut vertices = HashSet::new();
    if loops.iter().map(Vec::len).sum::<usize>() != before.topology.boundary_edge_count
        || !loops.iter().flatten().all(|v| vertices.insert(*v))
    {
        return Err("The open boundaries do not form separate simple loops");
    }
    if has_planar_boundary(mesh, &loops) {
        return Err("Closing a flat sheet would create overlapping faces");
    }
    let added: usize = loops
        .iter()
        .map(|ring| {
            let points: Vec<_> = ring.iter().map(|&v| mesh.positions[v as usize]).collect();
            let center = points.iter().sum::<Vec3>() / points.len() as f32;
            ring.len() * (2 * hole_patch_rings(&points, center) - 1)
        })
        .sum();
    if max_triangles > 0 && mesh.triangle_count().saturating_add(added) > max_triangles {
        return Err("Closing these holes would exceed the project's triangle limit");
    }
    let mut data = mesh.to_data();
    if !data.uv.is_empty() {
        // Patches have no authored UVs. Leave them collapsed in UV space
        // and keep the coordinates of the rest of the piece.
        data.uv.resize(data.triangles.len() + added, [glam::Vec2::ZERO; 3]);
    }
    fill_boundary_loops(&mut data, &loops);
    Ok((Some(data), format!("Closed {} open boundary loop(s)", loops.len())))
}

// A cap over a flat connected sheet overlaps its surface and gives no volume.
fn has_planar_boundary(mesh: &Mesh, loops: &[Vec<u32>]) -> bool {
    let mut visited = vec![false; mesh.vertex_count()];
    let mut component = Vec::new();
    for ring in loops {
        let start = ring[0];
        if std::mem::replace(&mut visited[start as usize], true) {
            continue;
        }
        let face = mesh.vertex_triangles(start)[0] as usize;
        let [a, b, c] = mesh.triangles[face].map(|v| mesh.positions[v as usize].as_dvec3());
        let normal = (b - a).cross(c - a).normalize();
        let mut planar = true;
        component.clear();
        component.push(start);
        let mut i = 0;
        while i < component.len() {
            let vertex = component[i];
            i += 1;
            let delta = mesh.positions[vertex as usize].as_dvec3() - a;
            planar &= delta.dot(normal).abs() <= delta.length() * 1e-6;
            for &face in mesh.vertex_triangles(vertex) {
                for neighbor in mesh.triangles[face as usize] {
                    if !std::mem::replace(&mut visited[neighbor as usize], true) {
                        component.push(neighbor);
                    }
                }
            }
        }
        if planar {
            return true;
        }
    }
    false
}

fn preserve_polygon_edges(source: &Mesh, target: &Mesh) {
    let Some((quads, ngons)) = source.known_polygons() else { return };
    let mut remap = vec![None; source.vertex_count()];
    for (new, &old) in target.source_vertices().iter().enumerate() {
        if let Some(slot) = remap.get_mut(old as usize) {
            *slot = Some(new as u32);
        }
    }
    let translate = |edges: &crate::EdgeSet| {
        edges
            .iter()
            .filter_map(|&[a, b]| {
                let a = remap[a as usize]?;
                let b = remap[b as usize]?;
                let count =
                    target.vertex_triangles(a).iter().filter(|&&i| target.triangles[i as usize].contains(&b)).count();
                (count == 2).then_some([a.min(b), a.max(b)])
            })
            .collect()
    };
    target.set_quad_diagonals(translate(quads));
    target.set_ngon_edges(translate(ngons));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives;
    use glam::Vec2;

    #[test]
    fn repairs_faces_without_losing_vertex_or_corner_attributes() {
        let mut data = primitives::cube(1, 1.0).to_data();
        let original = data.triangles.len();
        data.triangles[0].swap(1, 2);
        data.triangles.push(data.triangles[0]);
        let base = data.positions.len() as u32;
        data.positions.extend([Vec3::ZERO, Vec3::X, 2.0 * Vec3::X]);
        data.triangles.push([base, base + 1, base + 2]);
        data.mask = vec![0.25; data.positions.len()];
        data.color = vec![Vec3::splat(0.5); data.positions.len()];
        data.groups = vec![3; data.triangles.len()];
        data.hidden = vec![true; data.triangles.len()];
        data.uv = data.triangles.iter().map(|t| t.map(|i| data.positions[i as usize].truncate())).collect();

        let fixed = repair_import(&Mesh::from_data(data), ImportRepairAction::Faces, 0).unwrap();
        assert!(!fixed.report.has_findings());
        let data = fixed.mesh.unwrap().to_data();
        assert_eq!(data.triangles.len(), original);
        assert!(data.mask.iter().all(|&v| v == 0.25));
        assert!(data.color.iter().all(|&v| v == Vec3::splat(0.5)));
        assert_eq!(data.groups, vec![3; original]);
        assert_eq!(data.hidden, vec![true; original]);
        assert_eq!(
            data.uv,
            data.triangles.iter().map(|t| t.map(|i| data.positions[i as usize].truncate())).collect::<Vec<_>>()
        );
        assert!(data.signed_volume() > 0.0);
    }

    #[test]
    fn closes_a_hole_with_a_budget_and_keeps_existing_uvs() {
        let cube = primitives::cube(1, 1.0);
        let mut data = cube.to_data();
        let mut keep = vec![true; data.triangles.len()];
        keep[0] = false;
        data.uv = vec![[Vec2::ONE; 3]; data.triangles.len()];
        data.retain_triangles(&keep);
        let mesh = Mesh::from_data(data);
        assert!(repair_import(&mesh, ImportRepairAction::CloseHoles, mesh.triangle_count()).is_err());

        let fixed = repair_import(&mesh, ImportRepairAction::CloseHoles, 0).unwrap();
        assert!(!fixed.report.has_findings());
        let data = fixed.mesh.unwrap().to_data();
        assert_eq!(data.uv.iter().filter(|&&uv| uv == [Vec2::ONE; 3]).count(), mesh.triangle_count());
        assert_eq!(data.uv.iter().filter(|&&uv| uv == [Vec2::ZERO; 3]).count(), 3);
    }

    #[test]
    fn preserves_known_polygon_edges_and_rejects_flat_caps() {
        let mesh = Mesh::new(vec![Vec3::ZERO, Vec3::X, Vec3::X + Vec3::Y, Vec3::Y], vec![[0, 1, 2], [0, 3, 2]]);
        let vertices = mesh.source_vertices();
        let mut diagonal = [0, 2].map(|v| vertices.iter().position(|&source| source == v).unwrap() as u32);
        diagonal.sort_unstable();
        mesh.set_quad_diagonals([diagonal].into_iter().collect());
        mesh.set_ngon_edges(Default::default());
        let fixed = repair_import(&mesh, ImportRepairAction::Faces, 0).unwrap();
        assert_eq!(fixed.report.inconsistent_winding_edges, 0);
        assert_eq!(fixed.mesh.unwrap().known_polygons().unwrap().0.len(), 1);

        let plate = primitives::plane(2, 1.0);
        assert!(matches!(
            repair_import(&plate, ImportRepairAction::CloseHoles, 0),
            Err("Closing a flat sheet would create overlapping faces")
        ));
    }
}

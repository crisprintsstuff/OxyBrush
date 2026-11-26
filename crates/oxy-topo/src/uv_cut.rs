//! Triangle corner topology for the cut surface in RFC 0044.

use std::collections::{HashMap, HashSet, hash_map::Entry};

/// A UV mesh whose vertices are shared only across uncut edges.
///
/// Each seam pair is ordered by source vertex ID, so entry zero on either
/// side corresponds to the same endpoint. A seam tip may share an endpoint.
pub struct UvCutTopology {
    pub triangles: Vec<[usize; 3]>,
    pub source_vertices: Vec<u32>,
    pub seams: Vec<[[usize; 2]; 2]>,
}

impl UvCutTopology {
    /// Split corner fans at the given interior mesh edges. Return `None`
    /// for degenerate triangles, non-manifold edges or missing seam sides.
    pub fn from_seams(triangles: &[[u32; 3]], seams: &[[u32; 2]]) -> Option<Self> {
        Self::build(triangles, seams, false)
    }

    /// Preserve the required seams and cut every remaining edge outside a
    /// spanning forest of face adjacencies. Each component unfolds as a disk.
    pub(crate) fn complete(triangles: &[[u32; 3]], required: &[[u32; 2]]) -> Option<Self> {
        Self::build(triangles, required, true)
    }

    fn build(triangles: &[[u32; 3]], seams: &[[u32; 2]], complete: bool) -> Option<Self> {
        let cuts: HashSet<_> = seams.iter().map(|&[a, b]| [a.min(b), a.max(b)]).collect();
        let mut edges = HashMap::with_capacity(triangles.len());
        let mut parent: Vec<_> = (0..triangles.len().checked_mul(3)?).collect();
        let mut pairs = Vec::with_capacity(cuts.len());
        let mut remaining = cuts.len();
        let mut face_forest: Option<Vec<_>> = complete.then(|| (0..triangles.len()).collect());
        for (face, tri) in triangles.iter().enumerate() {
            for i in 0..3 {
                let j = (i + 1) % 3;
                let a = tri[i];
                let b = tri[j];
                if a == b {
                    return None;
                }
                let (edge, corners) =
                    if a < b { ([a, b], [face * 3 + i, face * 3 + j]) } else { ([b, a], [face * 3 + j, face * 3 + i]) };
                match edges.entry(edge) {
                    Entry::Vacant(entry) => {
                        entry.insert(Some(corners));
                    }
                    Entry::Occupied(mut entry) => {
                        let other = entry.get_mut().take()?;
                        let required = cuts.contains(&edge);
                        remaining -= usize::from(required);
                        // Joining a new face along one edge preserves a disk.
                        // A cycle would identify two edges already in that disk.
                        if required || face_forest.as_mut().is_some_and(|f| !join(f, other[0] / 3, face)) {
                            pairs.push((edge, [other, corners]));
                        } else {
                            for k in 0..2 {
                                join(&mut parent, other[k], corners[k]);
                            }
                        }
                    }
                }
            }
        }
        if remaining != 0 {
            return None;
        }
        let mut source_vertices = Vec::new();
        let mut indices = vec![usize::MAX; parent.len()];
        let mut corner_to_uv = Vec::with_capacity(parent.len());
        for (corner, &source) in triangles.iter().flatten().enumerate() {
            let entry = &mut indices[root(&mut parent, corner)];
            if *entry == usize::MAX {
                *entry = source_vertices.len();
                source_vertices.push(source);
            }
            corner_to_uv.push(*entry);
        }
        pairs.sort_unstable_by_key(|&(edge, _)| edge);
        Some(Self {
            triangles: corner_to_uv.as_chunks::<3>().0.to_vec(),
            source_vertices,
            seams: pairs
                .into_iter()
                .map(|(_, sides)| sides.map(|endpoints| endpoints.map(|c| corner_to_uv[c])))
                .collect(),
        })
    }
}

fn root(parent: &mut [usize], mut node: usize) -> usize {
    while parent[node] != node {
        parent[node] = parent[parent[node]];
        node = parent[node];
    }
    node
}

fn join(parent: &mut [usize], a: usize, b: usize) -> bool {
    let a = root(parent, a);
    let b = root(parent, b);
    parent[a.max(b)] = a.min(b);
    a != b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_opens_a_closed_shell_and_preserves_the_required_seam() {
        let source = [[0, 2, 1], [0, 1, 3], [1, 2, 3], [2, 0, 3]];
        let cut = UvCutTopology::complete(&source, &[[0, 1]]).unwrap();
        assert_eq!(cut.source_vertices.len(), source.len() + 2);
        assert_eq!(cut.seams.len(), 3);
        assert!(cut.seams.iter().any(|&[side, _]| side.map(|i| cut.source_vertices[i]) == [0, 1]));
    }

    #[test]
    fn completion_opens_an_annulus_with_one_cut() {
        let source = [[0, 1, 4], [4, 1, 5], [1, 2, 5], [5, 2, 6], [2, 3, 6], [6, 3, 7], [3, 0, 7], [7, 0, 4]];
        let cut = UvCutTopology::complete(&source, &[]).unwrap();
        assert_eq!(cut.source_vertices.len(), source.len() + 2);
        assert_eq!(cut.seams.len(), 1);
        assert!(UvCutTopology::complete(&source, &[[0, 1]]).is_none());
    }

    #[test]
    fn cut_closed_shell_retains_source_and_pairs_the_cut_edges() {
        let source = [[0, 2, 1], [0, 1, 3], [1, 2, 3], [2, 0, 3]];
        let cut = UvCutTopology::from_seams(&source, &[[1, 0], [1, 2], [2, 0]]).unwrap();
        assert_eq!(cut.source_vertices.len(), 7);
        assert_eq!(cut.seams.len(), 3);
        for (triangle, source) in cut.triangles.iter().zip(source) {
            assert_eq!(triangle.map(|i| cut.source_vertices[i]), source);
        }
        for [a, b] in cut.seams {
            assert!(a.into_iter().zip(b).all(|(a, b)| a != b && cut.source_vertices[a] == cut.source_vertices[b]));
        }
        assert!(cut.triangles[1..].iter().all(|t| t[2] == cut.triangles[1][2]));
    }

    #[test]
    fn cut_open_strip_keeps_uncut_edges_shared() {
        let source = [[0, 1, 2], [2, 1, 3], [2, 3, 4]];
        let cut = UvCutTopology::from_seams(&source, &[[1, 2]]).unwrap();
        assert_eq!(cut.source_vertices.len(), 7);
        assert_eq!(cut.triangles[1][0], cut.triangles[2][0]);
        assert_eq!(cut.triangles[1][2], cut.triangles[2][1]);
        assert_ne!(cut.triangles[0][1], cut.triangles[1][1]);
        assert!(UvCutTopology::from_seams(&source, &[[0, 1]]).is_none());
        assert!(UvCutTopology::from_seams(&[[0, 1, 2], [2, 1, 3], [1, 2, 4]], &[]).is_none());
    }
}

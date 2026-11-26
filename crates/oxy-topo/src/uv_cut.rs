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
        fn root(parent: &mut [usize], mut corner: usize) -> usize {
            while parent[corner] != corner {
                parent[corner] = parent[parent[corner]];
                corner = parent[corner];
            }
            corner
        }

        let cuts: HashSet<_> = seams.iter().map(|&[a, b]| [a.min(b), a.max(b)]).collect();
        let mut edges = HashMap::with_capacity(triangles.len());
        let mut parent: Vec<_> = (0..triangles.len().checked_mul(3)?).collect();
        let mut pairs = Vec::with_capacity(cuts.len());
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
                        if cuts.contains(&edge) {
                            pairs.push((edge, [other, corners]));
                        } else {
                            for k in 0..2 {
                                let a = root(&mut parent, other[k]);
                                let b = root(&mut parent, corners[k]);
                                parent[a.max(b)] = a.min(b);
                            }
                        }
                    }
                }
            }
        }
        if pairs.len() != cuts.len() {
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
            triangles: corner_to_uv.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect(),
            source_vertices,
            seams: pairs
                .into_iter()
                .map(|(_, sides)| sides.map(|endpoints| endpoints.map(|c| corner_to_uv[c])))
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

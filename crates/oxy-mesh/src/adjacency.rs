/// Compressed sparse row adjacency list.
#[derive(Debug, Clone, Default)]
pub(crate) struct Csr {
    offsets: Vec<u32>,
    items: Vec<u32>,
}

impl Csr {
    pub fn get(&self, i: usize) -> &[u32] {
        &self.items[self.offsets[i] as usize..self.offsets[i + 1] as usize]
    }

    pub fn vertex_triangles(vertex_count: usize, triangles: &[[u32; 3]]) -> Self {
        let mut counts = vec![0u32; vertex_count + 1];
        for t in triangles {
            for &v in t {
                counts[v as usize + 1] += 1;
            }
        }
        for i in 0..vertex_count {
            counts[i + 1] += counts[i];
        }
        let offsets = counts.clone();
        let mut cursor = counts;
        let mut items = vec![0u32; triangles.len() * 3];
        for (ti, t) in triangles.iter().enumerate() {
            for &v in t {
                items[cursor[v as usize] as usize] = ti as u32;
                cursor[v as usize] += 1;
            }
        }
        Csr { offsets, items }
    }

    pub fn vertex_neighbors(vertex_count: usize, triangles: &[[u32; 3]], vt: &Csr) -> Self {
        let mut offsets = Vec::with_capacity(vertex_count + 1);
        let mut items = Vec::with_capacity(triangles.len() * 3);
        let mut scratch = Vec::new();
        offsets.push(0);
        for v in 0..vertex_count as u32 {
            scratch.clear();
            for &t in vt.get(v as usize) {
                scratch.extend(triangles[t as usize].iter().copied().filter(|&o| o != v));
            }
            scratch.sort_unstable();
            scratch.dedup();
            items.extend_from_slice(&scratch);
            offsets.push(items.len() as u32);
        }
        Csr { offsets, items }
    }
}

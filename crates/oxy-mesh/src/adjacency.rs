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
        use rayon::prelude::*;
        // Blocks of vertices in parallel; each block builds its own sorted,
        // deduplicated lists, then the blocks are joined in vertex order.
        const BLOCK: usize = 16 * 1024;
        let blocks: Vec<(Vec<u32>, Vec<u32>)> = (0..vertex_count.div_ceil(BLOCK))
            .into_par_iter()
            .map(|b| {
                let (v0, v1) = (b * BLOCK, ((b + 1) * BLOCK).min(vertex_count));
                let mut items = Vec::with_capacity((v1 - v0) * 6);
                let mut lens = Vec::with_capacity(v1 - v0);
                let mut scratch = Vec::new();
                for v in v0 as u32..v1 as u32 {
                    scratch.clear();
                    for &t in vt.get(v as usize) {
                        scratch.extend(triangles[t as usize].iter().copied().filter(|&o| o != v));
                    }
                    scratch.sort_unstable();
                    scratch.dedup();
                    items.extend_from_slice(&scratch);
                    lens.push(scratch.len() as u32);
                }
                (items, lens)
            })
            .collect();
        let mut offsets = Vec::with_capacity(vertex_count + 1);
        let mut items = Vec::with_capacity(blocks.iter().map(|b| b.0.len()).sum());
        offsets.push(0);
        for (block_items, lens) in blocks {
            items.extend_from_slice(&block_items);
            for len in lens {
                offsets.push(offsets.last().copied().unwrap_or(0) + len);
            }
        }
        Csr { offsets, items }
    }
}

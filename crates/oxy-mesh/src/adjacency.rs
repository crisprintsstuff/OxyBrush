/// Compressed sparse row adjacency list.
#[derive(Debug, Clone, Default)]
pub(crate) struct Csr {
    offsets: Vec<u32>,
    items: Vec<u32>,
}

impl Csr {
    /// Bytes allocated.
    pub fn bytes(&self) -> usize {
        (self.offsets.capacity() + self.items.capacity()) * 4
    }

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
}

/// The neighbours of one vertex (the other corners of its triangles),
/// sorted and without repeats, gathered when asked for instead of stored:
/// a stored list cost 14 bytes per triangle, a sixth of the whole mesh.
/// Vertices with up to 32 triangles around them need no allocation.
pub struct Neighbors {
    len: usize,
    inline: [u32; 64],
    spill: Vec<u32>,
}

impl Neighbors {
    pub(crate) fn gather(v: u32, around: &[u32], triangles: &[[u32; 3]]) -> Self {
        let mut n = Neighbors { len: 0, inline: [0; 64], spill: Vec::new() };
        if around.len() * 2 > n.inline.len() {
            // More than 32 triangles around it (a cone's tip, say).
            n.spill = around.iter().flat_map(|&t| triangles[t as usize]).filter(|&o| o != v).collect();
            n.spill.sort_unstable();
            n.spill.dedup();
            return n;
        }
        // Every other corner of every triangle (each neighbour usually
        // twice), then sorted with an insertion sort, fast for the dozen or
        // so there are, and repeats dropped.
        let mut len = 0;
        for &t in around {
            for &o in &triangles[t as usize] {
                if o != v {
                    n.inline[len] = o;
                    len += 1;
                }
            }
        }
        let buf = &mut n.inline[..len];
        for i in 1..len {
            let x = buf[i];
            let mut j = i;
            while j > 0 && buf[j - 1] > x {
                buf[j] = buf[j - 1];
                j -= 1;
            }
            buf[j] = x;
        }
        let mut out = 0;
        for i in 0..len {
            if out == 0 || buf[out - 1] != buf[i] {
                buf[out] = buf[i];
                out += 1;
            }
        }
        n.len = out;
        n
    }
}

impl std::ops::Deref for Neighbors {
    type Target = [u32];
    fn deref(&self) -> &[u32] {
        if self.spill.is_empty() { &self.inline[..self.len] } else { &self.spill }
    }
}

impl<'a> IntoIterator for &'a Neighbors {
    type Item = &'a u32;
    type IntoIter = std::slice::Iter<'a, u32>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

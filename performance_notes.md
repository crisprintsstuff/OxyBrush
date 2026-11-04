# OxyBrush performance notes: sculpting on high-res meshes

*A contributor writeup for the OxyBrush maintainer. Everything here is something I measured on your code, plus the code I used, so you can take what is useful and ignore the rest.*

## About these measurements (please read first)

- **Code version:** `0.3.0-alpha` at commit `8419198` (PR #22, "brush-presets"). Upstream has moved on since, so line numbers and some details may differ.
- **Hardware:** a 1-core, 3 GB Linux sandbox, with no GPU and no Windows. Every number is **single-thread, CPU only**. Where I say something should scale across cores, that is reasoning from where the time goes, not a measurement. **Please re-run the benchmarks below on your reference machine** (they are small; code is in the appendix).
- **Method:** your `dab_timing` example, plus a few small examples of my own. Where timings were noisy (the box is shared) I report the minimum over many repeats.
- **Your rules are respected.** Nothing here uses `unsafe`, adds a C dependency, or touches the file format. Everything is plain `std` atomics and `rayon`, which the workspace already has. Every behaviour change is **opt-in and off by default**, so existing tests and deterministic strokes are unchanged.
- **Test suites** for `oxy-mesh`, `oxy-sculpt`, `oxy-history`, `oxy-topo` and `oxy-doc` pass with these changes. The full app and renderer compile. I did **not** get the `oxy-app` test suite to finish building in my environment, and I have **not** run anything on a real GPU.

## Summary

Your mesh core is already well designed (Morton-ordered chunks, CSR adjacency, stamp-based dedup, incremental normals). The brush math itself is almost free: about 0.13 ms of a 2 ms dab. The slowness on dense meshes comes from three things around it.

| # | Finding | Evidence | Size of the fix |
|---|---|---|---|
| 1 | **Strokes run per raw mouse event with no work limit.** `CursorMoved` runs the raycast and every dab the segment asks for, synchronously. | At 5.2M tris, today's behaviour needs ~1.95 CPU-s per second of input (it can never catch up). Budgeted: ~0.45 CPU-s. | ~190 lines + tests, opt-in |
| 2 | **`update_after_move` is ~70% of every dab, and about two thirds of it is serial.** | 0.6 ms dedup + 0.6 ms normals + 0.34 ms bounds for a 14.6k-vertex dab. Only the normals parallelize. | ~120 lines, same single-thread speed |
| 3 | **GPU vertex packing does needless work on the UI thread.** | ~34 ns/vertex now; ~7–20 ns with a lookup table; ~2 ns when the shader ignores the field. | ~60 lines |
| 4 | **`CHUNK_TRIANGLES = 1024` beats 4096** in my sweep. | Raycast ~2× cheaper; undo/upload unit 4× smaller. Dab time unchanged. | one constant |

A caveat on #1: at **1.3M** triangles this stress drag was fine even on one core (0.47 CPU-s per input-s). The problem appears as meshes get denser. I only demonstrated it at 5.2M, and the crossover will depend on brush radius and hardware.

---

## 1. Apply strokes once per frame, within a time budget

### What happens today

In `app.rs`, `WindowEvent::CursorMoved` calls `editor.pointer_moved`, which for a sculpt stroke does the raycast and then `Stroke::add_sample`. `add_sample` loops `while d <= len`, so a long segment places however many dabs it needs. Winit delivers all queued events before `RedrawRequested`. With a 500–1000 Hz mouse and dabs that cost several milliseconds, the app falls further behind the hand the harder the user drags, and the frame rate collapses along with it.

### Fix, part A: queue the pointer, apply it once per frame

In `editor.rs`, `Drag::Stroke` gets a `queued: Option<Vec2>` field. When coalescing is enabled, `pointer_moved` only records the position; a new `flush_stroke()` applies it. I kept the old stroke body unchanged and moved it into `stroke_to`:

```rust
// Editor fields (defaults keep headless use and tests immediate)
pub coalesce_strokes: bool,   // default false
pub dab_budget_ms: f32,       // default 0.0 = unlimited

// in pointer_moved's match:
Drag::Stroke { queued, .. } if self.coalesce_strokes => {
    *queued = Some(pos);
    true
}
Drag::Stroke { .. } => self.stroke_to(pos),   // the previous arm body, unchanged

/// Call once per frame, before drawing.
pub fn flush_stroke(&mut self) -> bool {
    let queued = match &mut self.drag {
        Drag::Stroke { queued, .. } => queued.take(),
        _ => None,
    };
    queued.is_some_and(|pos| self.stroke_to(pos))
}

// pointer_up must apply the last queued sample before committing:
pub fn pointer_up(&mut self) {
    self.flush_stroke();
    /* ... existing code ... */
}
```

In `app.rs`, enable it only for interactive runs (your `--demo` / `--screenshot` flows drive the editor synchronously, so I left them alone), and flush at the top of the redraw:

```rust
if auto.screenshot.is_none() {
    e.coalesce_strokes = true;
    e.dab_budget_ms = std::env::var("OXYBRUSH_DAB_BUDGET_MS")
        .ok().and_then(|v| v.parse().ok()).unwrap_or(10.0);   // 0 = off, handy for A/B
}
// ... at the start of the redraw path:
self.editor.flush_stroke();
```

**Trade-off:** one sample per frame under-samples very fast direction reversals. In my deliberately extreme test (scrubbing back and forth 4 times per second) the result had about 8% less total displacement. If that matters, buffer the frame's samples and replay a bounded number of them.

### Fix, part B: a dab budget in `oxy-sculpt`

Coalescing alone does not help if one frame's worth of dabs is itself too slow. The budget measures what a dab costs (median of the last 8, including all symmetry copies) and allows only as many dabs per call as fit the time budget. When the stroke wants more, it **widens the spacing and scales strength up to compensate**, so nothing is queued or dropped and the stroke stays attached to the pointer. It is opt-in via `Stroke::with_budget(ms)`.

```rust
#[derive(Debug, Clone)]
pub struct DabBudget {
    pub ms: f32,                    // time allowed per add_sample call
    pub max_dabs: u32,              // hard cap per call (I used 8)
    pub max_strength_scale: f32,    // cap on strength compensation (I used 4.0)
    recent: [f32; 8],
    count: usize,
    spacing_scale: f32,
}

impl DabBudget {
    pub fn new(ms: f32) -> Self {
        Self { ms, max_dabs: 8, max_strength_scale: 4.0, recent: [0.0; 8], count: 0, spacing_scale: 1.0 }
    }
    pub fn dab_ms(&self) -> Option<f32> {            // median of recent dabs
        let n = self.count.min(self.recent.len());
        if n == 0 { return None; }
        let mut v = self.recent;
        v[..n].sort_unstable_by(f32::total_cmp);
        Some(v[n / 2])
    }
    fn record(&mut self, ms: f32) {
        self.recent[self.count % self.recent.len()] = ms;
        self.count += 1;
    }
}

/// How many dabs fit the budget, and how much to widen spacing so the dabs
/// the stroke wanted still cover the segment. Returns (max_dabs, spacing_factor >= 1).
fn plan_dabs(wanted: f32, dab_ms: Option<f32>, budget_ms: f32, hard_cap: u32) -> (u32, f32) {
    let cap = hard_cap.max(1);
    let n_max = match dab_ms {
        Some(c) if c > 0.0 => ((budget_ms / c) as u32).clamp(1, cap),
        _ => cap,
    };
    (n_max, (wanted / n_max as f32).max(1.0))
}
```

`Stroke` gets two fields (`budget: Option<DabBudget>`, `strength_scale: f32`) and `add_sample` changes like this. With no budget the behaviour is identical to before.

```rust
let nominal = (self.settings.radius * self.settings.spacing).max(1e-6);
let seg = sample.position - last.position;
let len = seg.length();
let (step, max_dabs) = match &mut self.budget {
    Some(b) => {
        let wanted = (len + self.carry) / nominal;
        let (n_max, k) = plan_dabs(wanted, b.dab_ms(), b.ms, b.max_dabs);
        b.spacing_scale = k;
        self.strength_scale = k.min(b.max_strength_scale);
        (nominal * k, n_max)
    }
    None => (nominal, u32::MAX),
};
let mut d = (step - self.carry).max(0.0);   // max(0): carry can exceed step if the cap stopped the loop early
let mut placed = 0;
while d <= len && placed < max_dabs {
    let t = if len > 0.0 { d / len } else { 0.0 };
    /* build StrokeSample s at t, exactly as before */
    self.apply_dab(mesh, s, touch);
    placed += 1;
    d += step;
}
self.carry = len - (d - step);
```

Each dab is timed, and the strength is scaled, by wrapping the existing `apply_dab`:

```rust
fn apply_dab(&mut self, mesh: &mut Mesh, s: StrokeSample, touch: &mut dyn FnMut(&Mesh, u32)) {
    if self.budget.is_none() { return self.apply_dab_inner(mesh, s, touch); }
    let start = std::time::Instant::now();
    self.apply_dab_inner(mesh, s, touch);
    let ms = start.elapsed().as_secs_f32() * 1e3;
    if let Some(b) = &mut self.budget { b.record(ms); }
}
// apply_dab_inner = the old apply_dab, plus right after make_dab:
//     base.strength *= self.strength_scale;
```

A cheap way to surface it in the UI is two getters: `Stroke::dab_ms()` and `Stroke::spacing_scale()`.

### Measured result

`stroke_lag` (appendix B) simulates one second of a fast scrubbing drag: a 1000 Hz mouse and 60 Hz frames, brush radius 0.3 on an icosphere.

| Mesh | Mode | Dabs | CPU-s per input-s | Worst single call | Total displacement |
|---|---|---|---|---|---|
| 1.3M tris | today (per event) | 214 | 0.47 | 3.6 ms | 5968 |
| 1.3M tris | per frame, 8 ms budget | 157 | 0.30 | 7.2 ms | 5468 |
| **5.2M tris** | **today (per event)** | 214 | **1.95** | 13.4 ms | 23872 |
| **5.2M tris** | **per frame, 8 ms budget** | 56 | **0.46** | 13.3 ms | 22785 |
| **5.2M tris** | **per frame, 12 ms budget** | 68 | **0.44** | 12.9 ms | 22559 |

Anything above 1.0 in the "CPU-s per input-s" column means the program cannot keep up with the hand. At 5.2M the budgeted stroke widened spacing to about 3.6× (8 ms budget) and kept about 95% of the displacement. The worst call stays near 13 ms even then, because one dab is the minimum unit of work. On a multi-core machine dabs are cheaper, and the budget adapts on its own without a code change.

### Tests I wrote for it

- `plan_dabs` is a pure function: unit tests cover "fits", "widens", "one dab minimum", and "hard cap".
- No budget means bit-identical positions and the same dab count as before (the determinism guard).
- A budget no dab can meet gives at most one dab per call and fewer than a third of the unbudgeted dabs.
- The stroke still reaches its endpoint, and total displacement stays within 0.3–1.5× of the unbudgeted stroke.

I deliberately avoided wall-clock assertions, since they are flaky in CI.

---

## 2. `update_after_move`: shorten the serial part

### Where a dab's time goes

I temporarily instrumented `apply_kernel` and `update_after_move` (single thread, 1.3M tris, radius 0.3, ~14.6k vertices moved):

```
gather  0.39 ms    kernel 0.13 ms    writeback 0.03 ms    update_after_move 1.51 ms
  └─ dedup (tris + verts) 0.60 ms   normals 0.60 ms   chunk-bounds growth 0.34 ms
```

On a many-core machine the normals pass (already `par_iter`) shrinks, but the dedup and bounds passes are serial loops, so they set a floor on dab time that more cores can't lower.

### What did NOT work (worth knowing so nobody repeats it)

My first rewrite computed each affected triangle's face normal **once** into a scratch list and had vertices sum cached face normals. It was **~35% slower** single-threaded (1.86 ms vs 1.35 ms). The extra indirection (stamp check, slot lookup, extra array) costs more than the cross products it saves, because those gathers are already cache-friendly thanks to your Morton ordering. Recomputing per vertex, as you do, is the right call.

### What worked: keep the math, parallelize the serial passes

Three changes, all behaviour-preserving:

1. **Derive the affected vertex set directly.** The affected vertices are the moved ones plus their neighbours, which is exactly the set of corners of triangles touching a moved vertex. Use the neighbour table you already have. This skips the intermediate triangle list and does roughly half the stamp checks (about 102k vs 176k for the 14.6k-vertex case).
2. **Atomic stamps so the dedup can run on `rayon`.** `AtomicU32` is `std`, so there is no `unsafe`. The plain `load` before the `swap` skips the locked operation for ids that are already claimed, which is most of them.
3. **Grow chunk bounds from the moved vertices only, per thread.** Unmoved corners are already inside the chunk box (that's the invariant), so only moved corners need to grow it. Accumulate sparse per-thread boxes, then apply them.

```rust
/// Generation stamps for parallel dedup. Cloning gives a fresh zeroed set:
/// stamps are scratch, not state. (Needed because AtomicU32 isn't Clone and
/// Mesh derives Clone.)
#[derive(Debug, Default)]
struct Stamps(Vec<AtomicU32>);
impl Stamps {
    fn new(n: usize) -> Self { Stamps((0..n).map(|_| AtomicU32::new(0)).collect()) }
    fn reset(&mut self) { self.0.iter_mut().for_each(|a| *a.get_mut() = 0); }
    /// True for exactly one caller per (id, stamp), even across threads.
    #[inline]
    fn claim(&self, id: u32, stamp: u32) -> bool {
        let a = &self.0[id as usize];
        a.load(Ordering::Relaxed) != stamp && a.swap(stamp, Ordering::Relaxed) != stamp
    }
}
impl Clone for Stamps { fn clone(&self) -> Self { Stamps::new(self.0.len()) } }
```

The `Mesh` fields `tri_stamp: Vec<u32>` and `vert_stamp: Vec<u32>` become a single `vert_stamp: Stamps`. The triangle stamp is no longer needed.

```rust
pub fn update_after_move(&mut self, moved: &[u32]) {
    if moved.is_empty() { return; }
    self.stamp = self.stamp.wrapping_add(1);
    if self.stamp == 0 { self.vert_stamp.reset(); self.stamp = 1; }
    let stamp = self.stamp;
    const PAR_ABOVE: usize = 2048;

    // 1. moved vertices + their neighbours, de-duplicated (parallel when large).
    let verts = collect_unique(moved, |&v| self.vertex_neighbors.get(v as usize),
                               |&v| v, &self.vert_stamp, stamp);

    // 2. normals: unchanged maths, parallel above a threshold so tiny dabs avoid fork/join cost.
    let normals: Vec<Vec3> = if verts.len() > PAR_ABOVE {
        verts.par_iter().map(|&v| self.compute_vertex_normal(v)).collect()
    } else {
        verts.iter().map(|&v| self.compute_vertex_normal(v)).collect()
    };
    for (&v, n) in verts.iter().zip(normals) {
        self.normals[v as usize] = n;
        self.dirty[self.vertex_chunk[v as usize] as usize] = true;
    }

    // 3. bounds: grow each triangle's chunk by the moved corner only, sparse per thread.
    let grow = |part: &[u32]| -> Vec<(u32, Aabb)> {
        let mut acc: Vec<(u32, Aabb)> = Vec::new();
        let mut last = (u32::MAX, 0usize);               // cache: neighbours are usually in one chunk
        for &v in part {
            let p = self.positions[v as usize];
            for &t in self.vertex_triangles.get(v as usize) {
                let c = self.triangle_chunk[t as usize];
                if last.0 != c {
                    let i = acc.iter().position(|e| e.0 == c).unwrap_or_else(|| {
                        acc.push((c, Aabb::EMPTY)); acc.len() - 1 });
                    last = (c, i);
                }
                acc[last.1].1.grow(p);
            }
        }
        acc
    };
    let boxes: Vec<(u32, Aabb)> = if moved.len() > PAR_ABOVE {
        moved.par_chunks(1024).map(grow).flatten_iter().collect()
    } else { grow(moved) };
    for (c, b) in boxes {
        let chunk = &mut self.chunks[c as usize];
        chunk.aabb = chunk.aabb.union(&b);
        self.loose_bounds[c as usize] = true;
    }
}

/// Distinct ids among `items` (via `seed`) and what `expand` yields for them.
fn collect_unique<'a>(items: &[u32], expand: impl Fn(&u32) -> &'a [u32] + Sync,
                      seed: impl Fn(&u32) -> u32 + Sync, stamps: &Stamps, stamp: u32) -> Vec<u32> {
    const SERIAL_BELOW: usize = 4096;
    let run = |part: &[u32]| -> Vec<u32> {
        let mut out = Vec::with_capacity(part.len() * 2);
        for item in part {
            let s = seed(item);
            if stamps.claim(s, stamp) { out.push(s); }
            for &id in expand(item) { if stamps.claim(id, stamp) { out.push(id); } }
        }
        out
    };
    if items.len() <= SERIAL_BELOW { return run(items); }
    let parts: Vec<Vec<u32>> = items.par_chunks(1024).map(run).collect();
    let mut out = Vec::with_capacity(parts.iter().map(Vec::len).sum());
    parts.into_iter().for_each(|mut p| out.append(&mut p));
    out
}
```

### Honest status

- **Single thread:** about equal to your version (min 1.27–1.33 ms vs 1.33 ms; noisy). I'm not claiming a single-thread speedup.
- **Multi-core:** the serial share of this function drops from roughly two thirds to a few small loops (dirty flags, applying the per-chunk boxes). That should scale, but **I could not measure it.** Please run `update_timing` (appendix A) with `RAYON_NUM_THREADS=1` and with the default.

### Correctness tests (important for a change like this)

- A test moves scattered vertex subsets (300, 3,000 and 20,000, so both the serial and parallel paths run), repeated over 6 rounds so stamps are reused. After each round it asserts that every normal matches `recompute_all_normals()` to 1e-5, and that **every chunk box still contains every corner of its triangles**.
- A test checks that a cloned mesh updates independently (the stamps are not shared or stale).
- I ran the equivalence test under `RAYON_NUM_THREADS=8` six times, and the whole sculpt suite under 16 threads, to shake out races in the atomic dedup. All passed. This was on one core, so threads were interleaved by the OS rather than truly concurrent. A TSan or Miri run on your CI would be a stronger check.

---

## 3. GPU vertex packing (`oxy-render::sync`)

### What happens today

`vertex()` runs for every dirty vertex, on the UI thread, and for each one does an adjacency lookup (`vertex_triangles(i).first()`) plus HSV-to-RGB float math in `group_color`. But when the GPU supports `PRIMITIVE_INDEX` (the `exact_face_colors` path), the shader takes the colour from `face_colors[primitive_index]` and **never reads the per-vertex value**. Also, each dirty chunk is a separate `write_buffer`.

### Measured (32 dirty chunks, ~16k vertices, single thread)

| Variant | Time | ns / vertex |
|---|---|---|
| current | 0.54 ms | ~34 |
| group-colour lookup table | 0.11–0.32 ms | ~7–20 |
| field skipped (exact path) | 0.03–0.04 ms | ~2 |

(The lookup-table timings varied between runs on this noisy machine.)

### Change

```rust
// Built once per rebuild (topology/face change): one entry per group id.
let lut: Vec<u32> = {
    let max = mesh.groups().iter().copied().max().unwrap_or(0);
    (0..=max).map(group_color).collect()
};
// Only needed when there is no primitive_index; otherwise pass an empty slice.
let vertex_lut: &[u32] = if self.exact_face_colors { &[] } else { &lut };

fn vertex(mesh: &Mesh, i: usize, lut: &[u32]) -> GpuVertex {
    let group_color = if lut.is_empty() { 0 } else {
        mesh.vertex_triangles(i as u32).first()
            .map_or(0, |&t| lut[mesh.groups()[t as usize] as usize])
    };
    GpuVertex {
        position: mesh.positions[i].to_array(),
        normal: mesh.normals[i].to_array(),
        mask: mesh.mask[i],
        group_color,
    }
}
```

On rebuild, the full vertex array and the face colours are packed in parallel (`into_par_iter`) using the same table. When nothing is hidden, the mesh's own `triangles` slice is used as the index buffer instead of copying 12 bytes per triangle into a new `Vec`.

For dirty uploads, merge neighbouring chunks into runs (chunks own contiguous vertex ranges) so a big brush is a few uploads, and pack large runs in parallel:

```rust
let mut runs: Vec<std::ops::Range<usize>> = Vec::new();
for &c in &dirty {
    let r = mesh.chunks[c as usize].vertices.clone();
    if r.is_empty() { continue; }
    let (start, end) = (r.start as usize, r.end as usize);
    match runs.last_mut() {
        Some(last) if last.end == start => last.end = end,
        _ => runs.push(start..end),
    }
}
let m: &Mesh = mesh;
for run in runs {
    let staging: Vec<GpuVertex> = if run.len() > 4096 {
        run.clone().into_par_iter().map(|i| vertex(m, i, &gpu.group_lut)).collect()
    } else {
        run.clone().map(|i| vertex(m, i, &gpu.group_lut)).collect()
    };
    queue.write_buffer(&gpu.vertices, run.start as u64 * size_of::<GpuVertex>() as u64,
                       bytemuck::cast_slice(&staging));
}
```

`GpuMesh` gains `group_lut: Vec<u32>` (empty on the exact path), and `oxy-render` gains `rayon.workspace = true`. A unit test checks that the table-based packing equals the old per-vertex computation exactly.

**Not verified:** this compiles but I have not run it on a real GPU. The shader and buffer layout are unchanged, so I expect identical output, but a quick `--screenshot` comparison on your side would confirm it.

---

## 4. Chunk size

I swept `CHUNK_TRIANGLES` over 512 / 1024 / 2048 / 4096 / 8192 with `dab_timing` and my frame benchmark. Dab time was roughly flat (within noise). 1024 gave about half the raycast time of 4096, and a 4× smaller unit for undo snapshots and dirty GPU uploads. It also gives rayon more, smaller items for small brushes. Very small chunks add per-chunk overhead, so please re-check on your reference machine. Nothing in the tests assumed 4096.

---

## 5. Things I measured or noticed but did not change

**Full-mesh passes (single thread):**

| | 1.3M tris | 5.2M tris |
|---|---|---|
| `Mesh::from_data` | 231 ms | 1,112 ms |
| `Mesh::clone` | 48 ms | 270 ms |
| `recompute_all_normals` | 27 ms | 111 ms |
| `refresh_all` | 40 ms | 169 ms |

`from_data` is the standout. It runs on load, remesh, subdivision-level change and topology undo, and its remap and fill loops look serial. Parallelizing those loops is worth a look for the "rebuilding lower levels" item on your roadmap.

**Ideas that I believe would help but did not build or test** (treat these as hypotheses):

- **Frustum-cull chunk draws.** The index buffer is already in chunk order, so each chunk's triangles are a contiguous index range (record the range per chunk while compacting hidden triangles). Copy each frame's `mesh.chunks[c].aabb` into the renderer (don't rely on dirty flags, since bounds can grow for chunks that aren't marked dirty), test them against the view-projection frustum, and issue one `draw_indexed` per run of visible chunks. When zoomed in on a 10M-triangle mesh this should cut vertex work a lot, mostly on integrated GPUs.
- **Smaller vertex and G-buffer formats.** The vertex is 32 bytes; a packed normal and mask would be about 20. The normal/depth target is `Rgba32Float` (16 B/pixel), and `Rgba16Float` may be enough.
- **Staged GPU uploads.** I did not pursue this. I believe `wgpu`'s `write_buffer` already stages internally, so writing into a buffer the GPU is still using probably isn't a stall risk here, but I didn't verify that.

---

## Appendix A: `update_timing` (min and median over repeats)

```rust
// crates/oxy-mesh/examples/update_timing.rs
use oxy_mesh::{Vec3, primitives};
use std::time::Instant;

fn main() {
    let level: u32 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(8);
    let mut mesh = primitives::icosphere(level, 1.0);
    println!("{} tris, {} chunks, {} rayon threads",
             mesh.triangle_count(), mesh.chunks.len(), rayon::current_num_threads());
    for radius in [0.05f32, 0.1, 0.2, 0.3, 0.5] {
        let c = Vec3::Z;
        let moved: Vec<u32> = (0..mesh.vertex_count() as u32)
            .filter(|&v| mesh.positions[v as usize].distance(c) < radius).collect();
        let mut times: Vec<f64> = (0..60).map(|_| {
            let t = Instant::now();
            mesh.update_after_move(&moved);
            t.elapsed().as_secs_f64() * 1e3
        }).collect();
        times.sort_by(f64::total_cmp);
        println!("{:>7} moved verts: min {:.3} ms  median {:.3} ms",
                 moved.len(), times[0], times[times.len() / 2]);
    }
}
```

Run it twice and compare: `RAYON_NUM_THREADS=1 cargo run --release -p oxy-mesh --example update_timing 8`, then again without the variable. The ratio on your machine is the number I couldn't get.

## Appendix B: `stroke_lag`

This needs `Stroke::with_budget`, `dab_count`, `dab_ms` and `spacing_scale`. Run it with `cargo run --release -p oxy-sculpt --example stroke_lag 9 0.3`.

```rust
// crates/oxy-sculpt/examples/stroke_lag.rs
use glam::Vec3;
use oxy_mesh::primitives;
use oxy_sculpt::{BrushKind, BrushSettings, Stroke, StrokeSample, Symmetry};
use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let level: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(8);
    let radius: f32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(0.3);
    let base = primitives::icosphere(level, 1.0);

    // Scrub back and forth over +-0.6 rad, 4 passes in 1 s, sampled at 1000 Hz.
    let sample_at = |i: usize| {
        let phase = (i as f32 / 1000.0 * 4.0).fract();
        let tri = if phase < 0.5 { phase * 2.0 } else { 2.0 - phase * 2.0 };
        let a = -0.6 + 1.2 * tri;
        let p = Vec3::new(a.sin(), 0.0, a.cos());
        StrokeSample { position: p, normal: p, pressure: 1.0 }
    };
    let settings = BrushSettings { kind: BrushKind::Draw, radius, strength: 0.5, spacing: 0.15, ..Default::default() };
    let total = |m: &oxy_mesh::Mesh| m.positions.iter().map(|p| p.length() - 1.0).sum::<f32>();

    // A: what the app does today, every pointer event runs its dabs.
    let mut mesh = base.clone();
    let mut stroke = Stroke::new(settings, Symmetry::default(), false);
    let t0 = Instant::now();
    for i in 0..1000 { stroke.add_sample(&mut mesh, sample_at(i), &mut |_, _| {}); }
    println!("per-event, no budget: {} dabs, {:.2} CPU s per input s, displacement {:.1}",
             stroke.dab_count(), t0.elapsed().as_secs_f64(), total(&mesh));

    // B: once per frame (60 Hz), budgeted.
    for budget in [8.0f32, 12.0] {
        let mut mesh = base.clone();
        let mut stroke = Stroke::new(settings, Symmetry::default(), false).with_budget(budget);
        let t0 = Instant::now();
        for frame in 0..60 {
            stroke.add_sample(&mut mesh, sample_at((frame * 1000 / 60 + 16).min(999)), &mut |_, _| {});
        }
        println!("per-frame, {budget} ms: {} dabs, {:.2} CPU s per input s, displacement {:.1}, spacing x{:.1}",
                 stroke.dab_count(), t0.elapsed().as_secs_f64(), total(&mesh), stroke.spacing_scale());
    }
}
```

## Suggested order, if you want to adopt any of this

1. **Section 1** (coalesce plus budget). It is the biggest win for how sculpting feels at high resolution, and it is opt-in. `OXYBRUSH_DAB_BUDGET_MS=0` gives you an instant A/B switch.
2. **Section 3** (vertex packing). It is small, local and low risk.
3. **Section 2** (`update_after_move`) after you've confirmed on a many-core machine that it helps. It is the one change I could not show a benefit for here.
4. **Section 4** (chunk size) is a one-line experiment.

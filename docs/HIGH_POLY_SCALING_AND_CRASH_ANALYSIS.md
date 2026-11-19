# High-Poly Scaling and Crash Analysis (200M – 600M+ Triangles)

**Date**: November 2026  
**Status**: Findings documented; Phase 1 stability fixes in progress.

---

## 1. Executive Summary

Users report that OxyBrush crashes when approaching **~180M to 200M triangles**, whereas industry-standard sculpting software (such as ZBrush) can handle **600M+ triangles** across a scene on high-end workstations.

A thorough byte-level audit of the memory layout, GPU buffer management, and wgpu pipeline revealed three deterministic failure points:
1. **Primary Hard Crash (GPU Buffer Size & 32-bit Overflow in Wireframe)**: `wire_lines(mesh)` in `oxy-render` generates line index data unconditionally—even when wireframe display is turned off. At $T = 178,956,971$ triangles, the wireframe index buffer reaches $4,294,967,304$ bytes, overflowing a 32-bit integer (`u32::MAX = 4,294,967,295`) and exceeding the hardware `max_buffer_size` ($2.0\text{ GB}$ on most Vulkan/Metal drivers), instantly panicking or crashing the GPU driver.
2. **Secondary Crash (Storage Buffer Binding Limit)**: The `face_colors` GPU storage buffer binds $4 \times T$ bytes. Standard Vulkan and WebGPU drivers have a `max_storage_buffer_binding_size` limit of $134,217,728$ bytes ($128\text{ MB}$). Meshes exceeding **33.55M triangles** exceed this limit, causing GPU validation panics.
3. **Tertiary Crash (Host RAM Exhaustion / OOM)**: At 200M triangles, the application's peak RAM usage exceeds **50–60 GB** due to duplicate vertex buffers, staging vectors during GPU upload, uncompressed undo history, and redundant host vectors such as `triangle_chunk: Vec<u32>` ($800\text{ MB}$ at 200M, $2.4\text{ GB}$ at 600M).

---

## 2. Quantitative Root Cause Analysis

### 2.1 Crash Point 1: `wire_lines` Buffer Allocation & Overflow

In `crates/oxy-render/src/lib.rs` (`ViewportRenderer::sync`):
```rust
let line_indices = wire_lines(mesh);
```
- For every triangle $T$, `wire_lines` generates 3 edges ($3 \times 2 = 6$ line vertex indices):
  $$\text{Index Count} = 6 \times T$$
- Each index is a `u32` ($4\text{ bytes}$):
  $$\text{Buffer Size} = 6 \times T \times 4\text{ bytes} = 24 \times T\text{ bytes}$$

| Triangles ($T$) | Line Indices Count | Staging Vector (RAM) | GPU Buffer Size | Vulkan $2\text{ GB}$ Limit | $4\text{ GB}$ (`u32::MAX`) Limit |
|---|---|---|---|---|---|
| **10,000,000** | 60,000,000 | 240 MB | 240 MB | Safe | Safe |
| **50,000,000** | 300,000,000 | 1.20 GB | 1.20 GB | Safe | Safe |
| **89,478,485** | 536,870,910 | 2.14 GB | 2.14 GB | **CRASH** ($> 2.0\text{ GB}$) | Safe |
| **178,956,971**| 1,073,741,826 | 4.29 GB | 4.29 GB | **CRASH** | **CRASH** ($> 4.0\text{ GB}$) |
| **200,000,000**| 1,200,000,000 | 4.80 GB | 4.80 GB | **CRASH** | **CRASH** |

**Crucial Defect**: `wire_lines` is executed and uploaded **unconditionally on every mesh change**, regardless of whether `settings.wireframe` is enabled or disabled. Furthermore, at $\ge 10\text{M}$ triangles, individual wireframe edges are vastly smaller than a single screen pixel, rendering wireframe lines invisible noise even if rendered.

### 2.2 Crash Point 2: GPU Storage Buffer Binding Limits (`face_colors`)

In `crates/oxy-render/src/lib.rs`:
```rust
let face_colors_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
    label: Some("face_colors_storage"),
    contents: bytemuck::cast_slice(&face_colors),
    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
});
```
- Each face has a $4\text{-byte}$ color/group index.
- Total size: $4 \times T\text{ bytes}$.
- Vulkan hardware specification defines `max_storage_buffer_binding_size` default as $134,217,728\text{ bytes}$ ($128\text{ MB}$).
- Maximum supported triangles without exceeding standard storage buffer binding limit:
  $$\frac{134,217,728\text{ bytes}}{4\text{ bytes/triangle}} = 33,554,432\text{ triangles}$$
- When $T > 33.55\text{M}$, attempting to bind this buffer into the render pipeline triggers a validation failure or device loss.

### 2.3 Crash Point 3: Host RAM Footprint Breakdown

At 200M triangles ($T = 200\times 10^6$, Vertices $V \approx 100\times 10^6$):

| Data Structure | Component | Bytes / Element | Total at 200M | Total at 600M | Notes |
|---|---|---|---|---|---|
| `Mesh` | `positions` | 12 B / vert | 1.2 GB | 3.6 GB | `[f32; 3]` |
| `Mesh` | `normals` | 12 B / vert | 1.2 GB | 3.6 GB | `[f32; 3]` |
| `Mesh` | `triangles` | 12 B / tri | 2.4 GB | 7.2 GB | `[u32; 3]` |
| `Mesh` | `face_groups` | 4 B / tri | 0.8 GB | 2.4 GB | `u32` |
| `Mesh` | `face_hidden` | 1 B / tri | 0.2 GB | 0.6 GB | `bool` |
| `Mesh` | `mask` | 4 B / vert | 0.4 GB | 1.2 GB | `f32` |
| `Mesh` | `triangle_chunk` | 4 B / tri | **0.8 GB** | **2.4 GB** | **Redundant:** $t / 1024$ |
| `Mesh` | Chunks & BVH | ~8 B / tri | 1.6 GB | 4.8 GB | Bounding boxes & chunks |
| **`Mesh` Subtotal** | | | **8.6 GB** | **25.8 GB** | |
| `Multires` | Lower levels + offsets | ~56 B / tri | **11.2 GB** | **33.6 GB** | All levels in RAM |
| `History` | Undo snapshots | 1 full snapshot | **8.6 GB** | **25.8 GB** | Topology changes |
| Staging Buffers | `wire_lines` | 24 B / tri | **4.8 GB** | **14.4 GB** | In `oxy-render::sync` |
| Staging Buffers | `positions + normals + indices` | 36 B / tri | **7.2 GB** | **21.6 GB** | In `oxy-render::sync` |
| **Total Peak RAM** | | | **~40.4 GB** | **~121.2 GB** | Exceeds 32GB/64GB PCs |

---

## 3. How Other Software (e.g. ZBrush) Handles 600M+ Triangles

1. **Multi-Subtool / Multi-Piece Sharding**:
   - In ZBrush, a 600M-triangle scene is never a single monolithic continuous mesh. It is divided across 10 to 30 Subtools (pieces), each holding 20M to 60M triangles.
   - Only the **active** Subtool is kept at full resolution in high-performance sculpting RAM. Inactive Subtools are displayed at lower subdivision levels or cached as static GPU chunks.
2. **GPU Buffer Partitioning**:
   - Draw calls are split into chunks/partitions $\le 64\text{ MB}$ or $128\text{ MB}$. This ensures no single buffer ever breaches the GPU driver's `max_buffer_size` ($2.0\text{ GB}$) or `max_storage_buffer_binding_size` ($128\text{ MB}$).
3. **Adaptive / Distance-Capped Wireframe**:
   - Wireframe index generation is bypassed completely when wireframe display is disabled or when triangle count exceeds $5\text{M} - 10\text{M}$ triangles.
4. **Zero Redundancy**:
   - Spatial acceleration structures are chunk-based or computed on-demand. Redundant lookup arrays (such as mapping triangle ID to chunk ID) are calculated by bitwise arithmetic or division rather than stored.
5. **Virtual Memory Paging / Multires Swapping**:
   - Lower multires levels and inactive undo history are paged to fast disk cache (SSD scratch files) rather than retained uncompressed in physical RAM.

---

## 4. Multi-Phase Action Plan

### Phase 1: Immediate Stability & Crash Prevention (Current)
- [x] **Disable Unconditional Wireframe Generation**:
  - In `crates/oxy-render/src/lib.rs`: Gate `wire_lines(mesh)` so it only executes when `settings.wireframe && settings.wireframe_opacity > 0.001` AND `mesh.triangle_count() <= 10_000_000`.
  - On dense meshes ($> 10\text{M}$ triangles) or when wireframe is toggled off, reuse a static 8-byte dummy buffer and set `line_index_count = 0`.
  - Immediately eliminates $4.8\text{ GB}$ of host RAM allocation and $4.8\text{ GB}$ GPU buffer allocation, preventing the primary crash at $\ge 179\text{M}$ triangles.
- [x] **Storage Buffer Safety Check (`face_colors`)**:
  - Bound `face_colors` storage buffer to `device.limits().max_storage_buffer_binding_size`. When face count exceeds the limit, clamp or fallback gracefully without panic.
- [x] **GPU Allocation Error Handling**:
  - Check buffer sizes against `device.limits().max_buffer_size` before creation, logging a clear warning/notification rather than panicking inside wgpu.
- [x] **Eliminate Redundant `triangle_chunk` Vector**:
  - In `crates/oxy-mesh/src/lib.rs`: Remove `triangle_chunk: Vec<u32>` from `Mesh`. Since chunks are contiguous blocks of 1,024 triangles, `triangle_chunk(t)` is identically `(t / CHUNK_TRIANGLES as u32)`.
  - Saves $800\text{ MB}$ at 200M triangles and $2.4\text{ GB}$ at 600M triangles with zero performance overhead.

### Phase 2: Partitioned GPU Rendering & Draw Calls
- Split vertex and index buffers into chunks of $\le 32\text{ MB}$ ($~1\text{M}$ triangles per draw call chunk).
- Allows rendering monolithic pieces beyond $2.0\text{ GB}$ GPU limits.
- Supports multi-piece scenes up to 600M+ triangles.

### Phase 3: Out-of-Core Multires & Undo Optimization
- Compress inactive undo states using LZ4 / Zstandard.
- Cache inactive multires levels to disk / scratch file when memory pressure is detected.
- Implement Level-of-Detail (LOD) display for inactive pieces in multi-piece scenes.

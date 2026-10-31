# RFC-0001: OxyBrush product concept and architecture

- **Status:** Accepted (living document)
- **Scope:** product vision, 1.0 scope, architecture, roadmap

## 1. Concept

OxyBrush is an open-source desktop app for sculpting high-resolution organic and hard-surface forms. Every frame should feel like pushing clay, and the model fills the screen. It's built for sculptors first and is not meant to become a general 3D suite.

**Inspirations (workflow only, no code, assets or branding):**
- The ZBrush-style loop: block out, refresh topology, refine, add detail. Brushes stay responsive at millions of polygons, and matcap shading is preferred over lighting setups.
- ArtCraft (storytold): Rust desktop software as a serious creative tool, the artist in direct control, and openness as part of the product's identity.

**Non-goals:** animation, rendering, game-engine features, and AI generation in the core. AI features may only ever be opt-in plugins.

**Principles, in priority order:**
1. Latency is a feature.
2. The canvas comes first.
3. No dead ends: undo, autosave, crash recovery.
4. Accessible by default.
5. Brushes are data, tools are plugins.

## 2. 1.0 scope (required)

| Area | Features |
|---|---|
| Scene | Up to ~32 pieces. Show, hide, isolate, duplicate, merge, mirror. |
| Starting shapes | Sphere, cube, cylinder, plane, torus. OBJ/PLY/STL import. |
| Brushes | Draw, Clay, Clay Strips, Inflate, Smooth, Flatten, Pinch, Crease, Grab, Snake Hook, Layer, Mask. |
| Brush controls | Radius, strength, falloff editor, pressure curves, spacing, lazy stroke, front faces only, invert, auto-smooth. |
| Symmetry | X/Y/Z mirror and radial symmetry. |
| Topology | Voxel remesh (with union), subdivide, decimate. |
| Masks and groups | Paint, invert, blur, sharpen, clear. Face groups. Hide by group. |
| Viewport | Matcaps (bundled CC0 plus user-supplied), cavity, wireframe, ortho/perspective, grid. |
| Undo | Memory-budgeted history with a history panel. |
| Input | Pen pressure on Windows/macOS/Linux. Full mouse-only workflow. |
| Files | `.oxb` project format, autosave, crash recovery. OBJ/STL/PLY/glTF export. |
| UI | Quick palette at the cursor, keymap editor, UI scaling, themes, onboarding. |

**Performance gates** (reference hardware: GTX 1660-class, Apple M1, Radeon 680M):
- 60 fps viewport at 5M triangles.
- 60 fps sculpting at 2M triangles.
- ≤8 ms per dab touching about 100k vertices.
- ≤2 s voxel remesh to about 1M triangles.
- ≤1.5 s cold start.

## 3. Long-term goals (not in 1.0)

- **1.x:** multiresolution, dynamic topology, sculpt layers, vertex color, alphas and stencils, curve brushes.
- **2.x:** WASM component plugin API, quad auto-retopology, UVs and baking, live booleans, insert-mesh brushes, armature spheres, pose gizmo.
- **Research track:** reading ZBrush `.ZTL`/`.ZPR` files by interoperability reverse engineering, following the SWFormat process (`docs/formats/zbrush/`). Open-ended; stops by rule if the data is encrypted. Open formats (OBJ, STL, PLY, glTF, later USD) remain the supported interchange.
- **Exploratory:** GPU compute sculpting, USD, a Python bridge, OpenXR, opt-in AI plugins.

## 4. Architecture

- **Chunked mesh.** Triangles are sorted in Morton order into chunks of about 4k triangles. Each vertex is owned by exactly one chunk, and every chunk's vertices are **contiguous**. As a result:
  - a dab touches only the chunks near it (an AABB-vs-sphere test);
  - normals and bounds are recomputed only for affected triangles;
  - the GPU upload is a single `write_buffer` per dirty chunk;
  - an undo snapshot is a copy of two slices per chunk.
- **Stroke engine.** Pointer samples are spaced into dabs (spacing × radius), mirrored for symmetry, and applied by pure brush kernels. Weights and new positions are computed in parallel with rayon and then written back. Grab captures a weighted vertex set at stroke start.
- **History.** Undo and redo *swap* stored data with the live mesh. Strokes store chunk snapshots, and topology operations store the whole previous mesh. Topology versions are globally unique, so the renderer always knows when to rebuild index buffers.
- **Rendering.** wgpu with matcap shading from the view-space normal, a mask tint, and back faces shown tinted. Matcaps are generated procedurally at runtime.
- **App.** winit + egui (via egui-winit / egui-wgpu, not eframe), with AccessKit for screen readers. Editor state is GPU-free and unit-tested.

**Ecosystem choices:**
- wgpu: Vulkan, Metal, DX12, with GL as a fallback.
- winit
- egui and AccessKit
- glam
- rayon
- tobj
- Planned:
  - meshopt for decimation;
  - fast-surface-nets for remeshing;
  - wasmtime for plugins;
  - fluent for localization;
  - cargo-dist for packaging.

Bevy was rejected: sculpting needs exact control over the render loop and latency, and an engine's release cycle would mean constant migration work.

## 5. Accessibility

**1.0:**
- Keyboard navigation and AccessKit labels.
- Remappable hotkeys and sticky modifiers (no chords that must be held).
- A stroke stabilizer.
- A mouse-only workflow and a one-handed preset.
- 75–300% UI scale and a high-contrast theme.
- Colorblind-safe mask and group palettes.
- Reduced motion.
- Fluent localization.

**Long-term:** spoken mesh statistics, voice commands, switch/eye-tracker access, 3D mouse support.

## 6. Schedule (≈3 full-time-equivalent developers)

| Phase | Months | Exit criterion |
|---|---|---|
| 0 Foundations | 0–2 | A 5M-triangle OBJ orbits at 60 fps. Pressure is read on all three OSes. |
| 1 Sculpt core → 0.1 | 2–6 | 60 fps sculpting at 1M triangles. Dab cost is tracked in CI. |
| 2 Topology and documents → 0.3 | 6–10 | A full blockout session with no data loss under crash fuzzing. |
| 3 Experience → 0.9 | 10–14 | An artist usability study and an accessibility audit. |
| 4 Hardening → 1.0 | 14–18 | All performance gates met. No open P0/P1 bugs. |

## 7. Governance

- **License:** MIT OR Apache-2.0. Assets are CC0 or CC-BY.
- **Contributions:** DCO sign-off, no CLA.
- **Process:** RFCs for user-facing changes, an artist advisory group, the Contributor Covenant code of conduct.
- **Privacy:** no telemetry by default.

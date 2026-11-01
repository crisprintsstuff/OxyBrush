# OxyBrush roadmap and status

> Living document: the current plan and what is done. The original design
> and reasoning are in [RFC-0001](rfcs/0001-oxybrush.md); this file
> supersedes its schedule (§6) and 1.0 scope (§2–3) where they differ.
> Effort is part-time weeks for one experienced contributor and is an
> estimate, not a promise.
>
> **Last updated:** 2026-11-01

## Status at a glance

| | |
|---|---|
| **Latest release** | [v0.0.1-alpha](https://github.com/crisprintsstuff/OxyBrush/releases/tag/v0.0.1-alpha) (2026-11-01): Windows installer + portable zip, Linux Flatpak + tarball |
| **Done** | Phases 0, 1 and 2 (first pass), release packaging |
| **Next** | Phase 3 — sculpting depth |
| **Open from earlier phases** | Pen pressure verification on Windows Ink and Linux tablets |
| **Supported platforms** | Windows 10/11 and Linux (X11/Wayland); macOS builds in CI, best-effort |

Legend: ✅ done · 🚧 in progress · ⏳ planned · 🔬 research

---

## Completed

### Phase 0 — Foundations ✅
Workspace, CI (Windows/Linux/macOS), wgpu matcap renderer, chunked mesh core, headless screenshot test.

### Phase 1 — Sculpt core ✅ (one item open)
- ✅ Stroke engine with spacing, symmetry, per-chunk undo
- ✅ Brushes: Draw, Clay, Smooth, Inflate, Flatten, Grab, Mask
- 🚧 **Pen pressure**: wired through winit touch/pen force; **not yet verified** with Windows Ink or Linux (libinput/Wayland tablet-v2) tablets. Needs alpha-tester feedback; may need a dedicated tablet input layer.

### Phase 2 — Topology and documents ✅ (first pass)
- ✅ Brushes: Clay Strips, Pinch, Crease, Layer, Snake Hook (12 total); radial symmetry
- ✅ Multiple pieces per document; voxel remesh and merge (union); quadric decimation
- ✅ Face groups, hide/isolate/delete hidden, group color overlay
- ✅ `.oxb` project format, autosave, crash recovery, unsaved-changes prompt
- ✅ Background jobs for remesh/merge/decimate; scene-wide undo

### Release packaging ✅
Inno Setup installer, Flatpak (`io.github.crisprintsstuff.OxyBrush`), portable archives, app icon, SHA-256 checksums, release-triggered workflow. *(Originally planned for Phase 3.)*

---

## Phase 3 — Sculpting depth ⏳ (next; ~3–4 months) → 0.1

The fundamentals that most separate OxyBrush from mature sculpting tools. Ordered so each step ships something visible and multiresolution lands before the features that depend on it.

| # | Feature | Details | Effort |
|---|---|---|---|
| 3.1 | **Stroke control** | Stabilizer (lazy mouse), drag-to-place stamp, spray, straight line, pressure-curve editor | 2–3 wk |
| 3.2 | **Masking and selection** | Box/lasso hide and mask; mask by cavity, curvature, face group; blur/sharpen mask; grow/shrink visible area | 2–3 wk |
| 3.3 | **Alphas and stencils** | Grayscale image shapes each dab; bundled CC0 alpha set; user alphas | 2–3 wk |
| 3.4 | **More brushes** | Polish, Planar Trim, Topological Move (moves connected surface only), Dam-style crease, Noise | 2–4 wk |
| 3.5 | **Multiresolution** | Subdivision levels; sculpt at any level with detail propagated; rebuild lower levels from an imported dense mesh | 6–10 wk |
| 3.6 | **Performance to 10M triangles** | SIMD brush kernels, parallel normal/bounds updates, chunk frustum culling, faster decimation (current: ~10 s for 1.3M→130k) | 4–8 wk, ongoing |

**Exit criteria:** sculpt at 60 fps on a 10M-triangle multires mesh on reference hardware; every item has tests; README and user docs updated.

## Phase 4 — Workflow and experience ⏳ → 0.5

| Feature | Details | Effort |
|---|---|---|
| **Dynamic topology** | Brush adds/removes triangles locally, so you can sketch from a sphere without remeshing | 4–6 wk |
| **Vertex color painting** | Paint color per vertex; materials per region | 3–4 wk |
| **Sculpt layers** | Detail layers with strength sliders; morph target | 4–6 wk |
| **Symmetry repair and projection** | Symmetrize, mirror-and-weld, project detail between meshes | 2–3 wk |
| **Deformers and posing** | Bend, twist, taper, inflate; pose gizmo with topological masking | 4–6 wk |
| **Viewport** | Shadows, ambient occlusion, wireframe overlay, floor grid, reference image planes, turntable export | 3–5 wk |
| **Interface** | Quick palette at the cursor, keymap editor, custom shelves, remembered settings | 3–4 wk |
| **Accessibility audit** | Review with disabled artists; fix findings (RFC-0001 §5) | 2 wk + fixes |

## Phase 5 — Hardening → 1.0 ⏳

All RFC-0001 performance gates met on reference hardware; file-format freeze (`.oxb` v1 compatibility promise); no open P0/P1 bugs; user guide; signed Windows installer; Flathub submission (from-source Flatpak manifest).

## Parallel tracks

| Track | Status | Next step |
|---|---|---|
| **3D-print tools** | ⏳ | Wall-thickness check, hollowing with drain holes, split into parts with alignment keys, real-world units. Quick wins; can land any time. |
| **Tablet input** | 🚧 | Collect alpha feedback; add a tablet layer (Windows Ink/WinTab, Wayland tablet-v2, XInput2) if winit is insufficient. |
| **ZBrush file research** | 🔬 M0 | Tooling done. Blocked on owner-provided `ztl-dump`/`ztl-diff` output from real files. See [docs/formats/zbrush/ROADMAP.md](formats/zbrush/ROADMAP.md). |

## After 1.0 (1.x / 2.x)

- Automatic quad retopology (algorithms from the Instant Meshes / QuadriFlow research)
- Live (non-destructive) booleans
- Armature spheres for base meshes; curve/tube and insert-mesh brushes
- Polygon modeling (extrude, bevel, inset, edge loops) and creased subdivision preview
- UV unwrap; normal/displacement map baking
- FBX/USD/glTF export; send-to-Blender bridge
- WASM plugin API and scripting
- Hair/fiber and mesh instancing
- Exploratory: GPU compute sculpting, OpenXR, opt-in AI plugins

## Not planned

- A 2.5D "pixel with depth" canvas: a legacy approach modern sculpting tools don't need.
- Bundled AI generation in the core (plugins only, opt-in).

## Known limitations (v0.0.1-alpha)

- Pen pressure unverified on Windows/Linux tablets
- Decimation is slow on very dense meshes (runs in the background)
- Remesh resets mask and face groups; open surfaces remesh to a thin shell; resolution capped at 320
- Settings are not remembered between sessions
- Windows installer is not code-signed (SmartScreen warning)

---

### Updating this file

Update the status table, phase checkmarks and "Last updated" whenever a feature lands, the plan changes, or a release ships. Large plan changes go through an RFC (see CONTRIBUTING.md).

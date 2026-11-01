# OxyBrush roadmap and status

> Living document: the current plan and what is done. The original design
> and reasoning are in [RFC-0001](rfcs/0001-oxybrush.md); this file
> supersedes its schedule (§6) and 1.0 scope (§2–3) where they differ.
> Effort is part-time weeks for one experienced contributor and is an
> estimate, not a promise.
>
> **Last updated:** 2026-11-01 (multiresolution levels landed, 3.5; friendlier interface pass; layouts, arrangeable panels and remembered settings)

## Status at a glance

| | |
|---|---|
| **Latest release** | v0.1.0-alpha (2026-11-01): Windows installer + portable zip, Linux Flatpak + tarball, macOS universal DMG |
| **Done** | Phases 0, 1 and 2 (first pass), release packaging, Phase 3 items 3.1–3.4 |
| **Next** | Phase 3 — rebuilding lower levels (3.5), performance (3.6), touch groundwork and iPad (3.7–3.10) |
| **Open from earlier phases** | Pen pressure verification on Windows Ink and Linux tablets |
| **Supported platforms** | Windows 10/11, Linux (X11/Wayland), macOS 11+ (Apple Silicon and Intel; new in 0.1.0, untested on real Macs so far). iPadOS planned (Phase 3.7–3.10). |

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
Inno Setup installer, Flatpak (`io.github.crisprintsstuff.OxyBrush`), macOS universal app + DMG (0.1.0), portable archives, app icon, SHA-256 checksums, release-triggered workflow. *(Originally planned for Phase 3.)*

---

## Phase 3 — Sculpting depth and iPad 🚧 (3.1–3.4 shipped in v0.1.0-alpha)

The fundamentals that most separate OxyBrush from mature sculpting tools, plus the groundwork for an iPad version. Ordered so each step ships something visible and multiresolution lands before the features that depend on it.

| # | Feature | Details | Status |
|---|---|---|---|
| 3.1 | **Stroke control** | Stabilizer (lazy mouse), Stamp (drag to size), spray, Line, pressure curve | ✅ 0.1.0 |
| 3.2 | **Masking and selection** | Box/lasso hide and mask; mask by cavity, peaks, face group; blur/sharpen/grow/shrink mask; grow/shrink visible area | ✅ 0.1.0 |
| 3.3 | **Alphas and stencils** | Alphas ✅ (7 procedural CC0 + user PNGs, rotation, follow stroke). Stencils (screen-space overlays) ⏳ | 🚧 |
| 3.4 | **More brushes** | Polish, Planar Trim, Topological Move, Noise ✅. A dedicated Dam-style crease ⏳ (Crease covers most uses) | ✅ 0.1.0 |
| 3.5 | **Multiresolution** | Subdivision levels (Loop), sculpt at any level with detail propagated upward, saved in `.oxb` (MRES section), undoable ✅. Rebuild lower levels from an imported dense mesh ⏳; tangent-space detail (so detail rotates with large low-level bends) ⏳. Level switches on ~5M triangles take 2–4 s on the UI thread (mesh rebuild; see 3.6) | 🚧 |
| 3.6 | **Performance to 10M triangles** | SIMD brush kernels, parallel normal/bounds updates, chunk frustum culling, faster decimation (current: ~10 s for 1.3M→130k) (4–8 wk) | ⏳ |
| 3.7 | **Touch and tablet groundwork** (desktop first) | Gesture input layer (two-finger orbit, pinch zoom, three-finger pan, two-finger tap undo); on-screen Smooth/Mask/Invert buttons replacing modifier keys ✅; touch-friendly layout with larger targets ✅ (no hover-dependent UI ⏳); save when the app is suspended; per-platform memory limits for undo and mesh size. Also improves Windows/Linux touchscreens and convertibles. (3–5 wk) | 🚧 |
| 3.8 | **iPad build** | iPadOS target (wgpu/Metal, winit), small platform crate using `objc2` for iOS APIs, document picker and Files-app integration, generated Xcode project, CI job producing an `.ipa`. (1–2 wk) | ⏳ |
| 3.9 | **iPad and Apple Pencil polish** | Palm rejection (Pencil vs. finger), pressure tuning, simplified panel layout, testing on real iPads. (4–8 wk) | ⏳ |
| 3.10 | **TestFlight beta** | Signing and upload from CI; internal testers, then a public beta link. **Blocked on maintainer:** Apple Developer Program membership and App Store Connect API key (planned for later). (~1 wk once unblocked) | ⏳ blocked |

**Exit criteria:** sculpt at 60 fps on a 10M-triangle multires mesh on reference hardware; OxyBrush runs on an iPad with Apple Pencil pressure (distributed via TestFlight once 3.10 is unblocked); every item has tests; README and user docs updated.

### iPad notes

- **iPad only.** No iPhone build is planned; the screen is too small for sculpting.
- **Order:** 3.7 needs no Apple account and benefits every platform, so it can start any time. 3.8–3.9 need a Mac or a macOS CI runner; 3.10 needs the paid Apple Developer Program.
- **Decision pending (before 3.8):** calling iOS APIs from Rust needs `unsafe`, which AGENTS.md currently forbids workspace-wide. The proposal is to allow it only in one small, reviewed platform crate.
- **Bonus of the Apple account:** the same membership allows notarizing the macOS app, removing the "Open Anyway" step for Mac users.

## Phase 4 — Workflow and experience ⏳ → 0.5

| Feature | Details | Effort |
|---|---|---|
| **Dynamic topology** | Brush adds/removes triangles locally, so you can sketch from a sphere without remeshing | 4–6 wk |
| **Vertex color painting** | Paint color per vertex; materials per region | 3–4 wk |
| **Sculpt layers** | Detail layers with strength sliders; morph target | 4–6 wk |
| **Symmetry repair and projection** | Symmetrize, mirror-and-weld, project detail between meshes | 2–3 wk |
| **Deformers and posing** | Bend, twist, taper, inflate; pose gizmo with topological masking | 4–6 wk |
| **Viewport** | Shadows, ambient occlusion, wireframe overlay, floor grid, reference image planes, turntable export | 3–5 wk |
| **Interface** | ✅ First friendliness pass (welcome window, brush/alpha/material previews with thumbnail pickers, brush icons and tooltips, toolbar with Raise/Carve, view strip and axis gizmo, gradient background, collapsible sections, status-bar hints). Layouts (Standard, Compact, Touch), arrangeable panels (shelf left/bottom/right, panel side, centered size/strength bar), drag-to-reorder brushes and remembered interface settings ✅. ⏳ Quick palette at the cursor, keymap editor, custom shelves (subsets/favorites), remembering brush settings | 3–4 wk |
| **Accessibility audit** | Review with disabled artists; fix findings (RFC-0001 §5) | 2 wk + fixes |

## Phase 5 — Hardening → 1.0 ⏳

All RFC-0001 performance gates met on reference hardware; file-format freeze (`.oxb` v1 compatibility promise); no open P0/P1 bugs; user guide; signed Windows installer; notarized macOS app; Flathub submission (from-source Flatpak manifest).

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

## Known limitations (v0.1.0-alpha)

- macOS is new: untested on real Macs, not notarized (first launch needs "Open Anyway"), and double-clicking `.oxb` files doesn't open them yet
- Pen pressure unverified on real tablets (all platforms)
- Decimation is slow on very dense meshes (runs in the background)
- Remesh resets mask and face groups; open surfaces remesh to a thin shell; resolution capped at 320
- Subdivision levels: switching to a ~5M-triangle level pauses the UI for a few seconds; remesh, decimate and single subdivide remove the levels (undo restores them)
- Only interface settings are remembered between sessions (not brush settings)
- Windows installer is not code-signed (SmartScreen warning)

---

### Updating this file

Update the status table, phase checkmarks and "Last updated" whenever a feature lands, the plan changes, or a release ships. Large plan changes go through an RFC (see CONTRIBUTING.md).

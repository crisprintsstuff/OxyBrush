# OxyBrush roadmap and status

> Living document: the current plan and what is done. The original design
> and reasoning are in [RFC-0001](rfcs/0001-oxybrush.md); this file
> supersedes its schedule (§6) and 1.0 scope (§2–3) where they differ.
> Effort is part-time weeks for one experienced contributor and is an
> estimate, not a promise.
>
> **Last updated:** 2026-11-07 (since v2.5.7-beta: Curve stroke mode (RFC 0004); Curve Move brush (RFC 0005); per-piece symmetry centre (`SYMC` section) so off-centre models mirror; OxyCharacter round trip — open `.oxb` from the command line, "Linked to OxyCharacter" badge, Send back, sidecar kept on Save As, topology hint, `VMAP` section. v2.5.7-beta: mask extract with live preview and Accept, floor grid that follows the model, Pen & tablet pressure settings, macOS Quit asks to save. v2.5.6-beta: Midnight theme and Pill corner style, docs refresh. v2.5.5-beta: insert preview, visible cut line and symmetric cuts, gizmo pivot fix, Windows Ink pen side button, generic names for branded options, Matte theme contrast, menus that fit on screen)

## Status at a glance

| | |
|---|---|
| **Latest release** | v2.5.7-beta (2026-11-07): Windows installer + portable zip, Linux Flatpak + tarball, macOS universal DMG |
| **Done** | Phases 0, 1 and 2 (first pass), release packaging, Phase 3 items 3.1–3.4, subdivision levels (3.5, first part), touch layout (3.7, first part), performance passes (3.6: stroke coalescing, dab budgeting, Rayon GPU & geometry acceleration), interface pass, Phase 4 items (Python plugin system, local MCP agent, keymap editor & presets, vertex color, transform gizmo & posing, Kelvinlet brushes, mask extract, themes & appearance, wireframe overlay, viewport background, focal length, autosave control), pipeline basics (glTF/GLB, PLY, LOD chains, CLI), OxyCharacter round trip (linked documents, Send back, `VMAP` exact vertex mapping), and native ZBrush (.ztl, .zpr) multi-subtool import |
| **Next** | Phase 3 leftovers — rebuild lower levels from a dense mesh (3.5), stencils (3.3), SIMD kernels and culling for 10M triangles (3.6), iPad (3.8–3.10). Phase 4 — dynamic topology, sculpt layers, symmetry repair, vertex color in exports |
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

## Phase 3 — Sculpting depth and iPad 🚧 (3.1–3.4 shipped in v0.1.0-alpha; subdivision levels and touch layout in v0.2.0-alpha; performance first pass in v0.3.0-alpha)

The fundamentals that most separate OxyBrush from mature sculpting tools, plus the groundwork for an iPad version. Ordered so each step ships something visible and multiresolution lands before the features that depend on it.

| # | Feature | Details | Status |
|---|---|---|---|
| 3.1 | **Stroke control** | Stabilizer (lazy mouse), Stamp (drag to size), spray, Line, pressure curve; editable Curve strokes ([RFC 0004](rfcs/0004-curve-stroke.md), after 2.5.7) | ✅ 0.1.0 |
| 3.2 | **Masking and selection** | Box/lasso hide and mask; mask by cavity, peaks, face group; blur/sharpen/grow/shrink mask; grow/shrink visible area | ✅ 0.1.0 |
| 3.3 | **Alphas and stencils** | Alphas ✅ (7 procedural CC0 + user PNGs, rotation, follow stroke). Stencils (screen-space overlays) ⏳ | 🚧 |
| 3.4 | **More brushes** | Polish, Planar Trim, Topological Move, Noise ✅. A dedicated Dam-style crease ⏳ (Crease covers most uses) | ✅ 0.1.0 |
| 3.5 | **Multiresolution** | Subdivision levels (Loop), sculpt at any level with detail propagated upward, saved in `.oxb` (MRES section), undoable ✅. Rebuild lower levels from an imported dense mesh ⏳; tangent-space detail (so detail rotates with large low-level bends) ⏳. Level changes above 1M triangles run in the background (5.2M: ~0.7 s to switch, ~2–4 s to add) | 🚧 |
| 3.6 | **Performance to 10M triangles** | First pass ✅: parallel mesh build (3–4× faster level switches), parallel subdivision, grow-only chunk bounds per dab, level changes off the UI thread, ~2× faster decimation (1.3M→130k: ~6 s). ⏳ SIMD brush kernels, chunk frustum culling, faster remesh at high resolution, measurements on real hardware at 10M | 🚧 |
| 3.7 | **Touch and tablet groundwork** (desktop first) | Gesture input layer (two-finger orbit, pinch zoom, two-finger tap undo) ✅; on-screen Smooth/Mask/Invert buttons replacing modifier keys ✅; touch-friendly layout with larger targets ✅; tooltips on a long-press, so they are not hover-only ✅; save when the app is suspended or focus lost ✅; vertical quick menu at cursor ✅. | ✅ 1.6.2 |
| 3.8 | **iPad build** | iPadOS target (wgpu/Metal, winit), small platform crate using `objc2` for iOS APIs, document picker and Files-app integration, generated Xcode project, CI job producing an `.ipa`. (1–2 wk) | ⏳ |
| 3.9 | **iPad and Apple Pencil polish** | Palm rejection (Pencil vs. finger), pressure tuning, simplified panel layout, testing on real iPads. (4–8 wk) | ⏳ |
| 3.10 | **TestFlight beta** | Signing and upload from CI; internal testers, then a public beta link. **Blocked on maintainer:** Apple Developer Program membership and App Store Connect API key (planned for later). (~1 wk once unblocked) | ⏳ blocked |

**Exit criteria:** sculpt at 60 fps on a 10M-triangle multires mesh on reference hardware; OxyBrush runs on an iPad with Apple Pencil pressure (distributed via TestFlight once 3.10 is unblocked); every item has tests; README and user docs updated.

### iPad notes

- **iPad only.** No iPhone build is planned; the screen is too small for sculpting.
- **Order:** 3.7 needs no Apple account and benefits every platform, so it can start any time. 3.8–3.9 need a Mac or a macOS CI runner; 3.10 needs the paid Apple Developer Program.
- **Decision pending (before 3.8):** calling iOS APIs from Rust needs `unsafe`, which AGENTS.md currently forbids workspace-wide. The proposal is to allow it only in one small, reviewed platform crate.
- **Bonus of the Apple account:** the same membership allows notarizing the macOS app, removing the "Open Anyway" step for Mac users.

## Phase 4 — Workflow and experience 🚧 (items shipped from v0.4.0-alpha to v2.5.7-beta)

| Feature | Details | Effort |
|---|---|---|
| **Dynamic topology** | Brush adds/removes triangles locally, so you can sketch from a sphere without remeshing | 4–6 wk |
| **Vertex color painting** | ✅ Paint brush, fills (all / cavities / peaks) respecting the mask, saved in projects. ⏳ Color in exports, color-aware remesh/decimate, materials per region | 3–4 wk |
| **Sculpt layers** | Detail layers with strength sliders; morph target | 4–6 wk |
| **Symmetry repair and projection** | Symmetrize, mirror-and-weld, project detail between meshes | 2–3 wk |
| **Deformers and posing** | ✅ Mask-driven posing with the transform gizmo (pivot on the mask edge, soft falloff). ✅ Curve Move: bend and stretch a part along an editable curve ([RFC 0005](rfcs/0005-curve-move.md)). ⏳ Twist and taper along the curve, saved curves; bend, twist, taper, inflate deformers; topological masking from a click | 4–6 wk |
| **Transform gizmo** | ✅ Move, rotate and scale pieces and masked regions in the viewport. ⏳ Numeric input, snapping, local axes | 1–2 wk |
| **Brush presets and editor** | ✅ Presets (see below). ⏳ A brush editor and custom shelves of favorites | 2–3 wk |
| **Insert-mesh and primitive brushes** | ✅ Insert Primitives brush (v2.0.0-beta): sphere, cube, cylinder, cone, torus and plane, dragged out on the surface, with the existing geometry auto-masked and a new face group per primitive. ⏳ Stamping your own meshes (eyes, bolts, teeth) | — |
| **Hard-surface tools** | ✅ Screen-line cutting tools (v2.5.0-beta): Plane Clip (flatten to a plane), Line Split (new face group along a line), Cut Away (cut and cap watertight). ⏳ Edge-preserving polish | — |
| **More masking** | Mask by normal angle or topology; one mask per face group | 1–2 wk |
| **Viewport & lighting** | Screen-space ambient occlusion and cavity shading ✅, user-imported matcaps ✅, flat shading ✅, 45° view snapping ✅, gradient or flat background with colors ✅, focal length (15–200 mm, remembered) ✅, surface-aligned brush cursor (optional, off by default) ✅. ⏳ Dynamic studio lights (directional, point, spot), atmospheric distance/depth fog, shadows, wireframe overlay, floor grid, reference image planes, turntable/still image export, material per piece | 3–5 wk |
| **Path tracer renderer** | Progressive hardware-accelerated / compute path tracer for photorealistic render previews, realistic contact shadows and ambient lighting | 4–6 wk |
| **Python plugin system** | ✅ Pure-Python SDK and a Plugins menu that talk to the app over the local MCP server (v0.5.0-alpha), with example plugins. ⏳ Procedural brushes and import/export filters as plugins | — |
| **Brush presets** | ✅ Save, apply, share as codes and files; starter presets. ⏳ Custom shelves of favourite presets | — |
| **Interface** | ✅ First friendliness pass (welcome window, brush/alpha/material previews with thumbnail pickers, brush icons and tooltips, toolbar with Raise/Carve, view strip and axis gizmo, gradient background, collapsible sections, status-bar hints). Layouts (Standard, Compact, Touch, Workstation), arrangeable panels (shelf left/bottom/right, panel side, centered size/strength bar), drag-to-reorder brushes and remembered interface settings ✅. Command palette (Ctrl+K), type-to-pick brush finder (Space), recent files and a built-in searchable guide ✅. Quick menu at the cursor (right click / hold Q), middle-drag panel scrolling ✅. Keymap editor, mouse-navigation presets (OxyBrush and MMB orbit) and optional Ctrl-carve / Alt-mask gestures ✅. A tooltip on every control (what it does, plus the shortcut from one shared list), and the same text on a long-press in the Touch layout ✅. ⏳ Custom shelves (subsets/favorites), remembering brush settings | 3–4 wk |
| **Updates** | ✅ Opt-in update checks and verified downloads ([RFC 0002](rfcs/0002-updates.md)). ⏳ Release signing key (maintainer), smoother macOS install once notarized | — |
| **Local AI agents** | ✅ Opt-in MCP server on `127.0.0.1` (and `oxybrush-cli mcp` over stdio) so an agent on this machine can sculpt alongside you ([RFC 0003](rfcs/0003-mcp.md)). Off by default; edits are ordinary undo steps labeled `Agent:`. ⏳ More tools (gizmo, alphas as brushes, subdivision levels) as those settle | — |
| **Accessibility audit** | Review with disabled artists; fix findings (RFC-0001 §5) | 2 wk + fixes |

## Phase 5 — Hardening & release polish 🚧

- **Topology Diagnostics & Cleaning**: 2-manifold diagnostics in the UI (`Mesh > Topology Diagnostics...`), edge boundary inspection, degenerate triangle cleanup (`clean_degenerate_triangles`) with full undo/redo history.
- **Workstation layout**: optional dense docked layout (`UiLayout::Workstation`) with a two-column brush shelf and the main controls (Raise/Carve, size, strength) in the top bar; pairs with the Matte theme.
- **Robustness**: `.oxb` stays readable across versions, undo covers every edit, and the workspace forbids `unsafe`. ⏳ Measured 10M-triangle performance gate on reference hardware (3.6).

## Parallel tracks

| Track | Status | Next step |
|---|---|---|
| **3D-print tools** | ✅ | Wall-thickness check, hollowing with drain holes (sparse voxels), split into parts with alignment keys, real-world units and material weight estimation. |
| **Tablet input** | ✅ | Tablet pressure pipeline (CursorMoved/Touch/AxisMotion/TouchpadPressure), pressure response curves, multi-touch gestures (pinch zoom, orbit, two-finger tap undo). |
| **ZBrush file research** | 🔬 M0 | Tooling done. Blocked on owner-provided `ztl-dump`/`ztl-diff` output from real files. See [docs/formats/zbrush/ROADMAP.md](formats/zbrush/ROADMAP.md). |

## After 1.0 (1.x / 2.x)

- Automatic quad retopology (algorithms from the Instant Meshes / QuadriFlow research)
- Live (non-destructive) booleans
- Armature spheres for base meshes; curve/tube and insert-mesh brushes
- Polygon modeling (extrude, bevel, inset, edge loops) and creased subdivision preview
- UV unwrap; normal/displacement map baking
- ~~glTF export~~ ✅ (glTF/GLB and PLY export, LOD chain, split pieces, `oxybrush-cli`); FBX/USD export; send-to-Blender bridge
- WASM & Python plugin APIs and automation scripting
- Interactive path tracer viewport renderer & turntable rendering
- Multi-light studio setups, environment lighting & atmospheric depth fog
- Hair/fiber and mesh instancing
- Exploratory: GPU compute sculpting, OpenXR, opt-in AI plugins

## Not planned

- A 2.5D "pixel with depth" canvas: a legacy approach modern sculpting tools don't need.
- Bundled AI generation in the core (plugins only, opt-in).

## Known limitations (v2.5.7-beta)

- macOS is new: untested on real Macs, not notarized (first launch needs "Open Anyway"), and double-clicking `.oxb` files doesn't open them yet
- Decimation takes a few seconds on dense meshes (runs in the background)
- Remesh resets mask and face groups; open surfaces remesh to a thin shell; resolution capped at 320
- Subdivision levels: changes above 1M triangles run in the background (the piece can't be edited or saved until they finish); remesh, decimate and single subdivide remove the levels (undo restores them)
- Only interface settings are remembered between sessions (save brushes as presets to keep them)
- Vertex color isn't exported yet, and remesh, decimate and merge drop it
- Touch gestures (pinch, two-finger orbit and undo) and pen pressure are implemented but untested on real touchscreens and tablets
- Windows installer is not code-signed (SmartScreen warning)

---

### Updating this file

Update the status table, phase checkmarks and "Last updated" whenever a feature lands, the plan changes, or a release ships. Large plan changes go through an RFC (see CONTRIBUTING.md).

<p align="center">
  <img src="packaging/icons/oxybrush-128.png" alt="OxyBrush icon" width="96">
</p>

<h1 align="center">OxyBrush</h1>

<p align="center"><strong>Open-source digital sculpting. Sculpt 3D models like digital clay.</strong></p>

<p align="center">
  <a href="https://github.com/crisprintsstuff/OxyBrush/releases"><img alt="release" src="https://img.shields.io/github/v/release/crisprintsstuff/OxyBrush?include_prereleases&logo=github&label=release"></a>
  <a href="https://github.com/crisprintsstuff/OxyBrush/actions/workflows/ci.yml"><img alt="CI" src="https://img.shields.io/github/actions/workflow/status/crisprintsstuff/OxyBrush/ci.yml?branch=main&label=CI&logo=githubactions&logoColor=white"></a>
  <img alt="stage" src="https://img.shields.io/badge/stage-beta-blue">
  <a href="rust-toolchain.toml"><img alt="language" src="https://img.shields.io/badge/language-Rust%201.99-dea584?logo=rust"></a>
  <img alt="platform" src="https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-0078d4">
  <a href="#license"><img alt="license" src="https://img.shields.io/badge/license-MIT%2FApache--2.0-4c1"></a>
  <a href="https://github.com/crisprintsstuff/OxyBrush/releases"><img alt="downloads" src="https://img.shields.io/github/downloads/crisprintsstuff/OxyBrush/total?label=downloads"></a>
</p>

<p align="center">
  <a href="https://github.com/crisprintsstuff/OxyBrush/releases/latest"><b>Download</b></a> ·
  <a href="https://github.com/crisprintsstuff/OxyBrush/wiki"><b>Wiki & Docs</b></a> ·
  <a href="docs/TESTING_GUIDE.md">Testing guide</a> ·
  <a href="CHANGELOG.md">Changelog</a> ·
  <a href="docs/ROADMAP.md">Roadmap</a> ·
  <a href="https://github.com/crisprintsstuff/OxyBrush/issues/new?template=bug_report.yml">Report a bug</a>
</p>

![OxyBrush with two pieces: a sculpted sphere and a cube, with the brush shelf on the left, the toolbar on top and the settings panel on the right](docs/images/screenshot.png)

OxyBrush is for sculptors: concept artists, character artists, miniature and 3D-print makers, and students. The model fills the screen, tools show up when you call for them, and brushes respond immediately even on dense meshes.

> **Beta:** [v4.0.0-beta.1](https://github.com/crisprintsstuff/OxyBrush/releases/latest) runs on Windows, Linux and macOS. You can sculpt multi-piece scenes with subdivision levels, build shapes from a sphere with dynamic detail, bend parts with the Curve Move brush, draw strokes as editable curves, insert 3D primitives (Sphere, Cube, Cylinder, Cone, Torus, Plane) that grow on the surface as you drag, with auto-masking, draw tubes along a path and reshape them, even after adding them, retopologize a sculpt by drawing quads on it (with X mirroring), cut with screen lines (Plane Clip, Line Split, Cut Away) that follow symmetry, switch between navigation presets (including right-drag orbit) and a single-key tool shortcut set, preview matcaps in real time on hover, adjust smooth intensity on the fly by holding Shift, prepare models with dedicated 3D-print tools (dimensions, volume, resin/filament weight estimation, wall-thickness check with warning heatmap, sparse voxel hollowing with drain holes, and split with keyed pins/sockets), remesh with sparse voxel grids with pure quad topology and quad OBJ export, use graphics tablets and touchscreens with full pressure, the pen side button and multi-touch gestures (pinch zoom, orbit, two-finger tap undo), perform volumetric CSG Booleans, tune the Crease brush with the Pinch parameter, navigate via the sleek vertical quick menu, import native ZBrush (`.ztl`, `.zpr`) models, paint vertex color, pose with a transform gizmo, extract masks into solid shells, deform with Kelvinlet brushes, inspect with real-time wireframe overlays and a floor grid that follows the model, customize themes (including the navy Midnight theme with pill-shaped controls), decimate, run Python plugins and sandboxed in-app WebAssembly plugins, round-trip meshes with Blender through the OxyBrush Bridge add-on, and export for 3D printing and game engines. Expect rough edges, and keep copies of anything important. **Testers wanted** — see [Help test OxyBrush](#help-test-oxybrush).

## Download and install

Get the latest build from **[Releases](https://github.com/crisprintsstuff/OxyBrush/releases/latest)**.

| Platform | File | How |
|---|---|---|
| **macOS 11+** (Apple Silicon and Intel) | `…-macos-universal.dmg` | Open it and drag OxyBrush to Applications. The app isn't notarized yet, so the first launch is blocked: open **System Settings > Privacy & Security** and click **Open Anyway** (macOS 14 or older: right-click the app > **Open**). |
| **Windows 10/11** (64-bit) | `…-windows-x86_64-setup.exe` | Run it. Installs for your user without admin rights; adds a Start-menu entry, an uninstaller and `.oxb` file association. Windows warns about an unknown publisher (not code-signed yet): **More info → Run anyway**. |
| Windows, portable | `…-windows-x86_64-portable.zip` | Unzip and run `oxybrush.exe`. |
| **Linux** (Flatpak) | `…-linux-x86_64.flatpak` | `flatpak install --user OxyBrush-*.flatpak`, then launch from your app menu. Needs the Flathub remote for the runtime. |
| Linux, portable | `…-linux-x86_64.tar.gz` | Unpack and run `./oxybrush`. Needs a Vulkan or OpenGL driver and glibc ≥ 2.35. |

Each release has `SHA256SUMS.txt` for verifying downloads.

## Getting started

The first time OxyBrush starts, a welcome window helps you pick a starting shape (sphere, cube or plane), open a project or import a mesh, and choose a layout. **File > Close project** brings it back: it closes the project, asking to save unsaved work first. Hover over any brush or control for a short explanation and its shortcut. On a touch screen, press and hold a control to read the same text.

**Layouts** (Settings > Layout, or in the welcome window):

| Layout | For |
|---|---|
| **Standard** | Mouse and keyboard: brush list with names and icons |
| **Compact** | Small screens: tighter spacing and an icon-only brush grid |
| **Touch friendly** | Touch screens and pen tablets: large buttons, a brush tile grid, on-screen **Smooth / Mask / Invert** toggles instead of modifier keys, and undo/redo beside the viewport |
| **Workstation** | Dense layout: a two-column brush shelf and the main controls in the top bar (Preferences > Appearance has a matching **Workstation** quick preset) |

To fit more brushes in the shelf, turn off **Preferences > Appearance > Show brush names in the brush shelf**: the brushes become a grid of icons, with each name on hover. For more room for the model, **Narrow brush shelf (one column)** turns a side shelf into a single column: the current brush, alpha and material stacked, then one brush per row with its name underneath.

**Arrange it your way** (Settings > Arrange panels, or right-click the brush shelf or the size/strength bar):

- **Brush shelf:** left, bottom (a horizontal strip of brush previews) or right. **Drag brushes** to put the ones you use most first; number keys 1–9, 0 follow your order.
- **Settings panel and view strip:** right or left.
- **Size, strength and Raise/Carve:** floating at the center top or center bottom of the viewport (default: center top, where your eyes are), or in the toolbar.

![Brushes in a strip along the bottom, with size and strength centered at the top of the viewport](docs/images/arrange-bottom.png)

Your window size and position, layout, arrangement, brush order, interface scale, theme, material, autosave interval, whether the welcome window appears at startup, are remembered between sessions. The brush you were using, with its settings and each brush's strength, is remembered too if you turn on **Remember brush settings between sessions** in Preferences > General (off by default).

![The touch-friendly layout: brush tiles, on-screen Smooth / Mask / Invert, and larger controls](docs/images/layout-touch.png)

![The welcome window with starter shapes and the basic controls](docs/images/welcome.png)

1. **Orbit** by dragging on empty space, **zoom** with the wheel, **pan** with the right mouse button. **F** frames the model.
2. **Sculpt** by dragging on the model. Pick brushes with **1–9, 0** or the left shelf (each brush has an icon showing what it does to the surface); hold **Shift** to smooth, **Ctrl** to mask, **Alt** to invert; **[** / **]** resize the brush.
3. Need more detail? **Ctrl+D** adds a subdivision level; **D** / **Shift+D** step up and down between levels, keeping your detail. **Remesh** (right panel) rebuilds even topology.
4. **Ctrl+S** saves a project (`.oxb`); **Ctrl+E** exports glTF/GLB, OBJ, STL or PLY (for game engines, other 3D tools or 3D printing).
5. **Ctrl+K** finds any command by typing part of its name, and **Space** finds a brush the same way. **Ctrl+,** opens Preferences (General, Appearance, Plugins, Keymap, AI Agent, Updates). **F1** shows every shortcut, and **F2** opens the built-in guide (searchable, works offline).

**Sculpting characters from OxyCharacter:** OxyCharacter opens its character in OxyBrush as a project (`oxybrush character.oxb`) with a `character.oxb.oxylink.json` file next to it. OxyBrush shows **Linked to OxyCharacter** in the top bar; sculpt, add pieces, then click it (or **File > Send back to OxyCharacter**, or Ctrl+K) to save to the file OxyCharacter is watching, even after a Save As elsewhere. Sculpted changes to `OxyCharacter Body` come back as a morph; new pieces come back as accessories. To keep the body a morph, use subdivision levels (Ctrl+D) rather than remesh, subdivide once, decimate, Boolean, split or delete hidden: OxyBrush says so once if you change its topology. Save As copies the `.oxylink.json` next to the new file; OxyBrush never reads, moves or deletes it.

**Working with an AI agent** (off unless you turn it on): Edit > Preferences > AI Agent lets a program on this computer sculpt in the same scene as you. It listens only on this machine, shows **Agent connected** while one is here, and its edits undo like yours. Setup for common agents is in [docs/mcp.md](docs/mcp.md).

**Retopology:** **Mesh > Retopology (draw quads)…** lets you draw a clean, low-polygon quad mesh over a sculpt: click to place points on the surface, Shift+click between them to make quads, Tab+drag a border edge to extend, Ctrl+click an edge to add a loop, Shift+drag to relax (Alt+click pins points so relax leaves them). The result is a new piece that exports as quads; **Subdivide and project** gives it subdivision levels that carry the sculpt's detail ([RFC 0012](docs/rfcs/0012-retopology.md)).

**Plugins menu:** every installed plugin's commands are one click away in the **Plugins** menu. OxyBrush comes with in-app plugins that need nothing installed: **Displace** (inflate, deflate, roughen), **Mask by Height**, **Color Ramp** (color by height or direction), **Array** (copies in a row), **Measure** (size, volume and bounding box), **Group by Height** (face groups in bands), **Flip Faces** and **Squash**. Some show their result live before you **Accept** it, long ones can be stopped, and their settings are remembered. **Plugins > Install add-on…** adds an `.oxyaddon`; **Manage plugins…** lists them, with Remove for ones you installed. To write your own (in Rust, WebAssembly text or any language that compiles to WebAssembly), see the [plugin guide](docs/plugins/README.md) and [API reference](docs/plugins/api-reference.md).

**Python plugins & scripting:** Write Python scripts or install custom plugins in your local plugins folder using the zero-dependency pure-Python SDK (`python/oxybrush`). Manage plugins in **Edit > Preferences > Plugins** (`Ctrl+,`) or run `.py` scripts via file dialog and reload plugins on the fly. Example plugins are in `examples/plugins/`.

## Help test OxyBrush

Testers are the most useful contribution right now — especially with **pen tablets**, **varied GPUs** and **3D printing workflows**.

1. Follow the [alpha testing guide](docs/TESTING_GUIDE.md) (about 20 minutes).
2. Report problems with **Help > Report a bug…** in the app — it opens a GitHub issue form with your version, OS and GPU already filled in. Attach the log from **Help > Open log folder**.
3. Share impressions with **Help > Send feedback…**: what felt good, what was frustrating, what you missed.

## Features

- **19 sculpting brushes, plus 7 tools in the same brush shelf:**
  - **Shape:** Draw, Clay, Clay Strips, Smooth, Inflate, Flatten, Pinch, Crease, Layer, Polish, Planar Trim, Noise.
  - **Move:** Grab, Snake Hook, Topological Move, Curve Move, and the volume-preserving Kelvinlet brushes (Kelvin Grab, Kelvin Twist, Kelvin Scale).
  - **Tools:** Mask, Paint (vertex color), Insert Primitives, Tube, and the screen-line cutting tools (Plane Clip, Line Split, Cut Away).

  The shelf scrolls, and number keys 1–0 pick the first ten (drag to reorder). Each sculpting brush has falloff curves, spacing, auto-smooth and pen-pressure options.
- **Curve Move:** draw a curve along an arm or a tail, then drag its points; the part bends and stretches to follow, like a simple rig. Masks protect what shouldn't move, and symmetry moves both sides. Preview first, then Accept as one undo step. Design in [RFC 0005](docs/rfcs/0005-curve-move.md).
- **Regularized Kelvinlet brushes:** volume-preserving elastodynamic Grab, Twist, and Scale brushes based on *de Goes & James (SIGGRAPH 2017)* for organic pulling, non-pinching twisting, and radial expansion.
- **Floor grid (View > Shading):** a fading grid on the floor under the model. It stays at the bottom of the visible pieces and its spacing follows the model's size, so it moves and rescales as you sculpt.
- **Real-time wireframe overlay:** hardware-accelerated wireframe rendering (`Shift+W`) over shaded matcaps with depth bias to eliminate Z-fighting, and user-selectable wireframe colors that persist across sessions.
- **Mask extract for sculpting:** Extract shows a live preview of the shell; adjust its thickness and edge smoothing (Laplacian boundary smoothing) and click **Accept** to add it as a new piece, or **Cancel**. It makes to create watertight 2-manifold shells or flat surface patches with front, back, and rim face groups.
- **Customizable theme & appearance:** 9 theme presets (`Graphite`, `Studio` — flat neutral greys, bar sliders with a solid handle, slightly squarer corners, uppercase section headings and outlined selection —, `Matte`, `Midnight`, `Charcoal`, `Slate`, `Warm Umber`, `Studio Light`, `High Contrast`), 7 accent color presets (`Terracotta`, `Tangerine`, `Cyan`, `Emerald`, `Amber`, `Crimson`, `Violet`) plus a custom RGB picker, and corner styles (`Sharp`, `Rounded`, `Soft`, `Pill`). **Midnight** is a deep navy "glass" look with thin blue outlines; the **Midnight** quick preset pairs it with pill-shaped controls, the emerald accent and a navy viewport.
- **Stroke control:** stabilizer for smooth lines, Freehand / Line / Stamp / **Curve** stroke modes, spray, pressure curve. A Curve stroke stays editable after you draw it: drag its points to reshape it, click to add a point, Alt+click to remove one, change size, strength and the fades, and see the result live before you Accept ([RFC 0004](docs/rfcs/0004-curve-stroke.md)). In Freehand mode, hold still for a moment mid-stroke to turn that stroke into a curve (optionally after asking). Each point can have its own size and strength, curves can be closed into loops, and Ctrl+Z undoes curve edits while it is open.
- **Brush presets:** save any brush (kind, size, strength, falloff, stroke, alpha) as a preset; five starter presets included. Share a brush as a one-line code (`oxyb1:…`) you can paste into a chat, or as an `.oxbrush` file. Everything stays on your computer.
- **Alphas:** grayscale images shape each dab; seven built-in procedural alphas plus your own grayscale PNGs, with rotation and follow-stroke. **Capture from view** turns what the viewport shows into an alpha at up to 4096 px; captured and loaded alphas are saved with the project, and **Export alpha…** saves any alpha as a 16-bit PNG to edit in a 2D app.
- **Masking and selection:** box or lasso mask/unmask (following symmetry) and show/hide, mask by cavity, peaks or face group, blur/sharpen/grow/shrink the mask, grow/shrink the visible area.
- **Vertex color:** a Paint brush (Alt erases; works with alphas and symmetry), and fills for the whole piece, its cavities or its peaks that respect the mask. Saved in projects and carried through subdivision levels, remesh, merge, booleans and decimate (as are the mask and face groups). Exported in glTF/GLB, PLY and OBJ.
- **Transform gizmo (T):** move, rotate or scale a piece in the viewport. With a mask, only the unmasked part moves, and a soft mask edge bends smoothly, which makes it a quick posing tool (mask the body, rotate an arm at the shoulder). Rotate shows a ring per axis plus an outer ring for the view direction. Squares between the axes move or scale along two axes; **Shift** snaps to steps; the gizmo bar takes typed values, has pivot choices (bounding box, ground, origin, align to view) and operators: mirror, put on the ground, centre at origin.
- **Symmetry:** X/Y/Z mirroring with $C^2$ cubic directional weighting ($\omega_i = w_i^3$) preventing midline creasing and circular boundary tearing, plus radial symmetry (up to 64 copies around any axis). Each piece mirrors about its own centre, picked from its shape when it's imported or opened (models built around the origin keep the origin), with **Recentre** and **World origin** buttons to change it.
- **Multiple pieces per document:** add, duplicate, mirror-duplicate, delete, rename, show/hide, solo. Alt+click a piece to make it active.
- **Dynamic detail:** turn it on in the Topology section and sculpting brushes add triangles where they need more detail (and remove ones that have become too small), so you can build a shape from a plain sphere without remeshing. New triangles stay under the brush and come out even (edge flips and relaxation along the surface). The Detail slider and the brush size set how fine it gets; symmetry and masks are respected, and each stroke is one undo step. Works best up to a few hundred thousand triangles. Design in [RFC 0006](docs/rfcs/0006-dynamic-detail.md).
- **Viewport lights and shadows:** switch the viewport from the matcap to up to four lights (sun, point or spot) in **View > Shading > Light the viewport**, and set them up in **Lights…**: color, intensity, direction, spot cone, lights that follow the view, sky/ground and rim light, and soft shadows from suns and spots. Drag the numbered markers in the viewport to move the lights. Design in [RFC 0008](docs/rfcs/0008-viewport-lights.md).
- **Subdivision levels (multiresolution):** add levels, sculpt broad shapes on a low level and fine detail on a high one; edits on a low level carry the higher-level detail along. Levels are saved in the project and covered by undo. Configurable ceiling for high-density meshes (`max_triangles`).
- **Sparse voxel remesher:** a pure-Rust hierarchical sparse grid (VDB-style, from the published research; $8^3 = 512$ voxel leaf nodes with 512-bit masks). Remeshes past 1024 or 2048 voxels with low memory usage. Includes an **Adaptivity slider** to decimate flat planar regions while retaining high polygon density in curved sculpt areas. Toggle between `Sparse voxels (VDB)` and the legacy `Regular Grid` engine.
- **Volumetric CSG Booleans (`Mesh > Boolean`):** true Constructive Solid Geometry Boolean tools operating directly on VDB signed distance fields: **Difference** (subtract cutter), **Union** (weld into single manifold mesh), and **Intersection** (common volume). Immune to non-manifold edges, self-intersections, and coplanar face errors.
- **3D-print preparation tools (`Mesh > 3D Print Tools...`):** real-world millimeter scale and bounding dimensions ($W \times D \times H$), surface area ($cm^2$), divergence theorem watertight volume ($cm^3$), topological manifold validation, and photopolymer resin (~1.15 g/cm³) and PLA filament (~1.24 g/cm³) weight estimates. **Wall-thickness analyzer** casts inward rays along negative normals to detect fragile areas and paints a red/orange warning heatmap onto the mesh. **Volumetric hollowing** erodes the interior with sparse voxel and punches cylindrical drain holes for resin/powder drainage. **Planar split with alignment keys** cuts pieces along X/Y/Z planes into interlocking parts with male registration pins and matching clearance sockets (0.2mm tolerance) for post-print assembly.
- **Tablet input & multi-touch gestures:** continuous pen pressure pipeline for graphics tablets (Wacom, Huion, XP-Pen, Apple Pencil, Windows Ink, Linux Wayland/XInput2), an adjustable pressure curve (**Preferences > Pen & tablet**: a two-point curve you can drag into an S shape, curve shape, lightest-touch strength, the pressure that counts as full, Soft/Linear/Firm presets and a live readout of your pen), and multi-touch gesture engine: two-finger pinch-to-zoom, two-finger orbit/pan, and **two-finger tap for Undo**. Native trackpad pinch-zoom and rotation gestures.
- **Vertical quick menu at cursor:** right-click or hold `Q` on the viewport opens a sleek, compact vertical card (175px width) for Raise/Carve, brush Size, Strength, and brush-specific controls without obscuring your model.
- **Crease brush Pinch control:** adjustable Pinch slider in the brush settings and vertical quick menu controlling how tightly groove walls are gathered toward the crease line (0.0 = pure carving groove, 0.3 = default, 1.0 = sharp tight pinch).
- **Software hardening & autosave:** automatic non-blocking autosave on window focus loss and app suspension, robust corrupt file import guards rejecting out-of-bounds indices and non-finite coordinates, subdivision memory ceiling guards, and topological manifold diagnostics (`check_manifold`).
- **Level set filters & shelling:** surface dilation and erosion offset, hollow wall shelling for 3D printing, and 3D Laplacian smoothing directly on the narrow-band SDF.
- **Decimation:** quadric edge collapse that keeps open borders, face-group borders and vertex color. Large meshes are decimated in parallel (1.3M → 131k triangles in about 0.7 s on 4 cores).
- **Face groups and hiding:** group visible, group from mask, group by islands, Ctrl+Shift+click to isolate a group, invert visibility, delete hidden, and a group color overlay.
- **Projects:** `.oxb` project files (versioned, checksummed, compressed, saved atomically; open one by double-clicking it or dropping it on the window), a prompt before discarding unsaved changes, **autosave every 2 minutes** (or set to 0/"Off" in Settings), and **crash recovery** on the next launch.
- **Python plugins & scripting:** a pure-Python client SDK (`python/oxybrush`) with zero external dependencies. Connects over loopback Model Context Protocol (MCP JSON-RPC 2.0) to inspect scenes, configure brushes, execute 3D strokes, manage topology (remesh, decimate, subdivide), and import/export assets. Manage plugins in **Edit > Preferences > Plugins** (`Ctrl+,`) with dynamic discovery, reload, and non-blocking background execution. Includes procedural sample plugins (`examples/plugins/`).
- **Dense mesh performance optimizations:** stroke coalescing and adaptive dab budgeting (`OXYBRUSH_DAB_BUDGET_MS`, default 10 ms) eliminate stroke queuing during rapid gestures; parallel Rayon geometry passes, atomic stamps, 1024-triangle chunk granularity, parallel GPU vertex packing, and zero-copy GPU index buffer binding.
- **Background jobs:** remesh, booleans, merge and decimate run on a worker thread, so the UI stays responsive.
- **Chunked mesh engine:** a stroke only re-computes, re-uploads and snapshots the regions it touches.
- **Undo/redo:** covers strokes, topology changes, face groups and visibility, and adding, removing or merging pieces, within a memory budget.
- **Matcap viewport (wgpu):** five procedurally generated materials (no third-party assets), with **ambient occlusion and cavity shading** that darken folds, creases and pores so fine detail reads clearly (toggle in the view strip, strengths in View > Shading). Load your own matcaps (PNG or JPEG) from the material picker, and switch to **flat shading** in the View menu to see every polygon. **View > Background** switches the soft gradient and one flat color; Colors… sets the center and edge, or the single flat color, and Reset to default restores the original gradient. The choice is remembered. **View > Align cursor to surface** (off by default and remembered) draws the brush ring on the clay under the pointer; off the model it stays flat to the screen.
- **Focal length:** the perspective lens is a 35mm-equivalent focal length from 15 to 200 mm (View > Focal length, the lens button in the view strip, or the command palette), with a slider, a numeric field, presets and the field of view in degrees. The model stays the same size; orthographic view is unchanged. Remembered between sessions.
- **Import/export:** Native ZBrush (`.ztl`, `.zpr`) subtool import with geometry decompression, quad/triangle polygon reconstruction, polygroup extraction, and world transformation matrices. OBJ and STL import (each object or SubTool becomes its own piece; OBJ materials are ignored, so a broken `.mtl` or material line doesn't stop an import). Mesh files carry no units, so an import more than 8× larger or smaller than the scene (for example one in millimetres) is scaled by a power of ten to fit, and later imports into the same project use the same factor; one Undo keeps the original size. Export of all visible pieces to glTF 2.0 (`.glb`/`.gltf`, one object per piece), OBJ (one object per piece), binary STL and binary PLY.
- **Pipeline helpers:** make an LOD chain (1/2, 1/4, 1/8 of the triangles), split a piece by face groups or into loose parts, and the `oxybrush-cli` command-line tool for batch conversion, remeshing, decimation, LODs and splitting (`cargo run -p oxy-cli -- --help`).
- **Friendly interface & menu bar:** streamlined into 8 clean top categories: `File`, `Edit`, `Pieces`, `Mesh`, `Groups`, `Mask`, `View`, `Help`. A welcome window with starter shapes and the basics; large brush, alpha and material previews that open thumbnail pickers; brush icons and a short tooltip on every control (what it does, and its shortcut); a toolbar with Raise/Carve, brush size and strength, symmetry and subdivision levels; a view strip beside the viewport (drag to orbit, pan or zoom, focal length, and toggles for the mask, face groups, shading, wireframe and floor grid; handy with a pen) and a clickable axis gizmo; collapsible panel sections; and a status bar that explains what the held keys do. On a touch screen, press and hold a control to read its tooltip.
  - **Fast workflow:** a command palette (Ctrl+K) that finds any command, brush, recent file or guide topic as you type; a type-to-pick brush finder (Space); recent files in File > Open recent and on the welcome window; and a built-in, searchable guide (F2) that works offline.
- **Unified Preferences window (Ctrl+,):** a multi-tab dialog organizing General, Appearance (theme, accent color, background, wireframe), Plugins, Keymap, AI Agent, and Updates into one place. Rebind every command, brush and view key (conflict warnings, reset one or all) and the mouse. Navigation presets: **OxyBrush** (the controls in the table below), **MMB orbit** (middle-drag orbits, Ctrl+middle zooms, Shift+middle pans, 45° snap on Alt until you change it) and **Right-drag orbit** (right-drag orbits, Alt+right pans, Ctrl+right zooms; on a pen, the side button). Shortcut presets: **Default** and **Single-key tools** (B brush palette, W/E/R move/scale/rotate, and similar single keys). An optional sculpt setup makes Ctrl carve and Alt mask. Keys are remembered and can be exported. The touch layout's on-screen Smooth, Mask and Invert buttons keep those jobs.
- **Accessibility basics:** UI scaling (75–300%), a high-contrast theme, a full mouse-only workflow, and screen-reader labels through AccessKit.
- **Optional updates:** off by default. Preferences > Updates can tell you about new versions or download them in the background; installing always waits for you, and downloads are checksum-verified. Help > Check for updates checks once on demand. Only GitHub is contacted and nothing about you or your work is sent ([details](docs/rfcs/0002-updates.md)). Flatpak installs update through Flatpak.
- **Rust:** a standard Rust toolchain is all you need on Windows, Linux or macOS (the optional updater's TLS library compiles a little C with the compiler Rust already uses).

## Controls

| Input | Action |
|---|---|
| LMB on model | Sculpt the active piece |
| Alt+LMB on another piece | Make it the active piece |
| Ctrl+Shift+LMB | Isolate the face group under the cursor (on empty space: show all) |
| LMB on empty space | Orbit |
| Alt+LMB on empty space / RMB | Pan |
| Wheel | Zoom |
| Shift / Ctrl / Alt (hold) | Smooth / Mask / Invert |
| Ctrl+drag on empty space | Mask a rectangle or lasso, mirrored when symmetry is on (Ctrl+Alt: unmask; a box around nothing clears the mask; Ctrl+click: invert the mask) |
| Ctrl+Shift+drag | Show only the selection (Ctrl+Shift+Alt: hide it) |
| `L` | Rectangle / lasso selection |
| Ctrl+M | Mask the face group under the cursor |
| `1`–`9`, `0` | Select the first ten brushes in the shelf (drag brushes to rearrange) |
| `[` `]` and `-` `=` | Radius and strength |
| `S` / `U` / `I` / Shift+`O` | Slider under the pointer for size / strength / paint strength / focal shift (Single-key tools scheme: `O` for focal shift, `P` for orthographic) |
| `X` | Toggle X symmetry |
| `F` / Alt+`F` | Frame the active piece / all pieces |
| Ctrl+D | Add subdivision level |
| `D` / Shift+D | Subdivision level up / down |
| Ctrl+R | Voxel remesh |
| `G` | Face group colors |
| Shift+drag on empty space | Orbit in 45° steps |
| `T` | Transform gizmo: move → rotate → scale → off |
| `Shift`+drag a gizmo handle | Snap to steps (0.1, 15°, 10 %) |
| Ctrl+S / Ctrl+O | Save / open project |
| Ctrl+Z, Ctrl+Shift+Z | Undo, redo |
| Right click / hold `Q` | Quick menu at the cursor (size, strength, Raise/Carve) |
| Alt with Grab / Snake Hook | Move along the surface normal |
| Middle-drag on a panel | Scroll the panel |
| Ctrl+K | Command palette |
| Space | Find a brush by name |
| `Ctrl+,` | Preferences window (General, Appearance, Plugins, Keymap, AI Agent, Updates) |
| `F1` / `F2` | Full shortcut list / built-in guide |

These are the OxyBrush preset. **Settings > Keymap** can change any of them, including the numpad view keys, and can switch the mouse to the MMB orbit or Right-drag orbit preset. On macOS, **Cmd** works wherever Ctrl is listed and **Option** is Alt.

## Roadmap

Full plan and status: **[docs/ROADMAP.md](docs/ROADMAP.md)**.

| Phase | Goal | State |
|---|---|---|
| 0 | Foundations: workspace, CI, renderer, mesh core | ✅ |
| 1 | Sculpt core: brushes, symmetry, undo, pressure | ✅ (tablet pressure still being verified) |
| 2 | Pieces, voxel remesh, decimation, face groups, `.oxb` projects, autosave | ✅ first pass |
| — | Release packaging: Windows installer, Flatpak, macOS universal app | ✅ v0.0.1-alpha – v4.0.0-beta.1 |
| 3 | **Sculpting depth and iPad:** stabilizer & stroke types ✅, masking/selection ✅, alphas ✅, more brushes & Kelvinlets ✅, subdivision levels ✅, touch layout ✅, performance first pass ✅ · rebuilding lower levels, 10M-triangle performance, touch gestures, iPad app with Apple Pencil (TestFlight) ⏳ | 🚧 |
| 4 | Workflow: dynamic topology (first version ✅), vertex color ✅, layers, symmetrize/projection, deformers & posing, viewport, wireframe & lights ✅, themes ✅, quick palette ✅, keymap editor ✅, Python plugins ✅, local AI agent (MCP) ✅ | 🚧 |
| 5 | Hardening → **1.0** | ⏳ |
| Parallel | 3D-print tools · tablet input · ZBrush file research ([rules](docs/formats/zbrush/README.md)) | ✅ · 🚧 · ✅ Native .ztl/.zpr import live |
| After 1.0 | Auto-retopology, live booleans, armature spheres, poly modeling, UVs & baking, plugin API | long-term |

## For developers

### Building from source

You need Rust 1.88 or newer ([rustup](https://rustup.rs)).

```bash
git clone https://github.com/crisprintsstuff/OxyBrush
```

```bash
cd OxyBrush
```

```bash
cargo run --release
```

Open a project or mesh on launch:

```bash
cargo run --release -- path/to/model.oxb
```

**Windows:** install Rust with the MSVC toolchain (the rustup default). The Visual Studio Build Tools "Desktop development with C++" workload provides the linker; no other dependencies are needed.

**Linux packages:** X11 or Wayland development libraries and a Vulkan driver. On Debian or Ubuntu:

```bash
sudo apt install libxkbcommon-x11-0 libwayland-dev mesa-vulkan-drivers
```

**macOS and Windows:** nothing extra (Metal and DX12/Vulkan are used automatically).

### Architecture

```
crates/
  oxy-mesh     chunked triangle mesh, face groups/visibility, raycast, subdivision  (no GPU)
  oxy-topo     voxel remesh (with unions) and quadric decimation                  (no GPU)
  oxy-doc      scenes of pieces, .oxb project format, autosave/recovery           (no GPU)
  oxy-sculpt   stroke engine, brush kernels, symmetry, falloff                     (no GPU)
  oxy-history  scene-wide undo/redo (chunk snapshots, topology, pieces)           (no GPU)
  oxy-io       OBJ/STL and ZBrush import; glTF/GLB, OBJ, STL, PLY export          (no GPU)
  oxy-ztl      clean-room ZBrush (.ztl/.zpr) format parser & geometry loader      (no GPU)
  oxy-cli      headless command-line tool (binary: `oxybrush-cli`)                (no GPU)
  oxy-mcp      local MCP server for an on-machine AI agent                        (no GPU)
  oxy-render   wgpu matcap renderer, wireframe overlay, and orbit camera
  oxy-app      winit + egui desktop app (binary: `oxybrush`)
```

Every crate except `oxy-render` and `oxy-app` runs headless and is unit-tested. The full design is in [RFC-0001](docs/rfcs/0001-oxybrush.md).

### Python SDK & Plugins

The `python/` directory contains the zero-dependency pure-Python `oxybrush` client library:

```bash
cd python
pip install -e .
```

Or run scripts directly against OxyBrush without installing:

```bash
PYTHONPATH=python python3 examples/plugins/mesh_stats.py
```

### Performance status

Measured on a 4-core cloud VM (CPU only; numbers vary ±10–20% between runs there). Brushes (`cargo run --release -p oxy-sculpt --example dab_timing`, 1.3M-triangle sphere):

| Dab | Time |
|---|---|
| Small (~1k vertices) | ~0.6 ms |
| Large (~15k vertices) | ~2–2.4 ms |

Subdivision levels (`cargo run --release -p oxy-mesh --example multires_timing`):

| Operation | Time |
|---|---|
| Switch to a 5.2M-triangle level | ~0.7 s, in the background |
| Add a level up to 5.2M triangles | ~2–4 s, in the background |
| UI-thread work for either | ~60 ms |

Topology (`cargo run --release -p oxy-topo --example topo_timing`):

| Operation | Time |
|---|---|
| Voxel remesh, 330k triangles in, 154k out (resolution 128) | ~0.3 s |
| Voxel remesh, 330k triangles in, 2.5M out (resolution 512) | ~2.4 s |
| Decimate 1.3M → 130k triangles | ~0.7 s |

Viewport passes on your own GPU: `cargo run --release -p oxy-render --example shading_timing`.

Still to do for 1.0: dab cost at ~100k vertices (target ≤8 ms), SIMD brush kernels, chunk frustum culling, and measurement on the reference hardware at 10M triangles.

## Contributing

Everyone is welcome, whether you write code, make art, write docs, translate, or test. Read [CONTRIBUTING.md](CONTRIBUTING.md) first. In particular, every commit needs a DCO sign-off (`git commit -s`), and there is a **clean-room policy**.

## License

Code is dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option. Bundled assets are CC0.

*OxyBrush is an independent project, not affiliated with any commercial sculpting software vendor. ZBrush is a registered trademark of Maxon Computer GmbH; it is mentioned only to identify a file format OxyBrush aims to interoperate with.*

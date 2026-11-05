<p align="center">
  <img src="packaging/icons/oxybrush-128.png" alt="OxyBrush icon" width="96">
</p>

<h1 align="center">OxyBrush</h1>

<p align="center"><strong>Open-source digital sculpting. Sculpt 3D models like digital clay.</strong></p>

<p align="center">
  <a href="https://github.com/crisprintsstuff/OxyBrush/releases"><img alt="release" src="https://img.shields.io/github/v/release/crisprintsstuff/OxyBrush?include_prereleases&logo=github&label=release"></a>
  <a href="https://github.com/crisprintsstuff/OxyBrush/actions/workflows/ci.yml"><img alt="CI" src="https://img.shields.io/github/actions/workflow/status/crisprintsstuff/OxyBrush/ci.yml?branch=main&label=CI&logo=githubactions&logoColor=white"></a>
  <img alt="stage" src="https://img.shields.io/badge/stage-alpha-orange">
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

> **Alpha:** [v0.5.1-alpha](https://github.com/crisprintsstuff/OxyBrush/releases/latest) runs on Windows, Linux and macOS. You can sculpt multi-piece scenes with subdivision levels, paint vertex color, pose with a transform gizmo, remesh, decimate, run Python plugins, connect local AI agents, customize keymaps, save projects and export for game engines (glTF) or 3D printing, in a layout that suits mouse, small screens or touch. Expect rough edges, and keep copies of anything important. **Testers wanted** — see [Help test OxyBrush](#help-test-oxybrush).

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

The first time OxyBrush starts, a welcome window helps you pick a starting shape (sphere, cube or plane), open a project or import a mesh, and choose a layout. Hover over any brush or control for a short explanation and its shortcut. On a touch screen, press and hold a control to read the same text.

**Layouts** (Settings > Layout, or in the welcome window):

| Layout | For |
|---|---|
| **Standard** | Mouse and keyboard: brush list with names and icons |
| **Compact** | Small screens: tighter spacing and an icon-only brush grid |
| **Touch friendly** | Touch screens and pen tablets: large buttons, a brush tile grid, on-screen **Smooth / Mask / Invert** toggles instead of modifier keys, and undo/redo beside the viewport |

**Arrange it your way** (Settings > Arrange panels, or right-click the brush shelf or the size/strength bar):

- **Brush shelf:** left, bottom (a horizontal strip of brush previews) or right. **Drag brushes** to put the ones you use most first; number keys 1–9, 0 follow your order.
- **Settings panel and view strip:** right or left.
- **Size, strength and Raise/Carve:** floating at the center top or center bottom of the viewport (default: center top, where your eyes are), or in the toolbar.

![Brushes in a strip along the bottom, with size and strength centered at the top of the viewport](docs/images/arrange-bottom.png)

Your layout, arrangement, brush order, interface scale, theme, material, autosave interval and whether the welcome window appears at startup are remembered between sessions.

![The touch-friendly layout: brush tiles, on-screen Smooth / Mask / Invert, and larger controls](docs/images/layout-touch.png)

![The welcome window with starter shapes and the basic controls](docs/images/welcome.png)

1. **Orbit** by dragging on empty space, **zoom** with the wheel, **pan** with the right mouse button. **F** frames the model.
2. **Sculpt** by dragging on the model. Pick brushes with **1–9, 0** or the left shelf (each brush has an icon showing what it does to the surface); hold **Shift** to smooth, **Ctrl** to mask, **Alt** to invert; **[** / **]** resize the brush.
3. Need more detail? **Ctrl+D** adds a subdivision level; **D** / **Shift+D** step up and down between levels, keeping your detail. **Remesh** (right panel) rebuilds even topology.
4. **Ctrl+S** saves a project (`.oxb`); **Ctrl+E** exports glTF/GLB, OBJ, STL or PLY (for game engines, other 3D tools or 3D printing).
5. **Ctrl+K** finds any command by typing part of its name, and **Space** finds a brush the same way. **F1** shows every shortcut, and **F2** opens the built-in guide (searchable, works offline). **Settings > Keymap** changes any shortcut or the mouse buttons.

**Working with an AI agent** (off unless you turn it on): Settings > Local AI agent lets a program on this computer sculpt in the same scene as you. It listens only on this machine, shows **Agent connected** while one is here, and its edits undo like yours. Setup for common agents is in [docs/mcp.md](docs/mcp.md).

**Python plugins & scripting:** Write Python scripts or install custom plugins in your local plugins folder using the zero-dependency pure-Python SDK (`python/oxybrush`). The top-level **Plugins** menu lets you run any `.py` script via file dialog, launch installed plugins with one click, open your plugins directory, and reload plugins on the fly. Example plugins are in `examples/plugins/`.

## Help test OxyBrush

Testers are the most useful contribution right now — especially with **pen tablets**, **varied GPUs** and **3D printing workflows**.

1. Follow the [alpha testing guide](docs/TESTING_GUIDE.md) (about 20 minutes).
2. Report problems with **Help > Report a bug…** in the app — it opens a GitHub issue form with your version, OS and GPU already filled in. Attach the log from **Help > Open log folder**.
3. Share impressions with **Help > Send feedback…**: what felt good, what was frustrating, what you missed.

## Features

- **16 brushes:** Draw, Clay, Clay Strips, Smooth, Inflate, Flatten, Pinch, Crease, Layer, Grab, Snake Hook, Topological Move, Polish, Planar Trim, Noise, Mask. Each has falloff curves, spacing, auto-smooth and pen-pressure options.
- **Stroke control:** stabilizer for smooth lines, Freehand / Line / Stamp stroke modes, spray, pressure curve.
- **Brush presets:** save any brush (kind, size, strength, falloff, stroke, alpha) as a preset; five starter presets included. Share a brush as a one-line code (`oxyb1:…`) you can paste into a chat, or as an `.oxbrush` file. Everything stays on your computer.
- **Alphas:** grayscale images shape each dab; seven built-in procedural alphas plus your own grayscale PNGs, with rotation and follow-stroke.
- **Masking and selection:** box or lasso mask/unmask and show/hide, mask by cavity, peaks or face group, blur/sharpen/grow/shrink the mask, grow/shrink the visible area.
- **Vertex color:** a Paint brush (Alt erases; works with alphas and symmetry), and fills for the whole piece, its cavities or its peaks that respect the mask. Saved in projects and carried through subdivision levels.
- **Transform gizmo (T):** move, rotate or scale a piece in the viewport. With a mask, only the unmasked part moves, and a soft mask edge bends smoothly, which makes it a quick posing tool (mask the body, rotate an arm at the shoulder).
- **Symmetry:** X/Y/Z mirroring plus radial symmetry (up to 64 copies around any axis).
- **Multiple pieces per document:** add, duplicate, mirror-duplicate, delete, rename, show/hide, solo. Alt+click a piece to make it active.
- **Subdivision levels (multiresolution):** add levels, sculpt broad shapes on a low level and fine detail on a high one; edits on a low level carry the higher-level detail along. Levels are saved in the project and covered by undo.
- **Voxel remesh:** rebuilds clean, even topology. "Merge visible" remeshes several pieces into one (a union).
- **Decimation:** quadric edge collapse that keeps open borders and face-group borders.
- **Face groups and hiding:** group visible, group from mask, group by islands, Ctrl+Shift+click to isolate a group, invert visibility, delete hidden, and a group color overlay.
- **Projects:** `.oxb` project files (versioned, checksummed, compressed, saved atomically), a prompt before discarding unsaved changes, **autosave every 2 minutes** (or set to 0/"Off" in Settings), and **crash recovery** on the next launch.
- **Python plugins & scripting:** a pure-Python client SDK (`python/oxybrush`) with zero external dependencies. Connects over loopback Model Context Protocol (MCP JSON-RPC 2.0) to inspect scenes, configure brushes, execute 3D strokes, manage topology (remesh, decimate, subdivide), and import/export assets. In-app **Plugins** menu with dynamic discovery, reload, and non-blocking background execution. Includes procedural sample plugins (`examples/plugins/`).
- **Dense mesh performance optimizations:** stroke coalescing and adaptive dab budgeting (`OXYBRUSH_DAB_BUDGET_MS`, default 10 ms) eliminate stroke queuing during rapid gestures; parallel Rayon geometry passes, atomic stamps, 1024-triangle chunk granularity, parallel GPU vertex packing, and zero-copy GPU index buffer binding.
- **Background jobs:** remesh, merge and decimate run on a worker thread, so the UI stays responsive.
- **Chunked mesh engine:** a stroke only re-computes, re-uploads and snapshots the regions it touches.
- **Undo/redo:** covers strokes, topology changes, face groups and visibility, and adding, removing or merging pieces, within a memory budget.
- **Matcap viewport (wgpu):** five procedurally generated materials (no third-party assets), with **ambient occlusion and cavity shading** that darken folds, creases and pores so fine detail reads clearly (toggle in the view strip, strengths in View > Shading). Load your own matcaps (PNG or JPEG) from the material picker, and switch to **flat shading** in the View menu to see every polygon. **View > Background** (also under Settings) switches the soft gradient and one flat color; Colors… sets the center and edge, or the single flat color, and Reset to default restores the original gradient. The choice is remembered. **View > Align cursor to surface** (also in Settings, off by default and remembered) draws the brush ring on the clay under the pointer; off the model it stays flat to the screen.
- **Focal length:** the perspective lens is a 35mm-equivalent focal length from 15 to 200 mm (View > Focal length, the lens button in the view strip, or the command palette), with a slider, a numeric field, presets and the field of view in degrees. The model stays the same size; orthographic view is unchanged. Remembered between sessions.
- **Import/export:** OBJ and STL import (each object in an OBJ file becomes its own piece). Export of all visible pieces to glTF 2.0 (`.glb`/`.gltf`, one object per piece), OBJ (one object per piece), binary STL and binary PLY.
- **Pipeline helpers:** make an LOD chain (1/2, 1/4, 1/8 of the triangles), split a piece by face groups or into loose parts, and the `oxybrush-cli` command-line tool for batch conversion, remeshing, decimation, LODs and splitting (`cargo run -p oxy-cli -- --help`).
- **Friendly interface:** a welcome window with starter shapes and the basics; large brush, alpha and material previews that open thumbnail pickers; brush icons and a short tooltip on every control (what it does, and its shortcut); a toolbar with Raise/Carve, brush size and strength, symmetry and subdivision levels; a view strip beside the viewport (drag to orbit, pan or zoom, focal length, handy with a pen) and a clickable axis gizmo; collapsible panel sections; and a status bar that explains what the held keys do. On a touch screen, press and hold a control to read its tooltip.
  - **Fast workflow:** a command palette (Ctrl+K) that finds any command, brush, recent file or guide topic as you type; a type-to-pick brush finder (Space); recent files in File > Open recent and on the welcome window; and a built-in, searchable guide (F2) that works offline.
- **Keymap:** Settings > Keymap rebinds every command, brush and view key (conflict warnings, reset one or all) and the mouse. Presets: **OxyBrush** (the controls in the table below) and **MMB orbit** (middle-drag orbits, Ctrl+middle zooms, Shift+middle pans, 45° snap on Alt until you change it). An optional sculpt setup makes Ctrl carve and Alt mask. Keys are remembered and can be exported. The touch layout's on-screen Smooth, Mask and Invert buttons keep those jobs.
- **Accessibility basics:** UI scaling (75–300%), a high-contrast theme, a full mouse-only workflow, and screen-reader labels through AccessKit.
- **Optional updates:** off by default. Settings > Updates can tell you about new versions or download them in the background; installing always waits for you, and downloads are checksum-verified. Help > Check for updates checks once on demand. Only GitHub is contacted and nothing about you or your work is sent ([details](docs/rfcs/0002-updates.md)). Flatpak installs update through Flatpak.
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
| Ctrl+drag on empty space | Mask a rectangle or lasso (Ctrl+Alt: unmask; Ctrl+click: blur mask) |
| Ctrl+Shift+drag | Show only the selection (Ctrl+Shift+Alt: hide it) |
| `L` | Rectangle / lasso selection |
| Ctrl+M | Mask the face group under the cursor |
| `1`–`9`, `0` | Select the first ten brushes in the shelf (drag brushes to rearrange) |
| `[` `]` and `-` `=` | Radius and strength |
| `X` | Toggle X symmetry |
| `F` | Frame scene |
| Ctrl+D | Add subdivision level |
| `D` / Shift+D | Subdivision level up / down |
| Ctrl+R | Voxel remesh |
| `G` | Face group colors |
| Shift+drag on empty space | Orbit in 45° steps |
| `T` | Transform gizmo: move → rotate → scale → off |
| Ctrl+S / Ctrl+O | Save / open project |
| Ctrl+Z, Ctrl+Shift+Z | Undo, redo |
| Right click / hold `Q` | Quick menu at the cursor (size, strength, Raise/Carve) |
| Alt with Grab / Snake Hook | Move along the surface normal |
| Middle-drag on a panel | Scroll the panel |
| Ctrl+K | Command palette |
| Space | Find a brush by name |
| `F1` / `F2` | Full shortcut list / built-in guide |

These are the OxyBrush preset. **Settings > Keymap** can change any of them, including the numpad view keys, and can switch the mouse to the MMB orbit preset. On macOS, **Cmd** works wherever Ctrl is listed and **Option** is Alt.

## Roadmap

Full plan and status: **[docs/ROADMAP.md](docs/ROADMAP.md)**.

| Phase | Goal | State |
|---|---|---|
| 0 | Foundations: workspace, CI, renderer, mesh core | ✅ |
| 1 | Sculpt core: brushes, symmetry, undo, pressure | ✅ (tablet pressure still being verified) |
| 2 | Pieces, voxel remesh, decimation, face groups, `.oxb` projects, autosave | ✅ first pass |
| — | Release packaging: Windows installer, Flatpak, macOS universal app | ✅ v0.0.1 – v0.5.1-alpha |
| 3 | **Sculpting depth and iPad:** stabilizer & stroke types ✅, masking/selection ✅, alphas ✅, more brushes ✅, subdivision levels ✅, touch layout ✅, performance first pass ✅ · rebuilding lower levels, 10M-triangle performance, touch gestures, iPad app with Apple Pencil (TestFlight) ⏳ | 🚧 v0.3.0-alpha |
| 4 | Workflow: dynamic topology, vertex color ✅, layers, symmetrize/projection, deformers & posing, viewport, quick palette ✅, keymap editor ✅, Python plugins ✅, local AI agent (MCP) ✅ | 🚧 |
| 5 | Hardening → **1.0** | ⏳ |
| Parallel | 3D-print tools · tablet input · ZBrush file research ([rules](docs/formats/zbrush/README.md)) | ⏳ · 🚧 · 🔬 M0 |
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
  oxy-io       OBJ/STL import; glTF/GLB, OBJ, STL, PLY export                      (no GPU)
  oxy-cli      headless command-line tool (binary: `oxybrush-cli`)                (no GPU)
  oxy-mcp      local MCP server for an on-machine AI agent                      (no GPU)
  oxy-render   wgpu matcap renderer and orbit camera
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
| Voxel remesh, 330k triangles in, ~1M out (resolution 320) | ~4 s |
| Decimate 1.3M → 130k triangles | ~6 s |

Viewport passes on your own GPU: `cargo run --release -p oxy-render --example shading_timing`.

Still to do for 1.0: dab cost at ~100k vertices (target ≤8 ms), SIMD brush kernels, chunk frustum culling, faster high-resolution remesh, and measurement on the reference hardware at 10M triangles.

## Contributing

Everyone is welcome, whether you write code, make art, write docs, translate, or test. Read [CONTRIBUTING.md](CONTRIBUTING.md) first. In particular, every commit needs a DCO sign-off (`git commit -s`), and there is a **clean-room policy**.

## License

Code is dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option. Bundled assets are CC0.

*OxyBrush is an independent project, not affiliated with any commercial sculpting software vendor. ZBrush is a registered trademark of Maxon Computer GmbH; it is mentioned only to identify a file format OxyBrush aims to interoperate with.*

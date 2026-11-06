# Changelog

All notable changes to OxyBrush. The format is based on [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

## [1.5.1-beta] - 2026-11-05

### Fixed

- **Hover Tooltips Event Loop Waker (Windows & All Platforms):**
  - Resolved issue where hover tooltips would not render until an explicit key (such as `Ctrl`) or mouse action woke the winit event loop.
  - Correctly wired egui delayed repaints (`repaint_delay`) into winit's control flow (`ControlFlow::WaitUntil`) and requested redraws upon timer expiry, allowing tooltips to appear cleanly when hovering without needing additional input.
- **Interactive Color Swatches & Dedicated Dialogs (Wireframe & Accent Color):**
  - Resolved issue where clicking color swatches inside menu dropdowns would close the menu before opening color pickers.
  - Added dedicated floating **Theme & Appearance** window (`Settings > Theme & Appearance…` and Command Palette) and **Wireframe settings** window (`View > Wireframe settings…`, Command Palette, and right-click on the view strip wireframe icon).
  - Made all accent color preset swatches (`Terracotta`, `Cyan`, `Emerald`, `Amber`, `Crimson`, `Violet`, `Custom`) fully clickable with interactive response.
  - Added quick-pick color preset swatches for wireframe (`Dark Slate`, `Light Gray`, `Terracotta`, `Electric Cyan`, `Emerald Mint`, `Golden Amber`) alongside the custom RGB/HSV color picker and reset button.
  - Right-clicking the wireframe toggle button in the viewport view strip now opens the wireframe settings window.

## [1.5.0-beta] - 2026-11-05

### Added

- **Native ZBrush `.ztl` and `.zpr` File Import:**
  - Implemented full native decoder and parser in `oxy-ztl` supporting both ZBrush Tool (`.ztl`) and Project (`.zpr`) files without external dependencies.
  - Decompresses RLE-encoded subtool geometry streams and extracts vertex positions (both modern IEEE 754 floats and primitive $2^{20}$ fixed-point integer coordinates).
  - Extracts quad and triangle polygons, polygroups / face groups, and subtool names.
  - Automatically transforms SubTool local coordinates into world space using embedded inverse scale and translation matrices (`inv_scale`, `tx`, `ty`, `tz`), preserving outward normal orientation.
  - Fully integrated into `Editor::open`, `Editor::import`, `oxy_io::import`, and `oxy_io::import_pieces`, mapping each subtool to an independent piece in the scene hierarchy.
  - Updated file dialog filters in `oxy-app` with explicit "ZBrush tool or project (*.ztl, *.zpr)" options for Open and Import workflows.
- **Customizable Wireframe Color:**
  - Added user-configurable wireframe color support (`wireframe_color: [f64; 3]`) across rendering shaders (`wireframe.wgsl`, `shader.wgsl`), render pipeline uniform buffers, and application state.
  - Added interactive RGB/HSV color picker under `View > Wireframe` and in the Command Palette (`Ctrl+P`).
  - Wireframe color selection persists across sessions in `settings.txt`.

### Fixed

- **Window Focus Loss & Synthetic Input Handling (Issue #54):**
  - Handled `WindowEvent::Focused(focused)` to cleanly surrender keyboard focus, reset modifier keys, call `pointer_up()`, and clear transient pointer / quick-menu states when the application loses focus.
  - Filtered synthetic keyboard events (`is_synthetic: false`) in `WindowEvent::KeyboardInput` to prevent OS window-switching shortcut combinations (e.g., Alt+Tab, Super+Tab) from inadvertently triggering camera or projection hotkeys (such as perspective/orthographic toggle).
- **Hover Tooltip Responsiveness:**
  - Improved tooltip trigger reliability across the UI by disabling `show_tooltips_only_when_still` in `egui::Style` and adjusting delay to 0.2s with a 0.2s grace time, ensuring fast and consistent tooltip display across tools and buttons.

## [1.0.0-beta] - 2026-11-05

### Added

- **Mask Extract for Sculpting:**
  - Extract masked regions as a new piece with customizable extrusion thickness and boundary smoothing.
  - Generates watertight 2-manifold solid shells with side rim walls and distinct face groups (front: 0, back: 1, rim: 2) or flat single-sided surface patches (`thickness = 0`).
  - Laplacian boundary smoothing iterations to eliminate jagged stair-step edges along the mask rim.
  - Accessible via `Mask > Extract mask…`, the sidebar **Groups & mask** inspector panel with interactive controls, and Command Palette (`Mask: Extract`).
  - Preferences persist across sessions in `settings.txt`.
- **Customizable Theme System:**
  - Added 6 base theme presets: `Graphite` (default dark neutral), `Charcoal` (deep OLED dark), `Slate` (cool studio blue-gray), `Warm Umber` (sculpting clay tone), `Studio Light` (clean bright neutral), and `High Contrast`.
  - Added 7 accent color presets: `Terracotta` (signature OxyBrush clay), `Cyan` (electric blue), `Emerald` (calming mint), `Amber` (golden warm), `Crimson` (vivid ruby), `Violet` (studio purple), and `Custom` (full RGB color picker).
  - Added adjustable interface corner rounding styles: `Sharp` (0 px), `Rounded` (6 px default), and `Soft` (10 px).
  - All theme presets, accent colors, custom RGB values, and corner styles persist across sessions in `settings.txt`.
  - Dedicated "Theme & Appearance" submenu in Settings with live preview swatches, and quick switcher shortcuts in the Command Palette (`Ctrl+P`).

### Fixed

- **Grab Brush Symmetry Blending (Issue #53):**
  - Replaced the hard maximum cutoff with a $C^1$ smooth maximum partition-of-unity blend across the symmetry overlap window.
  - Eliminates the harsh midline crease or ridge line when pulling surfaces near or across the reflection plane, guaranteeing smooth continuous transitions without seam tearing or drift.
- **Transform Gizmo High-Poly Mesh Freeze:** Fixed severe UI lockup when dragging the transform gizmo on dense meshes (millions of triangles). OS pointer moves are now coalesced at frame rate and processed via direct, parallel normal rotation matrices ($\mathbf{n}' = \mathbf{R} \mathbf{n}$) and chunk bounds transformations without running heavy triangle cross-product or CSR topology deduplication during the drag. Soft-mask exact normal recomputation settles once upon mouse release.
- **Configurable Subdivision Triangle Cap:** Replaced hardcoded 24M triangle ceiling with configurable `max_triangles` setting in Preferences and `settings.txt` (presets for 24M, 48M, 96M, or Unlimited). Added `OXYBRUSH_MAX_TRIANGLES` environment variable override for high-memory workstations and informative limit status feedback.

## [0.6.0-alpha] - 2026-11-05

### Added

- **Pixar Regularized Kelvinlet Sculpting Brushes:** Integrated three elastodynamic brushes based on *de Goes & James (SIGGRAPH 2017)*:
  - `Kelvin Grab`: Fundamental translational regularized Kelvin state with Poisson's ratio $\nu = 0.45$. Unlike standard geometric translation, it preserves cross-sectional volume: as the surface is pulled forward, lateral regions pinch inward naturally like physical clay or rubber.
  - `Kelvin Twist`: Torsional regularized Kelvin state induced by an infinitesimal moment around the surface contact normal. Strictly divergence-free ($\nabla \cdot \mathbf{u} = 0$) and incompressible, rotating the surface smoothly without bunching or tearing. Alt inverts rotation direction.
  - `Kelvin Scale`: Dilatational/compressive elastodynamic dipole expanding or pinching volume radially with elastodynamic decay.
  - Precomputes the $3 \times 3$ elasticity tensor $\mathbf{K}(\mathbf{r})$ into the symmetry-blended grab matrix at stroke start for 60+ FPS real-time interactive deformation.
  - Full shelf integration with distinct profile icons, preview spheres, tooltips, presets, and guide topics.
- **Real-Time Wireframe Overlay:**
  - Hardware-accelerated wireframe rendering (`wireframe.wgsl`) over the shaded matcap mesh with depth testing and depth bias to eliminate Z-fighting.
  - Bound to `Shift+W` default shortcut with command palette integration.
  - Viewport view strip quick toggle button and View menu controls with adjustable line opacity slider (0.0 to 1.0).
  - Settings persistence in `settings.txt` across sessions.

## [0.5.2-alpha] - 2026-11-04

### Fixed

- **Embedded Python SDK auto-provisioning:** Fixed `ModuleNotFoundError: No module named 'oxybrush'` on standalone binary installs. OxyBrush now embeds the pure-Python `oxybrush` SDK (`client.py` and `__init__.py`) directly into the executable and automatically provisions it to `<AppData>/Local/OxyBrush/python` (on Windows) or `<data_local_dir>/oxybrush/python` (on Linux/macOS) upon launch.
- **Auto-provisioned plugin examples:** When starting OxyBrush on a new installation or opening the plugins directory, OxyBrush now automatically provisions the bundled example plugins (`mesh_stats_reporter.py`, `mesh_stats.py`, and `spiral_generator.py`) into the user plugins directory (`<AppData>/Local/OxyBrush/plugins` on Windows) alongside `README.txt`.
- **Standalone execution fallback:** Added automatic OxyBrush application data directory discovery to all example scripts so they can be run directly from terminal/PowerShell outside of OxyBrush without requiring manual `PYTHONPATH` exports.

## [0.5.1-alpha] - 2026-11-04

### Fixed

- **Status bar error diagnostics for plugins:** When an unhandled exception occurs in a Python plugin, the status bar now extracts the exact exception diagnostic (e.g. `AttributeError: ...`, `SyntaxError: ...`) from the last line of `stderr` instead of truncating at the generic first line `Traceback (most recent call last):`. The full traceback continues to be recorded in the application log.

### Added

- **Python SDK convenience methods and dataclasses:**
  - Added `MeshStats` dataclass and `session.get_mesh_stats()` returning active vertex count, triangle count, piece count, bounding box extents, and subdivision level.
  - Added high-level helper methods on `Session`: `session.voxel_remesh()`, `session.apply_stroke()`, and `session.export_mesh()`.
  - Added multi-parameter support to `session.add_primitive()` allowing `kind` or `primitive_type`.
  - Added bundled `mesh_stats_reporter.py` example plugin demonstrating `get_mesh_stats()` reporting directly to the status bar.
- **Documentation:**
  - Published comprehensive 15-page GitHub Wiki covering sculpting brushes, multiresolution subdivision, voxel remeshing, masking & face groups, posing & transform gizmo, vertex coloring, Python plugin development, MCP agent tandem workflows, and 3D printing pipelines.

## [0.5.0-alpha] - 2026-11-04

### Changed

- **Every control explains itself.** Hover a button, slider, menu item, brush or dialog control for one short sentence about what it does, plus its shortcut when it has one. Shortcut words come from one list, shared with the menus and the command palette. On a touch screen, where there is no hover, press and hold a control: the same text stays up briefly after you lift your finger, and the control does not activate.

### Added

- **Python plugin and scripting system:**
  - Create and run custom Python scripts directly against OxyBrush using the new pure-Python `oxybrush` client SDK (`python/oxybrush`). Standard library only (no third-party dependencies or CPython build bindings).
  - Built-in **Plugins** menu: run any Python script via file picker, click to launch installed plugins, open the local plugins folder, and reload plugins dynamically.
  - Automatically discovers user plugins in `<local_data>/oxybrush/plugins` and parses metadata headers (`# Name: ...`, `# Description: ...`).
  - Asynchronous background execution: scripts run without blocking the UI thread, executing tool actions and interactive brush strokes over local loopback MCP with live viewport redraws.
  - Includes sample plugins in `examples/plugins/`: `spiral_generator.py` (procedural 3D curve sculpting) and `mesh_stats.py` (scene and piece topology statistics).
- **Viewport background:** View > Background (and Settings) switches between a soft gradient and one flat color. Colors… sets the center and edge, or the single flat color, and Reset to default restores the built-in gradient. The choice is remembered between sessions. The command palette (Ctrl+K) can open the colors, switch the style or reset it, and the guide (F2) describes it.
- **Focal length:** the perspective lens is a 35mm-equivalent focal length, from 15 to 200 mm. Set it in **View > Focal length**, with the lens button in the view strip, or from the command palette. A slider and a numeric field (hundredths of a millimeter), presets at 18, 35, 50, 85 and 135 mm, and the matching field of view in degrees. Reset returns to the original view (35° vertical). The model stays the same size when the lens changes; the mouse wheel is still zoom. Orthographic view is unaffected. The choice is remembered with the other interface settings.
- **Surface-aligned brush cursor** (View and Settings > *Align cursor to surface*, or the command palette; off by default and remembered): the brush ring lies in the surface under the pointer, sized to the brush, with a short line for the surface normal. The normal is averaged around the pointer and eased as it moves, so it stays steady on dense meshes. Off the model the ring stays flat to the screen. The built-in guide (F2) has a topic for it.
- **Keymap editor** (Settings > Keymap): rebind every command, brush and view key, with conflict warnings, a per-command reset and a reset of everything. View keys start on the numpad and can move to any other key. Bound keys are shown in menus, tooltips and the command palette.
- **Mouse navigation** is part of the keymap. The **OxyBrush** preset keeps the current controls. **MMB orbit** uses the middle button to orbit, Ctrl+middle to zoom and Shift+middle to pan, and the 45° snap key starts as Alt (it can be any modifier, or off).
- **Sculpt modifiers** can be reassigned. **Ctrl carves, Alt masks** uses Ctrl to invert the brush, Alt+drag on the mesh to mask, Ctrl+Alt to erase, Alt+click on empty space to invert the mask and Alt+drag on empty space to clear it. The touch layout's Smooth, Mask and Invert buttons keep those jobs.
- Keymaps are saved beside the other settings (`keymap.txt`, versioned; unknown lines are kept) and can be exported or imported as an `.oxkeys` file.
- **Local AI agent** (off by default): Settings > Local AI agent lets a program on this computer sculpt in the same scene as you, over the Model Context Protocol. It listens only on `127.0.0.1`, with a new token every launch. While an agent is connected the status bar says so, and Disconnect changes the token. Agent edits are normal undo steps labeled `Agent:`. Nothing is sent off the machine. `oxybrush-cli mcp` speaks the same tools over stdio for a file, and `oxybrush-cli mcp --attach` connects a stdio client to the running app. See [RFC 0003](docs/rfcs/0003-mcp.md) and [docs/mcp.md](docs/mcp.md).
- **Disable autosave:** The Settings > *Autosave every* slider can be set to 0 (displays as "Off") to turn off periodic background saves and crash-recovery file writes.
- **Performance optimizations for dense meshes:**
  - **Stroke coalescing & adaptive dab budget:** Coalesce mouse moves per frame during interactive sculpting and adapt dab spacing and strength when strokes exceed a time budget (`OXYBRUSH_DAB_BUDGET_MS`, default 10 ms), preventing input lag on high-resolution meshes.
  - **GPU vertex packing:** Group colors use a precomputed lookup table, bypassing per-vertex HSV calculations, and dirty chunk uploads are coalesced into contiguous ranges.
  - **Tuned chunk granularity:** Lowered `CHUNK_TRIANGLES` from 4096 to 1024, cutting raycast cost and reducing dirty upload / undo snapshot chunk size by 4×.
  - **Parallel `update_after_move` passes:** Replaced serial dedup loops with atomic generation stamps (`Stamps`) across Rayon chunks, derived affected vertex sets directly via adjacency tables, and accumulated per-thread chunk bounding box growths.
  - **Parallel GPU rebuild & zero-copy index buffer:** Packed rebuild vertices and large dirty staging runs in parallel with Rayon, and bound mesh triangles directly when no faces are hidden, eliminating redundant index allocations.

### Fixed

- **Grab across symmetry:** with mirror or radial symmetry on, Grab and Topological Move no longer tear the mesh open when the brush reaches the symmetry plane. A vertex covered by more than one symmetry copy was overwritten by whichever copy ran last, so neighbours along that overlap were pulled apart into a stair-step crack. Those copies are blended into one move, and the seam stays continuous. Snake Hook, Pinch and Crease use the same blend, including Grab through and holding Alt to move along the normal.
- **Neutral interface.** Panels, buttons, windows and the wells behind brush icons were a cool blue-gray: each chrome color raised the blue channel (a headless screenshot measured panel pixels at 31, 33, 37). They are neutral gray now, at the same brightness. The clay accent stays on the selected item, the pressed control, hover outlines and links. The built-in viewport backdrop is neutral gray too, for both the gradient and a reset to default; choosing gradient or flat, and the saved colors, still work.

## [0.4.0-alpha] - 2026-11-04

### Added

#### Sculpting and painting
- **Vertex color painting:** new **Paint** brush with a color picker and swatches (Alt erases paint, alphas and symmetry work), plus **Fill**, **Fill cavities**, **Fill peaks** and **Clear color**, all respecting the mask (Mesh > Vertex color, or the Paint brush settings). Undoable, saved in projects (new optional `VCOL` section; older versions skip it) and carried through subdivision levels.
- **Transform gizmo** (T, the view strip, or Mesh > Transform): move, rotate or scale the active piece with axis handles or the centre handle. Masked parts stay put and a soft mask edge gives a smooth falloff, so you can pose: mask the body, rotate an arm at the shoulder. The pivot sits on the mask edge. Undoable; Esc cancels a drag.
- **Quick menu at the cursor:** right-click the viewport (without dragging) or hold **Q** for brush size, strength and Raise/Carve right where you are sculpting. A held menu closes when you release Q.
- **Grab through:** a Grab brush option that moves everything under the brush circle, front to back, instead of only the surface you click.
- **Move along the normal:** hold **Alt** with Grab, Topological Move or Snake Hook to move the surface along its normal; the drag's length sets how far.
- **Brush presets:** Brush > Presets… saves the current brush under a name (kept in your user data folder), applies starter or saved presets with one click, and shares them as a one-line brush code (`oxyb1:…`, paste it to add the brush) or an `.oxbrush` file (import/export). Right-click a preset to copy its code, export or delete it. Presets are plain data; codes are size-limited when read.

#### Viewing
- **Flat shading** (View > Flat shading): see each polygon as a flat facet. Remembered between sessions. Thanks to the testers who asked in the discussions.
- **Custom matcaps:** load a PNG or JPEG of a lit sphere from the material picker (or View > Load matcap…). It is copied into your user data folder, so it's there next time, and the last one you used is remembered; right-click one to remove it.
- **45° view steps:** hold Shift while orbiting (dragging on empty space) to snap the view to 45° angles.

#### Workflow
- **Command palette** (Ctrl+K, or Edit/Help menus): type part of any command, brush, recent file or guide topic and press Enter. Letters only need to appear in order ("adl" finds Add subdivision level).
- **Brush finder** (Space): type part of a brush name, Enter picks it.
- **Recent files:** File > Open recent and the welcome window list the last 10 projects and meshes you opened, imported or saved (remembered between sessions; missing files are dropped when clicked).
- **Built-in guide** (Help > Guide, F2): short searchable topics on every part of OxyBrush, available offline. The online testing guide is still under Help.
- **Strokes from empty space** (Settings > *Strokes can start on empty space*, off by default): a drag that starts beside the model sculpts once it reaches the surface. Orbit with the middle mouse button or the view strip instead.
- **Middle-drag scrolls panels:** drag with the middle mouse button (or a pen button mapped to it) over the side panels to scroll them.
- **Optional update checks** (off by default): Settings > Updates > *Tell me about new versions* checks GitHub about once a day and shows a notice; *Download new versions automatically* also downloads the file. Help > Check for updates checks once. Downloads are verified against the release's SHA-256 checksums (and an ed25519 signature once release signing is set up). The Windows installer build can install and restart; portable, macOS and Linux builds get the verified file shown in its folder. Flatpak installs keep updating through Flatpak. See [RFC 0002](docs/rfcs/0002-updates.md).

#### Import, export and pipeline
- **More export formats:** glTF 2.0 (`.glb` and `.gltf`) keeps every piece as its own named object for game engines and other 3D tools; OBJ now writes pieces as separate objects; binary PLY added. Hidden faces are never exported.
- **Separate objects on import:** each object (`o`) or group (`g`) in an OBJ file becomes its own piece, named after it (one undo step for the whole import).
- **LOD chain:** the **Pieces** menu > *Make LOD chain* adds copies at 1/2, 1/4 and 1/8 of the triangles (in the background, undoable).
- **Split pieces:** **Pieces** > *Split by face groups* or *Split into loose parts* turns one piece into several (undoable).
- **Command-line tool** `oxybrush-cli`: `info`, `convert`, `remesh`, `decimate`, `lod` and `split` without opening the app, for batch work and scripts.

### Developers
- `oxy_mesh::Mesh::color` (per-vertex sRGB), `fill_color`, `fill_color_by_curvature`, `curvature`; `History::begin_with` / `cancel`.
- New crate `oxy-cli` (binary `oxybrush-cli`); `oxy_io::export_pieces` writes several pieces at once.
- New crate `oxy-update`; `cargo build -p oxy-app --no-default-features` builds without any network code.

## [0.3.0-alpha] - 2026-11-02

### Added
- **Viewport shading:** screen-space ambient occlusion (folds and the inside of shapes darken) and cavity shading (creases and pores darken, edges catch light), so sculpted detail reads clearly. On by default; toggle it in the view strip and set strengths in **View > Shading** (remembered between sessions). With both at 0 the viewport renders exactly as before.

### Changed
- **Faster with dense meshes** (measured on a 4-core cloud machine):
  - Switching to a 5.2M-triangle subdivision level: ~2.3 s → ~0.7 s; adding it: ~8 s → ~2–4 s.
  - Level changes above 1M triangles now run in the background, so the window stays responsive (the UI thread does ~60 ms of work at 5.2M). The piece can't be edited, deleted or saved until the change finishes; orbiting and other pieces still work.
  - Decimation: 1.3M → 131k triangles ~12 s → ~6 s.
  - Small brush dabs ~40% faster (chunk bounds are grown per dab and tightened when the stroke ends).
  - Opening projects and finishing remesh/decimate also build their meshes faster (parallel mesh build).

### Fixed
- Sculpting a piece while a remesh or decimation of it was running was silently overwritten by the result; edits to that piece (and undo) now wait for the job.

### Developers
- `cargo run --release -p oxy-render --example shading_timing` measures the viewport passes on your GPU.
- Shaders are validated in `cargo test` (naga), no GPU needed.

## [0.2.0-alpha] - 2026-11-01

### Added
- **Subdivision levels (multiresolution):** Ctrl+D adds a level, D / Shift+D step up and down. Sculpt at any level; changes on a lower level carry the higher-level detail along. Delete higher or lower levels from the Mesh menu. Levels are saved in `.oxb` projects (a new, skippable section, so older versions still open the file at the active level) and undo restores them after remesh or decimate.
- **Welcome window:** starter shapes (sphere, cube, plane), open/import, the basics and a layout choice. It appears on the first start, then only if "Show at startup" is on; **Help > Welcome screen** reopens it.
- **Previews and pickers:** large brush, alpha and material previews; click one for a grid of thumbnails (brush previews are rendered clay balls showing each brush's effect).
- **Viewport:** a soft gradient background; an axis gizmo (click an axis to view from that side); a view strip with drag-to-orbit, pan and zoom buttons (useful with a pen or touch screen), frame, orthographic, mask, group colors and solo.
- **Raise / Carve** toggle for the default brush direction (Alt still flips it while held), next to size and strength.
- **Layouts:** **Standard**, **Compact** (small screens) or **Touch friendly** (large targets, brush tiles, on-screen Smooth / Mask / Invert toggles, undo/redo beside the viewport). Choose in Settings or the welcome window; `--layout standard|compact|touch` starts with one.
- **Arrange the interface:** brush shelf on the left, bottom or right; settings panel on the right or left; size / strength / Raise-Carve floating at the center top (default) or bottom of the viewport, or in the toolbar. Settings > Arrange panels, or right-click the shelf or the bar.
- **Rearrange brushes:** drag brushes in the shelf to reorder them; number keys 1–9, 0 follow your order. Settings > Arrange panels > Reset brush order restores the default.
- **Settings are remembered:** layout, arrangement, brush order, interface scale, high contrast, material, autosave interval and "Show welcome window at startup" are saved in a small text file in your config folder. `--set key=value` overrides a setting at startup.

### Changed
- **Friendlier interface:** drawn icons for every brush showing its effect, descriptions in tooltips, collapsible right-panel sections with explanations on hover, a status bar that explains what the held keys do, and a warmer theme with more spacing and rounded controls.
- **D** now steps up a subdivision level instead of subdividing the mesh in place; the one-off subdivide is **Mesh > Subdivide (single mesh)**.

### Known limitations
- Switching to or adding a very dense level (~5M triangles) pauses the UI for 2–4 seconds.
- Touch layout: no multi-touch gestures (pinch zoom, two-finger orbit) yet; not tested on real touchscreens or tablets.
- Brush settings (radius, strength, falloff) are not remembered between sessions.

## [0.1.0-alpha] - 2026-11-01

### Added
- **macOS:** a universal `.dmg` that runs natively on Apple Silicon and Intel Macs (macOS 11 or newer).
- **Brushes:** Polish (flattens bumps without shrinking), Planar Trim (cuts down to a plane; inverted fills up to it), Topological Move (moves only the connected surface under the brush, so nearby separate parts stay put) and Noise (procedural surface noise with adjustable scale). 16 brushes in total.
- **Stroke control:** stabilizer ("lazy mouse") for smooth lines; stroke modes Freehand, Line (drag a straight line) and Stamp (drag out a single dab: distance sets the size, direction the rotation); spray (scatters position, rotation and strength); pressure curve.
- **Alphas:** grayscale images shape each dab. Seven built-in procedural alphas (Noise, Bumps, Scales, Cracks, Stripes, Rings, Square) and your own grayscale PNGs; rotation and follow-stroke options.
- **Masking and selection:** Ctrl+drag on empty space masks a rectangle or lasso (Ctrl+Alt unmasks); Ctrl+click on empty space blurs the mask; Ctrl+Shift+drag shows only the selection (Alt hides it); L toggles rectangle/lasso. Mask cavities, mask peaks, mask the face group under the cursor (Ctrl+M), blur, sharpen, grow and shrink the mask; grow and shrink the visible area.
- **Help menu for testers:** Report a bug (opens a GitHub issue form with your version, OS and GPU pre-filled), Send feedback, Copy system info, Open log folder, Getting started guide, Changelog.
- **Log file:** every run writes `oxybrush.log` to the per-user data folder; the previous run is kept as `oxybrush.previous.log`, and crashes (panics) are logged with a backtrace. Windows release builds previously had no log output at all.
- About window shows OS, GPU and log location, with a Copy system info button.
- Alpha testing guide, issue forms for bug reports and tester feedback.
- `docs/ROADMAP.md` (living roadmap and status), `AGENTS.md` (instructions for AI coding agents).

### Changed
- Escape cancels an in-progress selection, line or stamp.
- Ctrl+Shift+click now acts on release (so the same gesture can start a box selection).

### Known limitations
- The macOS app is not notarized: the first launch needs "Open Anyway" in System Settings (see the testing guide). Double-clicking `.oxb` files doesn't open them on macOS yet; use File > Open.
- Stencils (screen-space alpha overlays), multiresolution and the 10M-triangle performance work are still to come (Phase 3).
- Pen pressure is still unverified on real tablets on every platform.

## [0.0.1-alpha] - 2026-11-01

First public alpha.

### Added
- **Sculpting:** 12 brushes — Draw, Clay, Clay Strips, Smooth, Inflate, Flatten, Pinch, Crease, Layer, Grab, Snake Hook, Mask — with falloff curves, spacing, auto-smooth and pen-pressure options.
- **Symmetry:** X/Y/Z mirror and radial (up to 64 copies around any axis).
- **Pieces:** several meshes per document; add, duplicate, mirror-duplicate, delete, rename, hide, solo; Alt+click to make a piece active.
- **Topology:** voxel remesh, merge visible pieces (union), quadric decimation, subdivision (linear and smooth); long operations run in the background.
- **Face groups and visibility:** group visible / from mask / by islands, Ctrl+Shift+click to isolate a group, invert visibility, delete hidden, group color overlay.
- **Masking:** paint, unmask, invert, clear, show/hide.
- **Files:** `.oxb` project format (versioned, compressed, checksummed, atomic saves), autosave every 2 minutes with crash recovery, unsaved-changes prompt; OBJ/STL import as pieces; OBJ/binary STL export of visible pieces.
- **Undo/redo** for strokes, topology, face edits and piece changes, within a memory budget.
- **Viewport:** wgpu matcap rendering with five procedurally generated materials, orthographic/perspective, front/side/top views.
- **Interface:** brush shelf, inspector, history list, shortcut reference (F1), UI scaling, high-contrast theme, screen-reader labels (AccessKit).
- **Packaging:** Windows installer (per-user, `.oxb` association) and portable zip; Linux Flatpak and portable tarball; SHA-256 checksums.
- ZBrush file-format research tooling (`oxy-ztl`, milestone 0; no import yet).

### Known limitations
- Pen pressure not yet verified on Windows Ink or Linux tablets.
- Decimating very dense meshes is slow (~10 s for 1.3M triangles).
- Remesh resets mask and face groups; open surfaces remesh to a thin shell.
- Settings are not remembered between sessions.
- The Windows installer is not code-signed (SmartScreen warning).

[Unreleased]: https://github.com/crisprintsstuff/OxyBrush/compare/v1.0.0-beta...HEAD
[1.0.0-beta]: https://github.com/crisprintsstuff/OxyBrush/compare/v0.6.0-alpha...v1.0.0-beta
[0.6.0-alpha]: https://github.com/crisprintsstuff/OxyBrush/compare/v0.5.2-alpha...v0.6.0-alpha
[0.5.2-alpha]: https://github.com/crisprintsstuff/OxyBrush/compare/v0.5.1-alpha...v0.5.2-alpha
[0.5.1-alpha]: https://github.com/crisprintsstuff/OxyBrush/compare/v0.5.0-alpha...v0.5.1-alpha
[0.5.0-alpha]: https://github.com/crisprintsstuff/OxyBrush/compare/v0.4.0-alpha...v0.5.0-alpha
[0.4.0-alpha]: https://github.com/crisprintsstuff/OxyBrush/compare/v0.3.0-alpha...v0.4.0-alpha
[0.3.0-alpha]: https://github.com/crisprintsstuff/OxyBrush/compare/v0.2.0-alpha...v0.3.0-alpha
[0.2.0-alpha]: https://github.com/crisprintsstuff/OxyBrush/compare/v0.1.0-alpha...v0.2.0-alpha
[0.1.0-alpha]: https://github.com/crisprintsstuff/OxyBrush/compare/v0.0.1-alpha...v0.1.0-alpha
[0.0.1-alpha]: https://github.com/crisprintsstuff/OxyBrush/releases/tag/v0.0.1-alpha

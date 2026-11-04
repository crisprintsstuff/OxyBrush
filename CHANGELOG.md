# Changelog

All notable changes to OxyBrush. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/) (pre-1.0: anything may change).

## [Unreleased]

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

[Unreleased]: https://github.com/crisprintsstuff/OxyBrush/compare/v0.4.0-alpha...HEAD
[0.4.0-alpha]: https://github.com/crisprintsstuff/OxyBrush/compare/v0.3.0-alpha...v0.4.0-alpha
[0.3.0-alpha]: https://github.com/crisprintsstuff/OxyBrush/compare/v0.2.0-alpha...v0.3.0-alpha
[0.2.0-alpha]: https://github.com/crisprintsstuff/OxyBrush/compare/v0.1.0-alpha...v0.2.0-alpha
[0.1.0-alpha]: https://github.com/crisprintsstuff/OxyBrush/compare/v0.0.1-alpha...v0.1.0-alpha
[0.0.1-alpha]: https://github.com/crisprintsstuff/OxyBrush/releases/tag/v0.0.1-alpha

# Changelog

All notable changes to OxyBrush. The format is based on [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

### Added
- **Plugins menu** in the menu bar. Every installed in-app plugin has a submenu with its commands (they run at once, with the panel's current settings) and **Panel (settings)…** for its sliders and viewport drawing. Python scripts are listed too, and the menu has **Install add-on…**, **Run Python script…**, **Open plugins folder**, **Reload plugins** and **Manage plugins…**. Until now plugins were only reachable from Settings > Plugins.
- **Built-in in-app plugins**, installed into the plugins folder on first use:
  - **Displace:** Inflate, Deflate or Roughen the surface along its normals, by a percentage of the piece's size;
  - **Mask by Height:** mask below or above a height, with a soft edge, and the level drawn as a ring;
  - **Color Ramp:** vertex color by height (blue to orange to pale yellow) or by the direction the surface faces;
  - **Array:** copies of the active piece in a row along X, Y or Z, with a gap, as new pieces;
  - **Measure:** width × height × depth and volume of the active piece, with its bounding box drawn in the viewport.

  They are written in WebAssembly text, so they double as examples for plugin authors. Masked parts are left alone, and each is one undo step.
- **Remove** for plugins you installed (Settings > Plugins), with a confirmation click. Built-in plugins have none, since they would come back.

### Fixed
- The same Python script was listed twice when OxyBrush ran from the source tree.

## [3.0.3-beta] - 2026-11-08

### Added
- **Tube brush:** the tube tool is also a brush in the shelf, **Tube**, next to Insert Primitive. It draws a tube the same way, but **Accept adds it into the active piece** (one undo step, in a face group of its own) instead of making a new piece. The Tube window has a checkbox to switch between the two.
- **Add-on packages (`.oxyaddon`)**: one file that installs a plugin, either an in-app (WebAssembly) plugin or a **Python** plugin. **Settings > Plugins > Install add-on…** (or opening the file) puts it in the plugins folder, replacing an older version; it then appears with the other plugins. Make one with `oxybrush-cli addon pack <folder> <file.oxyaddon>`. Archives can't write outside their folder and are size-limited.
- **Plugin API 2** ([RFC 0011](docs/rfcs/0011-wasm-plugins.md)): in-app plugins can also read normals and vertex colors, set the **mask** and **vertex colors** of a piece, and **add new pieces** (new `add_pieces` capability). A command's changes to one piece are one undo step. Plugin commands now appear in the **command palette**. Plugins written for API 1 work unchanged.

### Changed
- **Parametric primitives:** **Pieces > + Add** (and the command palette) now opens a small window for the shape instead of adding it at once: radius or size, height, segments around, rows up the side, closed caps, tube radius and so on, with a live preview and its triangle count. Shapes are **low-poly by default** (a cube is 8 vertices, a cylinder 50). **Even triangles (voxel remesh)** is a checkbox, with its resolution, for when you want a sculpt-ready mesh; cylinders and cones are no longer remeshed unless you ask. Accept (Enter) adds the piece, Cancel (Esc) discards it.
- **F frames the active piece**, centred on it, instead of the whole scene; **Alt+F** frames every visible piece. With one piece nothing changes.
- **Zoom drags follow ZBrush in the Right-drag orbit preset:** Ctrl+right-drag **down** zooms in. Other presets keep drag up = zoom in. Both directions can be flipped in **Settings > Keymap > Zoom direction**: **Invert zoom drag** and **Invert mouse wheel zoom**.

## [3.0.2-beta] - 2026-11-08

### Added
- **In-app plugins (WebAssembly)** ([RFC 0011](docs/rfcs/0011-wasm-plugins.md)): plugins that run inside OxyBrush in a sandbox, with no Python and nothing to switch on. A plugin is a folder with `plugin.json` and `plugin.wasm` (or `.wat`) in the plugins folder; **Settings > Plugins** lists them, and each gets a panel with its sliders and command buttons. They can read meshes and the scene, change vertex positions (each change is an undo step named after the plugin) and draw lines in the viewport, but only what their manifest declares, and nothing else: no files, no network. Commands run in the background; a runaway plugin is stopped. Comes with an example, **Squash**.
- **Blender bridge** ([RFC 0010](docs/rfcs/0010-blender-bridge.md)): with the new **OxyBrush Bridge** add-on for Blender (`integrations/blender/`), **Object > Send to OxyBrush** opens the selected meshes in OxyBrush, placed as they were, with a **Linked to Blender** badge. **File > Send back to Blender** (or the badge), or **Send back to Blender and close**, returns them: objects whose vertices are unchanged keep their UVs, materials, modifiers and shape keys in Blender; pieces made in OxyBrush arrive as new objects. OxyBrush also opens such links from the command line with `--link`.
- **Capture alpha from view** (Alpha section and command palette): makes an alpha from what the viewport shows: a depth image of the visible model, nearest parts white, cropped to the model. Sculpt a detail, look at it straight on, capture it, and stamp it elsewhere. It is added to the alphas and selected; captures last until OxyBrush closes.
- **Close holes** (Mesh menu and command palette): fills every opening in the active piece with a smooth patch of new faces, sized like the faces around it, in a face group of its own. Handy after Delete hidden, a cut or an import with gaps, and before 3D printing. One undo step.

## [3.0.1-beta] - 2026-11-08

### Added
- **Tube tool** ([RFC 0009](docs/rfcs/0009-tube-tool.md)): **Pieces > + Add > Tube (draw)…** — drag a path from the model or empty space and get a smooth tube along it. Drag its points to reshape it, **Shift+drag** a point to make the tube thicker or thinner there, click the line to add a point and **Alt+click** to remove one. The Tube window sets thickness, sides, rounded ends and a mirror copy across X. **Accept** adds it as a new piece in one undo step. For horns, tentacles, tails, limbs and cables.
- **More shapes under Add piece:** **Cylinder**, **Cone** and **Torus**, alongside sphere, cube and plane (Pieces > + Add, the Pieces menu and the command palette). Cylinder and cone are built with even triangles, ready to sculpt (in the background, so the window doesn't pause).
- **Split hidden** (Mesh menu and command palette): moves the active piece's hidden faces into a new piece of their own, shown, and keeps the visible faces in the original. One undo step.
- **Focal shift** (brush settings and the quick menu): moves the middle of the falloff curve, from firmer (more of the brush at full strength) to softer. The cursor's inner ring follows it, and it is saved in brush presets. At 0 the brush is exactly as before.
- **Sliders under the pointer:** **S** (size), **U** (strength), **I** (Paint strength) and **Shift+O** (focal shift) open a single slider right where the pointer is. The same key again, Esc or a click outside closes it. In the Single-key tools scheme focal shift is **O** and the orthographic toggle moves to **P**. All are in Settings > Keymap.
- **Viewport lights and shadows** ([RFC 0008](docs/rfcs/0008-viewport-lights.md)): **View > Shading > Light the viewport** shades the model with up to four lights (sun, point or spot) instead of the material, and **Lights…** sets them up: color (with warm, daylight and cool presets), intensity, direction or position, spot cone and aim, lights that follow the view, sky and ground light, rim light, exposure, the surface color, roughness and specular, and soft **shadows** from suns and spots. While the Lights window is open, numbered markers in the viewport can be dragged to move the lights. Face-group colors, paint, masks, ambient occlusion and cavity still show. Off by default; the matcap stays the default look.

### Changed
- **Single-key tools shortcut scheme:** the orthographic toggle is now **P**, because **O** opens the focal shift slider. The default scheme keeps **O** for orthographic.
- **History lists the newest step first**, with steps you can redo (undone) greyed at the top.
- **Ctrl on empty space inverts or clears the mask:** with the default mask gestures, **Ctrl+click on empty space now inverts the mask** (with no mask, that masks everything) instead of blurring it, and a **Ctrl box or lasso drawn around none of the model clears the mask**. A box over the model still masks what it encloses. Blur is in the Mask menu and the command palette.
- **Floor grid button on the view strip:** the strip beside the viewport has a floor grid toggle under the wireframe button, so it no longer takes a trip to **View > Shading**.

### Fixed
- **Grab no longer leaves jagged seams.** Two causes, both fixed: with **X symmetry**, where the mirrored brush's reach began a vertex could move less than its neighbour, cutting a jagged line along that edge (the V lines between two mirrored bulges, worst when pulling along the normal with Alt); and **Front faces only** dropped vertices outright once their surface turned past sideways, tearing steep walls. It now fades those vertices out instead. This applies to every brush, so strokes over steep walls also blend in more softly.
- **Open log folder** (and the updater's "show the downloaded file") opened a web browser on Windows instead of the file manager. Folders now open with Explorer, Finder or the desktop's file manager directly.
- **Tab kept moving through the menus:** after Tab hid the panels, the next Tab moved keyboard focus through the menu bar instead of bringing the panels back. Tab now always shows or hides the panels, except while typing in a text field.
- **Brush count in the README and website:** the README said "25 brushes" counting the masking, painting, insert and cutting tools, and the website still said 17. Both now say 19 sculpting brushes plus 6 tools, and the README lists them by kind.
- **Brush shelf icons:** in the brush list beside the viewport the icons were small and didn't line up with the current brush / alpha / material tiles above them; at the bottom of the window the brush thumbnails were smaller than those tiles and sat lower. They now match the tiles in size and line up with them.
- **Windows: app icon:** the window and taskbar showed a generic icon. OxyBrush now sets its window and taskbar icons, and the icon file stores the small sizes uncompressed, the way Windows draws them reliably (PNG is kept for the 256 px size only).

## [3.0.0-beta] - 2026-11-08

### Added
- **Two-point pressure curve (Preferences > Pen & tablet):** drag two points on the large curve to shape the pressure response, like tablet drivers do; an **S-curve** preset gives fine control at both the lightest and the hardest presses. Double-click the curve to straighten it. Remembered between sessions.
- **Curve stroke mode:** a stroke that stays editable after you draw it. It becomes a smooth curve through a few control points on the surface: drag a point to reshape it, click the curve to add a point, Alt+click to remove one. Size, strength, the other brush settings and **Fade in / Fade out** update the result live on the model; **Accept** (Enter) or starting the next stroke applies it as one undo step, **Cancel** (Esc) discards it. Orbiting and zooming keep it open. Works with symmetry, the mask and alphas. Design in [RFC 0004](docs/rfcs/0004-curve-stroke.md).
- **Hold to adjust:** while drawing a normal Freehand stroke, hold still for about half a second (a ring fills up at the cursor) and the stroke becomes an editable curve; keep moving to extend it, then adjust it like a Curve stroke. On by default; switch it off in the Stroke section.
- **Curve strokes, second round:** click a control point to give it its own **size** and **strength** (a ring shows the size there; the stroke blends smoothly between points); **Ctrl+Z / Ctrl+Shift+Z** undo and redo edits of the open curve; **Join the ends** makes a loop; and **Hold to adjust** can **Ask first** ("Edit this stroke as a curve?", off by default). The curve's fades now work even when pen pressure doesn't set strength.
- **OxyCharacter round trip:** a document opened from OxyCharacter (a `.oxb` with a `<file>.oxylink.json` next to it) shows **Linked to OxyCharacter** in the top bar. Clicking it, **File > Send back to OxyCharacter** or the command palette saves to the file OxyCharacter is watching (atomically, as any save), even after a Save As elsewhere, and confirms with "Sent to OxyCharacter: changes appear there". Save As copies the `.oxylink.json` byte for byte next to the new file; OxyBrush never reads, moves or deletes it. The first time `OxyCharacter Body` loses its topology (remesh, subdivide once, decimate, Boolean, split, delete hidden…), a hint explains that it will return as a separate mesh and that subdivision levels (Ctrl+D) keep it a morph.
- **`VMAP` project section:** for each vertex of a loaded piece, its index in the file the piece was first loaded from (OxyBrush reorders vertices on load), so a program like OxyCharacter can match vertices exactly. Written only while the piece's topology is unchanged and it has no subdivision levels; older versions skip it.
- **Plugin API, phase 1 (read access, [RFC 0007](docs/rfcs/0007-plugin-api-read-access.md)):** three new local agent tools, also in the Python SDK. `scene_state` describes the document as JSON: pieces with bounds and versions that change whenever a piece does, and the camera as data (eye, target, field of view, view and projection matrices). `changes` lists what changed since a cursor and can wait for the next change, so plugins don't have to poll. `get_geometry` writes a piece's arrays (positions, normals, indices, and colors, mask, face groups or hidden faces on request) to a compact binary file and returns its layout; unlike exports, it keeps vertex colors and keeps pieces separate.
- **Curve Move brush ([#82](https://github.com/crisprintsstuff/OxyBrush/issues/82)):** draw a curve along a part (an arm, a tail), then drag its points; the part bends, stretches and turns to follow, with the result shown live and applied as one undo step (Enter), or cancelled (Esc). **Through the middle** (on by default) runs the curve through the part's core. Size sets the reach (the part follows fully within the inner half, then lets go along the falloff); a selected point has its own reach and strength; **Reset** puts the points back. Masks protect what shouldn't move, and symmetry moves the mirrored side too. Design in [RFC 0005](docs/rfcs/0005-curve-move.md).
- **Dynamic detail ([#81](https://github.com/crisprintsstuff/OxyBrush/issues/81)), first version:** in the Topology section (and the command palette), **Add detail as you sculpt** makes sculpting brushes split triangles that are too big for the brush and merge ones that have become too small, only where you sculpt. **Detail** sets how fine, relative to the brush size, so a smaller brush gives finer detail. Symmetry copies refine too; masked areas, hidden faces and open rims keep their triangles; Mask, Paint, Grab-like brushes and cuts don't change triangles; pieces with subdivision levels are left alone. Each stroke is one undo step. Smooth up to a few hundred thousand triangles; slower above. Design in [RFC 0006](docs/rfcs/0006-dynamic-detail.md).
- **Symmetry centre per piece:** the Symmetry section has **Recentre** (put the mirror planes through the middle of the active piece, e.g. after moving it) and **World origin**. When you turn on X, Y or Z mirroring, or move the centre, the mirror plane is outlined over the piece for a moment.
- **`SYMC` project section:** each piece's symmetry centre. Older versions skip it and mirror about the world origin as before.

### Fixed
- **Symmetry on models away from the origin ([#80](https://github.com/crisprintsstuff/OxyBrush/issues/80)):** mirroring, radial copies, symmetric cuts, Insert Primitives and the mirrored cursor dots all worked about the world origin, so on a model that isn't centred there (common with imports) the mirrored side of a Grab or any other stroke landed in empty space and nothing happened. Each piece now has its own symmetry centre, chosen once from its shape when it is imported, opened or added, and saved with the project, so it doesn't drift while you sculpt. A model already built around the origin keeps it, even if it isn't perfectly symmetric (an arm raised on one side); only a shape that is clearly mirrored about somewhere else moves it. Duplicates, extracts and split parts keep the centre of the piece they came from. The local AI agent's strokes use it too.
- **Side panels keep their width ([#80](https://github.com/crisprintsstuff/OxyBrush/issues/80)):** the settings panel no longer grows and shrinks as sections open and close; drag its edge to resize it.
- **Pen pressure in the Linux Flatpak ([#81](https://github.com/crisprintsstuff/OxyBrush/issues/81)):** the Flatpak only had X11 as a fallback, so on a Wayland desktop OxyBrush couldn't switch to X11 for a connected tablet, and pen pressure was lost. It now has the X11 socket too.
- **macOS: pen pressure from drawing tablets ([#70](https://github.com/crisprintsstuff/OxyBrush/issues/70)):** pressure from Wacom, Huion, XP-Pen and other tablets had no effect on macOS; the brush always used full pressure. The windowing library only passed on trackpad Force Touch pressure; the bundled winit now also forwards the pen's pressure (a third small patch, see `third_party/README.md`).

## [2.5.7-beta] - 2026-11-07

### Added
- **Floor grid (View > Shading > Floor grid, on by default):** a grid on the floor under the model, with a bolder line every ten cells, fading out with distance. It sits just under the bottom of the visible pieces and its spacing follows the model's size, so it moves and rescales as the model changes. Light lines on dark backgrounds, dark lines on light ones. Remembered between sessions; also in the command palette.
- **Pen & tablet settings ([#70](https://github.com/crisprintsstuff/OxyBrush/issues/70)):** a new **Preferences > Pen & tablet** tab with a large pressure curve, **Soft / Linear / Firm** presets, the curve shape, a **Lightest touch** strength (so light strokes are never too faint) and **Full pressure at** (so you don't have to press as hard for a full stroke). While you draw, a dot shows your pen's pressure on the curve. The settings are remembered between sessions; clicking the small curve in the brush's Stroke section opens the tab.

### Changed
- **Mask extract previews first:** Extract mask now shows the shell as a live preview instead of adding a piece straight away. Thickness and edge smoothing update the preview as you drag the sliders; **Accept** (Enter) adds it as a new piece in one undo step, and **Cancel** (Esc) leaves the scene unchanged. Editing the mask while the preview is open updates it too.
- **Previews look like previews:** the mask extract preview and the Insert Primitives preview are drawn with a blue glaze, so they can't be mistaken for the model until they are added.

### Fixed
- **macOS: Quit asks to save ([#69](https://github.com/crisprintsstuff/OxyBrush/issues/69)):** Cmd+Q, Quit in the Dock and logging out closed OxyBrush without asking to save unsaved changes. They now ask, like closing the window does. (The bundled copy of winit has a second small patch for this, see `third_party/README.md`.)

## [2.5.6-beta] - 2026-11-07

### Added
- **Midnight theme:** deep navy panels that read as layered glass, thin blue outlines on every control, softer secondary text and soft shadows under windows and menus. A new **Pill** corner style gives buttons, fields and list rows fully rounded ends. The **Midnight** quick preset (Preferences > Appearance) applies the theme, pill corners, the emerald accent and a dark navy viewport; your material and shortcuts are left alone. Also in the command palette ("Theme: Midnight").

### Changed
- **Documentation:** the README, homepage, changelog and in-app guide describe 2.5.5 and later correctly (current brush, theme and preset names; a 2.5.0-beta changelog section; Windows Ink side button in the guide). "Pixar Kelvinlet" is now "Kelvinlet" in user-facing text; the paper is still cited.

## [2.5.5-beta] - 2026-11-07

### Changed
- **Generic names for options that used other products' branding:** the "ZBrush Classic" layout is now **Workstation**, the theme is **Matte**, the "ZBrush Orange" accent is **Tangerine** (a new color), the "ZBrush" keymap presets are **Right-drag orbit** and **Single-key tools**, the "Red wax" material is **Crimson clay**, Polyframe is **Wireframe + groups**, and the curve tools are **Plane Clip**, **Line Split** and **Cut Away**. The top bar always says Raise/Carve, Size and Strength. Saved settings, keymaps and brush presets that use the old names still load.
- **Menus fit on screen:** the View menu is split into Camera, Shading, Brush cursor and Material submenus, and any menu taller than the window scrolls instead of running off the screen.

### Fixed
- **Pen side button on Windows (Windows Ink):** holding the pen's side button while touching the tablet started a sculpting stroke over the model, because the windowing library dropped the button from Windows Ink pen input ([#59](https://github.com/crisprintsstuff/OxyBrush/issues/59)), and Ctrl + side button drew a mask instead of zooming ([#63](https://github.com/crisprintsstuff/OxyBrush/issues/63)). It now acts as the right mouse button: with the **Right-drag orbit** navigation preset it rotates the view, and Ctrl + side button zooms (in the default preset right-drag pans), and a tap opens the quick menu. OxyBrush ships a patched copy of winit 0.30.13 for this (`third_party/winit`).
- **Matte (formerly "ZBrush Classic") theme:** section headers, the Carve and Y/Z mirror toggles and the status bar were drawn in near-black (the orange accent darkened all strong text). Buttons are also lighter than the panels now, so they read as clickable.
- **Insert Primitives shows what you're inserting:** the primitive (and its symmetry copies) is drawn solid on the surface and grows as you drag, instead of appearing only on release.
- **Cut tools show their line:** the clip/slice/trim line, shaded side and direction arrow are visible while dragging. They were hidden (or blinked) because "Hide circle during stroke" also hid tool previews; it now hides only the brush circle.
- **Cut tools follow symmetry:** clip, slice and trim cuts are repeated for mirror and radial symmetry, as one undo step. A trim that would remove the whole piece is refused with a message.
- **Transform gizmo keeps a moved pivot:** after Alt-dragging the pivot and then transforming, the gizmo stays with the geometry instead of snapping back to its old spot.

## [2.5.0-beta] - 2026-11-06

Names below are the current ones; 2.5.0-beta shipped them as ClipCurve, SliceCurve, TrimCurve and "ZBrush Classic" (renamed in 2.5.5-beta).

### Added
- **Screen-line cutting tools:** **Plane Clip** flattens the shaded side onto a plane, **Line Split** gives the two sides their own face groups, and **Cut Away** removes the shaded side and caps the cut watertight. Ctrl+Shift+drag draws the line; Alt+tap adds a corner point, Alt+double-tap a smooth point, and Space+drag moves the whole line. Each cut is one undo step.
- **Workstation layout:** a dense layout with a two-column brush shelf and the main controls in the top bar (Settings > Layout). Existing layouts are unchanged.
- **Topology diagnostics (Mesh > Topology Diagnostics…):** watertightness, boundary and non-manifold edges and degenerate triangles, with one-click removal of degenerate triangles (undoable).

### Fixed
- **macOS save dialog crash ([#57](https://github.com/crisprintsstuff/OxyBrush/issues/57)):** resizing the Save or Save As dialog no longer crashes; file dialogs are now asynchronous.
- **Subdivision levels:** sculpting at a high level now carries the shape changes down to the lower levels, so stepping down keeps your strokes.
- **Strokes on dense meshes:** dab spacing is capped so strokes stay smooth instead of beading at high triangle counts.
- **Updater:** clearer manual install steps and an "Open download folder" button for Linux and portable packages.

## [2.0.0-beta] - 2026-11-06

### Added

- **Insert Primitives Brush (IMM / Multi-Mesh):**
  - Interactive drag-to-scale and orient placement of 3D primitives (**Sphere**, **Cube**, **Cylinder**, **Cone**, **Torus**, and **Plane**) directly on mesh surfaces.
  - Live 3D wireframe preview rendered on surface normals with active symmetry transformations (radial and mirror axes).
  - Automatic pre-existing mesh masking: all original vertices are 100% masked (`mask = 1.0`) while newly inserted primitive vertices remain unmasked (`mask = 0.0`), enabling immediate transposition and posing with the Gizmo (`W`).
  - Distinct polygroup face group assignment and full undo/redo history recording for topology modifications.
  - Interactive shape selection buttons in the brush shelf, quick menu, and properties panel.
- **ZBrush Classic UI Theme & Preset:**
  - Added dedicated ZBrush classic dark theme preset closely mirroring the signature ZBrush palette and styling, accessible from Preferences > Theme.
- **ZBrush Keymap & Navigation Profile:**
  - Added official Maxon ZBrush navigation keybindings and cheat sheet shortcut bindings (Right-click / Alt-navigation, brush palettes, masking gestures), selectable in Preferences > Keymap.
- **Tablet Hardware Detection & Wayland Fallback:**
  - Automatic graphics tablet hardware detection via `/proc/bus/input/devices` and `/sys/class/input`.
  - Auto-fallback to X11/XWayland on Linux when a drawing tablet is detected to eliminate Wayland driver pointer dropouts and missing pressure events.
  - Added CLI flags `--x11`, `--backend <x11|wayland>`, and `OXY_BACKEND=x11` environment variable.

### Fixed

- **3D Print Tools (Watertight Hollow & Keyed Split):**
  - Fixed OpenVDB signed distance field exterior signing using angle-weighted vertex pseudonormal interpolation, eliminating false negative cavities along sharp geometric edges and corners.
  - Implemented dynamic narrow band allocation $\max(4.0, \lceil \text{wall}/h \rceil + 4.0)$ preventing narrow band boundary leaks on thick hollow shells.
  - Replaced high-memory raster grid splits with instantaneous analytic half-space signed distance field intersections, delivering sub-second, 100% watertight plane splits with keyed alignment pins and sockets.
  - Fixed CCW vertex winding on primitive cylinder caps and cone side triangles.

## [1.7.7-beta] - 2026-11-06

### Fixed

- **Critical Viewport & Per-Frame Performance Optimization ($O(1)$ Quad Metric Caching):**
  - Resolved severe viewport and application lag when meshes reached 500k+ polygons caused by un-cached quad topology analysis executing every single frame in the UI status bar.
  - Implemented thread-safe `OnceLock` lazy caching for quad and triangle counts and diagonal sets on `Mesh`, reducing per-frame status bar overhead from ~60–300ms down to $0\,\text{ns}$ ($O(1)$).
  - Optimized viewport GPU mesh synchronization (`ViewportRenderer::sync`) to bypass diagonal lookups and massive `HashSet` allocations for pure triangle meshes, rendering line lists via direct edge pushes (<1ms).
  - Replaced quadratic `HashMap` edge pairing in `find_quad_diagonals` with a cache-friendly, multi-threaded Rayon sorted edge list (`par_sort_unstable_by_key`), reducing initial quad analysis time by over 50%.
  - Added corner angle checks to prevent equilateral triangle diamonds on subdivided meshes (e.g., icospheres) from being falsely detected and reported as quads.

## [1.7.6-beta] - 2026-11-06

### Fixed

- **Grab Brush Symmetry Blending ($C^1$ Smooth Maximum):**
  - Resolved harsh midline seam cutoff and crease artifacts when sculpting across symmetry mirror planes with the Grab brush.
  - Restored $C^1$ polynomial smooth maximum blending between overlapping symmetry dab influences, guaranteeing continuous, smooth transitions across the mirror plane.
- **Grab Brush Strength Slider Scaling:**
  - Resolved an issue where the strength slider had no effect on Grab brush displacement (brush always acted at 100% full displacement regardless of slider value).
  - Wired brush strength directly into linear delta displacement and inverted normal translation, properly scaling displacement from 0% to 100%.

## [1.7.5-beta] - 2026-11-06

### Added

- **Realtime Matcap Preview on Hover:**
  - Viewport material shading updates live as you mouse over built-in material thumbnails or custom imported matcaps in the shelf and View > Material menu, instantly reverting when the pointer leaves.
- **Dynamic Shift-Smooth Intensity Switching:**
  - Holding `Shift` to smooth dynamically switches the UI strength slider (in top controls, right-click quick menu, and brush settings) to control the Smooth brush's intensity directly on the fly, mirroring ZBrush behavior.
- **Quad Topology Preservation & Detection for OpenVDB Remeshing:**
  - OpenVDB dual contouring quads are now recognized as first-class quad topology rather than plain triangle soup.
  - Wireframe overlay (`Shift+W`) hides internal quad diagonals, rendering crisp, uniform quad grids across the model.
  - Status bar displays quad counts (e.g., `X quads`) for meshes with quad geometry.
  - OBJ exporter automatically groups paired triangles into native quad faces (`f a b c d`).
  - Saved `remesh_engine` preference across sessions in settings.

## [1.7.0-beta] - 2026-11-05

### Added

- **3D-Print Preparation Tools (`crates/oxy-topo/src/print3d.rs`, `Mesh > 3D Print Tools...`):**
  - **Scale & Dimensions:** Configurable real-world millimeters scale (`unit_scale`). Displays physical bounding dimensions ($W \times D \times H$ in mm), surface area ($cm^2$), and divergence theorem watertight volume ($cm^3$).
  - **Material Weight Estimators:** Automatically estimates photopolymer resin weight (~1.15 g/cm³) and PLA filament weight (~1.24 g/cm³) in grams.
  - **Watertight Manifold Check:** Topologically validates 2-manifold closed surface condition without boundary holes or non-manifold edges.
  - **Wall-Thickness Analyzer:** Parallel inward raycasting along negative normals measuring local wall thickness across all vertices. Flags fragile areas below the user-defined threshold and visualizes thin walls via a warning heatmap on the sculpt.
  - **OpenVDB Volumetric Hollowing:** Inward narrow-band level set erosion creating uniform hollow shells, with automatic cylindrical drain holes punched through the shell for resin and powder evacuation.
  - **Split into Interlocking Parts with Alignment Keys:** Planar cutting along X, Y, or Z axes producing Part A with male registration pins and Part B with matching sockets (+0.2mm clearance tolerance) for easy post-print assembly.

- **Tablet Input & Multi-Touch Gestures:**
  - **Continuous Stylus Pressure Pipeline:** Fixed issue where `CursorMoved` unconditionally reset pen pressure to 1.0 during active strokes. Fully supports graphics tablet drivers (Wacom, Huion, XP-Pen, Apple Pencil, Windows Ink, Linux XInput2/Wayland) with pressure sensitivity, `TouchpadPressure`, and analog axis events.
  - **Multi-Touch Gesture Layer:**
    - 1 finger: Sculpting or painting with pressure.
    - 2 fingers: Pinch-to-zoom, two-finger orbit and pan, and **two-finger tap for Undo**.
    - Trackpad gestures: Native `PinchGesture` zoom and `RotationGesture` camera roll.

- **Vertical Right-Click Quick Menu:**
  - Reorganized the quick menu at the cursor (right-click or held `Q`) from a wide horizontal bar into a sleek vertical card (175px width) featuring Raise/Carve selectors, vertical Size and Strength sliders, and brush-specific controls.

- **Crease Brush Pinch Parameter:**
  - Exposed a configurable `Pinch` slider (`0.0..=1.0`) in both the brush settings panel and the vertical quick menu, controlling how strongly groove walls are pulled together. Saved in brush presets.

- **Software Hardening & Safety Guards:**
  - **Autosave on Suspend / Blur:** Immediate non-blocking autosave when the window loses focus (`Focused(false)`) or when suspended by the OS.
  - **Corrupt File Import Guards:** Validates triangle vertex indices against vertex counts and rejects non-finite (`NaN`/`Inf`) coordinates across OBJ, STL, and PLY loaders.
  - **Subdivision Memory Ceiling:** Enforces safety ceilings (`max_triangles`) using saturating multiplications to prevent integer overflow or out-of-memory crashes on dense multires meshes.
  - **Topological Manifold Diagnostics:** Added `check_manifold` in `oxy-mesh::topo` detecting open boundary edges, non-manifold edges, and degenerate triangles.

## [1.6.1-beta] - 2026-11-05

### Fixed

- **Preferences Window Layout & Geometry:**
  - Resolved an issue where the Preferences dialog contents were rendered horizontally in a single cut-off row due to missing an explicit vertical layout inside the scroll area.
  - Removed fixed viewport centering anchor (`anchor(CENTER_CENTER)`), restoring proper window title bar positioning, smooth dragging, and standard close button accessibility.
  - Cleaned up tab navigation styling with dedicated selectable button items and tooltips.

## [1.6.0-beta] - 2026-11-05

### Added

- **OpenVDB Sparse Volumetric Grid & Voxel Remesher (`oxy-vdb`):**
  - Implemented Academy Software Foundation (ASWF) OpenVDB-style hierarchical sparse volumetric B-tree in pure, safe Rust.
  - Standard $8^3 = 512$ voxel leaf nodes with 512-bit active value masks (`BitMask512`) and linear spatial index-to-world transforms.
  - Narrow-band signed distance field (SDF) rasterization (`mesh_to_level_set`) with parallel triangle processing and normal-oriented winding.
  - Exterior topological flood-filling identifying and populating interior leaves (`-bg`), enabling solid manifold geometry without dense memory overhead.
  - Lifts the previous $320^3$ resolution ceiling; sculptors can now remesh up to 1024 or 2048 voxels with low memory usage.
  - Dual contouring surface extraction (`volume_to_mesh`) with built-in surface relaxation and zero false crossings against unallocated background.
  - **Adaptivity Slider:** Simplifies flat planar regions while retaining maximum polygon density in high-curvature sculpt areas.
  - **Engine Selector:** Toggle between high-performance `OpenVDB (Sparse)` (default) and legacy `Regular Grid` remeshing.

- **Volumetric CSG Booleans (`Mesh > Boolean (OpenVDB)`):**
  - Added full Constructive Solid Geometry Boolean tools operating directly on VDB signed distance fields:
    - **Boolean Difference:** Cleanly subtracts a cutter piece from the active sculpt piece.
    - **Boolean Union:** Seamlessly welds two pieces into one watertight manifold surface without internal geometry or seam cracks.
    - **Boolean Intersection:** Retains only the common overlapping volume of two pieces.
  - Fully immune to non-manifold edges, self-intersecting polygons, and co-planar face errors.
  - Runs asynchronously off the main thread with full compound undo/redo history.

- **Unified Preferences Window System (`Ctrl+,` or `Edit > Preferences…`):**
  - Replaced the large Settings dropdown menu with an industry-standard, multi-tab floating Preferences dialog.
  - Tabs for **General** (UI scale, layout, autosaver), **Appearance** (theme, accent color, viewport background, wireframe), **Plugins**, **Keymap** (presets, search, interactive chord remapping), **AI Agent** (status, model, endpoint), and **Updates**.

- **Top Menu Bar Reorganization:**
  - Streamlined top menu bar into 8 clean, consistent categories: `File`, `Edit`, `Pieces`, `Mesh`, `Groups`, `Mask`, `View`, `Help`.
  - Moved Preferences to `Edit > Preferences…` (`Ctrl+,`) and relocated plugin management into Preferences.

- **Level Set Filtering & Shelling:**
  - Added volumetric level set dilation/erosion offset (`offset`), hollow wall shelling for 3D printing (`shell`), and 3D Laplacian smoothing directly on the narrow-band SDF (`smooth_laplacian`).

### Fixed

- **Grab Brush Symmetry Overlap Creasing & Boundary Tearing:**
  - Resolved circular boundary tears and midline creasing when grabbing across or near symmetry planes.
  - Replaced singular radial derivative blending with $C^2$ cubic directional weighting ($\omega_i = w_i^3$) and maximum envelope blending, guaranteeing zero derivative boundaries ($\frac{d}{dw_1} = 0, \frac{d^2}{dw_1^2} = 0$) and seamless symmetry transitions.

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

[Unreleased]: https://github.com/crisprintsstuff/OxyBrush/compare/v3.0.0-beta...HEAD
[3.0.0-beta]: https://github.com/crisprintsstuff/OxyBrush/compare/v2.5.7-beta...v3.0.0-beta
[2.5.7-beta]: https://github.com/crisprintsstuff/OxyBrush/compare/v2.5.6-beta...v2.5.7-beta
[2.5.6-beta]: https://github.com/crisprintsstuff/OxyBrush/compare/v2.5.5-beta...v2.5.6-beta
[2.5.5-beta]: https://github.com/crisprintsstuff/OxyBrush/compare/v2.5.0-beta...v2.5.5-beta
[2.5.0-beta]: https://github.com/crisprintsstuff/OxyBrush/compare/v2.0.0-beta...v2.5.0-beta
[2.0.0-beta]: https://github.com/crisprintsstuff/OxyBrush/compare/v1.7.7-beta...v2.0.0-beta
[1.7.7-beta]: https://github.com/crisprintsstuff/OxyBrush/compare/v1.7.6-beta...v1.7.7-beta
[1.7.6-beta]: https://github.com/crisprintsstuff/OxyBrush/compare/v1.7.5-beta...v1.7.6-beta
[1.7.5-beta]: https://github.com/crisprintsstuff/OxyBrush/compare/v1.7.0-beta...v1.7.5-beta
[1.7.0-beta]: https://github.com/crisprintsstuff/OxyBrush/compare/v1.6.1-beta...v1.7.0-beta
[1.6.1-beta]: https://github.com/crisprintsstuff/OxyBrush/compare/v1.6.0-beta...v1.6.1-beta
[1.6.0-beta]: https://github.com/crisprintsstuff/OxyBrush/compare/v1.5.1-beta...v1.6.0-beta
[1.5.1-beta]: https://github.com/crisprintsstuff/OxyBrush/compare/v1.5.0-beta...v1.5.1-beta
[1.5.0-beta]: https://github.com/crisprintsstuff/OxyBrush/compare/v1.0.0-beta...v1.5.0-beta
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

# Changelog

All notable changes to OxyBrush. The format is based on [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

### Changed

- **OxyRemesh.** Open surfaces can use isoline extraction from fitted orientation-field charts when the charts are unfolded and cover enough of the surface.

### Fixed

- **Brush presets** ([#605](https://github.com/crisprintsstuff/OxyBrush/issues/605)). Right-click a preset to open its actions within the library. Each row also has an actions button for sharing, renaming, duplicating and deleting presets.
- **Curve cutters** ([#598](https://github.com/crisprintsstuff/OxyBrush/issues/598)). Plane Clip, Line Split and Cut Away follow a multi-point Curve. Choose Line to draw a straight cut.

- **Scene Project Stamp.** Dragging to resize the stamp reuses the target meshes for the gesture, reducing preview work on dense scenes.

- **Alpha Roll and curve strokes.** The Alpha section stays open when its selected alpha changes. Roll distance applies a typed value when editing finishes. Drag the Curve stroke or Curve Move title bar to see the curve before accepting.
- **Sculpt Layers.** Sculpt and paint strokes stay editable when switching or adding subdivision levels. Strength changes affect fine geometry and color even from a lower level and survive Undo, Redo, and saving.

## [4.1.2] - 2026-11-26

### Fixed

- **OxyRemesh Swiss cheese** ([#551](https://github.com/crisprintsstuff/OxyBrush/issues/551), [#586](https://github.com/crisprintsstuff/OxyBrush/pull/586)). Closed inputs no longer get replaced by open/holey extracts. Dual extract densifies unit edges, collapses short dual edges / 3-cycles, fills short polygon holes, and seals remaining gaps (with a closed voxel remesh fallback when the field shell cannot enclose). After seal, keep the largest island so a watertight-but-shattered result is never returned as Ok. Isoline extract stays gated until coverage is good; full Autoremesher-quality parameterization remains RFC 0044 Phase 4.

### Fixed

- **Split view.** Each pane redraws with its own camera, accepts sculpt and navigation input, and displays references for its own view.
- **Poly Edit selection.** Box, Lasso, and Ellipse show an outline during the drag.

## [4.1.1] - 2026-11-26

### Changed

- **Inspector declutter.** Mode-aware settings panel (hides Brush/Stroke/Alpha during Poly Edit, Retopo, OxySpheres and Tube); Brush options and Alpha options collapse by default; Topology starts collapsed while sculpting; Project detail and Decimate live under Mesh (Keep % sits next to Decimate there). New **Focus** layout (Pieces + Brush only). Settings panel width is remembered. Builds on the Topology section headers from [#575](https://github.com/crisprintsstuff/OxyBrush/issues/575).
- **Topology panel layout** ([#575](https://github.com/crisprintsstuff/OxyBrush/issues/575)). Voxel remesh and OxyRemesh each have their own section header; Shell/engine and OxyRemesh advanced knobs sit in collapsed option rows.

### Fixed

- **OxyRemesh extraction** ([#551](https://github.com/crisprintsstuff/OxyBrush/issues/551)). Face walks no longer burn dual edges on failed cycles (which left empty Degenerate results). Collapse classification follows Instant Meshes §3.8 more closely; low-support dual vertices are filtered again; when a clear majority surface exists, floating islands are stripped instead of failing the whole job.

## [4.1.0] - 2026-11-26

### Added

- **Poly Edit Select: Ellipse and rename.** The area row is labeled **Select** (was Pick) and offers **Ellipse** beside Click, Box and Lasso. Drag shows the outline; hold **Shift** on Ellipse for a circle. [RFC 0035](docs/rfcs/0035-poly-edit-area-selection.md)
- **Scale cover Phase 2** ([RFC 0046](docs/rfcs/0046-scale-cover.md)). **Flow** orientation follows the piece's longest bounds axis so scale rows share body grain; scatter can stay inside a **mask** or **face group**; **Types by face group** applies several plate kinds in one step. Surface instances gain the same placement/orient options (`INS3` for scatter-on-mask/group).
- **OxySpheres quad Resolution and Blend** ([#477](https://github.com/crisprintsstuff/OxyBrush/issues/477), [RFC 0038](docs/rfcs/0038-oxyspheres-quad-base-mesh.md)). Quad base mesh gets the same **Resolution** and **Blend** sliders as voxel skin: Resolution densifies rings on long limbs (160 keeps one ring per sphere); Blend softens three-way joins. Subdivision is unchanged.
- **MCP `set_geometry`** ([RFC 0053](docs/rfcs/0053-mcp-geometry-write.md), [#555](https://github.com/crisprintsstuff/OxyBrush/issues/555)). Python / local agents can write vertex colors, positions (displacement) and mask from an `OXYG` file (same format as `get_geometry`), as one undo step.
- **Also paint** ([RFC 0052](docs/rfcs/0052-also-paint.md)). On sculpt brushes (Draw, Clay, …), turn on **Also paint** in brush settings to lay the Paint color under each dab while you sculpt. Color Jitter and the color picker apply; Alt still carves. Grab, cutters and Scene Project are not included yet.
- **Split view** ([RFC 0054](docs/rfcs/0054-split-view.md)). View > Split view: Off, Left / right, or Top / bottom. Each pane has its own camera; click a pane to sculpt or orbit there; drag the divider to resize.
- **Project detail** ([RFC 0051](docs/rfcs/0051-detail-reproject.md), [#516](https://github.com/crisprintsstuff/OxyBrush/issues/516)). Mesh > Project detail (also Rebuild and the command palette) snaps the active piece onto the other visible pieces by closest point, so a remesh or cage recovers a sculpt's surface without changing triangles. Mesh > Project onto… picks one source. Background job; one undo; subdivision levels are removed.
- **Show remesh density** ([RFC 0050](docs/rfcs/0050-remesh-density-preview.md), [#552](https://github.com/crisprintsstuff/OxyBrush/issues/552)). Topology checkbox draws a grid of the current Remesh voxel / OxyRemesh cell size on the active piece before you rebuild.
- **Smooth preview outside Poly Edit** ([RFC 0048](docs/rfcs/0048-dynamic-smooth.md), [#447](https://github.com/crisprintsstuff/OxyBrush/issues/447)). Topology shows the same Catmull–Clark smooth surface as Poly Edit's Smooth > Preview (Off / 1 / 2 / 3) while you keep editing the low-polygon cage; **Subdivide (smooth)** applies it. Creases still shape the result.
- **Favorites and preset editor** ([RFC 0049](docs/rfcs/0049-custom-brush-shelves.md)). Star brushes for a Favorites strip on the shelf; star presets so they sort first; rename and duplicate saved presets; Ctrl+K / Space find `Preset: …` entries.
- **Alpha Roll, Tile and rotate jitter** ([RFC 0047](docs/rfcs/0047-alpha-pack-tile-rotate.md)). **Roll** scrolls a tileable alpha along the stroke (paint-roller); **Roll distance** stretches or compresses it. **Tile** wraps the pattern inside each dab; **Repeats** sets density. **Rotate jitter** (per stroke or per dab) turns the alpha randomly without Spray. Built-in **Scale strip** is a tileable U-scale row for rolling along limbs.
- **Scale cover** (Dino mode). Tools > Scale cover… places overlapping scale plates on the active piece using surface instances: built-in teardrop, diamond, round, plate, ridge and spike meshes (procedural, original), scatter / mask / face-group placement, size, spacing, lift, variation, seed and orient. Apply creates or updates the cover; Edit source and Make real reuse the Instances ops. [RFC 0046](docs/rfcs/0046-scale-cover.md)

### Changed

- **App icon.** Window, taskbar, installer and Flatpak icons use the new cyan/magenta circuit-brush mark (`packaging/icons`, regenerated with `make_icons.py`).
- **Orbit past ±90°.** Dragging the view up or down no longer stops just short of straight above or below; the camera tumbles through and can go upside down.
- **Accumulate defaults.** Draw, Clay, Inflate and Clay Strips start with Accumulate **on** again so overlapping dabs build like clay ([#553](https://github.com/crisprintsstuff/OxyBrush/issues/553), [#554](https://github.com/crisprintsstuff/OxyBrush/issues/554)). Turn it off for a flat cap within one stroke.
- **Document tabs are labeled Projects.** Menus, keymap group (**Projects**), shortcuts help and dialogs say **project** instead of **tab** (e.g. New project, Close this project). Keyboard Tab and Preferences category tabs are unchanged.
- **Recipe Box is a movable window.** Open from View > Recipe Box; drag it anywhere; close with ×. Starts hidden; visibility is remembered (`recipe_box` in settings). The fixed edge **RECIPES** tab is gone.
- **OxyRemesh faces** slider goes to **100 000** (was capped at 50 000 in the UI while the job already allowed more).
- **Scale alphas.** Rebuilt the procedural **Scales** stamp as overlapping U-plates, and added **Snake scales**, **Diamond scales**, and a single **Scale plate** for Stamp strokes (all original CC0). [RFC 0047](docs/rfcs/0047-alpha-pack-tile-rotate.md)

### Fixed

- **Wireframe on Add Primitive preview** ([#568](https://github.com/crisprintsstuff/OxyBrush/issues/568)). The live preview for + Add (and Insert Primitive) shows polygon edges so you can see divisions while adjusting, without turning on the global wireframe.
- **OxySpheres Make mesh feedback** ([#477](https://github.com/crisprintsstuff/OxyBrush/issues/477)). When Make mesh adds a new piece instead of updating the last one, the status bar says why (sculpted, painted, masked, levels, …). Keep spheres tooltips make clear it does not change meshes. A failed Quad skin build says existing meshes were not changed.
- **OxyRemesh no longer replaces the mesh with floating shards** ([#551](https://github.com/crisprintsstuff/OxyBrush/issues/551)). When face extraction would produce disconnected islands, the job fails with a clear status and the piece stays as it was (use Voxel remesh / Ctrl+R meanwhile). Full field-extraction hardening remains on the RFC 0044 track.
- **Poly Edit pick on undo.** Undoing (or redoing) a Poly Edit cage edit restores the face/edge/point pick that belonged to that mesh topology, so the selection no longer disappears after Undo.

## [4.0.0] - 2026-11-25

First stable release (drops the `4.0.0-beta.*` suffix).

### Added

- **Surface Noise.** Tools > Surface Noise… (also the command palette) opens a window that displaces the active piece along its normals with procedural noise: Scale, Strength, Octaves, Seed, and Smooth / Turbulent / Cellular patterns. Preview updates as you change the controls; Apply bakes one undo step. Closing without Apply restores the piece. Masked parts stay put unless Use mask is off. [#541](https://github.com/crisprintsstuff/OxyBrush/issues/541), [RFC 0045](docs/rfcs/0045-surface-noise.md)
- **OxyRemesh.** Mesh > OxyRemesh (also the Rebuild panel and command palette) rebuilds the active piece as a lighter **pure-quad** mesh with field-aligned edges (Instant Meshes research, pure Rust). Carries vertex color, mask and face groups, and snaps onto the source surface. Adaptivity and anisotropy knobs, optional guide from the last freehand stroke, and optional singularity attractor at the mask centroid. Runs in the background; one undo step. [#529](https://github.com/crisprintsstuff/OxyBrush/issues/529), [RFC 0044](docs/rfcs/0044-auto-retopology.md)
- **Amplify brush.** Exaggerates existing bumps and recesses relative to the dab's mean plane. Alt reduces that contrast without smoothing the form away. [#519](https://github.com/crisprintsstuff/OxyBrush/issues/519)
- **Scene Project brush.** Projects the surface under the brush onto other visible pieces in the scene (interactive shrinkwrap). Alt projects along the surface normal instead of the view. [#295](https://github.com/crisprintsstuff/OxyBrush/issues/295)
- **Cutter shapes for Plane Clip, Line Split and Cut Away.** With those brushes selected, the shelf shape row (and **L**) picks Curve (the usual two-point cutting line) or a rectangle, lasso, circle or ellipse. Closed shapes flatten, assign a new polygroup, or trim the region they enclose; Alt flips inside/outside. [#492](https://github.com/crisprintsstuff/OxyBrush/issues/492)
- **Poly Edit Box and Lasso pick.** In Faces, Edges or Points mode, Pick offers Click, Box and Lasso. Drag a rectangle or freehand outline to pick visible elements; plain drag replaces the pick, Ctrl/Cmd adds, Alt subtracts, Escape cancels. [#299](https://github.com/crisprintsstuff/OxyBrush/issues/299), [RFC 0035](docs/rfcs/0035-poly-edit-area-selection.md)
- **Color Filter.** Tools > Vertex color > Color Filter… (also the command palette) opens a window of whole-piece vertex color adjustments: Fill, Hue, Saturation, Value, Brightness, Contrast, Smooth and Red/Green/Blue. Drag a bar to preview; let go to apply as one undo step. Masked parts stay put unless Use mask is off. Fill uses the current Paint color. [#489](https://github.com/crisprintsstuff/OxyBrush/issues/489)
- **Rebuild lower level.** Mesh > Rebuild lower level recovers one Catmull–Clark coarse level under an all-quad mesh with no subdivision stack (RFC 0036, #366, #455).
- **Replay last stroke.** Edit > Last stroke > Replay stamps the Freehand stroke you just made again, without undoing it. One undo step; available until remesh or a new freehand stroke. Edit as curve still reshapes the path instead.
- **3MF export.** File > Export type offers 3MF: visible pieces as separate mesh objects in millimetres, using the scene unit size (Preferences > Units / 3D print). [#308](https://github.com/crisprintsstuff/OxyBrush/issues/308)
- **Reference view planes.** Assign Back, Left, and Bottom orientations in addition to Front, Right (formerly Side), and Top. [#514](https://github.com/crisprintsstuff/OxyBrush/issues/514)

### Changed

- **X-ray scope remembered.** The X-ray piece scope (all visible / active / inactive) is saved in settings and restored on launch. New installs default to inactive pieces. [#521](https://github.com/crisprintsstuff/OxyBrush/issues/521)
- **Banner and app icon.** The README banner uses the new Oxytocin Labs artwork (cyan/magenta circuit-brush mark with the dragon sculpt). Window, taskbar and installer icons match that mark (`packaging/icons`, regenerated with `make_icons.py`).
- **Focal shift in the toolbar.** The Size / Strength bar (toolbar or floating over the view) also has Focal shift, same as the quick menu and brush settings.

### Fixed

- **Poly Edit window.** The window is resizable and scrolls when the content is taller than the window, so every control stays reachable on a short display (same idea as Deform).
- **Paint strength.** Strength sets how opaque a stroke is, without overlapping dabs stacking past that value. Half strength looks about half painted. [#513](https://github.com/crisprintsstuff/OxyBrush/issues/513)
- **Viewport shading.** Guard against invalid ambient occlusion values when cavity is enabled on its own. Skip screen-space shading when X-ray fades every visible piece.

## [4.0.0-beta.24.7] - 2026-11-24

### Added

- **Reference placement.** See position, width, and opacity update in the viewport as you adjust them. Rotate images in 90° steps or flip them horizontally and vertically. Apply records one undoable edit. [#498](https://github.com/crisprintsstuff/OxyBrush/pull/498), [#499](https://github.com/crisprintsstuff/OxyBrush/pull/499)
- **Reference landmark alignment.** Match two image landmarks to two points on the sculpt. Preview the position, rotation, and scale. Review or repick individual landmarks before applying the fit. [#506](https://github.com/crisprintsstuff/OxyBrush/pull/506), [#507](https://github.com/crisprintsstuff/OxyBrush/pull/507)
- **Viewport X-ray.** Toggle X-ray from the viewport toolbar or View menu. Fade just the active piece to inspect the opaque meshes behind it. Right-click the toolbar button for opacity and scope. [#501](https://github.com/crisprintsstuff/OxyBrush/pull/501)
- **Orthographic grids.** The viewport grid faces the camera in snapped front, side, and top orthographic views. [#510](https://github.com/crisprintsstuff/OxyBrush/pull/510)

### Changed

- **Paint smoothing.** Hold Shift with Paint to blend vertex colors while preserving geometry. [#510](https://github.com/crisprintsstuff/OxyBrush/pull/510)
- **Visibility shortcuts.** Assign Grow Visible and Shrink Visible in the Keymap editor. [#510](https://github.com/crisprintsstuff/OxyBrush/pull/510)

### Fixed

- **Piece isolation and navigation.** Show Only follows the active piece and restores prior visibility when switched off. Use Up and Down after focusing a Pieces row. [#510](https://github.com/crisprintsstuff/OxyBrush/pull/510)
- **OxySpheres quad rebuilds.** Rebuilding a hidden generated mesh reveals it. Mirrored limbs use matching triangulation diagonals for more consistent shading. Keep spheres saves the armature on its own. [#509](https://github.com/crisprintsstuff/OxyBrush/pull/509)

## [4.0.0-beta.24.6] - 2026-11-24

### Added

- **References and viewport X-ray.** Embed front, side, and top PNG or JPEG references in a project. Fade all visible meshes or just inactive pieces, or draw the image as an overlay. [#493](https://github.com/crisprintsstuff/OxyBrush/pull/493), [#495](https://github.com/crisprintsstuff/OxyBrush/pull/495)
- **Selection shapes.** Hold Ctrl or Ctrl+Shift to choose rectangle, lasso, circle, ellipse, or curve beside the brush shelf. [#495](https://github.com/crisprintsstuff/OxyBrush/pull/495)

### Changed

- **Kelvin brushes.** Grab localizes small pulls and blends into a broader elastic response for longer drags. Alt pulls along the normal. Twist uses sideways drag and Scale uses vertical drag. Alt reverses Twist and Scale. [#495](https://github.com/crisprintsstuff/OxyBrush/pull/495), [#496](https://github.com/crisprintsstuff/OxyBrush/pull/496)
- **Retopology.** Open and edit existing low-poly meshes in Retopology. [#490](https://github.com/crisprintsstuff/OxyBrush/pull/490)
- **OxySpheres.** Subdivide the Quad base mesh output. [#485](https://github.com/crisprintsstuff/OxyBrush/pull/485)

### Fixed

- **Brush strength.** Switching brushes preserves their current strength values. [#486](https://github.com/crisprintsstuff/OxyBrush/pull/486)
- **Custom material color.** The color picker opens inside the Materials panel. [#484](https://github.com/crisprintsstuff/OxyBrush/pull/484)

## [4.0.0-beta.24.5] - 2026-11-24

### Added

- **Paint Color Jitter.** Vary hue, saturation, and value per stroke or per dab. Symmetry copies share a color sample. Presets save the settings, and editable strokes keep their seed for replay. [#482](https://github.com/crisprintsstuff/OxyBrush/pull/482)

### Changed

- **Lit surface controls in Materials.** Choose Matcap or Custom, and adjust the surface color, roughness, and specular from Materials. Picking a matcap activates it under viewport lights. [#479](https://github.com/crisprintsstuff/OxyBrush/pull/479)

### Fixed

- **OxySpheres quad build feedback.** Make mesh shows rejected quad builds beside the button and keeps the armature open for adjustments or a switch to Voxel skin. [#479](https://github.com/crisprintsstuff/OxyBrush/pull/479)

## [4.0.0-beta.24.4] - 2026-11-23

### Added

- **Delete and Dissolve in Poly Edit.** **Delete** removes the faces touching the selected edges, opening a hole or trimming an existing border. **Dissolve** removes the edges and merges the adjacent faces. [#473](https://github.com/crisprintsstuff/OxyBrush/pull/473)

### Changed

- **Accumulate defaults.** Draw, Clay, and Inflate start with Accumulate off. The Soft clay buildup preset and Clay Strips start with it on. Saved brush settings, presets, and recorded strokes keep their behavior. [#474](https://github.com/crisprintsstuff/OxyBrush/pull/474)

### Fixed

- **Mirrored Knife cuts.** With Mirror across X enabled, the Knife also cuts matching faces on the other side. Mirrored cuts share a single undo step. [#471](https://github.com/crisprintsstuff/OxyBrush/pull/471)
- **Smoother captured alphas.** Capture from view samples the silhouette at subpixel positions to reduce jagged outlines while preserving the interior depth detail. [#471](https://github.com/crisprintsstuff/OxyBrush/pull/471)

## [4.0.0-beta.24.3] - 2026-11-23

### Added

- **Per-brush Accumulate.** Draw, Inflate, Clay, and Clay Strips have an **Accumulate** checkbox just below **Auto smooth**. It starts on. Turn it off to limit each point to the strongest dab within one stroke. Start another stroke to build further. Presets and remembered brush settings keep the choice. [#469](https://github.com/crisprintsstuff/OxyBrush/pull/469)
- **Experimental OxySpheres quad base mesh.** Choose **Output > Quad base mesh** before clicking **Make mesh** to get a low-density cage with edge loops along a chain or a single three-way junction. **Voxel skin** is still the default. If the arrangement is unsupported, the armature stays open so you can adjust it or switch outputs. [#465](https://github.com/crisprintsstuff/OxyBrush/pull/465)

### Fixed

- **OxySpheres updates the correct piece.** Make mesh tracks the piece it generated across reordering and save/reopen. Identical duplicates stay independent, and painted, masked, or partly hidden meshes are preserved. Older projects without a saved mesh reference create a new piece on their first regeneration. [#463](https://github.com/crisprintsstuff/OxyBrush/pull/463)

## [4.0.0-beta.24.2] - 2026-11-23

### Changed

- **OxySpheres: Make mesh updates the last mesh while it's unsculpted.** Change the spheres and Make mesh again: the mesh they made last is updated instead of a new "mesh 2, 3, 4…", as long as you haven't sculpted it; once you have, a new piece keeps your sculpting. One undo step either way.
- **Lighter inactive pieces only while you edit** ([#451](https://github.com/crisprintsstuff/OxyBrush/issues/451)). With the option on, the other pieces with subdivision levels are drawn at their lowest level while you sculpt or edit a piece (a stroke, Paint Groups, a gizmo or Poly Edit drag), and in full again as soon as you stop. Only the drawing changes: a lowest-level copy is kept next to each piece, so nothing is rebuilt at the end of a stroke, and every piece keeps its levels for saving, export and undo. In 4.0.0-beta.24 the pieces dropped to their lowest level for as long as they weren't active. Projects saved with pieces shown lighter still open with their own levels.

### Fixed

- **OxySpheres: thin limbs come out round.** The mesh was sized by the whole figure, so a thin limb got two or three voxels across and came out jagged even at the highest Resolution. Only the space near the surface is sampled now, finely enough for the thinnest sphere. (The quick view while editing is still sized by the figure.)
- **Knife inset loops keep a polygon rim.** Closed cuts inside a face use distinct corner joins, so extra points follow the rim polygons instead of creating a fan of triangles.

## [4.0.0-beta.24] - 2026-11-23

### Added

- **Knife: cut a face out of a face** ([#450](https://github.com/crisprintsstuff/OxyBrush/issues/450)). Click points inside one face, without touching its edges, then click the first point again: the loop becomes a new face, and the rest of the face becomes a ring of faces joining the loop to the face's corners (a square loop in a quad gives a quad with four quads round it, as an inset does). A loop that crosses itself is refused.
- **Paint the pick in Poly Edit** ([#442](https://github.com/crisprintsstuff/OxyBrush/issues/442)). Ctrl/Cmd+drag picks every face, edge or point the pointer passes over; started on a picked one, it takes them out instead. A Ctrl/Cmd+click still picks or unpicks one.
- **Curve selection as a pen path** ([#446](https://github.com/crisprintsstuff/OxyBrush/issues/446)). Drag from the first point and let go to apply (clicking points and pressing Enter still works). Tap Alt while drawing to add a point the line bends smoothly through, and tap again quickly to make it a sharp corner. Hold Alt as you apply for the other side, as before.
- **Ctrl+Shift+Alt+click hides a face group** ([#435](https://github.com/crisprintsstuff/OxyBrush/issues/435)), the opposite of Ctrl+Shift+click, which shows only that group.

### Changed

- **OxySpheres are a piece of their own, never used up** ([RFC 0034](docs/rfcs/0034-oxyspheres.md)). **Make mesh** adds a new piece made from the spheres and keeps the spheres as an OxySpheres piece (sphere icon in the Pieces list, hidden out of the mesh's way), so you can change them and make another mesh any time, however much you've sculpted the first. **Done** keeps the spheres without making a mesh. Click the OxySpheres piece, or use Pieces > Edit OxySpheres…, to open it again. Before, the spheres were kept on the mesh and lost once it was sculpted. Projects from 4.0.0-beta.23.3 open with those spheres back as an OxySpheres piece.
- **Lighter inactive pieces show their lowest level** ([#451](https://github.com/crisprintsstuff/OxyBrush/issues/451)). They used to show the highest level under a triangle budget (500,000), so an ordinary piece didn't change and the option seemed to do nothing. The budget setting is gone.

### Fixed

- **Knife cuts over an edge onto the next face reach the point you clicked.** A cut across a cap (an 8- or 12-sided face, say) and down a side often stopped at the rim, because progress along the cut was measured along the straight line between the two clicks in space instead of along the line drawn on screen. Where the cut carries on past a point clicked inside a face to that face's edge, the preview now draws that stretch dashed.
- **New OxySpheres start on the symmetry plane** ([#449](https://github.com/crisprintsstuff/OxyBrush/issues/449)). With X symmetry on, the first sphere, and so the mirror plane, was at the view's orbit point, off centre once the view had moved.
- **Shift with Paint Groups smooths jagged group borders** ([#440](https://github.com/crisprintsstuff/OxyBrush/issues/440)). It only changed the group of whole faces, which left the stair-stepped edges of the triangles; now the points along each border slide into a smooth line and the points around even out, as Deform's "Polish, keep group edges" does. Points move along the surface; one undo step puts back both the points and the groups.
- **The brush ring shows while you resize from the quick menu** ([#433](https://github.com/crisprintsstuff/OxyBrush/issues/433)), at the place the menu was opened, as it does with S.
- **macOS: the Plugins menu lists your plugins from the start** ([#448](https://github.com/crisprintsstuff/OxyBrush/issues/448)). It stayed empty until Settings > Plugins was opened.

## [4.0.0-beta.23.3] - 2026-11-22

### Added

- **OxySpheres** ([#437](https://github.com/crisprintsstuff/OxyBrush/issues/437), [RFC 0034](docs/rfcs/0034-oxyspheres.md)). Block out a figure or creature with a tree of spheres, then turn it into one smooth mesh to sculpt. Pieces > + Add > **OxySpheres…** opens one sphere; drag from a sphere to pull out a new one, **Shift+drag** to move one, **Ctrl+drag** to resize it, click a link to add a sphere on it, and **Alt+click** or **Delete** to remove a sphere with the ones below it. With Mirror across X (on with X symmetry), both sides change together. The viewport shows the mesh as it will be. **Make mesh** (Enter) adds it as a new piece in one undo step; **Resolution** sets how fine it is and **Blend** how round the joins are. **Pieces > Edit OxySpheres…** reopens the spheres while the mesh hasn't been sculpted, remeshed or subdivided since. Projects keep the spheres in a new optional `ARMS` section that older versions skip.

### Changed

- **Poly Edit edge Bridge joins edges, not whole holes.** Click a border edge, then another: one quad joins them (Rows for more). Pick a row of border edges on each side and click one of each for a strip of quads; picking two whole hole borders (Ctrl/Cmd+double-click) still makes a tube. Before, any two edges bridged the whole holes they were on.

### Fixed

- **The Knife always finishes a cut.** A cut that started or ended inside a face (not on an edge) dropped those ends, so a cut between two neighbouring faces, or within one face, cut nothing. Ends inside a face now carry on to that face's edge. A click on the last point again also cuts, so a pen alone can finish (as well as Enter and Cut).

## [4.0.0-beta.23.2] - 2026-11-22

### Added

- **Circle, ellipse and curve selections** for masking and visibility (**L** or the Mask panel's Selection row). A circle drags out from its centre and an ellipse from corner to corner; with either, Ctrl+drag draws it wherever it starts. A curve is clicked point by point and applied with Enter: everything on one side of it is masked or shown (Alt: the other side). Symmetry, Alt after the drag, and Space to move the shape work as with the rectangle and lasso.
- **Poly Edit Knife** ([#370](https://github.com/crisprintsstuff/OxyBrush/issues/370), [RFC 0033](docs/rfcs/0033-poly-edit-knife.md)). Turn on **Knife** in the Poly Edit window and click points on the model to draw a cut; clicks snap to corners and edges. Enter cuts: the path follows the visible surface, adds a point on every edge it crosses and splits every face it crosses, as one undo step. Backspace takes the last point back, Esc cancels, and clicking the first point again cuts a closed loop.
- **Poly Edit To Circle** ([#370](https://github.com/crisprintsstuff/OxyBrush/issues/370)). Round the picked points, edges or face corners toward a fitted circle. Amount controls how far they move, and the edit is one undo step.
- **Per-brush pressure curves** ([#426](https://github.com/crisprintsstuff/OxyBrush/issues/426)). Stroke shows strength and size curves. Customize either one for the active brush, or let it follow the defaults in Pen & tablet. Brush presets keep the overrides.
- **Alpha library** ([#425](https://github.com/crisprintsstuff/OxyBrush/issues/425)). Imported PNG alphas are available in every tab and after a restart. Projects still embed the alphas imported into them and the ones their editable strokes use.
- **Mirror duplicate axes** ([#427](https://github.com/crisprintsstuff/OxyBrush/issues/427)). Choose X, Y or Z under **Pieces > Mirror duplicate** or **Mirror duplicate linked**. All six commands are also in the command palette.
- **Lighter inactive pieces** (Preferences > General, off by default): a piece with subdivision levels that you aren't working on is shown at its highest level under a triangle budget (500,000 by default, adjustable), which frees its full-detail mesh: about 66 bytes a triangle, plus the graphics card's copy. Picking it brings its own level back, exactly as it was, in the background (about a second for a 6-million-triangle piece; leaving it takes about a quarter of a second). Export, Join, Merge, Booleans, copy, the Blender bridge, agents, plugins and undo always see the full level. Pieces tied to others (linked copies and their originals, surface instance hosts and sources, pieces with living strokes) stay in full. The Pieces list marks lighter pieces with `~` before their triangle count. A project saved while pieces are lighter records each one's own level in a new optional `LITE` section, so it comes back after reopening; older versions skip it and open the piece at the lighter level, with every level still there.

### Fixed

- **Every slider follows the theme.** Seam blend (Booleans), the surface instance settings (Lift, Turn, Vary size, Vary turn, mask threshold), the Recipe Box move panel (Twist, Size) and the interface tint sliders (hue, intensity, brightness) were plain sliders, so with Studio or Sculptor controls they didn't match the rest.
- **Undo can no longer scramble a mesh rebuilt outside the history.** A stroke's undo step only fits the mesh it was recorded on; a piece rebuilt meanwhile without an undo step (a linked copy after its original was remeshed) now skips that step instead of moving the wrong points.

## [4.0.0-beta.23] - 2026-11-22

### Added

- **Linked copies** ([#327](https://github.com/crisprintsstuff/OxyBrush/issues/327)). Pieces > **Duplicate linked** and **Mirror duplicate linked** make a copy that follows its original: sculpt, paint or remesh the original and the copy changes with it, live (sculpt one ear, its mirror image follows). Move, turn or scale the copy with the transform gizmo and it stays linked in its new place; **Unlink copy** makes it an ordinary piece. Copies can't be sculpted themselves. Projects keep the link in a new optional `LINK` section; older versions open the copy as an ordinary piece.
- **Settings per brush** ([#305](https://github.com/crisprintsstuff/OxyBrush/issues/305)). Each brush keeps its own falloff, focal shift, spacing, stroke mode, stabilizer, auto smooth, spray, pressure switches and **alpha**, and gets them back when you pick it again; strength and gravity were per brush already. Size stays shared unless **Size per brush** is on. Both are in Preferences > General, and Remember brush settings between sessions keeps every brush's settings.
- **Separate pressure curves for strength and size** ([#305](https://github.com/crisprintsstuff/OxyBrush/issues/305)): Preferences > Pen & tablet has a Strength curve and a Size curve, so a light touch can make a thin line at full strength, or the other way round. Living-stroke recipes save both (recipe version 4; older recipes replay as before).
- **Seam blend for Booleans** ([#285](https://github.com/crisprintsstuff/OxyBrush/issues/285)): the seam where two pieces meet is rounded over a few voxels (2 by default; 0 keeps a sharp crease), in the Boolean row.
- **Duplicate keeps subdivision levels** ([#366](https://github.com/crisprintsstuff/OxyBrush/issues/366)). Duplicate and Mirror duplicate used to copy only the level shown; the copy now has every level, with its detail and paint, so you can step it up and down like the original (Mirror duplicate mirrors each level). It also keeps the piece's material.
- **Hierarchy commands in the Pieces list's right-click menu** ([#414](https://github.com/crisprintsstuff/OxyBrush/issues/414)). On a piece with children: **Select hierarchy** picks it and everything under it, **Join hierarchy** joins them into one piece as they are, and **Merge hierarchy** merges and remeshes them. With several pieces picked, **Join picked** and **Merge picked** are there too, and **Split loose parts** and **Split by groups** for any piece.
- **Bridge for edges in Poly Edit** ([#378](https://github.com/crisprintsstuff/OxyBrush/issues/378)): click a border edge of one hole, then one of another, to join the two holes with a tube of faces (Rows and Twist as for faces; follows Mirror).
- **Shift smooths face group borders with the Paint Groups brush** ([#303](https://github.com/crisprintsstuff/OxyBrush/issues/303)). Hold Shift and stroke over a jagged border: each face joins the group most of its neighbours are in, so spikes and stray faces go and straight borders stay. The surface doesn't move; each stroke is one undo step.
- **Ctrl/Cmd+double-click picks a loop in Poly Edit** ([#298](https://github.com/crisprintsstuff/OxyBrush/issues/298)): on an edge, its edge loop (on a hole's edge, the whole border of the hole); on a face, its face loop across the nearest edge. Added to what's already picked.
- **Ctrl+Shift+drag on empty space inverts visibility** ([#329](https://github.com/crisprintsstuff/OxyBrush/issues/329)): a visibility box or lasso drawn around none of the model swaps hidden and shown faces, as Ctrl+Shift+click on empty space shows everything.
- **Hold Space to move a box or lasso while drawing it** ([#329](https://github.com/crisprintsstuff/OxyBrush/issues/329)), for masking or visibility: it keeps its shape and follows the pointer until you let go of Space.
- **The brush ring shows while you resize with S** ([#418](https://github.com/crisprintsstuff/OxyBrush/issues/418)). The size and focal shift sliders open under the pointer, which hid the ring; it now stays on the model where you pressed the key, over the slider, and grows and shrinks as you drag.

### Changed

- **Smoother Boolean seams that stay put** ([#285](https://github.com/crisprintsstuff/OxyBrush/issues/285)). A sharp seam is thinner than a voxel, so it came out as a sawtooth, and every Boolean rebuilt and smoothed the whole piece, softening earlier seams a little each time. Now the seam is blended (see above), and away from it each point goes back onto the surface it came from, so the rest of the piece, earlier seams included, keeps its shape. On two overlapping spheres the seam's spread around the surface drops from 0.42 to 0.11 voxels, and a later Boolean elsewhere barely changes it (0.11 to 0.13 voxels; it used to double). A Boolean takes 10–20% longer.
- **A stray click no longer clears the pick in Poly Edit** ([#298](https://github.com/crisprintsstuff/OxyBrush/issues/298)). Clicking empty space by accident, or a press there meant to orbit, kept dropping faces, edges or points picked one by one. Now **Esc** clears the pick (a first Esc still drops a half-done bridge or split; with nothing picked, Esc closes the tool), and so does Ctrl/Cmd+click on empty space.

### Fixed

- **A long lens no longer puts the camera inside the model** ([#419](https://github.com/crisprintsstuff/OxyBrush/issues/419)). Zooming in moved the camera toward the orbit centre in the middle of the model, so at 200 mm a close-up passed through the front surface, and switching back to a shorter lens left the camera inside. Zooming in near the model and changing the focal length now move the orbit centre onto the surface in the middle of the view first, so the camera slows down at the surface. Further out, the orbit centre stays where it was.
- **The wireframe no longer shows edges from the far side** ([#417](https://github.com/crisprintsstuff/OxyBrush/issues/417)). Lines were lifted toward the camera by a fixed amount in depth-buffer units, which at the back of a model was large enough to show hidden edges near the silhouette of a low-poly shape. They are now lifted by 0.6% of their distance.
- **Painted detail survives stepping down a subdivision level and back** ([#416](https://github.com/crisprintsstuff/OxyBrush/issues/416)). Each level now keeps the paint it has beyond what the level below gives, as it does for sculpted detail, and adds it back on the way up; paint added on a lower level still shows through. Projects save it in a new optional `MRPT` section, so it also survives saving at a lower level and reopening; older versions skip it.
- **Cylinders and cones from Pieces > Add keep their side quads** ([#415](https://github.com/crisprintsstuff/OxyBrush/issues/415)). One row with many segments makes tall, thin quads, and from 26 segments the shape-based guess paired some of their triangles with their neighbours', so the wireframe and quad subdivision showed turned edges. The quads are now recorded when the shape is made.
- **Renaming a piece** ([#413](https://github.com/crisprintsstuff/OxyBrush/issues/413)): the name is selected when renaming starts, so typing replaces it, and a click elsewhere ends renaming even when the name wasn't changed (the field used to stay focused, even while sculpting).

## [4.0.0-beta.22] - 2026-11-21

### Added

- **Mix themes** in **Preferences > Appearance > Themes > Mix**: take the **colors** (panels, windows, text, buttons), the **controls** (Standard, Studio's bar sliders, or Sculptor's clay sliders, card tiles and silver icons) and the **shadows** under windows and menus (Standard, Studio's tight ones, Midnight's soft, wide ones) each from any theme. For example, Sculptor with Graphite's colors and Studio's shadows. Each part follows the chosen theme until you change it; choosing a theme or a Look starts again from it, and **Clear mix** goes back. Saved with the other appearance settings.
- **One bar at the top of the window on Windows and Linux** ([RFC 0028](docs/rfcs/0028-unified-title-bar.md)). The menus, the project name and the minimize, maximize and close buttons now share one bar in place of the system title bar and the menu row, so the model gets more room; the tabs stay in their row under it. Drag the empty part of the bar to move the window (including snapping to screen edges), double-click it to maximize, right-click it for the window menu where the system has one, and resize from the window's edges and corners. **Preferences > General > Use system title bar** brings back the old layout at the next start. macOS keeps its system title bar (see the next entry).
- **The menus are in the macOS menu bar** ([RFC 0028](docs/rfcs/0028-unified-title-bar.md)), at the top of the screen as in other Mac apps, so the window starts with the tabs. The OxyBrush menu has About, **Settings…** (⌘,), Hide and **Quit** (⌘Q, which still asks about unsaved tabs), and a Window menu has Minimize, Zoom and full screen. Menus show ⌘ shortcuts from your keymap, except the text editing ones (⌘A, ⌘C, ⌘V, ⌘X, ⌘Z), which stay with text fields. The sliders from View > Shading are in a **Shading** window there. Commands added to the command palette so every menu command is one: Clear groups, Grow and Shrink visible, Grow and Shrink mask, Paint brush, Subdivide (single mesh), Command palette and Find brush. *Not yet tried on a Mac; please report anything odd.*
- **Organize the Recipe Box.** Click a slot to rename the recipe, change its category, or move it earlier or later. Click a category variation to place it or drag it to reorder. Deleting a recipe leaves copies already placed on models editable.
- **More ways to place surface instances.** **Masked surface** places live copies on visible faces that meet the mask threshold. **Even scatter** spaces seeded copies across the visible surface in a background job. Both modes save with the project.

- **UVs survive subdivision.** Catmull–Clark and Loop levels preserve UV islands when you subdivide, switch levels, save and reopen, or export OBJ or glTF/GLB.

### Fixed

- **Quads in OBJ and FBX files stay quads, even when they aren't flat.** OxyBrush used to split every polygon into triangles and then guess which pairs had been quads from their shape, so a quad folded across its diagonal stayed two triangles, and neighbouring triangles were sometimes paired into the wrong quads. Quad subdivision then broke those areas into shapes that were hard to sculpt. Import now keeps the file's own polygons, including n-gons, for the wireframe, quad subdivision levels and export, and through the Blender bridge. A non-convex quad is split along its inside diagonal. Projects save these polygons in a new `POLY` section, so they survive save and reopen; older versions skip it and guess the quads as before. Files made only of triangles are unchanged.
- **Blender models arrive upright and open in the running window.** The bridge converts between Blender’s Z-up and OxyBrush’s Y-up coordinates in both directions. A send reuses an untouched active tab. An occupied tab offers to open the model there or in another tab.
- **Blender bridge returns work on meshes with shape keys.** Position-only edits move the Basis and keep each key's offset. Sending a new piece back again updates the object that the bridge previously created.
- **Floating windows cover the Recipe Box drawer.** Floating panels such as Preferences now draw above the drawer and receive clicks over it.

## [4.0.0-beta.21.4] - 2026-11-21

### Added

- **Blender bridge extension ZIP.** Install the OBJ bridge from the release downloads. The extension manifest now passes Blender validation ([#396](https://github.com/crisprintsstuff/OxyBrush/pull/396)).

- **New tabs keep your material.** A new tab now starts with the material (matcap) of the tab you were in, instead of the default one. **Preferences > General > New tabs start with** can also choose the default material, or a material you pick there (built in or one you loaded).
- **Placed recipes can be moved, kept in step and saved back** ([RFC 0026](docs/rfcs/0026-parts-bin.md), phase 3). In a placed recipe's group menu in the Strokes window:
  - **Move on surface**: click on the model where it should go. The Move window sets its **Twist** and **Size**, **Apply** keeps it where it is with them, and **Cancel** (or Esc) leaves it. A ring and an arrow show where it sits. It's sculpted again there in one undo step; strokes made after it stay as they were.
  - A mirrored pair is **Linked**: **Edit…**, **Edit path** and **Move on surface** on one side change the other side too, in the same undo step. Fade and delete stay per side. **Unlink** stops it, and undo links them again.
  - **Save back to Recipe Box** replaces the recipe in the box with the version on the model, after asking (and saying that fades aren't saved). **Save as new recipe** adds it under a new name and never replaces another recipe. Nothing is written to the box unless you choose one of these.
  - A "Recipe Box" topic in the in-app guide.

### Changed

- **The Parts Bin is now the Recipe Box**, and the strokes you save in it are **recipes**: **Save as recipe…** in the Strokes window, the Recipe Box drawer beside the brush shelf, and View > Recipe Box. The box now says what it's for, a library of reusable sculpted details to drag onto any model, instead of counting strokes. Only the names and wording changed: saved libraries and projects with placed recipes open as before.

### Fixed

- **Grab, Topological Move, Kelvin Grab and Snake Hook pull round shapes with symmetry on.** Since 4.0.0-beta.21 a pull on or near the mirror plane came out with a flat top, as if capped ("the points are capped, so it isn't round"), and on the plane the brush's edges moved twice as far. Now a pull on the plane is exactly the brush itself, the point under the cursor follows it exactly, the brush is unchanged where the mirror copy doesn't reach, and near the plane the two sides rise as one round shape, a little higher between them than at either brush, as two overlapping pulls do.
- **A recipe dragged from the Recipe Box lands under the pointer.** It used to land off to one side by the width of the panels.
- **The mirror side of a mirrored pair is a mirror image.** It was a turned copy, so an asymmetric recipe (an L, a hook) came out the wrong way round on the other side.
- **Placed recipes stay recipes after saving and reopening.** The project kept stroke numbers from the session, which change when the project opens, so a reopened recipe lost track of its strokes. Projects now store their place in the list (`PART` version 2); files from 4.0.0-beta.21 are read by stroke group.
- **Undoing a drop takes the recipe's strokes and record away too**, and redo brings them back. The first mirrored pair of a session also showed its mirror side as unlinked.
- **Left clicks in the Pieces list work again.** Clicking a piece's eye selected the piece instead of hiding it, and double-click to rename and Alt+click to isolate did nothing: the whole row's click area sat on top of its own buttons. The row now takes only the clicks that miss them, so it still selects the piece, and right-click still opens its menu.

## [4.0.0-beta.19.2] - 2026-11-20

### Added

- **Hierarchical scene tree outliner for pieces with transform propagation** ([#332](https://github.com/crisprintsstuff/OxyBrush/issues/332), [#358](https://github.com/crisprintsstuff/OxyBrush/pull/358)):
  - Tree-view outliner in the Pieces panel with depth indentation, branch connector lines, and fold/unfold chevrons (`▾`/`▸`) with collapsed child counts.
  - Drag-and-drop parenting: drop into the middle zone of a piece to parent under it, or drop below the list / click **Unparent** to unparent.
  - Transform propagation: transforming a parent piece with the gizmo (`T`) moves all descendants together around the parent's pivot (like a joint or limb), while transforming child pieces leaves the parent intact.
  - Saved in `.oxb` project files via the skippable `PARN` chunk with full undo/redo history support.
- **4K alpha resolution & 16-bit depth preservation** ([#359](https://github.com/crisprintsstuff/OxyBrush/issues/359), [#361](https://github.com/crisprintsstuff/OxyBrush/pull/361)):
  - Raised alpha import clamp from 1024 to 4096 (`MAX_ALPHA_SIZE = 4096`), allowing high-resolution detail stamping on dense meshes.
  - Native 16-bit grayscale depth (`u16`) preserved on import for 16-bit PNG files without bit-depth loss or banding.
  - Aspect ratio preservation: non-square alpha images are centered and padded with black borders rather than stretched.
  - Area-box filtering ensures 1px ink lines and high-frequency details are antialiased without dropping when downsampled.
  - Inspector previews downsampled to ≤128x128 for UI textures, saving 16 MB GPU VRAM per 4K alpha.
- **Preserve volume option in voxel remesh** ([#354](https://github.com/crisprintsstuff/OxyBrush/issues/354), [#357](https://github.com/crisprintsstuff/OxyBrush/pull/357)). When enabled, remeshing projects vertices back to the level-set zero crossing and applies volume-calibrated normal adjustment to compensate for voxelization chord loss and smoothing shrinkage. Successive remeshes retain their silhouette, mass, and volume without shrinking or squashing. Also available in the CLI via `--preserve-volume`.

### Fixed

- **Inspector panel narrow resizing and control wrapping** ([#360](https://github.com/crisprintsstuff/OxyBrush/issues/360), [#362](https://github.com/crisprintsstuff/OxyBrush/pull/362)):
  - Converted rigid single-line widget rows (Alpha selector, Brush size, Symmetry, Topology, Groups) to wrapping layouts (`ui.horizontal_wrapped`) so buttons stack gracefully.
  - Added `.min_size(180.0)` and flexible piece renaming/truncation so the inspector panel can be dragged much narrower without egui snapping back.
- **Deform window title bar dragging** ([#351](https://github.com/crisprintsstuff/OxyBrush/issues/351), [#355](https://github.com/crisprintsstuff/OxyBrush/pull/355)): Prevented scrolled-away deformer sliders from triggering deformations when dragging the window by its title bar. Hit-testing for quick flicks is now strictly bounded to the scroll viewport's visible clip rectangle and ignores drags of other widgets or windows.

## [4.0.0-beta.19] - 2026-11-19

### Added

- **Compressed Undo History & Stroke Optimization** (`oxy-history`):
  - Inactive chunk snapshots (`Change::CompressedChunks`) and face attribute edits (`Change::CompressedFaces`) are automatically compressed using pure-Rust LZ4 (`lz4_flex`).
  - The active newest undo step remains uncompressed for instantaneous access and living stroke editing, while older history steps compress by 60%–99% (e.g. reducing 1 GB of face attributes to <10 MB).
  - Enables retaining 10x more undo steps within the memory budget and prevents out-of-memory crashes during extended high-poly sculpting sessions.
  - Automatically and seamlessly decompresses steps upon undo or redo.
- **Multiresolution Zero-Displacement Optimization** (`oxy-mesh`):
  - Avoided allocating full-length zero vectors for `details` when subdivision levels have zero displacement, saving 12 bytes per vertex (1.2 GB per 100M vertices) on each multiresolution level.
- **VRAM Culling for Inactive/Hidden Pieces** (`oxy-app`):
  - Gated viewport GPU sync and buffer retention to visible pieces: hidden pieces release their vertex, index, and storage buffers immediately, freeing gigabytes of VRAM in multi-piece scenes.
- **Partitioned GPU Rendering & Draw Calls**:
  - Split draw calls and index buffers into bounded partitions of 1,048,576 triangles (`PARTITION_TRIANGLES`, 12 MB indices, 4 MB face colors slice).
  - Each partition binds its corresponding 4 MB sub-slice of the face colors storage buffer, strictly respecting hardware `max_storage_buffer_binding_size` (128 MB) and `min_storage_buffer_offset_alignment` (256-byte aligned).
  - Eliminates contiguous VRAM allocation fragmentation and supports rendering dense pieces and multi-piece scenes up to 600M+ triangles.
- **Option to invert brush alphas** ([#339](https://github.com/crisprintsstuff/OxyBrush/issues/339)). The **Invert alpha** toggle flips the alpha values (white becomes black and black becomes white) during strokes, in presets, and in the Living Strokes edit panel. The thumbnail updates live to reflect the inversion.
- **Poly Edit gizmo axes and pivot** ([#324](https://github.com/crisprintsstuff/OxyBrush/issues/324)). The gizmo bar's **Pivot & axes** menu aligns the axes to the world, the view, or the average normal of what is picked. Alt+click a point, edge or face to snap the pivot to it; Alt+drag a handle to move or turn the gizmo without moving geometry. **Reset pivot** puts it back.
- **Reorder pieces by drag-and-drop** ([#297](https://github.com/crisprintsstuff/OxyBrush/issues/297)). In the Pieces panel, drag any piece up or down by its grip handle (or drag its name button) to reorder it in the scene. A drop line indicates where the piece will land, and undo restores its previous position.
- **Join picked pieces** ([#297](https://github.com/crisprintsstuff/OxyBrush/issues/297), [#284](https://github.com/crisprintsstuff/OxyBrush/issues/284)). Ctrl/Cmd+click pieces in the Pieces list to pick multiple pieces, then click **Join picked** to combine them as they are without booleans or remeshing.
- **Isolate piece with Alt-click** ([#297](https://github.com/crisprintsstuff/OxyBrush/issues/297)). Alt-clicking the eye icon of any piece isolates that piece by hiding all others; Alt-clicking it again restores visibility for all pieces in the scene.
- **Pick the Export file type** ([#300](https://github.com/crisprintsstuff/OxyBrush/issues/300), [#344](https://github.com/crisprintsstuff/OxyBrush/issues/344)). Choose the export file type from **File > Export type** in addition to the export dialog.

### Changed

- Section headings in the Studio theme (the settings panel) are shown as written, "Like This", instead of in all capitals.
- Stamp stroke mode redraws the stamp once per frame instead of on every pointer move, so dragging stays smooth with a pen or a fast mouse ([#342](https://github.com/crisprintsstuff/OxyBrush/issues/342)).

### Fixed

- **High-poly stability and crash prevention at extreme triangle counts**:
  - Gated wireframe index generation so line buffers are only generated and uploaded when wireframe is active and triangle count is <= 10M triangles, eliminating up to 4.8 GB of host RAM and 4.8 GB GPU buffer allocation per mesh rebuild and preventing 32-bit integer overflow and GPU driver buffer allocation crashes.
  - Enforced GPU `max_storage_buffer_binding_size` (128 MB) on `face_colors` storage buffers with fallback to per-vertex color, preventing driver validation crashes on meshes with >33.5M triangles.
  - Guarded mesh vertex and index buffer creations against GPU `max_buffer_size`.
  - Removed redundant `triangle_chunk` vector from `Mesh`, reducing RAM usage by 4 bytes per triangle (saving 800 MB at 200M triangles and 2.4 GB at 600M triangles).

## [4.0.0-beta.18] - 2026-11-18

### Fixed

- Starting from an empty scene no longer pins the brush size to its minimum with the size on the model: the size range now follows the camera distance until there is something to measure, so the slider and `[` `]` work (#328).
- The **Floor grid** toggle now works in an empty scene too; the grid is drawn at the origin, sized to the view.

### Added
- **Blur and sharpen the mask with a click** (as in ZBrush). Ctrl+click on the model blurs the whole mask and Ctrl+Alt+click sharpens it; dragging still paints or unmasks. With nothing masked, a click still paints a dab.
- **Sort brushes A–Z** in the Arrange menu lists the brush selector alphabetically (#304).
- **Hide masked** (Visibility menu and the command palette) hides the masked faces (#329).
- Pressing Alt just before letting go of a mask box or lasso masks everything *outside* it instead (#329).

## [4.0.0-beta.17] - 2026-11-18

### Added
- **Import several files at once** ([#311](https://github.com/crisprintsstuff/OxyBrush/issues/311)). The Import dialog takes more than one file (Shift-click or Ctrl/Cmd-click), and dropping several files on the window does the same. Each object in each file becomes its own piece, and the whole batch is one undo step. Files are imported in name order, because the system file dialog doesn't promise the order they were clicked. A file that can't be read is skipped and named in the status bar; the rest still import, including into an empty scene. When the batch is fitted to the scene, every file uses the same scale factor (from the batch's overall size, or the factor already chosen for this project), so a small part stays in proportion to a large one; one Undo restores the original sizes and a second removes the batch. A large import runs in the background, with the same progress line as a remesh.
- **Empty scene** (File > New, and the welcome window). Starts a project with no pieces, so you can build it with Insert Primitive, + Add or Import. Saving it and opening it again keeps it empty. A tool that needs a piece says so; framing or a brush stroke on an empty scene does nothing to it.
- **Group the masked area** (Edge loop at mask border, on by default, [#296](https://github.com/crisprintsstuff/OxyBrush/issues/296)): the triangles inside the mask get a face group of their own, along with the loop, so that area can be isolated afterwards. With Both sides on, the outside keeps its groups. Hidden faces stay out of the new group. It is the same undo step as the loop. Turn the option off to leave the masked area's groups as they were.
- **Choose the export file type** (File > Export type). The selected type is proposed in the export dialog; a typed extension still takes precedence. This makes glTF, OBJ, STL and PLY selectable on macOS, where the save panel has no file-type list.

### Changed
- **Brush defaults:** Front faces only is off by default, and Grab, Snake Hook, Topological Move, Kelvin movement brushes and Curve Move start at strength 0.5.
- **Resize behavior:** window resizing coalesces intermediate surface updates and keeps the UI layout synchronized with the current size.

### Fixed
- **Crease symmetry:** Crease and Sharp Crease no longer leave a ridge along the mirror plane.
- **Kelvin Grab:** ordinary pulls retain their elastic effect while long pulls remain smooth.
- **Poly Edit picks:** clicking empty space clears the picked faces, edges or points; dragging there still orbits without clearing the pick.
- **Group by islands:** hidden faces no longer join visible islands or have their face groups overwritten.

### Planning
- RFC 0026 for the Parts Bin is accepted, and RFC 0027 for tabbed projects is a draft; neither feature is in the app yet.

## [4.0.0-beta.16] - 2026-11-16

### Added
- **Edit a stroke's settings** ([RFC 0025](docs/rfcs/0025-editable-and-portable-strokes.md)). In the Strokes panel, **Edit…** (or double-click a stroke) opens its brush, Raise or Carve, size, strength, falloff, alpha and spacing. Changing one sculpts that stroke again; strokes made after it stay as they were. A slider shows a lighter preview while you drag and applies the full stroke when you let go. Each change is an undo step. Esc puts the stroke back as it was when you opened Edit. A stroke that can't be edited says why: it was made before this version kept what was drawn, Living strokes was off, dynamic detail was on, it is a mirrored copy or one side of a split, or it came from Curve Move, Deform or the gizmo (those aren't strokes).
- **Living strokes remember what was drawn** ([RFC 0025](docs/rfcs/0025-editable-and-portable-strokes.md), first step). With Living strokes on, each brush stroke also keeps its path, pressure and brush settings, and running them again makes exactly the same stroke. Saved with the project in a new optional `STRI` section; older versions skip it and keep the strokes as before. Curve strokes, stamps, lines and a freehand stroke you keep instead of editing as a curve are remembered, including a captured or imported alpha after you save and open the project. Strokes made with dynamic detail on (whether or not the mesh was rebuilt), Curve Move, and whole-piece tools (Deform, the gizmo) aren't.

### Changed
- **Hard edge angle** (View > Shading). How sharp a corner must be before the faces meeting there shade flat is now a slider, from 0° to 180°. The default is 40°: a six-sided cylinder (faces 60° apart) and an eight-sided one (45°) shade flat along their length, and nine or more sides stay smooth. **180°** is shown as Off and keeps every edge smooth. It is remembered with the other view settings and is not saved in the project; there is no undo. The view updates when you let go of the slider, so a large mesh is rebuilt once rather than on every frame of the drag.

## [4.0.0-beta.15] - 2026-11-16

### Added
- **Sharp or smooth corners point by point in tube profiles.** Double-click a point in the Profile editor to make it a sharp corner (its handle turns square), and again to round it; with Mirror its partner changes too. **All smooth** and **All sharp** replace the Smooth checkbox. The **D-shape** preset now has a flat bottom with sharp corners and a round top. Saved with the project in a new optional `TPRC` section; older versions keep such a tube as ordinary triangles.
- **Merge down** ([#274](https://github.com/crisprintsstuff/OxyBrush/issues/274)): a button in the Pieces panel (and in the command palette) joins the active piece with the piece below it in the list, exactly as they are, like Join does for every visible piece. One undo step.
- **Move several pieces together** ([#274](https://github.com/crisprintsstuff/OxyBrush/issues/274)): Ctrl/Cmd+click pieces in the Pieces list to pick them along with the active one (they're highlighted). The transform gizmo then sits in the middle of them all and moves, turns and scales them together, about the world axes, as one undo step; masked parts of each stay put, and Esc cancels a drag. **One piece** in the gizmo bar, or a plain click on a piece, drops the pick.
- **Poly Edit soft selection** ([#275](https://github.com/crisprintsstuff/OxyBrush/issues/275)): with **Soft selection** on (Poly Edit window, under Gizmo), the transform gizmo also moves the points near what's picked, less the further they are, for smooth bulges and bends instead of sharp steps. **Radius** sets how far it reaches and **Falloff** how quickly it fades, from soft to sharp; orange dots show the points it will move. Mirror moves the other side alike, and the pick stays just what you picked.
- **Poly Edit mode buttons in the brush shelf** ([#275](https://github.com/crisprintsstuff/OxyBrush/issues/275)): while Poly Edit is open, Faces, Edges, Points and Auto buttons sit at the top of the shelf (the same as 1-4).
- **The transform gizmo in Poly Edit** ([#272](https://github.com/crisprintsstuff/OxyBrush/issues/272)). With faces, edges or points picked, T (or **Gizmo: All / Move / Rotate / Scale** in the Poly Edit window) puts the gizmo in the middle of them, and its handles move, turn and scale just what's picked. **Shift+drag** extrudes picked faces or border edges first and moves the new ones, or on a scale handle insets the picked faces. The new geometry stays picked, so you can keep extruding. Mirror moves the other side too; each drag is one undo step and Esc cancels it.

### Changed
- **One transform gizmo for move, turn and scale.** T now shows a gizmo with all three at once, so you no longer press T to switch between them: arrows and squares move, rings turn (the outer ring around the view), cubes partway along each axis scale along it, and the circle round the centre scales evenly. T again hides it. The gizmo bar shows the moved, turned and scaled totals together, and can still show **Move**, **Rotate** or **Scale** on its own (and W/E in the single-key shortcut preset still do). Each drag is one undo step named for what it did.
### Fixed
- **Poly Edit keeps the right pick after a gizmo move.** Committing renumbers the points (the mesh keeps its own vertex order), and the pick kept after a plain gizmo move used the old numbers, so it could land on other elements.
- **Undo while editing a tube undoes the tube's own edits** ([#271](https://github.com/crisprintsstuff/OxyBrush/issues/271)). An open tube isn't in the history until it's accepted, so Ctrl/Cmd+Z undid the last change outside it. Now Undo and Redo step through the tube's points, thickness, profile and shape settings, one step per drag, click or slider, while the tube is open. Undo past its first edit closes the tube, without touching the scene; after that, and after Accept, Undo works on the scene again.
- **Mirror in the tube profile editor adds and removes points on both sides** ([#273](https://github.com/crisprintsstuff/OxyBrush/issues/273)). Clicking the outline added a point on one side only, so the profile stopped being symmetric.
- **The wireframe shows low-poly primitives as quads again** ([#270](https://github.com/crisprintsstuff/OxyBrush/issues/270)). Since 4.0.0-beta.14 shades hard edges flat, a box or cylinder from Insert Primitive showed a diagonal across every quad in the wireframe, as if it were made of triangles, while Poly Edit counted quads. The wireframe now follows the polygons as before.

## [4.0.0-beta.14] - 2026-11-15

### Added
- **Tube profiles:** a Profile section in the Tube window sets the tube's cross-section. Start from Round, Square, Triangle, Star, D-shape or Ribbon, then drag points on a small canvas: click the outline to add one, right-click to remove one, and Shift to snap. **Smooth** curves through the points or keeps sharp corners, and **Mirror left and right** edits both sides at once. The tube updates live; the profile turns with Twist and shapes coil and braid strands too. It's saved with the project (new optional `TUBP` section) and kept when the tube is edited again, and the next tube starts with it. Older versions keep a profiled tube as ordinary triangles.
- **Area lights** ([RFC 0024](docs/rfcs/0024-area-lights-and-lit-matcap.md)): a fourth kind of viewport light, round with a **Size**, facing its aim point like a soft box. Its light wraps further round as it gets bigger, its highlights widen, and its shadows are sharp where things touch and softer further away (percentage-closer soft shadows). Drag its dot and diamond in the viewport like a spot's.
- **Light the matcap** (Lights window): keep the matcap as the material and let the lights brighten it where they shine and their shadows darken it, instead of switching to the plain lit surface.
- **Tube editing** ([#258](https://github.com/crisprintsstuff/OxyBrush/issues/258)):
  - **Sharp corners:** double-click a tube's point to make it a sharp corner. The tube runs straight into it and out again, mitred so it keeps its thickness, and the handle turns square. Double-click again for a smooth point. Corners are saved with the project (new optional `TUBC` section) and kept when the tube is edited again. Older versions keep a tube with corners as ordinary triangles.
  - **Merge and delete points:** drag a point onto its neighbour to merge them, or click a point and press **Delete** (or Backspace) to remove it. Alt+click still works.
  - **Orbit while editing:** with the Tube brush, dragging anywhere away from the tube's points and line orbits the view, instead of doing nothing.
- **Give any brush its own hotkey** ([#259](https://github.com/crisprintsstuff/OxyBrush/issues/259)): right-click a brush in the shelf, choose **Set hotkey…** and press a key, with Ctrl, Shift or Alt if you like. The shelf shows it on the brush, and **Remove hotkey** takes it away. A brush's key wins over a command on the same key, and the message says which command it replaced. Giving the key to another brush takes it from the first. The keys are saved with the keymap and listed under **Brush keys** in Preferences > Keymap and in the shortcut list. The number keys still pick by shelf order.
- **Hide just the settings panel** ([#260](https://github.com/crisprintsstuff/OxyBrush/issues/260)): the view strip's new top button, **Shift+Tab**, **View > Show settings panel** or the command palette hides the right-hand settings panel and brings it back, for more room around the model. The brush shelf and view strip stay, and the panel stays hidden after a restart. Tab still hides every panel at once; Shift+Tab after Tab brings them back.
- **Poly Edit element modes** ([RFC 0023](docs/rfcs/0023-poly-edit-element-modes.md)): **1 Faces**, **2 Edges**, **3 Points** (buttons at the top of the window too), like other modeling apps, so the window shows only that kind's actions instead of all of them at once. A click picks that kind: anywhere on a face, the nearest edge, the nearest point, so small targets are easy to hit. **4 Auto** keeps the old behaviour (what's under the pointer decides). Faces is the default. The keys only change meaning while Poly Edit is open; elsewhere they pick brushes.
- **Poly Edit: pick edges and points too.** Ctrl/Cmd+click picks several edges or points in their modes, as it did faces: crease or bevel several edges at once, move several points together. Switching modes turns the pick into the new kind: a face's sides or corners, the faces that picked edges or points close, the edges picked points make. So you can pick a face, press 2 and bevel its border.

### Fixed
- **Lights without shadows no longer end in a hard line.** Every light now fades gently where the surface turns away from it, set by its **Softness** (now always available, not only with shadows on). A light with shadows got that softening from its shadow; one without stopped abruptly, and the tone curve made the edge look like a line.
- **Low-poly models shade with crisp, flat faces.** A box from Insert Primitive or Poly Edit looked blotchy, with highlights and a dark band smeared across its faces and edges, under both the matcap and the viewport lights. Each corner had one normal, averaged from the faces meeting there, so every face shaded as if it were curved. The viewport now gives faces meeting at more than 60° their own normals at the shared corner, so a box shades as a box. Spheres, cylinders with six or more sides, and sculpts stay smooth. Only the viewport changes: the mesh, exports and brushes are as before. Finding these edges adds about 10 ms when a 1.3 million-triangle mesh is rebuilt (0.7 ms at 82,000) and nothing measurable per brush dab.

## [4.0.0-beta.13] - 2026-11-15

### Added
- **Poly Edit: pick several faces** ([#243](https://github.com/crisprintsstuff/OxyBrush/issues/243)): Ctrl/Cmd+click faces to pick them (outlined in green); dragging or clicking one of them then extrudes, insets, bevels, moves or deletes them all together. Ctrl/Cmd+click a picked face to unpick it, or empty space to clear; the Poly Edit window shows how many are picked, with a Clear button. The pick ends once the faces change.
- **Poly Edit: centred and paired edge loops** ([#243](https://github.com/crisprintsstuff/OxyBrush/issues/243)): **Shift+click** an edge with Insert loop to put the loop exactly in the middle. **Two loops** cuts a pair the same distance in from each end of the edge; dragging slides them in or out together, and Shift+click puts them at thirds.

### Fixed
- **The transform gizmo follows Mirror X/Y/Z.** Two shapes inserted with Mirror X, moved with the gizmo on one of them, both moved the same way, as if symmetry were off; other apps mirror the move (a tester compared three). Now the other side gets the mirrored move, turn or scale, live while dragging and with typed values, as one undo step. This applies when what the gizmo moves is centred on the mirror plane and the pivot is on one side. A part across the plane stretches symmetrically instead of tearing (the two sides blend within 2% of the piece's size of the plane). A whole model moved off-centre, or a pivot on the plane, still moves as one. Mirroring costs about 6 ms a frame on 655,000 moving points (2.6 ms without).
- **Gravity is much stronger.** At 1, one pass of a stroke used to sag the surface about a third of the brush radius, so even full Gravity gave a gentle sag where another app's gravity at half drapes a whole cap (a tester compared the two). It now sags about a full radius per pass (3.3 times as much), so 0.5 is a clear drape and 1 pulls a cap down into a lip in a few passes; the slider stays the way to set how strong, and low values (0.1–0.3) give the gentle droop the old range had. Twice that again tore the surface at 1, so it stops here. Existing presets and saved brush settings keep their numbers, so they sag more than before.
- **Stamp stroke mode is as strong as a freehand stroke, and shows the stamp while you drag.** A stamp was a single dab, which moves the surface only a tenth of the brush radius, so Draw at full strength barely raised a dimple (a tester compared it with another app). It now stacks six dabs in place, as much as a freehand stroke one brush-width long. The stamp also grows and turns with the drag, live, instead of appearing on release. Escape takes it back, it's one undo step, and a click without dragging stamps at the brush size.
- **Kelvin Grab no longer creases long pulls.** Pulling a horn or an ear out of a sphere left a slit running from tip to base. The Kelvinlet's elastic part, which bulges the sides and pinches behind the brush, is a small-move formula: on a long pull it pinched the surface behind the brush right across itself (up to a 170° fold), and its narrow peak drew the middle into a ridge. Now its regularization width is the brush radius (the paper's scale, not a third of it), its falloff is gentler, and the elastic part fades as the pull grows past the brush radius. Small moves keep the rubbery, volume-preserving feel; long pulls come out as smooth as with Grab. Kelvin Twist and Kelvin Scale are unchanged.
- **Poly Edit uses face groups painted since it was last open** ([#249](https://github.com/crisprintsstuff/OxyBrush/issues/249)). It kept the piece's polygons with their old groups, so after painting new face groups (Paint Groups brush, Groups menu), **Reach: Face group** still extruded the previous group. It now takes the current groups when it opens and whenever they change while it's open.

## [4.0.0-beta.12] - 2026-11-15

### Added
- **Edge loop at mask border** (Mask and Mesh menus, and the command palette): adds a loop of quads along the edge of the masked part, inside it or on both sides (**Both sides**, on by default), at a **Width** you set, so a hard edge stays crisp when you subdivide or smooth, for hard-surface work. A triangle counts as masked when all three corners are, so masking a face group (Ctrl+M) puts the loop exactly around that group. The loop gets a face group of its own; the rest of the surface keeps its shape (original points don't move, and a flat panel stays flat). One undo step; like Subdivide masked, it removes subdivision levels and UVs.
- **Paint Groups brush** (in the brush shelf after Paint): paint face groups directly onto the model. Each stroke makes a new group; **Alt**+stroke grows the group you start on. It follows symmetry and Front faces only, leaves hidden faces alone, never moves the surface, shows the group colors while you paint, and each stroke is one undo step (Escape cancels it mid-stroke). A dab takes 0.07–0.2 ms on a 1.3-million-triangle sphere (4 cores).
- **Units** (Preferences, next to Orbit around, [#234](https://github.com/crisprintsstuff/OxyBrush/issues/234)): choose **Metric** or **Imperial**, and how many millimetres one scene unit is (the same scale as the 3D print window, now remembered between sessions). The **Measure** plugin now shows its results with units: "Size 20.000 × 10.000 × 5.000 mm, volume 1.000 cm³", or inches and cubic inches. Plugins can read the choice too (plugin API 5: `units()` and `unit_scale()`).
- **Surface instances** (the new **Instances** section of the right panel, [RFC 0022](docs/rfcs/0022-surface-instances.md), first phase): copy one piece (a rivet, a spike, a scale) onto every face or point of another, or onto one face group, as live copies. They sit on the surface along its normal and follow it as you sculpt; **Edit source** shows the copied piece alone to sculpt, paint or reshape it, and every copy follows. Each instancer chooses which way copies face around the normal (a world direction by default, so neighbours never flip; or the face's longest or first edge; or random), their size (a share of the face, or scene units), lift, turn, and random size and turn with a seed. **Make real** turns the copies into a new piece of ordinary mesh, one face group per copy, and keeps the copies and their source so you can change them and make them real again. Every change is one undo step (a slider drag included). The copies are drawn with GPU instancing from the source's own mesh, so 920 spikes on a ball cost one draw; working them out again after a host stroke takes under 1 ms for 10,000 copies and 5–8 ms for 100,000 (4 cores). Saved with the project in a new `INST` section; older versions open the host and the (hidden) source as ordinary pieces. Exports don't include the copies yet: Make real first.
- **View > Inactive pieces:** choose how the pieces you aren't sculpting are shown: **Grey out** (as before), **Darken** (their own materials and colors, just darker), **Tint** with a color you pick, or **Off**, to see every piece in its true material, which matters now that each piece can have its own. A Strength slider sets how much; it's remembered between sessions and in the command palette.
- **Quad sphere and UV sphere in the Insert Primitive brush**, next to the geodesic sphere: the same two spheres as Pieces > + Add, coming in as quads (good for subdivision levels) or rings.
- **Booleans in the Pieces panel:** a Boolean row under Join and Merge + remesh picks the piece to combine with and offers **Subtract**, **Add** and **Overlap**, so you don't have to dig three menus deep. Mesh > Boolean is still there.
- **Layers in the right panel:** a **Layers** section under Pieces shows the active piece's sculpt layers while you work: Sculpt into and + New layer, then each layer with its eye, strength and … menu, and **Strokes…** for the full stroke list. It replaces Mesh > Strokes…; with Living strokes off, it offers to turn it on.
- **View > Shading > Show vertex color** (also in Tools > Vertex color and the command palette): hide a model's vertex color, painted or imported, to judge the form in plain clay, and show it again. It only changes the view: the colors are kept, saved and exported. Painting, or filling with color, shows them again (and says so), so a stroke never paints invisibly. Remembered between sessions.
- **Gravity, per brush** (brush settings, under Auto smooth): the surface under the brush also sags downward as you sculpt, for drapery, hanging folds and drooping flesh. Each brush keeps its own Gravity, so Clay can drape while Draw doesn't; presets and brush codes carry it, and with Remember brush settings it's kept between sessions. It pulls toward the world's down (the floor, -Y), follows pen pressure and the mask, and keeps mirrored strokes mirror images. At 1, one full-pressure pass sags the middle of the stroke about a third of the brush radius; it adds up with the length of the stroke, not with the spacing of the dabs, so dense dabs don't tear the surface. It's added to the brush's own move, so it costs nothing measurable per dab (Clay at radius 0.3: 1.1–1.3 ms per dab with or without it, within run-to-run noise). Not on Mask, Paint, the Grab-like brushes or the placing and cutting tools.
- **File > Revert** (also in the command palette, [#218](https://github.com/crisprintsstuff/OxyBrush/issues/218)): reloads the project as you last saved it, dropping the changes since then, for when an experiment goes wrong. It asks first (Revert / Cancel), keeps the view where it is, and is available once the project is saved to a file and has changes. Undo can't bring the dropped changes back. If the file has gone missing or can't be read, nothing changes. Not available while the objects came through the Blender bridge.

### Changed
- **A Tools menu, and a shorter Mesh menu.** The tools that shape a piece moved from Mesh to a new **Tools** menu: Deform…, Transform, Symmetrize, Poly Edit…, Retopology… and Vertex color. Mesh keeps resolution and repair: subdivision levels, Subdivide, Remesh, Decimate, booleans, 3D print, diagnostics, Delete hidden, Close holes and Split. Shortcuts and the command palette are unchanged.
- **Interface consistency pass.** The 3D print and Topology diagnostics windows now look and read like the rest of the app (plain labels in a grid, theme colors for warnings, tooltips on every control, and a Measure again button), and both are in the command palette. Menu items, window titles and Preferences headings use the same sentence case everywhere ("3D print…", "Topology diagnostics…", "AI agent"), and the Boolean menu says what each choice does ("Subtract it", "Add it (union)", "Keep only the overlap"). Brush and topology sliders show two decimals, as the quick bar does.

### Fixed
- **Poly Edit: edges behind the model no longer show through.** The piece's polygon edges were painted over the view, so after an extrusion the edges of faces behind the new part showed as if in front, and the view got messy fast. They're now drawn by the viewport with the model's depth, so whatever is in front hides them, as in other modeling apps. (With the smooth preview on, the cage still draws on top, since it floats off the smoothed surface.)
- **Polygons with more than four corners show as one face.** After an operation made an n-gon (Poly Edit, retopology), the wireframe also drew the lines it's split into for display; now, as with quads, only its sides are drawn.
- **The gizmo pivot stays put after Insert Primitive** when "Gizmo pivot follows the mask" is off. Inserting a shape masks what was there, and the pivot jumped to the new shape anyway; Ctrl+drag duplicating had the same problem.
- **3D print: the wall check no longer overwrites your paint for good.** Check walls painted thin areas red straight into the piece's vertex color, with no undo; it is now an undo step ("Mark thin walls"), so Undo brings the paint back.
- **3D print: the wall check's threshold and Hollow's wall thickness are separate.** They shared one value (with different ranges), so setting one changed the other.
- **Topology diagnostics no longer slows the app while open:** it checked the whole mesh on every frame; now only when the triangles change.
- **Touch layout:** the floating size and strength bar no longer covers the axis gizmo and the view strip; on a narrow viewport it wraps onto two rows.
- **Painted areas no longer have a bright outline.** Where vertex color fades to white at the edge of a stroke, the pale in-between was tinted at full strength and brightened, so every painted area had a light rim (a tester compared it with another sculpting app). The edge now blends smoothly from the material into the color; full-strength colors look as before.

## [4.0.0-beta.11] - 2026-11-14

### Added
- **Orbit around the cursor, or the last stroke** (Preferences > **Orbit around**, next to "Strokes can start on empty space", or the command palette):
  - **Cursor** (the default): orbiting turns around the point of the model under the pointer where you start the orbit, so you can turn around any spot you point at, even when your last stroke is off screen. Starting from empty space, it turns around the part of the model nearest the pointer (within a quarter of the view), or else the last pivot.
  - **Last stroke**: orbiting turns around where your last stroke ended.
  - **Centre of the view**: as before.
  - With Cursor or Last stroke, that point stays put on screen while you orbit. Framing (F) goes back to orbiting around the middle until the next stroke or orbit.
- **Keep this level only** (Topology, under Dynamic detail when it can't work, and in the Mesh menu with the other level commands): keeps the subdivision level you see as the piece's mesh and removes the others, in one undo step, so dynamic detail works on it.
- **Tube Twist, Spin and Braid** (in the Tube window, for the Tube brush and Pieces > + Add > Tube):
  - **Twist** turns the tube's cross-section (or a braid) along its length.
  - **Spin** coils the tube around its path like a spring, with a coil size (in tube thicknesses) and the number of turns.
  - **Braid** makes three round strands woven over and under each other along the path, inside the tube's thickness. **Spread** goes from tight (the strands touch) to loose (a strand's width of gap), and **Turn** sets how many weaves; the strands are sized so they never pass through each other, whatever the weave.
  - They combine (a braid can coil), stay editable with Edit tube…, and are saved with the project in a new `TUBX` section. Older versions open such tubes as ordinary mesh rather than as plain tubes. The next tube starts with the last shape used.
- **UVs (texture coordinates) are kept** ([RFC 0020](docs/rfcs/0020-uvs.md), first part): OBJ (`vt`) and glTF/GLB (`TEXCOORD_0`) imports bring their UVs, including UDIM coordinates outside 0..1; they stay on each face's corners, so points that a file splits only along UV seams are welded and sculpt as one surface. Sculpting, masking, undo, the transform gizmo and the Deform window keep them; they're saved with the project (a new `UVCO` section; older versions skip it) and written back out to OBJ (`vt`, quads kept where no seam runs through them) and glTF/GLB, in every compression. Checked with a textured Khronos sample model: its texture lands on OxyBrush's export, and on an OBJ round trip, exactly as on the original. Operations that rebuild the triangles (voxel remesh, dynamic detail, decimate, Subdivide, subdivision levels, Close holes) drop the UVs for now and say so once; Undo brings them back. Next: UVs through subdivision levels and Poly Edit, and a checker view.
- **glTF/GLB import** (File > Open and Import, drag and drop, `oxybrush-cli`, the agent's `import`): `.glb`, and `.gltf` with embedded or external `.bin` buffers. Each mesh in the file's scene becomes a piece, named after its node, placed where the scene puts it (node transforms applied, mirrored ones with their faces kept outward); points split only for normals or materials are welded so the surface sculpts as one. Vertex colors come in; triangle strips and fans become triangles; points and lines are skipped; skinned models come in their rest pose; materials, textures and animations are ignored. Files compressed with `KHR_mesh_quantization` or `EXT_meshopt_compression` (any meshopt mode or filter, as written by gltfpack and OxyBrush) open as usual. Draco-compressed files can't be read (its decoders are C++, which OxyBrush doesn't use) and say so. Checked on Khronos sample models (Box, Duck, Avocado, DamagedHelmet, BrainStem, Fox, Sponza, sparse accessors, every primitive mode) and on gltfpack output.
- **Compressed glTF/GLB export** (File > **glTF compression** under Export, or Preferences > General; `oxybrush-cli … --compress meshopt|quantize|none`; none by default):
  - **Quantized** stores positions as 16-bit, normals and colors as 8-bit, and indices as 16-bit for pieces up to 65,535 points (the standard extension `KHR_mesh_quantization`): about half the size for pieces of that size, three quarters for bigger ones.
  - **Meshopt** quantizes, then compresses every buffer with `EXT_meshopt_compression`, nothing further lost: points renumbered in the order the triangles first use them, vertex data delta-coded, triangles mostly a byte each. A tester's 45k-triangle character went from 1.08 MB to 0.27 MB (25%), the same remeshed to 164k triangles from 3.95 MB to 0.88 MB (22%). The encoder is written from the published specification; meshoptimizer's own decoder reads the files back to exactly the same triangles and shape keys.
  - Shape keys (stroke groups) and vertex color come along. Readers need the extension: three.js, Babylon.js, recent Blender and most engines have it; a few older importers don't, hence the choice. The difference from an uncompressed export isn't visible: rendered side by side in three.js, 18 of 500,000 pixels differ. Files pass the Khronos glTF Validator with no errors or warnings (it can't look inside meshopt data).
- **Sharp Crease brush** (in the brush picker after Noise): cuts a crisp V-shaped crease along the stroke, for hard folds, seams and panel lines; Alt raises a sharp ridge. Where Crease gathers the surface toward each dab's centre, Sharp Crease pulls it toward the stroke's line from both sides, across the stroke only and never past the line, and cuts deepest on the line, so the walls meet in a fold. Its groove is narrower for its depth than Crease's. It shares Crease's **Pinch** setting (how hard the walls are pulled together). About 1.1 ms a dab over 14k points, like the other brushes. Agents can pick it as `sharp_crease`.
- **Sculpt layers made ahead of time** (Mesh > Strokes…, with Living strokes on): **Sculpt into** picks the layer new strokes go into, and **+ New layer** starts one, so you can make a "Wrinkles" layer first and sculpt on it, instead of grouping strokes afterwards. New strokes follow the layer's strength and eye: on a layer at 50% a stroke settles to half when you let go, and the whole layer still fades, hides and exports as a shape key like any group. A hidden layer doesn't take strokes. Plugins and agents can choose it too (`sculpt_into`).

### Changed
- **Python plugins say they're plugins.** A Python plugin used to run through the local AI agent connection, so running one turned on Settings > Local AI agent (and left it on), the status bar said "Agent connected", and its edits were labelled "Agent:". Each run now gets a connection of its own (this computer only, a new token each time, closed when the script ends): the agent setting stays as you left it, the status bar shows **Plugin '*name*' running**, and its edits are undo steps named after it (**Plugin: *name*: …**).

### Fixed
- **The brush cursor follows a hovering pen on Windows.** With a pen tablet (reported with an XP-Pen), the brush ring stayed where the pen last touched until you pressed again: Windows sends a hovering pen's movement as pen input, not mouse moves, and OxyBrush only followed it while the pen touched the tablet.
- **Clay Strips shows a round cursor** like every other brush, instead of a square.
- **Dynamic detail seemed to stop working** after adding subdivision levels: it is off on pieces with levels (it would remove them), but the only sign was a status-bar line that was easy to miss. With Dynamic detail ticked, the Topology section now says so in a warning color, with a **Keep this level only** button that makes it work again; levels left behind by an operation that has since changed the triangles no longer block it.

## [4.0.0-beta.10] - 2026-11-14

### Added
- **A material per piece** (material picker and View > Material: **For: Whole scene | *piece* only**): give the active piece its own matcap, built-in or one you loaded, to tell parts apart (a jade eye, a bronze armour plate) while the rest shows the scene's material. Hovering previews it on that piece only, **Use the scene's material** puts it back, and it's saved with the project in a new `MATL` section (older versions skip it and show the scene's material). Up to 8 different piece materials show at once; a loaded matcap that isn't on this computer falls back to the scene's. With viewport lights on, every piece uses the lit surface as before. The picker now stays open until you click outside it.
- **Living strokes for plugins and agents** ([RFC 0017](docs/rfcs/0017-living-strokes.md), end of phase 2). Plugins (API 4) read the active piece's strokes and groups with `strokes_state` and run any Strokes window action with `stroke_command` (new capability `edit_strokes`), for example fading every stroke in a group; each action is one undo step under the plugin's name. The agent (MCP) gets `list_strokes` and `edit_strokes` with the same JSON, labelled `Agent:` in the history. See the [API reference](docs/plugins/api-reference.md) and [docs/mcp.md](docs/mcp.md).
- **Living strokes: Mirror copy, Split by side, groups as shape keys** ([RFC 0017](docs/rfcs/0017-living-strokes.md), third phase).
  - **Mirror copy** (Strokes window) puts the selected strokes on the other side of the piece's symmetry plane (X, Y or Z) as new strokes you can fade on their own, for when you sculpted a wrinkle with symmetry off. Each point lands on the point nearest its mirror position, so it works where the piece is symmetric; the status line says how many points found no match.
  - **Split by side** turns a stroke made with symmetry on into one stroke per side, so you can fade or delete just one side. Nothing on the model moves.
  - **glTF/GLB export** writes each stroke group as a shape key (morph target) named after it, on the piece without the groups, so the groups become sliders in Blender or a game engine. Visible groups are at 1 and hidden ones at 0, so the file looks like the viewport. Other formats are unchanged.
  - Plugins and agents get the same actions: `mirror` and `split_by_side` stroke commands, with an optional `axis`.

### Fixed
- **Voxel remesh of posed or rigged models:** toes (and other bent joints) no longer come out hollow and torn. When a rig bends a toe down, the skin in the crease underneath can fold back through the sole, leaving thin inside-out pockets; the remesh took their side from those triangles' normals, which point the wrong way there. Such triangles are now judged by the winding number, as overlapping parts already were, and the pocket counts as solid. On a game-character model whose toes tore, no remeshed point is now more than a voxel off the original surface (before: up to 3.6 voxels at resolution 128). Meshes whose normals all face inward (some exports) now remesh correctly too, about three times slower than a normal mesh. Very sharp folds (around 90° over a short joint) can still leave a small tunnel.

## [4.0.0-beta.9] - 2026-11-13

### Added
- **Subdivide masked** (Mask and Mesh menus, command palette): more resolution only where you mask, for example one feather of an imported model (Groups > Group by islands, then Ctrl+M on it), without subdividing the whole piece. The masked triangles split into four, and their neighbours split just enough (in two, or in four where two of their edges are split) to keep the surface closed, so the area blends in (red-green refinement). With **Smooth** on (Topology > Subdivision levels) the new points curve with the surface by the butterfly rule (Dyn, Levin & Gregory 1990) while every existing point stays put, so the rest of the piece doesn't move; off, they sit at edge middles. New points are masked too, so it can be repeated. One undo step; like Subdivide, it removes subdivision levels. 148 ms for a 1% area of a 1.3M-triangle piece. A step-by-step guide for testers, [Adding detail to one part of a model](docs/guides/detail-on-one-part.md), compares it with splitting the part into its own piece and with dynamic detail.
- **Subdivision without smoothing** (Topology > Subdivision levels > **Smooth**, on by default; also under Mesh > Subdivide). Off, a new level (Ctrl+D) or Subdivide only splits the faces, with new points at the middle of each edge and face, so the shape stays exactly as it is: for hard-surface pieces, or for more resolution without softening the forms. Works with quads and triangles. Each level keeps how it was made, so a piece can mix smoothed and split levels, and edits on a lower level carry a split level along by the same rule. Which levels weren't smoothed is saved in a new `MRLN` project section; older versions read the same points but treat every level as smoothed when a lower level is edited. Saved as `subdivision_smooth`.
- **Disc primitive** (Pieces > + Add > **Disc**, command palette): a flat round plate lying on the floor plane, facing up, with an **Outer radius**, an **Inner radius** (above 0 it's a ring with a hole), **Rings** from the middle out and **Segments around**. **Slice** keeps only the part between two angles (From, To), like a slice of pie or of a ring. It's made of quads, with triangles round the centre when there's no hole, and comes with its polygons, so Poly Edit and quad subdivision levels read them exactly: a ready base for hard-surface parts.
- **Living strokes: groups, keep part of a stroke, saving** ([RFC 0017](docs/rfcs/0017-living-strokes.md), second phase).
  - **Groups as sculpt layers:** **Group** puts the selected strokes in a named group ("Group 1", …). Each group row has a box to show or hide its strokes, a strength slider (0–150%, live while you drag) and a **…** menu to select its strokes, rename it (or double-click the name), merge it into another group, ungroup it or delete its strokes. **Add to group** moves strokes into a group, where they take its strength; **Ungroup** takes them out and keeps how they look. A stroke shows at its own strength times its group's.
  - **Keep only where unmasked:** mask the part of a stroke you don't want, select it and press this. That part is taken away; the rest stays and can still be faded.
  - **Strokes are saved with the project** in a new `STRK` section, with their groups, so they stay editable after reopening (also through autosave and recovery), on pieces with subdivision levels too. Turn off **Preferences > General > Save strokes with the project** for smaller files. Older versions skip the section and open the project as it looks. Opening a project with Living strokes off leaves its strokes fixed in place and says so in the Strokes window.
  - Every change in the window is one undo step, renaming included.

### Changed
- **Subdivision levels make quads** ([RFC 0019](docs/rfcs/0019-quad-subdivision.md)). Ctrl+D, Mesh > Subdivide and the retopology tool's Subdivide and project now use Catmull–Clark subdivision: every polygon becomes quads, so a quad base mesh (the human base meshes, a Poly Edit model, a retopology mesh, the quad sphere) keeps its edge loops at every level, the status bar's quad count is exact and OBJ export writes quads. A triangle becomes three quads, so the first level of a triangle mesh has six times the triangles (then four). Poly Edit creases carry into the levels (100% stays sharp; lower weights round off after a level or two). The old behavior, Loop subdivision (each triangle into four), is under Topology > Subdivision levels > **Makes: Triangles**, chosen before adding the first level and remembered (`subdivision` setting); a piece keeps the kind its levels were made with. Quad levels are saved in a new `MRCC` project section; older versions skip it and open the active level as a plain mesh. Pieces with triangle levels open and save as before.

### Fixed
- **Voxel remesh joined close parts with pinched edges** (fingers, toes, lips, nail folds). Where two surfaces came within about a voxel of each other, both were pinned to one point per voxel, so the result had edges shared by four triangles that bridged the parts; brushes, Smooth in particular, could then tear those spots into shreds. The remesh now gives a voxel one point for each piece of surface in it (Manifold Dual Contouring, Schaefer, Ju & Warren 2007), decides ambiguous voxel faces from the sampled values (the asymptotic decider, Nielson & Hamann 1991) so neighbouring voxels agree, separates the rare pinch that's left (a tunnel through a voxel) into two surfaces that touch, and drops specks under two voxels across. The result is a closed surface without pinched edges: on the human base meshes, 0 at resolutions 128 to 512 with both engines, where there were 43 to 75. Parts closer than about a voxel still join; raise the resolution to keep them apart. Remeshing takes about 5% longer.

## [4.0.0-beta.8] - 2026-11-13

### Added
- **Poly Edit: rounded bevels.** Face and edge **Bevel** have **Bevel rows** (1 to 16) and a **Profile** from 0 (flat, a straight cut) to 1 (round). Edge bevels round off across their rows, and where three or more bevelled edges meet the corner face follows the rows; face bevels go out in rings that turn from up the wall to across the cap. **Bridge** and **Split face** now follow **Mirror across X** too.
- **Human base meshes** (File > New > **Human (female)** / **Human (male)**, the Welcome window's **Female** and **Male** cards, Pieces > + Add, command palette): a realistic female and male body to start sculpting from, standing, life size (about 1.65 units tall), 10,570 quads each, with the body parts as face groups (102 of them: arms, legs, fingers, torso…). They're the realistic bodies from Blender's [Human Base Meshes](https://www.blender.org/download/demo/asset-bundles/human-base-meshes/) bundle v1.4.0, released by the Blender Foundation under CC0 1.0, and are credited in Help > About. Their polygons come with them, so the quad count is exact and Poly Edit opens on the real quads. `scripts/blend_meshes.py` (which reads a `.blend` file as data) and the `oxy-cli` `base_mesh` example rebuild them.
- **Keep the gizmo pivot in place** (gizmo bar > Pivot > **Follow the mask**, also Preferences > General): on by default, the transform gizmo's pivot moves to the edge of the mask (or the centre of what moves) after every change, as before. Untick it and the pivot is found once for the piece and then stays put while you mask and unmask; it still moves along when the gizmo moves the piece. Alt+drag, Ctrl+click and the Pivot menu's choices still place it; **Mask edge / centre** finds it again from the mask as it is now. Saved as `gizmo_pivot_follows`.
- **Shell option for the voxel remesh** (Topology > Rebuild > **Shell**): make a wall around the surface instead of filling it, with a **Thickness** (scene units) and a side: **Outward**, **Both sides** or **Inward**. An open sheet gets that thickness; a closed model comes out hollow. A note warns when the wall is thinner than about two voxels. Works with both engines and with Merge + remesh. The triangle estimate counts both walls.
- **Poly Edit: creases and smooth subdivision** ([RFC 0018](docs/rfcs/0018-poly-edit.md), third phase). **Crease** for edges (click; Reach for a loop or ring) and points (every edge at the point), at a **Crease weight** from 5% to 100%; click again to clear. Creased edges show in purple. **Smooth > Preview** (Off, 1, 2, 3 levels) shows the surface smooth subdivision makes, under the polygons you're editing, live while you drag (fewer levels on big pieces, so it stays quick). **Subdivide (smooth)** replaces the polygons with that surface (each face becomes quads) and keeps modeling; **Smooth and sculpt** does the same and closes Poly Edit, ready to sculpt. The subdivision is Catmull–Clark with semi-sharp creases (Catmull & Clark 1978; DeRose, Kass & Truong 1998): 100% stays sharp, lower weights round off after a level or two, borders keep their outline and an open sheet keeps its corners. Creases are saved in a new `CRSE` project section (older versions read the polygons without them) and follow the edges through edits, a loop cut across a creased edge included.
- **Poly Edit: bevel, bridge, slide, spin, split and equalize** ([RFC 0018](docs/rfcs/0018-poly-edit.md), second phase). **Faces:** **Bevel** (drag: an extrude whose cap shrinks in from its edges at 45°), **Bridge** (click a face, then another with as many corners: both are removed and a tube joins them, with **Rows** and **Twist** settings; a handle or a hole through) and **Equalize** (click to even out the spacing of their points along the surface). **Edges:** **Bevel** (drag to cut the edges off into strips of faces, wider as you drag; Reach Edge loop bevels a loop, and where three bevelled edges meet a corner face fills in), **Slide** (drag across an edge, or its loop, to slide it along the edges beside it, towards either side) and **Spin** (click to turn an edge one step within its two faces). **Points:** **Split face** (click a point, then another of the same face: the face is cut in two between them). The first pick of a bridge or split is shown in green; Esc drops it.
- **Repeat in Poly Edit** (**Y**, in the keymap as "Poly Edit: repeat on what's under the pointer"): applies the last action, with the amount of the last drag, to the face, edge or point under the pointer, as one undo step.
- **Living strokes** (Mesh > Strokes…, command palette; [RFC 0017](docs/rfcs/0017-living-strokes.md), first phase): an option, off by default, that keeps each stroke editable after you make it. Turn it on in Preferences > General or with the button in the Strokes window. The window lists the active piece's strokes, newest first (brush, when, how many points it moved); select one or several (Shift or Ctrl+click) and **fade** it with the Strength slider (0–150%, live while you drag), **Delete** it, or **Solo** it to see it without the strokes made after it. Later strokes keep what they did: a stroke's change is added or taken away, not replayed. Each change is one undo step, and undoing a stroke hides it from the list. A remesh, a subdivision level change, dynamic detail or anything else that changes the triangles keeps the surface and clears ("bakes") the list; so does **Bake all**, and the oldest strokes are baked when the list passes its memory budget (256 MB by default, set in Preferences). Mask strokes aren't listed. Strokes aren't saved with the project yet.
- **Quad and UV spheres** (Pieces > + Add > Sphere): the sphere window has a **Kind** choice. **Quads** (now the default there) is a cube pushed out into a ball: even quads all over, no poles, and every quad reads as a quad in Poly Edit and the quad count, with **Divisions per side** to set how many. **UV (rings)** has **Segments around** and **Rings** from pole to pole. **Triangles** is the icosphere it used to be. The quad sphere spaces its points by angle, so the quads at the cube's corners are close in size to the rest (within 2:1 by area). File > New > Sphere is unchanged.

### Changed
- **Remeshing a model with holes fills it** instead of making a thin wall around its surface. The holes are patched first (as Mesh > Close holes does) on a copy, then the model is remeshed solid. An open sheet, which encloses nothing, isn't turned into a thin wall any more: the remesh stops and the status bar suggests Shell. (The old thin wall was about one or two voxels thick, depending on the engine.)

### Fixed
- **Voxel remesh of overlapping parts** ([#174](https://github.com/crisprintsstuff/OxyBrush/issues/174)): a piece whose parts overlap (two shapes joined into one piece, a primitive added with the Insert brush, a fold sculpted through the model) remeshed with holes and inner walls, or, with the Regular grid engine, as a thin shell. The part of the surface buried inside the solid is now found with winding numbers (Barill et al., SIGGRAPH 2018) and the remesh follows the outside only. Hollow models keep their cavity. Merge + remesh and the booleans use the same fix. Remeshing a piece that doesn't overlap itself takes about 10% longer (264 → 291 ms for 330,000 triangles at resolution 128); one that does takes longer still (1.7 s for two overlapping 330,000-triangle spheres at 256).

## [4.0.0-beta.7] - 2026-11-13

### Added
- **Vertex colors come in with imports.** OBJ files bring their colors, written after each vertex (`v x y z r g b`, as 0–1 or 0–255) or as `#MRGB` comment blocks (the way some sculpting apps save painted models), and pieces kept apart keep their own colors. Nothing needs turning on: a painted model shows its colors and can be painted over with the Paint brush. Files whose color block doesn't match their vertices import without colors, as before.
- **PLY import** (File > Open / Import, drag and drop): ASCII and binary PLY files, with their vertex colors (`red green blue`, 0–255 or 0–1). Polygons are split into triangles; other data is skipped.
- **Brush size on the model** (Brush settings > **Size** > **On the model**, or the command palette; for all brushes): the brush becomes a size on the model, in scene units, instead of a size on screen. Zoom in and the circle grows with the model; zoom out and it shrinks, so a stroke covers the same area at any zoom and the Radius value stays the same. Switching keeps the circle the size it is. `[` / `]`, the S slider and the Radius sliders all change the size on the model; it's measured at the surface under the pointer. Curves and tubes keep the size they started with. The choice is saved, and the size too with "Remember brush settings" on. **On screen** stays the default.
- **Actual size and position in the gizmo bar.** Under the move/turn/scale totals, a second row shows the real **Size** (X, Y, Z, in scene units) and **Centre** of what the gizmo moves (the unmasked, visible part), along the world axes, updating live while you drag. Type a size and press Enter to scale to exactly that about the pivot (with **Even** on, the other sides follow in proportion), or type a centre to move it exactly there. Each is one undo step and counts in the bar's totals, so Cancel puts it back. Rotation has no absolute value: a piece's points are changed directly, so there's no stored orientation to show.
- **Poly Edit** (Mesh > Poly Edit (model polygons)…, command palette; [RFC 0018](docs/rfcs/0018-poly-edit.md), first steps): model a low-polygon piece by its faces. Point at a face and the faces the action would change light up; drag to **Extrude** (walls go up around them and they move out as a cap; touching faces move as one block unless **Each face on its own** is on) or **Inset** (a rim of faces inside their edges), drag to **Move** them along their normals, or click to **Delete** them. **Reach** spreads the action from the face under the pointer to its face loop, its face group (all of it or the part touching it), the flat region around it or its whole island, and **Mirror across X** does the same to the other side. Extruded caps and walls, and inset rims, get new face groups so the next action can reach them by group. Each drag is one undo step and Escape cancels it. Opening the tool on a piece makes its polygons from the mesh as it is (triangle pairs that form quads become quads), keeping its mask and color until the first change to its faces; pieces with subdivision levels or more than 400,000 triangles are refused with a note.
- **Poly Edit: edges and points.** Point at an edge or a point instead of a face and its own action applies. **Edges:** **Insert loop** (click to cut a loop across the quad ring where you clicked, drag along the edge to slide it), **Delete** (remove edges, or with Reach Edge loop or Edge ring a whole loop or ring, merging the faces on each side; points left in the middle of a straight run are dropped), **Extrude border** (drag a border edge, or the whole border with Reach Edge loop, outwards to pull out new faces), **Close hole** (one face fills the hole). **Points:** **Move** (drag across the screen; drop on another point to weld the two) and **Delete** (with the faces around it). Mirror across X applies to all of them.
- **Polygons with more than four corners** in the polygon data that Poly Edit and the retopology tool share, triangulated by ear clipping. They're saved in a new `PLYC` project section together with each face's face group; versions without it read the older `CAGE` section, where such faces are stored as triangles.
- **Fill mask** (Mask menu, Mask section and command palette; bindable in the keymap): masks the whole visible surface in one undo step.

### Changed
- **Ctrl+W is Group from mask**, as in other sculpting apps: the masked faces become a new face group, and with nothing masked it groups the visible faces as before. Group visible keeps its place in the Groups menu and can be given its own key in the keymap. The Group from mask tooltip said "unmasked faces"; it's the masked ones.
- **Ctrl+plus and Ctrl+minus zoom the view, not the interface** (Cmd on macOS, as with the other shortcuts). They used to resize the whole interface, a default of the interface toolkit that bypassed View > Theme > Size; now they are the new **Zoom in** / **Zoom out** commands (View menu, command palette, rebindable in the keymap; Ctrl+= works for zoom in too, without Shift). **Ctrl+0** frames the active piece, like F. The interface size is set only in View > Theme > Size.
- **Transform gizmo fields are live.** The fields under the gizmo show how far the piece has moved, turned or scaled since the gizmo opened (or the last Apply), and update while you drag a handle. Type a value and press Enter to make the total exactly that (typing 2 in Scale doubles the size it started at, however far you dragged). **Apply** keeps the changes and counts from zero again; the new **Cancel** puts them back; **Done** closes the gizmo and keeps your changes; to have Done put back what wasn't applied, like Cancel, turn on **Preferences > General > Closing the gizmo puts back changes not applied**. Cancel is an undo, so Redo brings the changes back. Mirror, Put on the ground and Centre at origin are applied at once.
- **Deform window on very large pieces.** On pieces over 250,000 points the deformer is worked out in the background, so the bar follows the pointer while the model catches up, and only the newest position is ever worked out. Letting go applies exactly where the bar is. The amount follows where the pointer is, not the sum of its movements, so a fast drag on a slow frame isn't lost, and a flick that starts and ends between two frames now applies instead of doing nothing. Updating normals and bounds after a whole-piece change is 2.6× faster (78 ms instead of 202 ms on 8.4M triangles, 4 cores).

### Fixed
- The guide said remesh and decimate remove vertex color; they keep it (since 3.0.5-beta).
- **Invert mask respects hidden faces:** it now only inverts the visible surface, and hidden parts keep their mask (it used to mask everything that was hidden).
- **Deform window:** letting go at zero after previewing put the shape back but not its shading.

## [4.0.0-beta.6] - 2026-11-13

### Added
- **Undo on this piece** (Ctrl+Alt+Z, redo Ctrl+Alt+Shift+Z, Edit menu and command palette): takes back the last change to the active piece only, even if you changed other pieces since. **Preferences > General > Undo works on the active piece** makes that what Ctrl+Z does, with **Edit > Undo in whole scene** for the usual undo. A change that also changed other pieces (Join, Merge, Split…) is only undone this way when nothing came after it; otherwise the status bar explains why. The History section can show only the active piece's changes.
- **Edit > Last stroke:** reopen the Freehand stroke you just made as an editable curve with the same brush (**Edit as curve**, also a keymap command), or use its path as a **Curve Move curve** or a **tube path**. The stroke is taken back first, and its path is placed where it was drawn even if the view has turned since. Works right after the stroke, whether or not hold-to-curve is on.

## [4.0.0-beta.5] - 2026-11-12

### Added
- **Deform window** (Mesh > Deform…, [RFC 0016](docs/rfcs/0016-deform-window.md)): 25 whole-piece deformers in three sections. **Shape:** move, rotate, scale, bend, S-bend, skew, S-skew, flatten, flatten to axis, flatten one side, twist, taper, squeeze, perspective and sag. **Surface:** noise, inflate, inflate keep size and spherize. **Smoothing:** smooth, polish (plain, keep detail, keep group edges, keep sharp edges) and relax. Each has large X/Y/Z toggles. Drag a bar to see the result live; let go to apply it as one undo step (the bar springs back, Escape cancels). The mask holds parts back unless **Use mask** is off. **Repeat on this piece** and **Repeat on other pieces** reapply the last one; **Fit to standard size** centres the piece and scales its longest side to 2 units; Mirror and Symmetrize are repeated in the window.
- **Mask border** and **Mask group borders** (Mask menu and command palette, [#170](https://github.com/crisprintsstuff/OxyBrush/issues/170)): mask the edge of the visible surface (an open mesh's border, or the edge of what's left visible when part of it is hidden), or the lines where face groups meet. Other visible points are unmasked, hidden ones keep their mask, and **Grow** widens the result. One undo step; when there's no border, nothing changes and the status bar says why. About 60 ms on 1.3M triangles.

### Changed
- **New interface font and softer text.** The interface uses [Inter](https://rsms.me/inter/) (Regular for text, SemiBold for headings) instead of the toolkit's thin default, Ubuntu Light, so text is sturdier and easier to read on dark panels. Text in the dark themes is toned down a little (about 205 instead of 228 on a 0–255 scale), and bold and highlighted text is no longer pure white. Inter is under the SIL Open Font License 1.1, which ships next to the app's licenses and is credited in Help > About.
- **Mirror X / Y / Z buttons are larger** (toolbar and Symmetry section), easier to hit with a mouse or a pen.

## [4.0.0-beta.4] - 2026-11-12

### Added
- **Button and brush icon colors** (View > Theme… > Colors): **Custom button color** gives buttons and other controls a color of your own instead of the theme's (a little lighter on hover; High Contrast keeps its own), with a note when button text might be hard to read on it. **Brush icons** can keep their terracotta **Clay**, **Match accent**, or use a **Custom** color, with a preview row; Mask, Paint and the other icons that show their own colors keep those parts. Saved as `button_color`, `brush_icon_color` and `brush_icon_custom`; **Reset everything** puts both back.
- **Mesh > Symmetrize**: makes the active piece symmetric. It keeps one half (+X onto −X, −X onto +X, and the same for Y and Z), replaces the other half with its mirror image and welds the two along the piece's symmetry centre, so the result is one closed surface. Triangles crossing the middle are cut there, so the seam is straight. Mask, vertex color and face groups are mirrored too. Runs in the background (about 0.1 s for 80k triangles); one undo step; subdivision levels are removed. Also in the command palette for X.
- **Split masked** (Mask menu, Mesh menu, the masking tools and the command palette): moves the masked part of the active piece into a new piece and keeps the unmasked part in the original. A face goes with the mask when its corners average at least half masked, so a soft mask edge is cut through its middle. Invert the mask first to split off the unmasked part. Both parts come out unmasked; one undo step.
- **Join** (Pieces panel, Pieces menu, command palette): combines the active piece and the other visible pieces into one piece exactly as they are, with no remesh and nothing welded. Triangles, mask, hidden faces and vertex color are kept, and each piece's face groups stay apart from the others', so a joined piece can still be hidden or selected by group. One undo step; subdivision levels are dropped (undo restores them).
- **Five bundled in-app plugins** for Plugin API 3, in `crates/oxy-plugin/plugins/` and copied into the plugins folder like the others (a folder you already changed is left alone):
  - **Group by Angle:** face groups from the angle between neighbouring faces, with a live preview. Complements Group by Height.
  - **Hide Faces by Angle:** hides faces by how far their normal points from an axis, with Invert and an option to leave masked faces alone. The first bundled plugin that writes hidden faces.
  - **Color by Group:** paints one face group in a color you pick, or every group in its own color. Uses the color setting. Masked vertices keep their color. Live preview.
  - **Fill Holes:** caps open holes with new triangles and does not move the existing surface. Reports progress and can be stopped.
  - **Simplify Mesh:** removes triangles while keeping the shape, open edges, face-group borders and fully masked vertices. Reports progress and can be stopped.

  Fill Holes and Simplify Mesh use `edit_topology`. After replacing the triangles they write the mask, color, face groups and hidden faces back, because a new mesh starts blank. Subdivision levels are cleared, and Undo restores them.

### Changed
- **Merge is now "Merge + remesh"** (Pieces panel; "Merge and remesh visible" in the Pieces menu). It always fused the visible pieces with a voxel remesh, which the Pieces panel's plain "Merge" button didn't say. Use **Join** to combine pieces without remeshing.

### Fixed
- **Retopology: Tab+drag pulls out a quad again.** Tab+drag on a border edge never worked in the app: the key handling that keeps Tab from moving focus between panels returned before OxyBrush noted that Tab was held, so the retopology tool never saw it. The tool's tests set the flag directly and missed it; they now go through the same key handling as the app. Dragging so far that the new quad would leave the model now says so in the status bar instead of doing nothing.

## [4.0.0-beta.3] - 2026-11-11

### Added
- **Pen gestures** ([RFC 0015](docs/rfcs/0015-pen-gestures.md)): with a pen (recognised by its pressure), dragging in a panel or list scrolls it, unless a control such as a slider takes the drag, and holding the pen still on a control for 0.8 s right-clicks it, opening its menu and pinning its tooltip without activating it. Dragging over a panel with a side button (right or middle) held scrolls it too. A new **Pen side buttons** navigation preset: lower side button orbits, with Shift pans, with Ctrl zooms; upper side button pans. **Preferences > Pen & tablet > Drag empty space to scroll**: with a pen (default), always (for drivers without pressure) or never. Labels don't start a text selection while pen gestures are on.
- **Slider thickness**: **Thin**, **Normal** or **Thick** slider bars, under **View > Theme… > Size & shape**. Only the bars change; the rest of the interface keeps its size. Thick bars are easier to see and to hit with a pen. Saved as `slider_size`; **Reset everything** puts it back to Normal.

### Changed
- **One Theme window** ([RFC 0014](docs/rfcs/0014-theme-window.md)): **View > Theme…** (or the command palette, or **Preferences > Interface**) opens a single window with three tabs: **Themes** (a gallery of the nine themes drawn in their own colors, and **Looks**: Classic, Workstation, Midnight), **Colors** (accent and viewport background) and **Size & shape** (interface scale and corners), plus **Reset everything**. It replaces the long Preferences > Appearance tab, View's Background menu and window, and the old Theme & Appearance window. **Preferences > Appearance** is now **Preferences > Interface** (brush-shelf options). Looks no longer change shortcuts, navigation or the material (set those in the Keymap and the material picker); "Default Studio" is now **Classic**, with the real default background. The command palette gains **Theme: Studio**.

## [4.0.0-beta.2] - 2026-11-11

### Added
- **Drag the axis gizmo to orbit:** dragging anywhere on the axis gizmo at the top right of the viewport turns the view freely, as dragging empty space does (the snap key steps it by 45°); clicking an axis end still snaps to that side.
- **Pin points in the retopology tool:** Alt+click a point, edge or face to pin its points (with X mirror, their mirror images too); Shift+drag relax leaves pinned points where they are, while you can still drag them by hand. Alt+click again or **Unpin all** in the window frees them. Pinned points are drawn as squares in the accent colour, pinning is an undo step, and pins are saved with the project (a new `PINS` section after the cage; older versions skip it).

### Fixed
- **Opening a file no longer leaves the Home window on top:** with **Show at startup** on, a project double-clicked in Finder (or opened with Open With, dropped on the window or picked in the Open dialog) opened behind the Home window. Opening any file now closes it, as starting OxyBrush with a file already did.
- **macOS: `.oxb` projects showed a generic "?" icon** in Finder, and the Dock and Open With didn't know OxyBrush opens them, when macOS had missed the app's document types (for example after opening it first from the disk image). OxyBrush now registers itself with macOS's Launch Services (the system `lsregister` tool, in the background) each time it starts from its installed location. When macOS runs it from a temporary translocated copy, it logs a hint to move it to Applications instead.

## [4.0.0-beta.1] - 2026-11-11

OxyBrush now uses [semantic versioning](https://semver.org): this is the first beta of 4.0.0 and follows 3.0.8-beta. Betas count up (`4.0.0-beta.2`, …) until 4.0.0 is stable.

### Added
- **Capture from view at up to 4096 px:** a size picker next to **Capture from view** offers 256, 512, 1024 (the new default, was a fixed 256), 2048 and 4096 px; larger captures have smooth edges instead of blocky ones. The model is now rasterized into a depth image instead of ray-cast per pixel, so even 4096 px is quick (about 1.2 s for a 1.3-million-triangle sculpt on four cores, where ray casting took two minutes); it runs on a worker thread so the interface stays responsive, and the size is remembered.
- **Alphas are saved with the project:** captured and loaded alphas are stored in the `.oxb` (a new `ALPH` section, 16-bit, compressed) and come back when you open it; opening it again doesn't duplicate them. A new alpha counts as an unsaved change. Older versions skip the section.
- **Export alpha…** saves the selected alpha (a capture, a loaded image or a built-in) as a 16-bit grayscale PNG to edit in a 2D app; **Load image…** brings it back. Also in the command palette.

### Fixed
- **File > New > Cube** (and the welcome window's cube) made a cube with open seams: 128 edges where two faces met weren't joined, so it shaded and sculpted badly; a cube added with **Pieces > Add** was fine. Vertex welding now also compares neighbouring cells, so points a rounding error apart always join (STL import welds this way too: about 25% slower, 0.5 s for 1.3 million triangles). Projects already started from the broken cube keep its seams; a voxel remesh repairs them.

## [3.0.8-beta] - 2026-11-11

### Added
- **File > Close project:** closes the project (asking to save unsaved work first) and goes back to the start screen. Also in the command palette.

### Fixed
- **OBJ files with material problems import:** materials are now ignored on import. `mtllib`/`usemtl` lines are skipped and no `.mtl` file is read, so a bare `usemtl` with no name (which failed the whole import with "material parse error"), a missing or broken `.mtl`, or names that aren't UTF-8 no longer stop an OBJ from opening. Material switches no longer split an object into several pieces.
- **Imports far out of scale are fitted to the scene:** mesh files carry no units, so a model in millimetres (common for STL) arrived tens of times larger than the starting sphere. An import more than 8× larger or smaller than the scene is now scaled by a power of ten (a unit conversion, like mm to cm), about the origin; later imports into the same document reuse that factor so parts from one source still fit together. The status bar says so, and one Undo ("Fit import to scene") restores the original size. Blender bridge and OxyCharacter round trips are unchanged. An agent's import labels both undo steps `Agent:`.
- **Box and lasso masking follow symmetry:** with Mirror or radial symmetry on, Ctrl+drag masking (and Ctrl+Alt unmasking) now picks the mirrored region too, as strokes do.
- **Window title bars are no longer huge:** they were about 53 px tall at 100% scale, because egui sizes them from the heading font (20 pt) plus the window margin above and below the title. The heading font is now 15 pt (14–18 pt depending on the layout) and the window margin 9 px (11 in Touch), so title bars are about 37 px (45 in Touch). In-window headings are a little smaller too.

### Changed
- **Studio theme refinements:** slider handles are solid bars with no outline, corners are a little squarer (60% of the chosen corner style's radius), and windows and menus have a smaller, fainter shadow.

## [3.0.7-beta] - 2026-11-11

### Added
- **Narrow brush shelf:** **Preferences > Appearance > Narrow brush shelf (one column)** turns a side shelf into a single narrow column, leaving more room for the model. The current brush, alpha and material are stacked on top, then one brush per row with its name underneath (icons only with brush names off).
- **Studio theme** (Preferences > Appearance, [#81](https://github.com/crisprintsstuff/OxyBrush/issues/81)): flat neutral greys with thin edges, sliders drawn as bars with a fill and a thin handle, small uppercase section headings with dividers, and the accent kept for what's selected or active, shown as an outline. Optional; Graphite stays the default.
- **Hide brush names in the shelf:** **Preferences > Appearance > Show brush names in the brush shelf**, on by default. Off shows the brushes as a grid of icons, so more fit at once; hover one for its name. Works with every theme and layout.
- **The window opens where you left it** ([#95](https://github.com/crisprintsstuff/OxyBrush/issues/95)): its size, position and maximized state are remembered between sessions. If it was last on a monitor that's no longer connected, it opens on one that is, at the same size. On Wayland, where apps aren't told their position, the size and maximized state are remembered.
- **Plugin API 3** ([RFC 0013](docs/rfcs/0013-plugin-api-3.md)): in-app plugins can now:
  - change face groups and hidden faces, and replace a piece's triangles (new `edit_topology` capability);
  - show their result live before you **Accept** it;
  - offer toggles, choices, colors, text and whole-number sliders in their panels, with tooltips;
  - report progress, with a **Stop** button that discards a stopped command's edits.

  Panel settings are remembered between sessions (**Reset** restores the defaults). A command's edits to a piece are still one undo step. Plugins written for API 1 and 2 keep working. Two new bundled plugins, **Group by Height** and **Flip Faces**, and **Displace** now previews.
- **Plugin guide and API reference** in [docs/plugins](docs/plugins/README.md), with a WebAssembly-text and a Rust example.

### Changed
- **Meshes use about 18% less memory** ([#81](https://github.com/crisprintsstuff/OxyBrush/issues/81)): each vertex's neighbours are now worked out from its triangles when needed instead of being stored. A 1.3-million-triangle mesh takes 68 MB instead of 82 MB, and it's built a little faster. The Smooth and Polish brushes take about 0.4 ms longer per dab on a 14,000-vertex brush, still well within a frame.

### Fixed
- **Insert Primitive, Duplicate and dynamic detail work with the triangle limit set to Unlimited.** Unlimited is stored as 0, and these three took it as a limit of zero triangles: inserting a primitive said "would exceed maximum triangle limit 0" and added nothing, Duplicate refused the same way, and dynamic detail stopped adding detail at once.
- **Stopping a plugin right after starting it works.** A Stop clicked in the moment between starting a plugin command and it beginning to run was forgotten, and the command ran to the end.
- **Double-clicking a project opens it on macOS** ([#135](https://github.com/crisprintsstuff/OxyBrush/issues/135)). macOS showed "can't open files of this type": the app didn't declare its file types, and macOS hands a double-clicked file to an app in a way OxyBrush didn't receive. Now `.oxb` projects and `.oxyaddon` add-ons open with OxyBrush from Finder, Open With and the Dock, and OBJ and STL files list it under Open With.
- **Drop files on the window to open them**, on every system: a project opens (asking to save first if needed), a mesh is imported, an add-on is installed.
- **Pen pressure on Linux** (X11 and XWayland, including the Flatpak; [#81](https://github.com/crisprintsstuff/OxyBrush/issues/81)). Tablets report pressure as a raw reading, from 0 up to 2047, 8191 or 65535 depending on the model, and OxyBrush only understood values between 0 and 1, so on most tablets pressure had no effect. The pen's pressure axis is now found by name and scaled properly. Other pen axes, such as tilt, can no longer be mistaken for pressure. The log notes when pressure first arrives, to help with tablet reports.

## [3.0.6-beta] - 2026-11-10

### Added
- **Retopology: Subdivide and project.** In the Retopology window, **Subdivide and project** gives the retopology mesh 1–5 subdivision levels, each moved onto the sculpt, in the background. The top level carries the sculpt's detail and level 1 stays exactly the quads you drew, so you can step between them like any subdivision levels, and keep sculpting. One undo step.
- **Vertex color in exports:** painted pieces export their color in **glTF/GLB** (`COLOR_0`), **PLY** (8-bit red, green, blue) and **OBJ** (`v x y z r g b`, read by Blender, MeshLab and ZBrush). Unpainted pieces export as before.
- **Brush settings are remembered between sessions:** the brush you were using, with its size, falloff, focal shift, stroke mode, alpha and the rest, comes back the next time OxyBrush starts, and every brush keeps its own strength, when **Preferences > General > Remember brush settings between sessions** is on. It's off by default, so every session starts with the default brushes.
- **Edit a tube after Accept:** **Pieces > Edit tube…** reopens the active piece's last tube with its path, thickness, sides and caps, and **Update tube** replaces it, as one undo step. Tubes stay editable after saving and reopening the project (a new `TUBE` section, which older versions skip), until they're sculpted, remeshed or regrouped.
- **Curve Move curves are saved with the project:** saving while a Curve Move curve is open keeps it (a new `CURV` section, which older versions skip), and opening the project brings it back with the brush size, strength and falloff it was bending with. The piece itself is saved unbent until you apply the curve.

### Fixed
- **Dynamic detail makes even triangles and stays at the brush.** Refinement used to spread past the brush in zigzag spikes and leave thin, stretched triangles that made the surface look crumpled. Now edges are split only under the brush, new points follow the curve of the surface instead of cutting across it, and edges are flipped and points relaxed along the surface, as in standard remeshing. On a test stroke, thin triangles went from 9% to none and the surface is visibly smoother. Refining a very large mesh takes about 10% longer.
- **The Interface scale slider no longer runs away** ([#116](https://github.com/crisprintsstuff/OxyBrush/issues/116)). It changed the scale while you dragged it, which moved the slider under the pointer and sent the scale jumping to 300%. Now the number follows the drag and the scale changes when you let go, in 5% steps, and the Preferences window stays where it is on screen.
- **Saving to network drives** ([#117](https://github.com/crisprintsstuff/OxyBrush/issues/117)). Saving to an SMB share could fail without a visible message, so the file was missing and quitting kept asking to save. Saving now works around what such drives refuse (hidden temporary files, a full flush to disk, replacing a file by renaming) without risking the existing file, checks the file is there afterwards, and shows a **Couldn't save** dialog with the reason and a **Save As…** button when it still fails. The reason is also written to the log.
- **Switching subdivision levels without editing no longer nudges the lower levels.** Each switch re-applied the last edit's spread to the levels below, so the base moved a little every time.
- **OBJ export uses each mesh's own quads** (for example a retopology mesh) instead of guessing them from the shape, so strongly bent quads no longer come out as two triangles.

## [3.0.5-beta] - 2026-11-10

### Added
- **Retopology tool** ([RFC 0012](docs/rfcs/0012-retopology.md)): **Mesh > Retopology (draw quads)…** rebuilds a sculpt as a clean, low-polygon mesh of quads that you draw by hand on its surface.
  - **Click** places a point on the surface; **drag** a point, edge or face to slide it along the surface; drop a point on another to **weld** them.
  - Hold **Shift** to preview the quad (green) or triangle (purple) the nearest points make; **Shift+click** makes it. **Shift+drag** relaxes the points under the cursor.
  - **Tab+drag** a border edge pulls out a new quad; **Ctrl+click** an edge inserts an edge loop; **Ctrl+Shift+click** deletes a point or face.
  - The result is a new piece, **Retopo**, with one undo step per action. It is saved with the project (new `CAGE` section, skipped by older versions), and its quads export as quads. Reopening the tool continues it.
  - **Mirror across X** (on when X symmetry is): every point, face, loop, move, relax, extend and delete is mirrored across the piece's X symmetry plane. Points placed near the plane snap onto it and are shared by both sides.

### Changed
- **Voxel remesh is much faster:**
  - On a 330k-triangle sculpt (4-core machine), remesh at resolution 128 takes 0.32 s instead of 5.4 s, at 256 0.71 s instead of 6.8 s, and at 512 2.4 s instead of about 13 s. Merge and booleans use the same code and speed up too.
  - Triangles now write distances only into the voxels near them, and the narrow band grows outward from there, instead of every voxel testing every triangle near it. Lookups also use a faster hash, and polygonizing runs in parallel.
  - The result is the same mesh: distances near the surface are exact, and further out they are within 0.06 of a voxel.
- **Remesh shows how many triangles it will make** under the Resolution slider (a warning above 8 million), so the higher resolutions the faster remesh allows don't come as a surprise. The local AI agent can now ask for resolutions above 320 with the default engine, like the slider.
- **Decimate is about 5× faster on large meshes:** 1.3M → 131k triangles takes 0.7 s instead of 3.8 s on a 4-core machine. Large meshes are decimated in spatially compact parts in parallel, with the vertices between parts left alone, then finished in one pass over the whole mesh. The result is just as close to the original (on a test sphere, slightly closer) and stays watertight.
- **Remesh, Merge, booleans and Decimate keep vertex color, mask and face groups.** Remesh and booleans take them from the closest point of the original surface; Decimate keeps them on the vertices it leaves. Merge keeps each piece in a face group of its own, so they can still be picked out.
- **Rotate gizmo with rings:** Rotate mode now shows a ring around each axis (red X, green Y, blue Z; the far half faint) and a white outer ring for turning around the view direction, like most 3D apps. Drag along a ring and the point you grabbed follows the pointer; Shift still snaps, and typed values, pivot choices and Alt/Ctrl still work.

## [3.0.4-beta] - 2026-11-09

### Added
- **Transform gizmo operators:**
  - **Plane handles:** small squares between two axes move or scale along both at once. The centre handle in Scale still scales evenly.
  - **Snapping:** hold **Shift** while dragging a handle (or turn on **Snap** in the gizmo bar) to move in steps of 0.1, turn in 15° steps and scale in 10 % steps. Steps are set under **Pivot**.
  - **Typed values:** the gizmo bar has fields for the current mode (Move by X/Y/Z, Turn by X/Y/Z degrees, Scale by, even or per axis) and **Apply** (or Enter). They work along the gizmo's axes, about its pivot, as one undo step.
  - **Pivot menu:** put the pivot on the mask edge or centre (default), the bounding box centre or bottom, or the world origin; set the axes to the world or align them to the view. A pivot you place travels with the piece.
  - **Operators menu:** **Mirror X / Y / Z** through the pivot (whole piece; faces stay facing out), **Put on the ground** (lowest point at Y = 0) and **Centre at origin**. Also in the command palette.
  - Masked parts stay put for all of them, as with dragging.
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

[Unreleased]: https://github.com/crisprintsstuff/OxyBrush/compare/v4.0.0-beta.24.5...HEAD
[4.0.0-beta.24.5]: https://github.com/crisprintsstuff/OxyBrush/compare/v4.0.0-beta.24.4...v4.0.0-beta.24.5
[4.0.0-beta.24.4]: https://github.com/crisprintsstuff/OxyBrush/compare/v4.0.0-beta.24.3...v4.0.0-beta.24.4
[4.0.0-beta.24.3]: https://github.com/crisprintsstuff/OxyBrush/compare/v4.0.0-beta.24.2...v4.0.0-beta.24.3
[4.0.0-beta.24.2]: https://github.com/crisprintsstuff/OxyBrush/compare/v4.0.0-beta.24...v4.0.0-beta.24.2
[4.0.0-beta.24]: https://github.com/crisprintsstuff/OxyBrush/compare/v4.0.0-beta.23.3...v4.0.0-beta.24
[4.0.0-beta.23.3]: https://github.com/crisprintsstuff/OxyBrush/compare/v4.0.0-beta.23.2...v4.0.0-beta.23.3
[4.0.0-beta.23.2]: https://github.com/crisprintsstuff/OxyBrush/compare/v4.0.0-beta.23...v4.0.0-beta.23.2
[4.0.0-beta.23]: https://github.com/crisprintsstuff/OxyBrush/compare/v4.0.0-beta.22...v4.0.0-beta.23
[4.0.0-beta.22]: https://github.com/crisprintsstuff/OxyBrush/compare/v4.0.0-beta.21.4...v4.0.0-beta.22
[4.0.0-beta.21.4]: https://github.com/crisprintsstuff/OxyBrush/compare/v4.0.0-beta.21.2...v4.0.0-beta.21.4
[4.0.0-beta.17]: https://github.com/crisprintsstuff/OxyBrush/compare/v4.0.0-beta.16...v4.0.0-beta.17
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

# What's coming in OxyBrush

A look ahead for testers. Nothing here is promised or dated: plans change as
we learn from you. The newest release is
[4.1.4](releases/v4.1.4.md).
The detailed engineering plan is in the [roadmap](ROADMAP.md).

Tell us what you think, and what you'd want first, on
[Discord](https://discord.gg/Vfm7Km7xvG) or with **Help > Send feedback…**
in the app.

## The big one: Living strokes

> Every stroke stays editable, even after you've sculpted on top of it.

**Status:** Living strokes are available in the Layers panel after you enable
them in Preferences > General. Sculpt and paint layers keep their strength
and visibility controls when you switch or add subdivision levels. You can
also save a masked gizmo pose in a layer. The Recipe Box is available. Save a group
of strokes as a recipe, then drag it onto another surface as a single copy
or a mirrored pair. Placed recipes stay editable as stroke groups, and you
can move them on the surface. The library changes when you explicitly save
back to it. See [RFC 0026](rfcs/0026-parts-bin.md) for the design.

With Living strokes off, once a stroke is done it's part of the model. If a wrinkle you made
twenty minutes ago is too deep, you either undo everything since, or sculpt
over it. Other sculpting apps work the same way: you can plan ahead with
layers, but a stroke you didn't put on a layer is final.

With **Living strokes**, OxyBrush remembers what each stroke did. A new
**Strokes** panel lists them, newest first, and for any stroke, without
undoing anything after it, you can:

- **Fade it:** drag its strength from 0 to 150% and watch the model change.
- **Delete it:** the stroke's change goes away; your later work stays.
- **Keep part of it:** mask the part you don't want and remove it from just
  that stroke.
- **See it alone:** hover a stroke to see where it touched the model; Solo
  shows only that stroke's effect.
- **Group strokes after the fact:** select a few strokes ("all the wrinkles"),
  give them a name, and fade or hide the whole group. It's a sculpt layer you
  didn't have to create in advance.

What to expect:

- **It's like layers, not time travel.** Fading a stroke adds or removes
  exactly what that stroke did. Strokes made on top of it keep what *they*
  did; they aren't redone. (An exact "redo everything after it" mode may come
  later for small models.)
- **Some tools set the strokes in stone.** Anything that rebuilds the
  model's triangles (voxel remesh, dynamic detail, decimate, booleans) bakes
  the stroke list first, and OxyBrush tells you when it does.
- **Strokes are saved with the project,** and older versions of OxyBrush still
  open the file.

It arrived in steps: first the panel with fade, delete and solo, then
groups and saving, then extras: copying a stroke to the other side,
splitting a symmetric stroke into one per side, and exporting groups as
shape keys in glTF.
The full design is in [RFC 0017](rfcs/0017-living-strokes.md).

**Questions for you:**
- Which would you use most: fading, deleting, or grouping into layers?
- Should a stroke made with symmetry be one entry, or one per side?
- Would you rather have this fast and "like layers", or exact but slower on
  big models?

## Also planned

### Sculpting
- **Stencils:** an image laid over the view that you sculpt through.
- **A dedicated brush editor window and multiple named shelves** (Phase 1 shipped: Favorites strip, starred presets, rename/duplicate, palette presets — [RFC 0049](rfcs/0049-custom-brush-shelves.md)).
- **Smooth preview persistence and brush-on-cage feedback** beyond Topology's live Catmull–Clark preview ([RFC 0048](rfcs/0048-dynamic-smooth.md), #447).

### Models and topology
- **Faster dynamic detail on big models** (it's smooth up to a few hundred
  thousand triangles today).
- **Retopology:** edge sliding. Contour quad strips are available in 4.1.3.
- **Edge-preserving polish** for hard-surface work.

### Painting and materials
- **Materials per region:** different matcaps or shading for different parts
  of a model.

### Performance
- **Smooth sculpting at 10 million triangles,** measured on reference
  hardware.

### Platforms
- **iPad with Apple Pencil:** an iPad version, then palm rejection and
  pressure tuning, then a TestFlight beta. (Waiting on an Apple developer
  account.)
- **Signed releases:** a signed Windows installer and a notarized macOS app,
  so the security warnings on first launch go away.

### Sculpting with copies
- **More surface instances:** the first part shipped in 4.0.0-beta.12
  (copies on every face, point or a face group, following the surface,
  Make real). Masked placement and even scattering also shipped. Next:
  copies on edges, several per face, turning single copies off, copies
  fitted into each face for chainmail and tiles, and glTF export that keeps
  them as instances ([RFC 0022](rfcs/0022-surface-instances.md)).

### Working with other apps
- **Gaussian splats** (scans from capture apps): open one as a reference
  next to your sculpt, turn it into a mesh with its colors, sculpt the
  splats by sculpting a mesh they're bound to, and send them to Blender
  and back ([RFC 0021](rfcs/0021-gaussian-splats.md), proposed).
- **More from the Blender bridge**, and more tools for local AI agents.

## Recently shipped (for context)

- **4.1.4.** Quad hands and feet, plus import inspection and undoable mesh repair. [Release notes](releases/v4.1.4.md)
- **4.1.3:** OxySpheres branch rotation and connected quad junctions, Retopology Contour, repeatable lower subdivision level recovery, and sculpt and paint layers across levels. [Release notes](releases/v4.1.3.md)
- **4.1.2:** Closed-surface extraction fixes for OxyRemesh. [Release notes](releases/v4.1.2.md)
- **4.1.1:** Focus layout, mode-aware inspector, and OxyRemesh extraction fixes. [Release notes](releases/v4.1.1.md)
- **4.1.0:** Split view, Scale cover Phase 2, Also paint, and OxySpheres Resolution and Blend. [Release notes](releases/v4.1.0.md)

- **4.0.0-beta.24.5:** Paint Color Jitter, lit surface controls in Materials,
  and inline errors for rejected OxySpheres quad builds.

- **4.0.0-beta.24.4:** updated Accumulate defaults, separate Delete and
  Dissolve edge actions, mirrored Knife cuts, and smoother captured alpha
  silhouettes.

- **4.0.0-beta.24.3:** per-brush Accumulate, an experimental OxySpheres
  quad base mesh, and reliable regeneration after duplicating a piece.

- **4.0.0-beta.24.2:** OxySpheres make rounder thin limbs and update the
  last unsculpted mesh. Closed Knife cuts keep a polygon rim, and inactive
  pieces can draw at their lowest subdivision level during edits.
- **4.0.0-beta.24:** OxySpheres stay editable as a separate piece after
  making a mesh. Poly Edit gained paint selection and Knife cuts across
  convex edges.
- **4.0.0-beta.21:** tabbed projects keep each project's tool, camera,
  and undo history. The Recipe Box lets you reuse groups of living strokes.
- **4.0.0-beta.18:** mask blur and sharpen with Ctrl+click, Hide masked,
  Alt-to-mask-outside for mask boxes and lassos, Sort brushes A–Z, and
  brush size and Floor grid fixes in an empty scene.
- **4.0.0-beta.17:** empty scenes, batch import, gizmo-axis Poly Edit extrude,
  masked-area face groups, export type selection, smoother window resizing,
  updated brush defaults, and fixes for crease symmetry, Kelvin Grab, Poly Edit
  picks and grouping visible islands.
- **4.0.0-beta.16:** living strokes remember what was drawn, including
  curves, stamps, lines and a captured alpha, Edit… to sculpt a stroke
  again with a new brush, Raise or Carve, size, strength, falloff, alpha
  or spacing, and an adjustable hard-edge angle (40° by default; 180° is
  Off).
- **4.0.0-beta.15:** one transform gizmo for move, turn and scale, the
  gizmo in Poly Edit with Shift+drag to extrude, Poly Edit soft selection,
  moving several pieces together, Merge down, and sharp or smooth corners
  point by point in tube profiles.
- **4.0.0-beta.14:** area lights with soft shadows, the matcap lit by the
  lights, tube profiles and sharp tube corners, Poly Edit Faces/Edges/Points
  modes, crisp flat faces on low-poly models, a hotkey for any brush, and
  Shift+Tab to hide just the settings panel.
- **4.0.0-beta.13:** the transform gizmo follows Mirror X/Y/Z, much
  stronger Gravity, Stamp strokes as strong as freehand and shown while
  dragging, Kelvin Grab without creases, and Poly Edit multi-face picks
  with centred and paired edge loops.
- **4.0.0-beta.12:** surface instances (live copies of one piece on the
  faces of another, with Make real), a Paint Groups brush, edge loops at
  the mask border for crisp hard edges, per-brush Gravity, a Units
  preference for Measure, File > Revert, and cleaner Poly Edit display.
- **4.0.0-beta.11:** orbit around the point under the cursor (or the
  last stroke), tube Twist, Spin and Braid, glTF/GLB import and
  compressed export, UVs kept from imports to exports, the Sharp Crease
  brush, sculpt layers you make ahead of time, and fixes for pen hover on
  Windows and dynamic detail after subdividing.
- **4.0.0-beta.10:** a material (matcap) per piece, Living strokes'
  third phase (Mirror copy, Split by side, groups exported as glTF shape
  keys), Living strokes for plugins and agents, and a voxel remesh that
  keeps the toes of posed models whole.
- **4.0.0-beta.9:** subdivision levels made of quads (with a Smooth
  toggle to split without rounding), Subdivide masked for more resolution
  on one part, Living strokes groups and saving, a Disc primitive, and a
  voxel remesh that keeps close fingers and toes apart.
- **4.0.0-beta.8:** Poly Edit grows up (bevel with rounded rows, bridge,
  edge slide, spin, split, creases and smooth subdivision with a live
  preview), Living strokes' first phase (fade, delete or solo any stroke),
  human base meshes, quad and UV spheres, a Shell option for the voxel
  remesh, and the fix for remeshing overlapping parts.
- **4.0.0-beta.7:** Poly Edit (first steps: extrude, inset, edge loops,
  close holes, weld), brush size on the model, vertex colors from OBJ and
  PLY imports, live size and position in the transform gizmo.
- **4.0.0-beta.6:** undo on the active piece only (Ctrl+Alt+Z), and
  reopening the last stroke as a curve, Curve Move curve or tube path.
  These are small first steps toward Living strokes.
- **4.0.0-beta.5:** the Deform window (bend, twist, taper, inflate, polish
  and 20 more), Mask border and Mask group borders, the Inter font.
- **4.0.0-beta.4:** Symmetrize, Split masked, Join, button and brush icon
  colors, five new plugins.
- **4.0.0-beta.3:** one Theme window, pen gestures for tablets.

The full list is in the [changelog](../CHANGELOG.md).

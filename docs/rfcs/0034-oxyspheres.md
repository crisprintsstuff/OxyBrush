# RFC 0034: OxySpheres

Status: Phase 1 implemented (4.0.0-beta.23.3; spheres as a piece of their own in 4.0.0-beta.24), issue [#437](https://github.com/crisprintsstuff/OxyBrush/issues/437)

## Summary

**OxySpheres** blocks out a shape with a tree of spheres (an armature),
then turns it into one smooth, closed mesh to sculpt. Each sphere is a
joint; each link from a sphere to its parent is the tapering shape that
joins the two. Pieces > + Add > **OxySpheres…** opens a new armature;
**Make mesh** (Enter) adds the mesh as a new piece and keeps the spheres
as a piece of their own, to change them and make another mesh any time.

## Why

Starting a figure, a creature or a prop from a sphere means pulling every
limb out of it with Grab or Snake Hook, which stretches the triangles and
loses volume. A base mesh from a few spheres gets the proportions and the
pose right first, with even triangles everywhere.

## Design

### The armature (`oxy_mesh::armature`)

- A tree: sphere 0 is the root; every other sphere has a parent.
  Validated on load (no cycles, positive finite radii).
- **Mirroring** across the X plane through the root: spheres come in pairs
  (each knows its twin) or sit on the plane. Adding, moving, resizing or
  deleting one does the same, mirrored, to its twin. A child pulled out of
  a sphere on the plane, near the plane, snaps onto it.
- **The shape** is the union, over every link, of the spheres swept along
  it with the radius going linearly from one end to the other (a "round
  cone"). Its signed distance is exact and cheap: for a point, the nearest
  swept sphere is found in closed form (the derivation is in
  `RoundCone::distance`). Links are joined with a quadratic smooth
  minimum, **Blend** wide, which rounds the creases where they meet.

### The skin (`oxy_topo::skin_armature`)

The distance is sampled on a regular grid (Resolution voxels along the
longest side, up to 320), each link writing only into its own box, and the
zero level set is extracted with the surface nets the voxel remesh
already uses. Two passes of relaxing and pulling each point back onto the
exact surface (along the distance's gradient) take the voxel steps off
without shrinking it. The result is closed, evenly dense, and ready for
the brushes, dynamic detail or subdivision levels.

Since 4.0.0-beta.24.2 the grid is sparse (only 8×8×8-voxel bricks near
the surface are sampled), and the voxel size is the smaller of the
figure's longest side over Resolution and a third of the thinnest
sphere's radius (no finer than 1/1500 of the figure): sized by the
figure alone, a thin limb got two or three voxels across and came out
jagged even at the highest resolution. The live view while editing keeps
the figure-wide size, to stay quick. A mirrored 35-sphere figure with
fingers (radius 0.025) now makes 351k triangles in about 1.1 s (release
build), whatever the Resolution, since its fingers set the voxel size.
A quad skin built from the spheres themselves (below) would avoid the
cost; it is the next phase.

Earlier release-build timings (`topo_timing`, a 35-sphere mirrored figure with
fingers): 12.8k triangles in 36 ms at 72, 63k in 168 ms at 160 (the
default), 253k in 714 ms at 320.

### Editing

- **Drag from a sphere** pulls out a new one (Add, the default); a click
  without moving only picks it. **Shift+drag** moves a sphere and
  **Ctrl+drag** resizes it in any mode; the window's Move and Size modes
  make that the plain drag. **Spheres below follow** moves a branch.
- **Click a link** to put a sphere on it (on both sides when mirrored).
- **Rotate** ([#626](https://github.com/crisprintsstuff/OxyBrush/issues/626)) poses a branch. Drag left or right from a sphere or its incoming link. The sphere and all its descendants rotate as a unit around the parent, or around the root's own center when rotating the whole armature. **Axis** selects View, X, Y, Z, or Link. Link twists descendants around the parent-to-child direction and uses View on the root. Mirror pairs turn together. A centerline branch uses X while mirroring is on, keeping its centerline joints on the mirror plane.
- Rotation previews as you drag and records one armature Undo step on release. The pose is stored in the existing sphere centers, so it can be saved and used by Make mesh with either output.
- **Alt+click** or **Delete** removes a sphere with everything below it;
  the root stays.
- Dragging away from the spheres orbits.
- The viewport shows the mesh as it will be, at a coarser resolution (48
  while dragging, 72 otherwise), with each sphere's outline and the links
  drawn over it; **Show the mesh** off shows the plain spheres instead.
- Undo and Redo step through the open spheres' own edits, as in an open
  tube; the scene's history changes once, when they are closed. The mesh
  is made on a worker thread.

### The spheres are a piece of their own (since 4.0.0-beta.24)

The spheres live on as an **OxySpheres piece**, never used up, the way a
sphere armature works in other sculpting tools: its mesh only draws the
spheres, and it is the thing you come back to.

- **Make mesh** (Enter) adds a new piece made from the spheres ("…
  mesh") and keeps the spheres, as a new OxySpheres piece or in place of
  the one edited, hidden so the mesh is in view. One undo step. Since
  4.0.0-beta.24.2, Make mesh can update the last generated piece while
  it is unchanged. The spheres track the exact piece, so a duplicate
  stays independent. If the target has been edited or removed, Make mesh
  creates a new piece.
- **Done** keeps the spheres as they are without making a mesh (shown).
  **Cancel** leaves them as they were. One undo step.
- **Opening them again:** a click on the active OxySpheres piece (there's
  nothing to sculpt on it) or Pieces > Edit OxySpheres…. Meshes made
  before stay as they are, however much they were sculpted.
- New spheres start on the symmetry plane (#449): with X symmetry on,
  the first sphere and so the mirror plane were at the orbit point, off
  centre once the view had moved.

In 4.0.0-beta.23.3 the spheres were kept on the mesh made from them, and
could be reopened only while that mesh was unchanged; Make mesh on an
edited piece replaced its mesh. That lost the spheres as soon as the
mesh was sculpted.

### Saving

The `ARMS` section (see `oxy_doc::format`), version 2, marks an
OxySpheres piece and holds its spheres and settings. Version 1, written
by 4.0.0-beta.23.3 on a mesh made from spheres, is still read: when the
mesh is unchanged, its spheres come back as an OxySpheres piece (hidden)
at the end of the list. Older versions skip the section, and
4.0.0-beta.23.3 skips version 2: they open an OxySpheres piece as an
ordinary mesh of the spheres.

The optional `ARML` section associates an OxySpheres piece with the
zero-based index of its last generated mesh. Older readers skip it.
Projects saved with version 2 of `ARMS` and without `ARML` can still
edit their spheres. Their first Make mesh creates a new piece and
records its identity for subsequent updates. Version 1 identifies the
mesh directly because the recipe is saved on the generated piece.

## Not in phase 1 (later phases)

- A quad skin that follows the links' directions (an adaptive base mesh
  with edge loops around each limb), built from the armature's own
  topology, as described for sphere skeletons in Ji, Liu and Wang, *B-Mesh:
  A Fast Modeling System for Base Meshes of 3D Articulated Shapes*
  (Pacific Graphics 2010).
  See [RFC 0038](0038-oxyspheres-quad-base-mesh.md) for the prototype proposal.
- Scaling a branch.
- Spheres that only shape the link they sit on, without adding a joint.
- Snapping a new armature to the surface of another piece, and binding an
  existing mesh to an armature to pose it.

## Naming

"OxySpheres" is OxyBrush's own name (the issue's suggestion); the generic
description is armature spheres. No other product's feature names are
used in the code or the interface.

# RFC 0009: Tube tool

- **Status:** proposed for the maintainer; first version implemented in the same change for review
- **Requested by:** a tester, who shared a 66-second screen recording of a
  tube tool in another sculpting app (Nomad Sculpt). Only the behaviour seen in
  that recording was taken: no documentation, code, UI layout, icons or
  assets of that app were used (see [CLEAN_ROOM.md](../CLEAN_ROOM.md)).

## Problem

Horns, tentacles, tails, limbs, cables and handles all start as a bent tube.
Today that means adding a cylinder, then bending it with Grab or Snake Hook,
or pulling it out of the model stroke by stroke. Neither gives an even,
smooth tube along a chosen path, and neither can be reshaped afterwards
without re-sculpting it.

## Decision

A **Tube** tool, under **Pieces > + Add > Tube (draw)…** (and in the
command palette).

1. **Draw** a path: drag from the model (the tube starts on its surface) or
   from empty space. The path lies on the view plane through its start
   point. On release it becomes a smooth curve through a few control points
   (the drawn path simplified with Ramer-Douglas-Peucker, at most 8 points),
   and the tube is shown along it, starting half as thick as the brush.
2. **Shape** it while it is open:
   - drag a point to move it (on the view plane through it, so orbiting and
     dragging again moves it in depth);
   - **Shift+drag** a point up or down to make the tube thicker or thinner
     there (it doubles per 120 points of drag); thickness blends smoothly
     between points;
   - click the line between two points to add one; **Alt+click** a point to
     remove it (at least two remain);
   - the **Tube** window sets the thickness (of the selected point, or of the
     whole tube), the number of sides, rounded end caps, and a mirror copy
     across the X symmetry plane (on by default when X symmetry is).
3. **Accept** (or Enter) adds the tube, and its mirror copy, as one new piece
   named "Tube", in one undo step. **Cancel** (or Esc) discards it. Orbit,
   pan and zoom leave it open.

**Tube brush.** The same tool is also a brush in the shelf, **Tube**, next
to Insert Primitive. With it, Accept adds the tube **into the active
piece** (one undo step; the tube's triangles get a new face group, and the
existing surface is unchanged), as other sculpting apps do. The Tube window
has a checkbox to send it to a new piece instead. While a tube is open,
pressing the model away from its points with the Tube brush does nothing.

The new piece is a separate shell. **Merge** (Pieces panel) fuses the
visible pieces into one surface with a voxel remesh; that stays the user's choice,
since a tube is often wanted as its own piece (to pose, hide or export).

## How it works

- **Geometry** (`oxy_mesh::primitives::tube`): a uniform
  Catmull-Rom curve through the points (clamped ends), resampled
  evenly by arc length so rings are about as far apart as the vertices
  around them (square-ish quads, good for sculpting). Rings follow the curve
  with parallel-transported frames, so the tube does not twist. Caps are
  small domes, so the result is closed (watertight) and faces outward;
  tests check both, and the volume.
- **Preview without touching the document.** The open tube is drawn in the
  renderer's preview slot (as the Insert Primitive preview is). The scene
  and history are unchanged until Accept, which goes through the normal
  add-piece step (architecture invariant 4). Nothing is written to `.oxb`
  while it is open.
- **Editing again after Accept** (since 3.0.6). Accept stores how the
  tube was made (points, thickness, sides, caps, mirror) on the piece, with
  the face group holding it and a fingerprint of its triangles: their count
  and a sum of sine waves of their vertex positions, which any move of a
  vertex changes (`oxy_doc::TubeRecipe`, the `TUBE` section, which older
  versions skip). **Pieces > Edit tube…** (or the command palette) reopens
  the piece's most recent tube that still matches: its old triangles are
  hidden while it is open, and **Update tube** replaces that face group, as
  one undo step ("Edit tube"); Cancel, or undo while it is open, shows the
  old one again. A tube that was sculpted, remeshed or regrouped no longer
  matches and is ordinary mesh. Recipes are not part of the undo history:
  older ones stay on the piece (up to 16), so undoing an edit makes the
  previous recipe match again.
- **Cost.** The tube is rebuilt only when it changes (points, thickness,
  sides, caps or mirror). A typical tube is a few thousand triangles; the
  per-dab and per-frame paths are untouched.

## Not in this version

- Points snapping onto the model while dragged (they move on the view plane).
- Joining the tube into the model automatically (use Merge).
- A profile other than a circle, twisting, or a spiral.
- Radial symmetry copies (only the X mirror).

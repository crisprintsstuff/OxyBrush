# RFC 0005: Curve Move — bend and stretch the model with a curve

- **Status:** proposed for the maintainer; implemented in the same change for review
- **Requested by:** a user in [#82](https://github.com/crisprintsstuff/OxyBrush/issues/82),
  who remembered a never-shipped idea from another sculpting app (curves
  used to move parts of a mesh) and asked whether OxyBrush's editable
  curves could "move" the mesh. Only that one-sentence idea was taken: no
  documentation, video, code, UI or assets of any other app were consulted
  (see [CLEAN_ROOM.md](../CLEAN_ROOM.md)).

## Problem

Posing or reshaping a long part (an arm, a tail, a horn, a tentacle) is
awkward with the tools OxyBrush has:

- **Grab** and the **Kelvin** brushes move a round blob, so bending a limb
  takes many strokes and tends to pinch it;
- the **transform gizmo** moves or rotates a masked part rigidly, about one
  pivot, so a smooth bend along the length needs several masks and pivots;
- **Snake Hook** pulls out new shapes rather than bending existing ones.

What's missing is a way to say "this is the part's centre line; now put the
centre line *here*" and have the part follow smoothly.

## Decision

A new brush, **Curve Move**, built on the editable curve from
[RFC 0004](0004-curve-stroke.md).

1. **Draw** a curve along the part. With **Through the middle** (on by
   default), each control point is placed halfway between where the view
   ray enters the surface and where it leaves it again, so the curve runs
   through the part's core instead of over its skin.
2. **Drag** the control points. They move freely in the view plane (not
   stuck to the surface). The mesh near the curve follows: bending,
   stretching and twisting with it. The curve as first drawn stays visible
   as a faint dashed line.
3. **Adjust** as with a curve stroke:
   - click the curve to add a point, Alt+click a point to remove one;
   - **Ctrl+Z / Ctrl+Shift+Z** undo and redo curve edits while it is open;
   - **Join the ends** makes a loop;
   - selecting a point shows **Point reach** (how far from the curve the
     model follows there, relative to Size) and **Point strength** (how fully
     it follows there, up to 100%);
   - **Reset** puts every point back where it was drawn.
4. **Apply** with Accept or Enter, or by starting another stroke on the
   model: one undo step. **Cancel** or Esc leaves the model untouched.

What follows, and how much:

- **Size** sets the reach. Within the inner half of it, the model follows
  the curve fully, as a solid part; over the outer half the brush's
  **Falloff** blends it back to unmoved. (Without that rigid core, the
  surface of a limb, partway down the falloff, lags behind its centre and
  the bent part swells.) The tip shows how big to make it: the circle
  should cover the part's thickness.
- **Strength** scales everything (100% by default).
- **Masked** areas never move, partly masked areas move partly.
- **Symmetry** (mirror and radial, about the piece's symmetry centre)
  moves the mirrored side too: each copy of the curve drives the vertices
  nearest to it.

## How it works

The deformer follows the *wire* technique of Singh and Fiume, "Wires: A
Geometric Deformation Technique" (SIGGRAPH 1998):

- The curve is sampled as a Catmull-Rom spline with the same number of
  samples per segment at rest and after moving, so the two correspond sample
  for sample (`oxy_sculpt::wire::sample_curve`).
- Along each, **rotation-minimizing frames** are built with the
  double-reflection method of Wang, Jüttler, Zheng and Liu, "Computation of
  Rotation Minimizing Frames" (ACM TOG 27(1), 2008), so the curve doesn't
  twist the mesh by itself. The moved curve's first frame is the rest
  frame turned by the smallest rotation between the two first tangents.
- **Binding** (when the curve is drawn, and again when its reach or rest
  shape changes): each vertex within reach of the rest curve is tied to its
  closest point there, as an offset in that point's frame, with a weight
  from the falloff, strength and mask. A hash grid of curve segments keeps
  this to the few segments near each vertex.
- **Deforming** (every frame a point moves): each bound vertex goes to the
  same offset in the frame of the matching point on the moved curve, blended
  by its weight, always from its original position, so results don't drift
  and Accept reproduces the preview exactly.

All of it is in `oxy-sculpt` (GPU-free, unit-tested). The app shows the
result on a preview copy of the piece, exactly like a curve stroke; the
piece and the history change only on Accept.

## Performance

`cargo run --release -p oxy-sculpt --example wire_timing`, a 1.3M-triangle
rod, all 655k vertices bound (the cloud test machine, CPU only):

| Step | When | Time |
|---|---|---|
| Bind | drawing the curve; changing Size, reach, strength or falloff | 100–200 ms |
| Deform | each frame a point is dragged | ~5 ms |

Updating the preview's normals for the moved chunks comes on top of the
deform, and grows with how much of the model is bound. Not yet measured on
10M+ triangle meshes; binding there would take seconds and should move off
the UI thread if that is a problem in practice.

## Not in this version

- **Twist** around the curve (rotating a point's frame) and per-point
  **scale** of the part (fatter or thinner): the frames are there, the
  controls are not.
- **Saving** a curve with the project, or keeping several curves as a rig.
  A curve lives until it is applied or cancelled.
- **Moving the curve with the gizmo** (all points at once); dragging points
  one at a time is the only way for now.
- An agent (MCP) tool for it.

## Alternatives considered

- **A gizmo mode with several pivots** along a masked part: rigid segments,
  no smooth bend, and much more setup.
- **Skinning to a bone chain** (linear blend skinning): needs weights
  painted or computed per bone; the wire approach gets smooth weights from
  distance and the falloff alone, which suits sculpting better.
- **Grab along a path**: moves surface, but can't keep a limb's shape while
  bending it.

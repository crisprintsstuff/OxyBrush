# RFC 0033: Knife in Poly Edit

Status: Implemented (4.0.0-beta.23.2; ends inside a face fixed in 4.0.0-beta.23.3), the Knife part of issue #370 (To Circle is [RFC 0032](0032-poly-edit-to-circle.md))

## Summary

Turn on **Knife** in the Poly Edit window, click points on the model to
draw a cut, and press **Enter** (or **Cut**). The faces the path crosses
are split along it, with a new point wherever it crosses an edge. One
undo step.

## Why

Adding an edge where you want one (a seam line, a panel edge, a loop that
doesn't run round the whole ring) needs either a chain of Split face
clicks, one face at a time, or an edge loop that goes further than
wanted. A knife draws the edge directly.

## Design

- **Clicks snap.** Within 10 points on screen of a corner, a click is on
  that corner; within 10 points of an edge, it's on the edge, under the
  pointer; otherwise it's a point inside the face.
- **The path follows the surface.** Between two clicks the cut runs where
  the surface meets the plane through the eye and both points, which is
  what lies under the line drawn on screen. It is traced face to face
  across shared edges (and through corners), preferring faces turned
  toward the eye, so it cuts what you see rather than the far side.
- **Splitting.** Each crossed edge gets a point, in order along the edge
  when several cuts cross it, keeping the edge's crease on both halves.
  Each crossed face is split in two along its piece of the path; points
  clicked inside a face become corners of the cut. A path that starts or
  ends inside a face carries on, straight along the cut, to that face's
  edge, because a polygon can't hold a loose edge (at first such ends were
  dropped, so a cut between two neighbouring faces, or within one, cut
  nothing). Both halves keep the face's group.
- **Controls.** Enter, **Cut**, or a click on the last point again (for
  a pen without a keyboard) applies, Backspace or **Undo point**
  takes the last point back, Esc clears the path and a second Esc turns
  the Knife off. Clicking the first point again closes the loop and cuts.
  Presses off the model still orbit. The path, the clicked points and the
  next point under the pointer are drawn over the model as you go.
- **Undo.** One step, "Poly Edit: knife". The Knife stays on for the next
  cut.
- No file format change: the result is ordinary polygons.

## Not included

- **Mirror.** The cut isn't repeated across the X symmetry plane yet.
- **Cutting through.** The cut follows the visible surface; it doesn't cut
  all the way through the model.
- **Angle snapping** and typed lengths.

## References

Behaviour only, from the public manuals linked in #370 and logged in
[CLEAN_ROOM.md](../CLEAN_ROOM.md): [Blender Knife](https://docs.blender.org/manual/en/2.81/modeling/meshes/editing/subdividing/knife.html)
and [Cinema 4D Line Cut](https://help.maxon.net/c4d/en-us/Content/html/TOOLKNIFELINE.html).
The tracing and splitting are written for OxyBrush's polygon cage.

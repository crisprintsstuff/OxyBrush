# RFC 0035 — Poly Edit area selection

**Status:** Draft ([#299](https://github.com/crisprintsstuff/OxyBrush/issues/299))

## Summary

Add Box and Lasso tools to the Poly Edit window for picking faces, edges,
or points on the active piece.

## Motivation

Clicking or painting picks is useful for selecting individual elements.
Picking a patch of geometry for a transform or extrusion takes fewer
gestures if the artist can outline the patch in the viewport.

## Design

- Offer **Box** and **Lasso** in the Poly Edit window in Faces, Edges, and
  Points modes. A drag draws the corresponding outline and release applies
  it. Escape cancels an unfinished drag. Existing navigation bindings
  continue to orbit, pan, and zoom.
- Plain drags replace the pick. Ctrl/Cmd adds and Alt subtracts. A face
  qualifies when a representative interior point projects into the area.
  For concave faces use the centroid of the largest triangle in the
  existing triangulation. Use midpoints for edges and positions for points.
- Select visible elements only. Check occlusion against the active piece
  using the same cage and viewport projection as ordinary picks. A back
  face behind a visible face should stay out of the pick. Edges and points
  on an exposed silhouette should remain selectable.
- Area selection replaces, adds to, or subtracts from the existing
  Poly Edit pick. The gizmo and all actions consume that pick as usual.
  Picking changes transient tool state without adding an undo step.
- Leave Auto mode, whole-piece picking, circle selection, and picking
  through the surface for follow-up feedback.

## Validation

Check all three element modes on a cube and a concave cage. Cover
occlusion, cancellation, modifiers, and a lasso crossing the viewport
edge. Existing click, paint, loop, and gizmo gestures should still work.
Measure release latency on a cage near the Poly Edit triangle limit.

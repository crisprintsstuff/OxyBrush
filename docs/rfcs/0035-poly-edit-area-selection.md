# RFC 0035 — Poly Edit area selection

**Status:** Implemented ([#299](https://github.com/crisprintsstuff/OxyBrush/issues/299)). Ellipse and select-through follow-ups ([#636](https://github.com/crisprintsstuff/OxyBrush/issues/636)).

## Summary

Add Box, Lasso and Ellipse tools to the Poly Edit window for selecting
faces, edges, or points on the active piece.

## Motivation

Clicking or painting picks is useful for selecting individual elements.
Picking a patch of geometry for a transform or extrusion takes fewer
gestures if the artist can outline the patch in the viewport.

## Design

- Offer **Box**, **Lasso** and **Ellipse** under **Select** in the Poly Edit
  window in Faces, Edges, and Points modes. A drag draws the corresponding
  outline and release applies it. On Ellipse, **Shift** snaps the bounding
  box to a square so the outline is a circle. Escape cancels an unfinished
  drag. Existing navigation bindings continue to orbit, pan, and zoom.
- Plain drags replace the selection. Ctrl/Cmd adds and Alt subtracts. A face
  qualifies when a representative interior point projects into the area.
  For concave faces use the centroid of the largest triangle in the
  existing triangulation. Use midpoints for edges and positions for points.
- By default, pick through the active piece, including occluded elements
  that project inside the outline. Turn on **Front only** to check occlusion
  against the active piece using the same cage and viewport projection as
  ordinary picks. A back face behind a visible face stays out of the pick.
  Edges and points on an exposed silhouette remain selectable.
- Area selection replaces, adds to, or subtracts from the existing
  Poly Edit pick. The gizmo and all actions consume that pick as usual.
  Selecting changes transient tool state without adding an undo step.
- Leave Auto mode and whole-piece picking for follow-up feedback.

## Validation

Check all three element modes on a cube and a concave cage. Cover
both depth modes, cancellation, modifiers, and a lasso crossing the viewport
edge. Existing click, paint, loop, and gizmo gestures should still work.
Measure release latency on a cage near the Poly Edit triangle limit.

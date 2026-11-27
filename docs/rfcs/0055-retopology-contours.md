# RFC 0055 — Retopology contours

**Status:** Implemented for closed sections and quad strips
**Issue:** [#576](https://github.com/crisprintsstuff/OxyBrush/issues/576)

## Summary

Add a **Contour** mode to Retopology. Drag across a limb with the
midpoint over the source. The first cut places a ring. Each subsequent
cut on the same limb extends the cage with a strip of quads.

## Design

- Use the visible source hit at the stroke midpoint to locate the
  section. The drag direction and the view define the cutting plane in
  either perspective or orthographic projection.
- Clip source triangles to the plane. Connect segments by shared mesh
  edges or vertices, and take the closed section at the hit. Reject open
  and branched sections. Sample 4–48 points at equal distances around
  the section, starting at the hit.
- Align subsequent rings by testing cyclic offsets in both directions.
  Add each strip as one undo step with consistent face winding.
- **New strip** starts another disconnected strip. Changing point count
  also starts a new strip. Each contour runs around the drawn section.
  The mirror checkbox still applies to other retopology edits.
- Cut dense source meshes on a worker. Apply the result only if the
  source and cage are still current.

## Validation

Cover closed and open inputs, hidden faces, the selected shell in a
multi-shell source, consistent ring spacing, consecutive quad strips,
and Undo and Redo through the normal Retopology tool.

## Reference

[RetopoFlow's public Contours manual](https://docs.retopoflow.com/v4/contours.html)
describes cutting loops across cylindrical forms. The mesh intersection,
ring alignment, controls and artwork are implemented in OxyBrush.

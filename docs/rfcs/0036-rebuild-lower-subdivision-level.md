# RFC 0036 — Rebuild lower subdivision levels

**Status:** Implemented ([#366](https://github.com/crisprintsstuff/OxyBrush/issues/366), [#455](https://github.com/crisprintsstuff/OxyBrush/issues/455), [#611](https://github.com/crisprintsstuff/OxyBrush/issues/611))

## Summary

Recover an editable coarse subdivision level beneath a compatible quad
mesh or the base of an existing Catmull–Clark stack. Repeat the command
while the current base has compatible subdivision connectivity.

## Motivation

An imported sculpt may have the topology of a subdivided quad mesh but
arrive as a single dense level. Recovered coarse levels make broad edits
possible while preserving the high-resolution sculpt.

## Design

- Offer **Rebuild lower level** in the subdivision controls. Check whether
  Catmull–Clark subdivision of the recovered base reproduces the input's
  closed quad connectivity. Report why recovery failed and leave the
  piece unchanged if another level cannot be recovered.
- Match vertices, edges and faces by connectivity. Keep all existing
  levels and the displayed level, and retain sculpted detail through the
  existing multiresolution displacement representation.
- Keep vertex paint, mask, groups and visibility on the displayed level.
  Allow coarse recovery only when the model has no UVs or explicit creases.
  If recovery flips a quad's triangulation, the two triangles must have
  matching groups and visibility. Bake editable strokes through the
  Strokes panel before recovery because it changes vertex order.
- Run large recoveries in a background job. Install the result as one
  undoable change and save it using the existing subdivision sections.
- Duplicate, Mirror duplicate, and Delete higher/lower levels cover the
  other parts of #366.

## Validation

Test two consecutive recoveries on a mesh with two known subdivisions.
Verify that fine positions and paint survive a trip through the
recovered coarse levels. Verify that undo, redo and project reopen keep
the active level and finer geometry. Check that an unsupported base
stops further recovery without changing the piece.

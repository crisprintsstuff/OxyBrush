# RFC 0040 — Delete and dissolve edges in Poly Edit

**Status:** Proposed ([#472](https://github.com/crisprintsstuff/OxyBrush/issues/472))

## Summary

Rename the existing edge removal action to **Dissolve** and add a
**Delete** action in Poly Edit's Edges controls.

## Behavior

- **Dissolve** removes an interior edge and combines the two incident
  faces. It keeps the existing restrictions on border edges.
- **Delete** removes each reached edge's incident faces, leaving a hole
  or trimming the border. Points left unused are cleaned up by the
  existing face deletion behavior. The preview marks which faces go.
- Picks, edge reach, mirroring, and undo follow the existing Poly Edit
  workflow. Deleting every face gives the existing message that directs
  the user to delete the piece.

## Validation

On a cube, Delete opens a hole and undo restores it. Dissolving an edge
loop merges its adjacent faces. Verify that a border edge removes its
single incident face and that overlapping picks remove each face once.

# RFC 0036 — Rebuild one lower subdivision level

**Status:** Draft ([#366](https://github.com/crisprintsstuff/OxyBrush/issues/366), [#455](https://github.com/crisprintsstuff/OxyBrush/issues/455))

## Summary

Recover one editable coarse subdivision level underneath a compatible
quad mesh that currently has no subdivision stack.

## Motivation

An imported sculpt may have the topology of a subdivided quad mesh but
arrive as a single dense level. A recovered coarse level makes broad
edits possible while preserving the high-resolution surface.

## Design

- Add **Rebuild lower level** to the subdivision controls. Start with a
  single level and require recorded quad topology. Identify a coarse
  topology only when its Catmull–Clark subdivision reconstructs the input
  connectivity unambiguously. Report why a mesh is unsupported and leave
  the piece unchanged.
- Keep the imported mesh as the original high level, with its positions
  and attributes. Remap it into subdivision order and use the existing
  multiresolution detail representation for its difference from the
  subdivided coarse mesh. Refuse inputs with attributes that cannot be
  preserved in the first implementation.
- Compute the result in a background job. Install it as one undoable
  change and keep the high level active. Use the existing subdivision
  sections to save the recovered stack.
- Duplicate, Mirror duplicate, and the existing Delete higher/lower
  commands already cover the other parts of #366.

## Validation

Round-trip a known subdivided quad mesh through import, recovery, a
coarse edit, and project save/reopen. Verify exact agreement with the
original high surface before coarse editing and after undo. Cover
unsupported topology, ambiguous reconstruction, and attribute seams.

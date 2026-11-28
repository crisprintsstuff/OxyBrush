# RFC 0059 — Surface fur

- **Status:** First viewport slice
- **Scope:** Short groomable coats on sculpt pieces.

## Workflow

Select a sculpt piece and open **Fur coat** in the inspector. Choose the whole
piece, masked surface, or a face group. Set **Spacing**, **Length**, **Width**,
and **Seed**, then click **Grow fur**. Smaller spacing makes a denser coat.
**Remove coat** clears the preview. Growing, resizing, and removing a coat
each support Undo and Redo.

The piece owns its coat. Roots follow position edits and appear only on visible
faces. Save and reopen the project to continue with the same roots. After a
topology change the coat stays saved and the preview pauses. Restore the
original topology with Undo, or choose **Grow fur** to bind a new coat.
The first slice grows fur on single-level sculpt meshes.

## Surface binding and preview

Reuse the area-weighted minimum-spacing sampler from
[Surface instances](0022-surface-instances.md). The sampler considers visible
triangles at generation time. Masked placement uses the mean corner mask and
face-group placement uses the triangle's group. Keep the triangle, barycentric
weights, and stable random key for each root. The seed changes layout, turn,
and length variation. Length and width edits preserve the existing placements.

Evaluate a root position and normal from its current triangle. Draw a shared
tapered strand ribbon at each root with the viewport instancing path. Cap a
coat at 20,000 preview strands. Cache the transforms by topology, face state,
and mesh edits. Generation runs as a background job. Strand geometry stays in
the viewport, so the original sculpt remains the piece in the outliner.

The optional `FUR1` project section stores parameters, a topology signature,
and bounded root records. Loading remaps face indices as meshes are packed
into chunks. An older reader can skip the section and open the sculpt.

## Next slice

Add an undoable **Comb** brush over a sparse set of surface-bound guides.
Nearby strands inherit the guide direction, with a deterministic blend
between guides. Start with cheeks, ears, and a muzzle to check flow and clean
borders. Add screen-space density tiers for distant coats and profile guide
edits on dense pieces. A later rebind can project edited guides onto a new
surface. Printing calls for a separate bake that thickens and fuses tufts.

## Acceptance

- Two coats grown on the same mesh with the same seed have the same roots.
- Fur stays on its selected region, follows sculpt edits, and respects hidden
  faces.
- Growing and removing a coat are undoable and the outliner keeps its
  original piece count.
- Saving and reopening preserves root placement after the mesh has been
  sculpted and packed into chunks again.
- A topology change pauses the old coat. Undo restores it with the surface.

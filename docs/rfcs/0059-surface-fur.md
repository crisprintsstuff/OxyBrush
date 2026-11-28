# RFC 0059 — Surface fur

- **Status:** Viewport coats and guide combing implemented
- **Scope:** Short groomable coats on sculpt pieces.

## Workflow

Select a sculpt piece and open **Materials > Fur** in the inspector. Choose the
whole piece, masked surface, or a face group. Set **Spacing**, **Length**, **Width**,
and **Seed**, then click **Grow fur**. Smaller spacing makes a denser coat.
**Remove coat** clears the preview. Growing, resizing, and removing a coat
each support Undo and Redo. Click **Comb** and drag on the surface to groom the
coat. The current brush size and strength set its reach and influence. Release
to record one undo step. Escape cancels a drag or leaves Comb when idle.

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
and length variation. Length and width edits preserve the existing placements
and grooming. Growing a new layout starts a new coat, and Undo restores the
previous groom.

Evaluate a root position and normal from its current triangle. Draw a shared
tapered strand ribbon at each root with the viewport instancing path. Cap a
coat at 20,000 preview strands. Cache the transforms by topology, face state,
and mesh edits. Generation runs as a background job. Strand geometry stays in
the viewport, so the original sculpt remains the piece in the outliner.

The optional `FUR1` project section stores parameters, a topology signature,
and bounded root records. Loading remaps face indices as meshes are packed
into chunks. An older reader can skip the section and open the sculpt.

## Guide grooming

Prepare a sparse set of guides on a worker when Comb is first used. Pick
guide roots in spatial cells, with separate guides for opposed normals.
Each strand stores weights for up to three nearby guides on its side of
the surface. A swept brush stroke updates the nearby guides in their
surface tangent frames. Guide influence leans strands toward the stroke
direction as the host deforms.

The optional `FGR1` section stores the guide headings and strand weights
with the coat. Binding indices refer to the saved root order, so the
mesh's triangle packing during load leaves the groom intact.

## Follow-up

Profile grooming and viewport frame time on dense sculpts and add
screen-space density tiers for distant coats. A later rebind can project
edited guides onto a new surface. Printing calls for a separate bake
that thickens and fuses tufts.

## Acceptance

- Two coats grown on the same mesh with the same seed have the same roots.
- Fur stays on its selected region, follows sculpt edits, and respects hidden
  faces.
- Growing and removing a coat are undoable and the outliner keeps its
  original piece count.
- Saving and reopening preserves root placement after the mesh has been
  sculpted and packed into chunks again.
- A topology change pauses the old coat. Undo restores it with the surface.
- Comb changes local guide headings and leaves the roots and sculpt geometry
  attached to the same surface. Undo, Redo, and reopening preserve the groom.

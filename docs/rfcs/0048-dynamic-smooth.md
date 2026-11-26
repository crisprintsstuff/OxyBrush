# RFC 0048 — Live smooth preview (Dynamic Subdivision request)

- **Status:** Phase 1 implemented
- **Issue:** [#447](https://github.com/crisprintsstuff/OxyBrush/issues/447)
- **Builds on:** [RFC 0018](0018-poly-edit.md) phase C (Catmull–Clark
  smooth preview), [RFC 0019](0019-quad-subdivision.md)
- **Name in UI:** **Smooth preview** (Topology panel and Poly Edit). Do
  **not** use proprietary product names for the feature (AGENTS.md
  trademarks).

## Problem

Artists want to see a smoothed, higher-resolution surface while they still
edit the low-polygon cage — then apply the subdivision when ready. Poly
Edit already offers **Smooth > Preview** and **Subdivide (smooth)** for
modeling. Outside that tool, the same live preview is missing, so
sculpting and browsing a low-poly base always show the flat cage
triangulation.

[#447](https://github.com/crisprintsstuff/OxyBrush/issues/447) asked for
this behaviour, linking Maxon’s public Dynamic Subdivision help. Only that
public description was consulted (see [CLEAN_ROOM.md](../CLEAN_ROOM.md)).

## Decision

1. Reuse the existing Catmull–Clark + crease path
   (`oxy_mesh::catmull_clark`, cage creases in `CRSE` / Poly Edit).
2. Expose the same **Smooth preview** (0–3 levels, face budget
   `MAX_PREVIEW_FACES`) in the **Topology** panel for the active piece
   when it is low-polygon and has no multires stack — without requiring
   Poly Edit to be open.
3. Keep the preview **display-only**: brushes and transforms still edit
   the cage mesh; the viewport draws the subdivided surface in its place
   (same `preview_in_place` path as Poly Edit).
4. **Apply** replaces the cage with the subdivided polygons (one undo
   step), matching Poly Edit’s **Subdivide (smooth)**.

### Naming

| Context | Name |
|---|---|
| Topology / Poly Edit / docs | **Smooth preview** |
| Apply action | **Subdivide (smooth)** |
| Code | `dyn_smooth_*` fields (session), shared helpers with Poly Edit |
| Forbidden | Proprietary “Dynamic Subdivision” / product feature names in UI |

## Phases

### Phase 1 (this change)

- Topology: Smooth preview Off / 1 / 2 / 3; Apply with level count.
- Ensure a linked cage (`cage_from_mesh` when needed), same eligibility
  rules as Poly Edit (no multires, ≤ 400k triangles).
- Wire into existing preview draw path; clear when the piece becomes
  ineligible.
- Unit tests for preview on/off and Apply outside Poly Edit.

### Phase 2 (later)

- Persist preview level per piece in `.oxb`.
- Brush strokes that deform the cage with continuous smooth feedback at
  higher budgets (incremental rebuild).
- Preview while a multires stack exists (draw high level, edit low).
- Optional chamfer / non-smoothed display modes only if artists ask.

## Out of scope

- Copying another product’s UI layout, icons, or trademarked names.
- GPU tessellation shaders (CPU Catmull–Clark preview stays the source of
  truth for Phase 1).

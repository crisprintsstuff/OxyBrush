# RFC 0051 — Detail reproject (Project detail)

- **Status:** Phase 1 implemented (Mesh / Rebuild / palette closest-point
  project onto other pieces); Phase 2–3 open
- **Builds on:** Retopology **Subdivide and project**
  (`Editor::retopo_project_detail`), [`oxy_topo::SurfaceProjector`](../../crates/oxy-topo/src/transfer.rs),
  Scene Project brush ([#295](https://github.com/crisprintsstuff/OxyBrush/issues/295))
- **Issue:** [#516](https://github.com/crisprintsstuff/OxyBrush/issues/516)
- **Name in UI:** **Project detail** (and **Project onto…** for one source).
  Do **not** use proprietary product feature names (AGENTS.md trademarks).

## Problem

After voxel remesh, OxyRemesh, or a hand-drawn retopo cage, artists need to
**recover the high-detail sculpt** onto the clean mesh. OxyBrush already
projects in two places:

| Tool | What it does | Gap |
|---|---|---|
| Retopo **Subdivide and project** | Adds subdivision levels and snaps each onto the reference surface | Only inside Retopology; needs a drawn cage |
| **Scene Project** brush | Projects under the brush during a stroke | Local / interactive, not a whole-piece bake |

Testers asked for a **shelf / Mesh** action that reprojects an existing
clean piece onto a sculpt without opening Retopo ([#516](https://github.com/crisprintsstuff/OxyBrush/issues/516)).

## Decision

Expose closest-point projection as **Mesh > Project detail**, reusing
`SurfaceProjector` (the same path Retopo and remesh attribute transfer use).

1. The **active** piece is the target (clean remesh / cage).
2. **Sources** are other pieces whose surface holds the detail.
3. Topology of the active piece stays; only positions move.
4. Subdivision levels are dropped (same as remesh / Symmetrize).
5. One background job, one undo step.

### Naming

| Context | Name |
|---|---|
| Menu / Rebuild / palette | **Project detail** |
| One chosen source | **Project onto…** → piece name |
| Code | `project_detail` / `project_detail_onto` |
| Forbidden | Proprietary “project” / “project all” product feature names |

## Design

### Phase 1 (this change)

- **Project detail:** sources = every other **visible** piece with triangles
  (same default as Scene Project). Status if none: show another piece or use
  Project onto….
- **Project onto…:** submenu / Mesh menu listing every other piece; projects
  onto that piece only (even if hidden).
- UI: Mesh menu, Mesh topology panel, Rebuild panel, command palette
  (`Cmd::ProjectDetail`).
- No new file format; no mask / paint transfer yet.

### Phase 2 (open)

- Optional **iterations** / light surface relaxation after project.
- Respect the **mask** (leave masked vertices put).
- Transfer **vertex color** (and optionally mask) from sources via
  `transfer_attributes`, without rewriting face groups unless asked.
- Project the **active subdivision level** in place without dropping the stack
  when that is safe.

### Phase 3 (open)

- Optional **subdivide then project** (N levels) outside Retopo, mirroring
  Retopo's Detail slider for an already-existing piece.
- Project onto a **picked face group** or only unmasked surface of the source.

## Workflow

1. Keep the high-detail sculpt as its own piece (or hide everything else).
2. Remesh / OxyRemesh / Retopo-keep a clean piece; make that piece active.
3. Show the sculpt (or Mesh > Project onto… it) and run **Project detail**.
4. Undo once to put the clean mesh back.

## Alternatives considered

- **Only Scene Project:** too slow for whole-mesh recovery; already covered.
- **Only Retopo Subdivide and project:** forces a Retopo session; #516 asked
  for a shelf action on an existing mesh.
- **New remesh mode that always projects:** remesh already snaps; this is for
  meshes that already exist (import, join, keep-level).

## Testing

- Editor test: cube projected onto a visible unit sphere lands near radius 1;
  topology unchanged; one undo; hidden source skipped for Project detail;
  Project onto still works when the source is hidden.
- Unit coverage of `SurfaceProjector` remains in `oxy-topo`.

# RFC 0050 — Remesh density preview

- **Status:** Phase 1 implemented
- **Issue:** [#552](https://github.com/crisprintsstuff/OxyBrush/issues/552)
- **Name in UI:** **Show remesh density**

## Problem

Before Remesh or OxyRemesh, artists want to see how large the rebuild cells
will be on the model (similar to a voxel-size overlay in other sculpt
apps). Only public feature ideas were consulted; no proprietary UI was
copied ([CLEAN_ROOM.md](../CLEAN_ROOM.md)).

## Decision

### Phase 1

- Topology checkbox **Show remesh density**.
- Viewport overlay: a translucent grid on the active piece’s bounds, with
  spacing from Remesh resolution (voxel size) and OxyRemesh face budget
  (ρ), using the finer of the two for the drawn cell size.
- Session-only (not saved in settings yet).

### Phase 2

- Surface-hugging cells (project the grid onto the mesh).
- Separate Remesh vs OxyRemesh colours; remember the toggle in settings.
- Live update while dragging Resolution / OxyRemesh faces.

## Out of scope

Copying another product’s overlay look, icons, or trademarked names.

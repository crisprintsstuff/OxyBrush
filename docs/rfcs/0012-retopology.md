# RFC 0012: Retopology tool (draw quads on a sculpt)

- **Status:** proposed for the maintainer; first version implemented in the same change for review
- **Requested by:** a tester, who described Autodesk Maya's Quad Draw tool
  and asked for a tool that works like it. Only that description and the
  generally known, publicly documented controls of such tools were used
  (see [CLEAN_ROOM.md](../CLEAN_ROOM.md)). No code, UI layout, icons or
  assets of Maya were used, and the feature has a generic name.

## Problem

A sculpt is millions of triangles in no useful order. To animate it, UV it,
or use it in a game, artists rebuild it as a light mesh of quads that
follow the form ("retopology"), and project the sculpted detail back as
maps. OxyBrush can remesh and decimate, but those can't give clean, hand
placed edge loops around eyes, mouths and joints. Artists have to export
and retopologize in another app.

## Decision

**Mesh > Retopology (draw quads)…** (and the command palette) opens the
tool over the active piece, the **live surface**. A small window lists the
controls, counts quads, triangles and loose points, sets the relax brush,
and has **Done** (Esc).

| Input | Does |
|---|---|
| Click on the surface | place a point (it snaps to the surface) |
| Drag a point, edge or face | move it along the surface; a point dropped on another **welds** them |
| Shift (hover) | preview the quad the 4 nearest open points make (green), or a triangle from 3 (purple) |
| Shift+click | make that face |
| Shift+drag | **relax** the points under the cursor (they stay on the surface) |
| Tab+drag a border edge | pull a **new quad** out of it |
| Ctrl+click an edge | **insert an edge loop** across its ring of quads |
| Ctrl+Shift+click | **delete** the point (and its faces) or face under the cursor |
| Drag empty space, right/middle drag | navigate as usual |
| Ctrl+Z / Ctrl+Shift+Z | undo / redo, one step per action |

"Open" points are loose points and points on the border of the mesh, which
is what a new face can use. Points and faces hidden behind the surface are
not drawn and can't be picked.

The new mesh is an ordinary piece named **Retopo**, created with the first
face. Closing and reopening the tool (with the sculpt or the Retopo piece
active) continues it. It can be exported like any piece; its quads export as
quads.

## How it works

- **The cage** (`oxy_mesh::retopo::Cage`): positions, and faces that are
  quads or triangles (a fourth corner of `NONE`), counter-clockwise from
  outside. A new face takes its winding from the faces it shares edges with,
  or else faces along the surface normal. Operations: add face, weld, remove
  vertex or face, insert loop (walks the quad ring; a triangle at the end of
  the ring is split, so no T-junction is left), extend a border edge, relax
  (inner points move toward the middle of their neighbours; borders stay).
- **The piece's mesh is built from the cage**: two triangles per quad, with
  the quads' diagonals recorded on the mesh (`Mesh::set_quad_diagonals`), so
  quad counts and quad OBJ export are exact rather than guessed. After each
  build the cage is renumbered so that cage vertex `i` is mesh vertex `i`.
  Brushes, undo or anything else that moves the mesh's vertices without
  changing its topology are copied back into the cage
  (`Cage::sync_from`, checked with `Mesh::topology_version`). If another
  tool changes the piece's topology (remesh, subdivide), the cage no longer
  matches and the tool starts a new one.
- **Undo:** each action is one step that swaps the piece's mesh and cage
  together (`History::record_cage`, a new `Cage` change). Moves and relaxes
  are recorded once, on release. Before the first face, loose points live
  only in the tool; Ctrl+Z removes them one by one.
- **Saving:** a new optional `CAGE` section after the piece
  (`vcount, positions, fcount, faces`), indexed like the piece's vertices,
  written only while the cage matches the mesh. Older versions skip it and
  load the Retopo piece as a plain mesh (architecture invariant 7).
- **Snapping:** placing and moving cast the camera ray onto the live
  surface; relaxed and inserted points are moved to the nearest surface
  point along their normal.
- **Drawing:** the cage is drawn as an overlay (faces, edges, points) and
  the Retopo piece itself is left out of the viewport while the tool is
  open, so it never fights the sculpt for depth. Which points are hidden
  is found by casting a ray from the camera to each point (in parallel).
  That costs about 25 ms for 2,500 points on a 330k-triangle sculpt, so it
  runs when the view settles and after each edit. While orbiting or
  dragging, points facing away are hidden and the rest keep the last result
  (0.15 ms). Measured with `cargo test -p oxy-app --release
  retopo_view_timing -- --ignored --nocapture`.
- **Nothing on the per-dab path changes;** the tool only runs while open.

## Symmetry

**Mirror across X** (in the window; on when X symmetry is on) applies every
action to its mirror image across the surface piece's X symmetry plane:
points, faces, moves, relax, extend, loops, welds and deletes. Mirror
pairs are found by position (mirrored points are placed at exactly
mirrored positions, and snapped points are made exact again afterwards),
so nothing extra is stored or saved. A point placed within a few pixels of
the plane goes onto it and is shared by both sides; points on the plane
slide along it.

## Projecting detail

**Subdivide and project** (in the window, 1–5 levels) gives the retopology
piece subdivision levels in a background job: each new level is Loop
subdivided from the one below and every vertex moved to the closest point
of the surface (the attribute-transfer grid: nearest surface vertex, then
its triangles). Its detail is stored against the level below without
touching the lower levels (`Multires::set_active_positions`,
`prepare_attributes`), so level 1 stays exactly the drawn quads. One undo
step (a topology change); the quads can be edited again after undoing.

## Not in this version

- Dragging out a strip of several quads at once, sliding an edge loop, edge
  extrude by the loop, or a "quad patch" fill of a hole.
- Snapping loose points onto edges while dragging, and auto-welding nearby
  points.
- Automatic retopology (still on the roadmap as its own item).

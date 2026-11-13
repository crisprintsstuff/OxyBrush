# RFC 0018: Poly Edit, a click-and-drag polygon modeling tool

Status: accepted; phases A and B landed (creases moved to phase C, where smooth subdivision first uses them)

## Summary

**Poly Edit** is a tool for building and changing low-polygon models
directly in OxyBrush. You point at a **point**, an **edge** or a **face**,
and click or drag to apply the **action** chosen for that kind of element,
such as extrude, inset, insert a loop or bevel. A **reach** setting decides
how far the action spreads from the element you clicked: just that element,
its loop, its face group, the flat region around it, or the whole island.

It is meant for hard-surface blockouts, base meshes to sculpt on, and quick
fixes to low-poly pieces, without a trip to another modeling app. It reuses
the quad cage that the retopology tool ([RFC 0012](0012-retopology.md))
already edits, so a Poly Edit model, a retopology cage and a low-poly
primitive are the same kind of data.

## Why

The maintainer asked for a tool like this. ZBrush has a polygon
modeling brush where hovering over an element picks the action, with
"targets" that set how far an action spreads. Today OxyBrush can only
model polygons with the retopology tool, which draws faces onto a surface
and can't extrude, inset, bevel or bridge.

What was looked at (see [CLEAN_ROOM.md](../CLEAN_ROOM.md)): web search
result summaries of Maxon's public ZBrush help pages for that brush. They
list the actions by element (for example extrude, inset, bevel, bridge,
delete, equalize and crease for faces; insert, slide, swivel and stitch
for edges; crease, slide and stitch for points) and the idea of targets
(single polygon, loops, flat islands, polygroups). The pages themselves
could not be fetched from the development environment, and no ZBrush
program, video, UI layout, icon or file was used. The operations are
textbook polygon modeling ones that every modeling package has; the
interaction, names and UI below are OxyBrush's own.

**Names.** Per AGENTS.md, no ZBrush feature names: the tool is **Poly
Edit**, "targets" are **reach**, polygroups are **face groups**, and there
is no "QMesh": that behaviour is **Extrude > Join touching faces**.

## Design

### 1. The data: a polygon cage for any piece

`oxy_mesh::retopo::Cage` already stores quads and triangles, builds the
piece's triangle mesh from them (`build_mesh`), syncs brush edits back
(`sync_from`), is saved in `.oxb` (`CAGE`, `PINS`) and has undo
(`Change::Cage`). Poly Edit extends it rather than adding a second
structure:

- **N-gons.** Deleting an edge between two quads, beveling a corner or
  bridging uneven loops naturally makes faces with more than four corners.
  Faces become variable-length (`Vec<u32>` corners plus offsets).
  `triangulate` uses ear clipping for n-gons, and fans for convex ones.
  Retopology keeps working, since quads and triangles are a subset.
- **Per-face data:** a face group per face, kept through every operation
  and copied to the built mesh's triangles. The extruded sides and caps,
  the inset rims and the bevel strips each get their own new group, so the
  next action can reach them by group.
- **Per-edge data:** crease weights (0 to 1), used by smooth subdivision
  (phase C in section 4).
- **Adjacency on demand.** An edge to faces map (`edge_faces` exists) and
  face loop / edge ring walks (the ring walk used by `insert_loop` exists).
  These are rebuilt after each edit. They are cheap at the sizes this tool
  is for.
- **`.oxb`.** The meaning of `CAGE` doesn't change. A new skippable
  section, `PLYC`, holds n-gon faces, face groups and creases. When it is
  present, readers that know it use it instead of `CAGE`. The writer also
  writes `CAGE` with the n-gons fanned into triangles, so older versions
  still open the file with a usable cage.
- **Size limit.** Poly Edit is a low-poly tool. **Edit as polygons** works
  on pieces up to about 200k faces and says so beyond that; sculpt
  detail stays in subdivision levels.

**Getting a polygon model.** Start from **File > New** primitives (already
low-poly and parametric), from a retopology cage, or use **Mesh > Edit as
polygons** on any low-poly piece. That pairs triangles into quads with the
existing `find_quad_diagonals`. Closing the tool keeps the cage linked to
the piece as the retopology tool does: sculpting a linked piece keeps
working, and the next Poly Edit session picks up the sculpted positions.

### 2. Interaction

- **Hover** shows what you'd act on. The point, edge or face under the
  cursor highlights, and so does everything the current reach adds (the
  whole loop, group or region), before you click.
- **Each kind of element has its own current action.** Pointing at a face
  and dragging extrudes (say), and pointing at an edge and clicking inserts
  a loop, with no mode switch between them.
- **Choosing actions.** A **Poly Edit window** (like the Deform and Tube
  windows) has three short lists, Point, Edge and Face. Each list shows the
  action, its reach, and its options, such as segments, amount, curvature,
  "Join touching faces" and "Keep face groups". The cursor quick menu (Space in
  the presets that use it), opened over an element, lists that element's
  actions.
- **Click or drag.** Actions with an amount (extrude, inset, bevel, slide)
  follow the drag with a live preview, and letting go applies them.
  Instant actions (delete, insert, flip, crease) happen on click.
  **Escape** during a drag cancels.
- **Repeat.** **Repeat on hovered** (a keymap command; its default key is picked so it doesn't clash with any keymap preset) applies the last
  action and amount to whatever is under the cursor.
- **Mirror.** X/Y/Z symmetry around the piece's symmetry centre, as in the
  retopology tool. Elements on the mirror plane are welded, not doubled.
- **Shift while dragging.** For extrude and inset, Shift switches from "along
  the normal" to a free move in the view plane, for quick reshaping.
- **Undo.** Each click or drag is one undo step (`record_cage`), labeled
  with the action ("Poly Edit: Extrude").

### 3. Reach

| Reach | Points | Edges | Faces |
|---|---|---|---|
| **This one** | the point | the edge | the face |
| **Loop** | — | the edge loop (stops at poles and borders) | the face loop through the clicked edge side |
| **Ring** | — | the edges across the face ring | — |
| **Between two clicks** | the path of points | the strip of edges | the strip of faces |
| **Face group** | its points | its border edges | every face in the group |
| **Face group, this island** | | | the group's faces connected to the click |
| **Flat region** | | | connected faces within an angle (option, default 5°) |
| **Island** | its points | its edges | every connected face |
| **Masked** | masked points | edges between masked points | faces whose corners are all masked |

### 4. Actions, by phase

Every action keeps the cage valid. Each face's winding stays consistent
with its neighbours, no edge is left with more than two faces unless it
already had them, and face groups are kept. A shared validity check runs
after every operation in tests.

**Phase A: the core** (the first tester release)

- **Faces:**
  - **Extrude** along the normal or the average normal. **Join touching
    faces** merges the new caps with neighbouring ones in the same region,
    so drawing out a block next to another becomes one surface.
  - **Inset** (one face at a time, or the reach as a region).
  - **Delete** (leaves a hole).
  - **Move** (along the normal, or free with Shift).
  - **Flip**.
  - **Set face group** (picked or new).
  - **Mask**.
- **Edges:**
  - **Insert loop**, with a slide position, using the existing
    `insert_loop` extended to n-gons.
  - **Delete edge / loop**, which dissolves into n-gons and can't leave
    points without faces.
  - **Slide**.
  - **Extrude a border edge** into a new face.
  - **Close hole** (one face, or quads if the hole has an even edge count).
- **Points:**
  - **Move**.
  - **Weld two points** (exists as `weld`).
  - **Delete**, which removes the point's faces.
- **Mesh menu:** **Edit as polygons** and **Done**.

**Phase B: shaping**

- **Bevel** for edges and faces, with segments and a profile (flat to round).
  *Landed with one segment and a flat profile; segments and profile are
  left for later.*
- **Bridge** two faces, loops or borders, with segments and a twist
  offset.
- **Split face** between two clicked points.
- **Spin edge**, which rotates an edge within its two faces.
- **Equalize** (relax spacing). The existing `relax` already does this,
  keeping points on the current surface.
- **Edge crease** on or off, or a weight. *(Moved to phase C.)*
- **Point crease**, for all of the point's edges. *(Moved to phase C.)*

**Phase C: smooth results**

- **Subdivide (smooth)** with creases. A Catmull–Clark step on the cage
  (Catmull & Clark, "Recursively generated B-spline surfaces on arbitrary
  topological meshes", CAD 1978), with semi-sharp creases from DeRose,
  Kass & Truong, "Subdivision surfaces in character animation"
  (SIGGRAPH 1998), cited in the code. **Preview smooth** shows the
  subdivided surface over the cage while you model.
- **Hand off to sculpting.** **Done > Subdivide and sculpt** builds a
  multires stack from the smoothed mesh. The current stack is Loop-based,
  so a Catmull–Clark first level feeds it.

### 5. Code layout

- **`oxy-mesh/src/retopo.rs`** is split into `cage.rs` (data, n-gons,
  per-face and per-edge data, validity) and `retopo.rs` (the drawing
  operations RFC 0012 needs).
- **New `oxy-mesh/src/polyedit.rs`.** Pure topology functions that take a
  cage and the reached elements and return the changed cage. They use no
  GPU and no app state, are fully unit-tested, and can be reached from the
  plugin API later.
- **New `oxy-mesh/src/catmull_clark.rs`** (Phase C).
- **New `oxy-app/src/polyedit.rs` and `polyedit_ui.rs`.** Hover picking,
  reach preview, drags, the window, keymap commands, palette entries and
  tips.
- **Picking** reuses the retopology tool's point and edge picking. Face
  picking uses the built mesh's raycast and maps the hit triangle back to
  its cage face.
- **Overlay** reuses the retopology overlay (cage edges, points), adding
  highlight colors for the hovered element and the reach.

### 6. Performance

Edits rebuild the triangle mesh from the cage, which is O(faces). A new
example, `cargo run --release -p oxy-mesh --example polyedit_timing`,
measures extrude, inset, bevel and rebuild at 1k, 10k, 50k and 200k faces.
The target is under 16 ms per drag update at 50k faces. If the rebuild
dominates, the update becomes chunk-local: the cage's faces map to known
triangle ranges. No work is added to the sculpting per-dab path.

### 7. Tests

- **Per action:** the result is valid; point, edge and face counts match
  the expected ones (for example, extruding one quad of a cube gives
  10 faces); face groups and creases are kept; and a mirrored action gives
  a mirrored result.
- **Round trips:**
  - Edit as polygons then Done keeps the shape.
  - `.oxb` save and open with `PLYC`.
  - An older reader still gets a usable `CAGE`.
- **Editor tests:** hover then drag then undo restores the cage exactly;
  Repeat works; Escape cancels a drag.
- **Headless screenshots** of the overlay and reach highlight.

## Phasing and size

| Step | What | Rough size |
|---|---|---|
| 0 | Cage: n-gons, face groups, `PLYC`, validity check (creases moved to B, where they are first used; the `cage.rs` split deferred) | with A1 |
| A1 | Tool shell: hover picking, reach preview, window, undo, symmetry; Extrude, Inset, Delete, Move | 1 PR |
| A2 | Remaining phase A actions; Edit as polygons; docs; tester release | 1 PR |
| B | Bevel, Bridge, Split, Spin, Equalize (and phase A's Slide and Repeat) | landed |
| C | Creases, Catmull–Clark with creases, preview, hand-off to sculpting | 1 PR |

## Alternatives considered

- **A separate half-edge mesh just for this tool.** It is cleaner in
  theory, but it would be a third geometry type next to `Mesh` and `Cage`,
  needing its own save, undo and sync code. Extending the cage keeps one
  path.
- **Modeling on the triangle mesh directly.** Without real quads, loops
  and rings are guesses. The quad pairing exists, but it is a heuristic
  for counting, not for editing.
- **Merging the retopology tool into Poly Edit.** Possible later: the two
  would share the engine, and retopology would just be Poly Edit with
  snapping to a surface. Kept separate for now, so the retopology tool
  doesn't change under its users.

## Open questions

1. Should the first release include n-gons, or keep quads and triangles
   only and add n-gons with Bevel and Bridge in phase B?
2. Should Poly Edit eventually absorb the retopology tool (as "Poly Edit,
   snap to surface")?
3. What should the default actions be per element? The proposal is Face:
   Extrude, Edge: Insert loop, Point: Move.

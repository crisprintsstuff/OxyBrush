# RFC 0029: Linked copies

Status: Implemented, from issue #327, with axis selection from #427

## Summary

**Pieces > Duplicate linked** and **Mirror duplicate linked** make a copy of
the active piece that keeps following it: sculpt, paint or remesh the
original and every linked copy changes with it, live. Each copy has its own
place: move, turn or scale it with the transform gizmo and it stays linked
where it was put. **Unlink copy** makes one an ordinary piece.

The name is generic on purpose: other products' names for this feature are
trademarks (AGENTS.md, "Trademarks"). Surface instances (RFC 0022) keep
their name: they place many copies on another piece's surface, which is a
different tool.

## Why

Paired parts (ears, horns, hands, wings) are sculpted twice or mirrored
again after every change. Mirror symmetry works only for one piece across
its own plane; linked copies work across pieces, at any position,
orientation and scale, and also for repeated parts that aren't mirrored.

## Design

- **Mirror axis.** Pieces > Mirror duplicate linked offers X, Y or Z. The
  selected coordinate is reflected about the plane through the piece's
  symmetry center (the origin when unset). Pieces > Mirror duplicate
  offers the same axes, with its plane through the world origin. The
  command palette lists each axis as its own command.

- **Data.** A piece may have `link: Option<PieceLink>`: the original's id
  and a transform (original space to copy space). A transform with a
  negative determinant is a mirrored copy. A copy of a copy links to the
  same original.
- **The copy is an ordinary piece** with its own mesh. Drawing, picking,
  bounds, saving, export, Join and the rest need nothing new. The cost is
  memory: a copy takes as much as its original, where GPU instancing would
  share it. Instancing would need a transform on pieces, which no other
  part of the app has; this keeps the change contained.
- **Sync.** Every frame, `Editor::sync_links` compares each pair's mesh
  versions (topology, edit, face) with those it last synced:
  - The original changed with the same points: the copy's points whose
    original moved (or were masked or painted) are updated, found by
    comparing against the positions last synced, so a dab touches only its
    part of the copy. Measured: 0.4 ms a frame at 41k points, 1.8 ms at
    655k (release build, 4 cores), against 31 ms when the whole copy was
    rebuilt.
  - The original's points or faces were rebuilt (remesh, subdivision, face
    groups): the copy is rebuilt from it.
  - Only the copy changed (a gizmo move, or undo of one): its transform is
    fitted again from the two meshes (least squares). If the copy changed
    shape and not just place, the original wins and the status line says
    so.
- **Sculpting a copy** is refused with a status message: two-way editing
  can't tell a small sculpt from a move, and one shape to edit keeps it
  simple.
- **Moving the original** moves its copies with it (a mirrored copy the
  other way), since their shape is the original's wherever it is.
- **Undo.** Duplicate linked is one step (a piece added); Unlink is one
  step (`Step::Link`). Changes the sync makes aren't steps of their own:
  undoing the original's change takes the copy back with it.
- **Saving.** A new optional `LINK` section after the copy's `PIEC`:
  `source:u32 transform:f32*16`. The copy's mesh is saved as it is, so
  older versions open an ordinary piece. A link to itself, to a copy, or
  with a broken transform is ignored on load.
- **Deleting the original** leaves its copies as they are until undo brings
  it back; a project saved meanwhile drops their links.

## Not included

- Sculpting a copy and having the original follow.
- GPU instancing of copies, to save memory.
- Showing the link in the Pieces list beyond the copy's name and the line
  under the Pieces menu.

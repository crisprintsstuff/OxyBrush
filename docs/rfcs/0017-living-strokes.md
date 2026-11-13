# RFC 0017: Living strokes

Status: accepted; phase 1 landed (record and edit, off by default: Preferences > General). Phase 2 (groups, saving) still to do

## Summary

Every brush stroke stays editable after you make it. A **Strokes** panel
lists the strokes on the active piece; for any of them, without undoing what
came after, you can fade it, delete it, mask part of it out, see it on its
own, or put it in a named group and treat the group like a sculpt layer.

## Why

In sculpting apps today a stroke is final once it's done. To change one you
made twenty minutes ago you either undo everything since, or you had to
expect the change in advance and sculpt on a layer. Layers help, but only if
you planned for them: most strokes end up on the base mesh, and "that one
wrinkle is too deep" means undoing or sculpting over it.

Looking at what other tools publish (public pages only, see
[CLEAN_ROOM.md](../CLEAN_ROOM.md)):

- ZBrush has sculpt layers you create ahead of time, and a linear undo
  history.
- SculptForge (GPL-3.0, so only its public notes were read) keeps undo
  history that branches instead of throwing away later steps.

Neither lets you reach back to one past stroke and change it while keeping
the work after it. Living strokes does that, and gives you layers you didn't
have to create in advance: group the strokes after the fact.

## What the user sees

**Mesh > Strokes…** (also in the command palette, and a button in the
History section) opens the Strokes panel for the active piece.

- **The list:** one row per stroke, newest first: brush icon, brush name,
  how long ago, and how many points it moved. Hovering a row highlights the
  area it touched on the model; clicking selects it (Shift and Ctrl to
  select several).
- **Strength:** a slider from 0 to 150% for the selected strokes. Dragging it
  updates the model live; letting go records one undo step. 0% hides the
  stroke without deleting it.
- **Delete:** removes the selected strokes' change from the surface. Later
  strokes stay.
- **Solo:** shows the model as it would be with only the selected strokes
  on top of the state before the first of them, until you click Solo again.
- **Mask it out:** "Keep only where unmasked" multiplies the selected
  strokes' change by `1 - mask`. Paint a mask over the half of a stroke you
  don't want, press it, clear the mask.
- **Groups:** "Group…" puts the selected strokes into a named group ("Wrinkle
  pass", "Scales"). A group row has its own strength slider and eye toggle,
  which makes it a sculpt layer. Groups can be renamed, merged and
  ungrouped.
- **Bake:** "Bake all" flattens everything into the base mesh and clears the
  list, to save memory or before sharing a file.

The panel names only what a stroke did. It is not a full history browser;
Undo/Redo keep working as they do now, and changes made in the panel are
undo steps too.

## How it works

### Recording a stroke

Strokes already snapshot each chunk they touch into the undo history
**before** changing it (`History::touch`). When the stroke is committed, the
same chunk list tells us exactly which vertices could have moved. For each,
`delta = position_after - position_before`; vertices whose delta is
(nearly) zero are dropped. A stroke becomes:

```text
StrokeRecord {
    id: u64,
    piece: u64,
    brush: BrushKind,            // for the icon and label
    time: Instant,
    vertices: Vec<u32>,          // sorted
    deltas: Vec<Vec3>,           // same length
    color: Option<Vec<[f32; 3]>>,// Paint strokes: color change, same scheme
    strength: f32,               // current factor, 1.0 when recorded
    group: Option<u32>,
}
```

A typical stroke over 5,000 points takes about 5,000 × 16 bytes = 80 KB;
a few hundred strokes fit in tens of MB. Strokes share a memory budget the
way undo does; when it's exceeded, the oldest strokes are baked (they stay
in the mesh, they just can't be edited any more), and the panel says so.

### Editing a stroke: additive model

The surface is treated as **the base plus the sum of every stroke's change
times its strength**. Changing a stroke's strength from `s` to `t` moves
each of its vertices by `delta × (t − s)`; deleting it is `t = 0`. That's a
sparse add over the stroke's own points, so it's as fast as one brush dab:
milliseconds even on million-triangle meshes, and it uses the existing
`Mesh::update_after_move` path with the stroke's vertices.

This is an approximation, and that's deliberate. Brushes are not linear: a
Smooth stroke made after a Draw stroke smoothed the shape the Draw stroke
made. With the additive model, fading the Draw stroke leaves the Smooth
stroke's own change as it was, rather than recomputing it. In practice this
behaves like sculpt layers in other tools, which add changes the same way,
and it's predictable: what you see is the sum of what each stroke did.

### Exact replay (later, optional)

For small meshes a stroke can also be stored as its *input* (the dab
positions, radii, strengths, normals and brush settings already worked out
by the stroke engine), so later strokes can be replayed from the edited one
onward. That gives exact results at a cost proportional to the number of
later strokes, so it would run as a background job and only be offered when
the estimate is short. It's not part of the first version.

### When the list is baked

Anything that changes the triangles makes stored vertex indices
meaningless, so the stroke list is baked first: remesh, decimate,
subdivide (single mesh), dynamic detail, booleans, Close holes, Split,
Join, Symmetrize, retopology, and importing over the piece. These already go
through `record_topology`; that call bakes the piece's strokes and the panel
shows a one-line note ("Strokes baked by Voxel remesh").

**Subdivision levels:** strokes are recorded on the level they were made
on. Switching levels bakes the strokes of the level you leave, since detail
moves between levels when you switch.

Whole-piece edits (the Deform window, the transform gizmo, Fit to standard
size) are recorded as strokes too, named after the tool, so they can be
faded like any stroke.

### Undo and the panel

Panel edits (strength, delete, mask out, grouping, baking) are undo steps
that store the affected strokes' old state. Undoing a stroke removes its
record; redoing it restores it.

### Saving

A new skippable `.oxb` section, `STRK`, holds the stroke records of each
piece (vertex indices, deltas as `f32`, optional colors, strength, group,
brush, and group names), LZ4-compressed like the other sections. Older
versions skip it and open the project as it looks, with everything baked.
A project setting can turn off saving strokes, to keep files small.

### Plugins and agents

The plugin API and the MCP server get read access to the stroke list (id,
brush, size, group) and the panel actions as commands, so a script can, for
example, fade every stroke in a group.

## Performance

- Recording: one subtraction per vertex in the touched chunks at commit,
  about the cost of the undo snapshot that's already taken. Measured target:
  under 2 ms for a 15k-vertex stroke (`dab_timing` scenario), to be reported
  in the PR.
- Strength changes and deletes: proportional to the stroke's own vertex
  count.
- Memory: covered by the budget above; the panel shows how much it uses.

## Phases

1. **Record and edit** (about 2–3 weeks): recording, the Strokes panel with
   strength, delete and solo, baking on topology changes, memory budget,
   undo for panel edits. Not saved yet.
2. **Groups and saving:** groups as layers, mask out, `STRK` section, the
   plugin/MCP surface.
3. **Beyond:** copy a stroke to another place or the mirrored side,
   per-group displacement export, exact replay for small meshes.

## Open questions

- Should strokes made with symmetry be one record (all sides) or one per
  side? One record is simpler; per side lets you fade just the left wrinkle.
  Proposed: one record, with "split by side" as a later action.
- Should the panel also list mask strokes? Proposed: no; masks are cheap to
  redo and would crowd the list.
- Default memory budget: proposed 256 MB, adjustable in Preferences.

Decided for phase 1: one record per symmetric stroke, mask strokes not
listed, a 256 MB budget adjustable in Preferences. Living strokes is an
option, **off by default** (Preferences > General, or the button in the
Strokes window), so people who don't use it pay no memory for it; turning
it off bakes the list.

## Not included

- A branching undo tree. Living strokes covers the main reason people want
  one (keeping later work while changing something earlier) without the
  complexity of navigating branches.
- Recording strokes on meshes over the triangle limit for subdivision; the
  same limit applies.

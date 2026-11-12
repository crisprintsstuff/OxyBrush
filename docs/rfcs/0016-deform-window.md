# RFC 0016: The Deform window

Status: implemented (unreleased)

## Why

Many sculpting tools have a panel of whole-object deformers: bend a piece,
twist it, taper it, inflate it, polish its surface, without a brush. Testers
asked for one. OxyBrush had pieces of this (the transform gizmo, Mesh >
Symmetrize, and the Squash and Displace plugins), but nothing that bends,
twists or polishes a whole piece at once.

## What it is

**Mesh > Deform…** (also in the command palette) opens a window with one row
per deformer. Each row is a bar to drag and three axis toggles, X, Y and Z,
as large buttons that are easy to hit with a mouse or a pen.

- **Drag a bar** left or right: the model shows the result live, from -100
  to 100. **Let go** to apply it as one undo step; the bar springs back to 0,
  so dragging again adds more. A click without dragging changes nothing.
  **Escape** while dragging puts the piece back.
- **The mask holds parts back,** like the transform gizmo: masked points
  stay put and partly masked points move partly. **Use mask** (on by
  default) can be turned off to deform the whole visible piece. Hidden
  points never move.
- Sizes are relative to the bounding box of what can move, so a deformer
  does the same thing to a small piece and a large one.

### Deformers

| Section | Deformer | What it does along the chosen axis / axes |
|---|---|---|
| Shape | Move, Rotate, Scale | Move, turn about the middle, resize |
| | Bend, S-bend | Curve the piece; the S-bend's halves curve opposite ways |
| | Skew, S-skew | Slant it; the S-skew moves the middle and keeps the ends |
| | Flatten, Flatten to axis, Flatten one side | Squash toward the middle plane, toward the middle line, or cut one side flat |
| | Twist, Taper, Squeeze, Perspective | Turn the ends opposite ways; widen one end; pinch the middle; grow one end |
| | Sag | Droop toward -axis, more the higher a point is |
| Surface | Noise, Inflate, Inflate keep size, Spherize | Random bumps, push out along normals (keep size scales back afterwards), pull toward a ball |
| Smoothing | Smooth, Polish, Relax | Laplacian smoothing (shrinks); Taubin smoothing that keeps volume (Taubin, *A signal processing approach to fair surface design*, SIGGRAPH 1995); slide points along the surface to even their spacing |
| | Polish keep detail / group edges / sharp edges | Polish that spares creases and pores (by curvature), keeps face-group borders as crisp lines, or keeps sharp edges (neighbours weighted by how alike their normals are) |

Deformers that work *along* an axis (bend, twist, taper, …) are applied once
per chosen axis. The others only move points along the chosen axes.

### Other controls

- **Fit to standard size:** centre the piece on the origin and scale it so its
  longest side is 2 units.
- **Mirror X / Y / Z** (the gizmo's mirror) and **Symmetrize** (Mesh >
  Symmetrize) are repeated in the window.
- **Repeat on this piece** applies the last deformer again. **Repeat on other
  pieces** applies it to every other visible piece, each around its own
  middle, as one undo step.

The toolbar's and Symmetry section's **Mirror X / Y / Z** buttons also became
larger (32×28 px).

## How

`oxy_mesh::deform` holds the math and has no GPU or UI code. A
`Deformation` is prepared once per drag from the piece (positions, normals,
weights, and only for the smoothing deformers the neighbour lists and face
group edges). `apply` recomputes new positions from the originals for any
amount, so each frame of the drag is independent of the previous one. The
editor snapshots the piece's chunks into the undo history when the drag
starts (`History::begin` / `touch`), writes the positions each frame with
`Mesh::update_after_move`, and commits on release, or cancels when the
amount is back at 0.

Measured on a 1.3M-triangle mesh (4-core VM, release build): preparing takes
about 13 ms for the shape deformers, 150 ms for smoothing (neighbour lists),
and 320 ms for "Polish, keep detail" (curvature). Each preview frame takes
6–11 ms for shape deformers and 15–100 ms for smoothing (depending on the
amount), plus about 25 ms to refresh normals and bounds.

## Not included

- Deformers that need the mesh topology to be symmetric (restoring symmetry
  by matching points across the middle). Mesh > Symmetrize covers the common
  case by rebuilding one half.
- Saving the axis choices between sessions.
